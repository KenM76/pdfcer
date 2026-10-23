//! Structure elements whose content was redacted.
//!
//! A structure element's `/ActualText`, `/Alt` and `/E` (ISO 32000-1 §14.7.2
//! Table 323; §14.9.3–§14.9.5) are read *instead of* the content the element
//! encloses, and they cover the element and all of its children. When a
//! marked-content sequence inside an element's subtree lost content, the
//! element and every ancestor that encloses it lose all three entries.
//!
//! Content items are matched by `(page, stream, MCID)`: an integer kid or an
//! MCR (Table 324) names an MCID on the element's `/Pg` (inherited from the
//! nearest ancestor that has one), or in the form XObject its `/Stm` names.
//! An `/OBJR` kid names a whole object and is left to the other carriers.
//!
//! The strip applies regardless of the residual scope: the text is bound to
//! the removed content by structure, not found by matching it.

use std::collections::BTreeSet;

use super::{CarrierAction, RedactionReport};
use crate::document::Document;
use crate::object::{Dict, Name, ObjId, Object};
use crate::writer::DirtySet;

/// Content items that lost content: `(page, form stream or None, MCID)`.
pub(super) type RedactedItems = BTreeSet<(ObjId, Option<ObjId>, i64)>;

/// Nesting deeper than this is not a real structure tree.
const MAX_DEPTH: usize = 256;

struct Walk<'a> {
    doc: &'a Document,
    items: &'a RedactedItems,
    dirty: &'a mut DirtySet,
    visited: BTreeSet<ObjId>,
    stripped: usize,
    unresolved: usize,
}

/// Strip the replacement text from every structure element enclosing a
/// redacted content item, and record the `struct_tree` carrier.
pub(super) fn strip_replacement_text(
    doc: &Document,
    items: &RedactedItems,
    dirty: &mut DirtySet,
    report: &mut RedactionReport,
) {
    let Some(root) = doc
        .catalog()
        .ok()
        .and_then(|c| c.get(b"StructTreeRoot"))
        .cloned()
    else {
        report.add_carrier("struct_tree", false, CarrierAction::Absent);
        return;
    };
    let mut walk = Walk {
        doc,
        items,
        dirty,
        visited: BTreeSet::new(),
        stripped: 0,
        unresolved: 0,
    };
    if !items.is_empty() {
        walk.root(&root);
    }
    disclose(walk.stripped, walk.unresolved, report);
}

fn disclose(stripped: usize, unresolved: usize, report: &mut RedactionReport) {
    if unresolved > 0 {
        report.add_carrier("struct_tree", true, CarrierAction::DisclosedNotScrubbed);
        report.note(format!(
            "redaction: {unresolved} structure-tree node(s) could not be walked; their \
             /ActualText, /Alt and /E may duplicate redacted content and were NOT scrubbed"
        ));
    } else if stripped > 0 {
        report.add_carrier("struct_tree", true, CarrierAction::Scrubbed);
    } else {
        report.add_carrier("struct_tree", true, CarrierAction::CheckedClean);
    }
    if stripped > 0 {
        report.note(format!(
            "redaction: {stripped} structure element(s) lost /ActualText, /Alt and /E because \
             content they enclose was redacted (ISO 32000-1 §14.7.2)"
        ));
    }
}

impl Walk<'_> {
    /// The tree root: its kids are walked, and a changed direct kid is
    /// written back through the root object.
    fn root(&mut self, root: &Object) {
        let Some(id) = root.as_reference() else {
            // A direct root would need the catalog rewritten; walk it only
            // to find out whether anything needed stripping.
            if let Some(dict) = root.as_dict() {
                let (_, changed) = self.element(dict, None, 0);
                if changed.is_some() {
                    self.unresolved += 1;
                }
            }
            return;
        };
        let Some(dict) = self.effective(id).and_then(Object::as_dict).cloned() else {
            self.unresolved += 1;
            return;
        };
        self.visited.insert(id);
        let Some(k) = dict.get(b"K").cloned() else {
            return;
        };
        if let Some(k) = self.kids(&k, None, 1) {
            let mut new = dict;
            new.insert(Name::from(b"K".as_slice()), k);
            self.dirty.replace(id, Object::Dict(new));
        }
    }

    fn effective(&self, id: ObjId) -> Option<&Object> {
        if self.dirty.is_deleted(id) {
            return None;
        }
        self.dirty
            .replacement(id)
            .or_else(|| self.doc.get(id).map(|o| &o.value))
    }

    /// Walk `/K` (one kid or an array). Returns the rewritten `/K` when a
    /// direct kid changed; whether any kid holds redacted content is
    /// accumulated through `hit`.
    fn kids_hit(
        &mut self,
        k: &Object,
        page: Option<ObjId>,
        depth: usize,
    ) -> (bool, Option<Object>) {
        match k {
            Object::Array(list) => {
                let mut hit = false;
                let mut out: Option<Vec<Object>> = None;
                for (i, kid) in list.iter().enumerate() {
                    let (h, changed) = self.kid(kid, page, depth);
                    hit |= h;
                    if let Some(c) = changed
                        && let Some(slot) = out.get_or_insert_with(|| list.clone()).get_mut(i)
                    {
                        *slot = c;
                    }
                }
                (hit, out.map(Object::Array))
            }
            single => self.kid(single, page, depth),
        }
    }

    fn kids(&mut self, k: &Object, page: Option<ObjId>, depth: usize) -> Option<Object> {
        self.kids_hit(k, page, depth).1
    }

    /// One kid: `(holds redacted content, replacement when direct and changed)`.
    fn kid(&mut self, kid: &Object, page: Option<ObjId>, depth: usize) -> (bool, Option<Object>) {
        if depth > MAX_DEPTH {
            self.unresolved += 1;
            return (false, None);
        }
        match kid {
            Object::Integer(mcid) => (self.redacted(page, None, *mcid), None),
            Object::Reference(id) => (self.indirect(*id, page, depth), None),
            Object::Dict(d) => {
                let (hit, changed) = self.node(d, page, depth);
                (hit, changed.map(Object::Dict))
            }
            _ => (false, None),
        }
    }

    fn indirect(&mut self, id: ObjId, page: Option<ObjId>, depth: usize) -> bool {
        if !self.visited.insert(id) {
            return false;
        }
        let Some(dict) = self.effective(id).and_then(Object::as_dict).cloned() else {
            return false;
        };
        let (hit, changed) = self.node(&dict, page, depth);
        if let Some(new) = changed {
            self.dirty.replace(id, Object::Dict(new));
        }
        hit
    }

    /// A dictionary kid: an MCR, an OBJR, or a structure element.
    fn node(&mut self, d: &Dict, page: Option<ObjId>, depth: usize) -> (bool, Option<Dict>) {
        let ty = d
            .get(b"Type")
            .and_then(Object::as_name)
            .map(|n| n.as_bytes());
        if ty == Some(b"OBJR".as_slice()) {
            return (false, None);
        }
        if ty == Some(b"MCR".as_slice()) || d.get(b"MCID").is_some() {
            let page = d.get(b"Pg").and_then(Object::as_reference).or(page);
            let stm = d.get(b"Stm").and_then(Object::as_reference);
            let hit = d
                .get(b"MCID")
                .and_then(Object::as_int)
                .is_some_and(|m| self.redacted(page, stm, m));
            return (hit, None);
        }
        self.element(d, page, depth)
    }

    /// A structure element: walk its kids, then strip it when any holds
    /// redacted content.
    fn element(&mut self, d: &Dict, page: Option<ObjId>, depth: usize) -> (bool, Option<Dict>) {
        let page = d.get(b"Pg").and_then(Object::as_reference).or(page);
        let (hit, k) = match d.get(b"K") {
            Some(k) => self.kids_hit(k, page, depth + 1),
            None => (false, None),
        };
        let carries = super::marked::REPLACEMENT_KEYS
            .iter()
            .any(|key| d.get(key).is_some());
        if k.is_none() && !(hit && carries) {
            return (hit, None);
        }
        let mut new = d.clone();
        if let Some(k) = k {
            new.insert(Name::from(b"K".as_slice()), k);
        }
        if hit && carries {
            for key in super::marked::REPLACEMENT_KEYS {
                new.remove(key);
            }
            self.stripped += 1;
        }
        (hit, Some(new))
    }

    fn redacted(&self, page: Option<ObjId>, stm: Option<ObjId>, mcid: i64) -> bool {
        page.is_some_and(|p| self.items.contains(&(p, stm, mcid)))
    }
}
