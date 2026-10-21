//! Discarding actions by type during an optimize run (Pass 616.3): form
//! submit, import and reset actions, and actions that reach another file.
//!
//! Carriers (ISO 32000-2 §12.6.1, Tables 29, 31, 165, 176, 190, 191, 199):
//! `/A`, `/AA` (inline or indirect), the catalog's `/OpenAction`, and a
//! navigation node's `/NA` and `/PA`. An `/A` on a Movie annotation is a
//! movie activation, not an action (Table 189), and an `/OpenAction` array is
//! a destination; neither is touched. The document JavaScript name tree holds
//! only JavaScript actions, so it never carries one of these.
//!
//! A removed action is spliced out of its `/Next` tree, so the actions after
//! it still run in the same pre-order (§12.6.2 NOTE 1). An indirect action
//! that stays has its own `/Next` rewritten once, as its own object.
//!
//! "Another file" is GoToR and Launch, and GoToE and Thread when they carry
//! `/F` (without it the target is this file, Tables 204 and 209). URI actions
//! are web links and stay.

use std::collections::{BTreeMap, BTreeSet};

use super::objects::{DiscardObject, ObjectDiscard};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::view::DocumentView;

/// Deeper `/Next` trees are left as they are; the standard sets no bound
/// (§12.6.2), so this is the cycle and stack guard.
const MAX_DEPTH: usize = 64;

/// Carrier keys whose value is one action.
const SLOT_KEYS: &[&[u8]] = &[b"A", b"OpenAction", b"NA", b"PA"];

/// One entry an action discard sets or removes, applied only while the entry
/// still holds `before` (an earlier step of the run may have changed it).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Rewrite {
    pub(crate) owner: ObjId,
    /// One key, or `AA` and a trigger.
    pub(crate) path: Vec<Vec<u8>>,
    pub(crate) before: Object,
    /// `None` removes the entry.
    pub(crate) value: Option<Object>,
}

/// The class `action` belongs to, if it is one an optimize run can discard.
fn class_of(action: &Dict) -> Option<DiscardObject> {
    let s = action.get(b"S").and_then(Object::as_name)?;
    match s.as_bytes() {
        b"SubmitForm" | b"ResetForm" | b"ImportData" => Some(DiscardObject::FormActions),
        b"GoToR" | b"Launch" => Some(DiscardObject::ExternalReferences),
        b"GoToE" | b"Thread" if action.get(b"F").is_some() => {
            Some(DiscardObject::ExternalReferences)
        }
        _ => None,
    }
}

fn is_action(dict: &Dict) -> bool {
    dict.get(b"S").and_then(Object::as_name).is_some()
}

struct Filter<'a, 'v, W> {
    view: &'a DocumentView<'v>,
    wanted: W,
    classes: BTreeSet<DiscardObject>,
    types: BTreeSet<String>,
}

impl<W: Fn(DiscardObject) -> bool> Filter<'_, '_, W> {
    fn drops(&self, dict: &Dict) -> Option<DiscardObject> {
        class_of(dict).filter(|&c| (self.wanted)(c))
    }

    /// The actions `value` stands for once the wanted types are gone, in
    /// order, and whether that differs from `value`.
    fn node(&mut self, value: &Object, depth: usize, seen: &mut Vec<ObjId>) -> (Vec<Object>, bool) {
        let same = (vec![value.clone()], false);
        let id = value.as_reference();
        if depth > MAX_DEPTH {
            return same;
        }
        let view = self.view;
        let Some(dict) = view.resolve(value).as_dict().filter(|d| is_action(d)) else {
            return same;
        };
        let class = self.drops(dict);
        if id.is_some_and(|r| seen.contains(&r)) {
            // `seen` holds only removed actions: a cycle back to one ends.
            return (Vec::new(), true);
        }
        if let Some(class) = class {
            self.classes.insert(class);
            if let Some(s) = dict.get(b"S").and_then(Object::as_name) {
                self.types
                    .insert(String::from_utf8_lossy(s.as_bytes()).into_owned());
            }
            seen.extend(id);
            let kids = self.next(dict, depth + 1, seen).0;
            if id.is_some() {
                seen.pop();
            }
            return (kids, true);
        }
        if id.is_some() {
            // An indirect action that stays rewrites its own `/Next`.
            return same;
        }
        let (kids, changed) = self.next(dict, depth + 1, seen);
        if !changed {
            return same;
        }
        let mut copy = dict.clone();
        set_next(&mut copy, kids);
        (vec![Object::Dict(copy)], true)
    }

    fn next(&mut self, dict: &Dict, depth: usize, seen: &mut Vec<ObjId>) -> (Vec<Object>, bool) {
        let Some(next) = dict.get(b"Next") else {
            return (Vec::new(), false);
        };
        let view = self.view;
        let items: Vec<Object> = match view.resolve(next) {
            Object::Array(list) => list.clone(),
            _ => vec![next.clone()],
        };
        let mut out = Vec::new();
        let mut changed = false;
        for item in &items {
            let (kids, ch) = self.node(item, depth, seen);
            changed |= ch;
            out.extend(kids);
        }
        (out, changed)
    }

    /// The new value of a carrier holding `value`: `None` when unchanged,
    /// `Some(None)` when nothing is left. Several survivors become one action
    /// carrying the rest after its own `/Next`, which keeps the order.
    fn slot(&mut self, value: &Object) -> Option<Option<Object>> {
        let (list, changed) = self.node(value, 0, &mut Vec::new());
        if !changed {
            return None;
        }
        let mut list = list.into_iter();
        let Some(head) = list.next() else {
            return Some(None);
        };
        let rest: Vec<Object> = list.collect();
        if rest.is_empty() {
            return Some(Some(head));
        }
        let Some(mut dict) = self.view.resolve(&head).as_dict().cloned() else {
            return Some(Some(head));
        };
        let mut kids = self.next(&dict, 1, &mut Vec::new()).0;
        kids.extend(rest);
        set_next(&mut dict, kids);
        Some(Some(Object::Dict(dict)))
    }
}

fn set_next(dict: &mut Dict, kids: Vec<Object>) {
    match <[Object; 1]>::try_from(kids) {
        Ok([one]) => dict.insert(Name::from(b"Next".as_slice()), one),
        Err(kids) if kids.is_empty() => {
            dict.remove(b"Next");
        }
        Err(kids) => dict.insert(Name::from(b"Next".as_slice()), Object::Array(kids)),
    }
}

/// Every object holding an action of a class `wanted` selects, as one
/// [`ObjectDiscard`] per object (`bytes` zero; the session measures it).
pub(crate) fn action_carriers(
    view: &DocumentView<'_>,
    live: &BTreeSet<ObjId>,
    catalog: ObjId,
    wanted: impl Fn(DiscardObject) -> bool,
) -> Vec<ObjectDiscard> {
    let dicts: BTreeMap<ObjId, &Dict> = live
        .iter()
        .filter_map(|&id| Some((id, view.value(id)?.as_dict()?)))
        .collect();
    let aa_objects: BTreeSet<ObjId> = dicts
        .values()
        .filter_map(|d| d.get(b"AA")?.as_reference())
        .collect();
    let mut filter = Filter {
        view,
        wanted,
        classes: BTreeSet::new(),
        types: BTreeSet::new(),
    };
    let mut out = Vec::new();
    for (&id, dict) in &dicts {
        filter.classes.clear();
        filter.types.clear();
        let rewrites = if aa_objects.contains(&id) {
            trigger_rewrites(&mut filter, id, dict, &[])
        } else {
            object_rewrites(&mut filter, id, dict)
        };
        let Some(&object) = filter.classes.iter().next() else {
            continue;
        };
        let place = if id == catalog {
            "catalog".to_owned()
        } else {
            format!("object {} {}", id.num, id.generation)
        };
        let types: Vec<&str> = filter.types.iter().map(String::as_str).collect();
        out.push(ObjectDiscard {
            object,
            owner: id,
            location: format!("{place}: {}", types.join(", ")),
            bytes: 0,
            strips: Vec::new(),
            rewrites,
        });
    }
    out
}

fn object_rewrites<W: Fn(DiscardObject) -> bool>(
    filter: &mut Filter<'_, '_, W>,
    id: ObjId,
    dict: &Dict,
) -> Vec<Rewrite> {
    if filter.drops(dict).is_some() {
        // Its referrers splice it out; what it holds is no longer reached.
        return Vec::new();
    }
    let movie = dict
        .get(b"Subtype")
        .and_then(Object::as_name)
        .is_some_and(|n| n.as_bytes() == b"Movie");
    let mut out = Vec::new();
    for &key in SLOT_KEYS {
        let Some(before) = dict.get(key).filter(|_| !(movie && key == b"A")) else {
            continue;
        };
        if let Some(value) = filter.slot(before) {
            out.push(rewrite(id, vec![key.to_vec()], before, value));
        }
    }
    if let Some(Object::Dict(aa)) = dict.get(b"AA") {
        out.extend(trigger_rewrites(filter, id, aa, b"AA"));
    }
    if is_action(dict) {
        let (kids, changed) = filter.next(dict, 1, &mut Vec::new());
        if let (true, Some(before)) = (changed, dict.get(b"Next")) {
            let mut probe = Dict::default();
            set_next(&mut probe, kids);
            let value = probe.remove(b"Next");
            out.push(rewrite(id, vec![b"Next".to_vec()], before, value));
        }
    }
    out
}

/// The entries of an additional-actions dictionary, held directly by `id`
/// (`prefix` empty) or inline under `prefix`.
fn trigger_rewrites<W: Fn(DiscardObject) -> bool>(
    filter: &mut Filter<'_, '_, W>,
    id: ObjId,
    aa: &Dict,
    prefix: &[u8],
) -> Vec<Rewrite> {
    let mut out = Vec::new();
    for (trigger, before) in aa.iter() {
        if let Some(value) = filter.slot(before) {
            let mut path = Vec::new();
            if !prefix.is_empty() {
                path.push(prefix.to_vec());
            }
            path.push(trigger.as_bytes().to_vec());
            out.push(rewrite(id, path, before, value));
        }
    }
    out
}

fn rewrite(owner: ObjId, path: Vec<Vec<u8>>, before: &Object, value: Option<Object>) -> Rewrite {
    Rewrite {
        owner,
        path,
        before: before.clone(),
        value,
    }
}

/// Apply `r` to `dict` if its entry still holds `r.before`; the old value
/// when it did. An inline `/AA` left empty is removed.
pub(crate) fn apply_rewrite(dict: &mut Dict, r: &Rewrite) -> Option<Object> {
    match r.path.as_slice() {
        [key] => set_entry(dict, key, r),
        [outer, key] => {
            let Some(Object::Dict(mut inner)) = dict.get(outer).cloned() else {
                return None;
            };
            let old = set_entry(&mut inner, key, r)?;
            if inner.is_empty() {
                dict.remove(outer);
            } else {
                dict.insert(Name::from(outer.as_slice()), Object::Dict(inner));
            }
            Some(old)
        }
        _ => None,
    }
}

fn set_entry(dict: &mut Dict, key: &[u8], r: &Rewrite) -> Option<Object> {
    if dict.get(key) != Some(&r.before) {
        return None;
    }
    match &r.value {
        Some(v) => dict.insert(Name::from(key), v.clone()),
        None => {
            dict.remove(key);
        }
    }
    Some(r.before.clone())
}
