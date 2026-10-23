//! Marked-content sequences whose content was redacted.
//!
//! A `BDC` property list may carry `/ActualText`, `/Alt` or `/E` (ISO
//! 32000-1 §14.9.3–§14.9.5; the same keys Table 323 defines for structure
//! elements), text that a reader extracts *instead of* the content inside the
//! sequence. Removing the content while leaving that entry leaves the
//! redacted words readable, so when anything inside a sequence is removed
//! (a glyph, a path, an image placement) the sequence loses all three
//! entries. The kept glyphs then extract through their own encoding.
//!
//! An inline property list is rewritten in place. A named one (a
//! `/Properties` resource, §14.6.2) is replaced by an inline copy without the
//! entries, which leaves the shared resource alone. An inline property list
//! may hold only direct objects, so a named list that references another
//! object is left as it is, and the residual sweep reports it.
//!
//! A sequence with an `/MCID` (§14.7.4.2) is a structure content item; its
//! identifier is recorded so `redact_struct` can strip the same keys from
//! the structure elements that own it.

use super::{Edit, Surgeon};
use crate::content::{ContentToken, ContentTokenKind};
use crate::object::{Dict, ObjId, Object};

/// The replacement-text keys of Table 323.
pub(super) const REPLACEMENT_KEYS: [&[u8]; 3] = [b"ActualText", b"Alt", b"E"];

/// One open `BDC`, until its `EMC`.
pub(super) struct MarkedOpen {
    /// The property-list operand's byte range and the list without the
    /// replacement keys, when it carries any and can be written inline.
    replacement: Option<((usize, usize), Dict)>,
    /// The sequence's marked-content identifier.
    mcid: Option<i64>,
    /// [`Surgeon::removed_so_far`] when the sequence opened.
    removed_at_open: u64,
}

impl Surgeon<'_> {
    /// Content items removed so far in this stream and the forms it paints.
    fn removed_so_far(&self) -> u64 {
        let nested: usize = self.forms.images.iter().map(|n| n.hits.len()).sum();
        self.glyphs_removed + self.vector_paths_cut + (self.image_hits.len() + nested) as u64
    }

    /// `BDC`: open a sequence. Every sequence is pushed so that `EMC` pops
    /// the matching entry.
    pub(super) fn marked_open(&mut self, operands: &[ContentToken]) {
        let list = operands.get(1).and_then(|t| self.property_list(t));
        let mcid = list
            .as_ref()
            .and_then(|(d, _)| d.get(b"MCID"))
            .and_then(Object::as_int);
        let replacement = list.and_then(|(d, named)| replacement_free(d, named));
        let span = operands.get(1).map(|t| (t.span.start, t.span.end()));
        self.marked.push(MarkedOpen {
            replacement: replacement.zip(span).map(|(d, s)| (s, d)),
            mcid,
            removed_at_open: self.removed_so_far(),
        });
    }

    /// `EMC`: close the innermost sequence.
    pub(super) fn marked_close(&mut self) {
        let Some(open) = self.marked.pop() else {
            return;
        };
        if self.removed_so_far() == open.removed_at_open {
            return;
        }
        if let Some(mcid) = open.mcid {
            self.redacted_mcids.insert((self.stream_id, mcid));
        }
        if let Some((span, stripped)) = open.replacement {
            self.strip(span, stripped);
        }
    }

    /// End of stream: an unbalanced `BDC` still covers what follows it.
    pub(super) fn marked_close_all(&mut self) {
        while !self.marked.is_empty() {
            self.marked_close();
        }
    }

    fn strip(&mut self, span: (usize, usize), stripped: Dict) {
        let mut bytes = Vec::new();
        crate::writer::serialize::write_object(
            &mut bytes,
            &Object::Dict(stripped),
            ObjId::new(0, 0),
            &[],
            &crate::writer::IdentityEncoder,
        );
        self.edits.push(Edit {
            start: span.0,
            end: span.1,
            bytes,
        });
        self.replacement_text_stripped += 1;
    }

    /// The property list `token` holds or names, and whether it was named.
    fn property_list(&self, token: &ContentToken) -> Option<(Dict, bool)> {
        let ContentTokenKind::Operand(operand) = &token.kind else {
            return None;
        };
        match operand {
            Object::Dict(d) => Some((d.clone(), false)),
            Object::Name(n) => self.named_property_list(n.as_bytes()).map(|d| (d, true)),
            _ => None,
        }
    }

    fn named_property_list(&self, name: &[u8]) -> Option<Dict> {
        let props = self
            .resources
            .get(b"Properties")
            .map(|o| self.doc.resolve(o))
            .and_then(Object::as_dict)?;
        props
            .get(name)
            .map(|o| self.doc.resolve(o))
            .and_then(Object::as_dict)
            .cloned()
    }
}

/// `dict` without the replacement keys, if it carries any and the result
/// can be written inline.
fn replacement_free(dict: Dict, named: bool) -> Option<Dict> {
    if !REPLACEMENT_KEYS.iter().any(|k| dict.get(k).is_some()) {
        return None;
    }
    let mut stripped = dict;
    for k in REPLACEMENT_KEYS {
        stripped.remove(k);
    }
    if named && stripped.iter().any(|(_, v)| has_reference(v)) {
        return None;
    }
    Some(stripped)
}

fn has_reference(obj: &Object) -> bool {
    match obj {
        Object::Reference(_) => true,
        Object::Array(items) => items.iter().any(has_reference),
        Object::Dict(d) => d.iter().any(|(_, v)| has_reference(v)),
        _ => false,
    }
}
