//! The page a structure destination resolves to (ISO 32000-2 §12.3.2.3),
//! shared by every operation that drops the structure tree and must turn
//! structure destinations into page destinations first.
//!
//! The kids are walked in order: a marked-content or object reference
//! gives its own `/Pg` or else the element's, an integer MCID gives the
//! element's `/Pg`, a structure element kid is searched in turn. `None`
//! means no kid names a page; §12.3.2.3 then makes the document's first
//! page the target ("shall"), which the caller supplies.

use std::collections::BTreeSet;

use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};

/// Guard for the structure-tree walk (ARCHITECTURE §10).
const MAX_DEPTH: usize = 64;

/// The explicit-destination types of Table 149, which tell a destination
/// array from any other array whose first element is a reference.
const DEST_TYPES: [&[u8]; 8] = [
    b"XYZ", b"Fit", b"FitH", b"FitV", b"FitR", b"FitB", b"FitBH", b"FitBV",
];

/// The page `elem` resolves to, `None` when no kid names one.
pub(crate) fn element_page<G: ObjectGraph + ?Sized>(graph: &G, elem: ObjId) -> Option<ObjId> {
    search(graph, elem, &mut BTreeSet::new(), 0)
}

fn search<G: ObjectGraph + ?Sized>(
    graph: &G,
    elem: ObjId,
    seen: &mut BTreeSet<ObjId>,
    depth: usize,
) -> Option<ObjId> {
    if depth > MAX_DEPTH || !seen.insert(elem) {
        return None;
    }
    let d = graph.value(elem)?.as_dict()?;
    let own = d.get(b"Pg").and_then(Object::as_reference);
    let kids = match d.get(b"K")? {
        Object::Array(a) => a.as_slice(),
        single => std::slice::from_ref(single),
    };
    for kid in kids {
        let found = match kid {
            Object::Integer(_) => own,
            Object::Dict(k) if is_content_ref(k) => {
                k.get(b"Pg").and_then(Object::as_reference).or(own)
            }
            Object::Reference(r) => match graph.value(*r) {
                Some(Object::Dict(k)) if is_content_ref(k) => {
                    k.get(b"Pg").and_then(Object::as_reference).or(own)
                }
                Some(Object::Dict(k)) if is_struct_elem_dict(k) => {
                    search(graph, *r, seen, depth + 1)
                }
                _ => None,
            },
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

/// A marked-content reference (Table 357) or object reference (Table 358).
fn is_content_ref(d: &Dict) -> bool {
    d.get(b"Type")
        .and_then(Object::as_name)
        .is_some_and(|n| matches!(n.as_bytes(), b"MCR" | b"OBJR"))
}

/// A structure element (Table 355): a structure type and a parent, and not
/// a page.
pub(crate) fn is_struct_elem_dict(d: &Dict) -> bool {
    let page = d
        .get(b"Type")
        .and_then(Object::as_name)
        .is_some_and(|n| n.as_bytes() == b"Page");
    !page && d.get(b"S").and_then(Object::as_name).is_some() && d.get(b"P").is_some()
}

/// The structure element an explicit destination array names, when its
/// first element is one and its second is a destination type.
pub(crate) fn struct_target<G: ObjectGraph + ?Sized>(graph: &G, dest: &[Object]) -> Option<ObjId> {
    let id = dest.first()?.as_reference()?;
    let kind = dest.get(1)?.as_name()?;
    if !DEST_TYPES.contains(&kind.as_bytes()) {
        return None;
    }
    graph
        .value(id)?
        .as_dict()
        .is_some_and(is_struct_elem_dict)
        .then_some(id)
}

/// A GoTo action, or a dictionary with no action type (a named
/// destination's `<< /D … >>`).
pub(crate) fn goto_like(d: &Dict) -> bool {
    d.get(b"S")
        .and_then(Object::as_name)
        .is_none_or(|n| n.as_bytes() == b"GoTo")
}
