//! Discarding object data during an optimize run (Pass 616.3): user
//! properties, the name-value data a CAD or diagram producer attaches to
//! the structure element of each drawn object (ISO 32000-1 §14.7.5.4,
//! 2.0 §14.7.6.4, Tables 361-362).
//!
//! Carriers: an attribute object whose `/O` is `/UserProperties`, held by a
//! structure element's `/A` (the element its own object or written inline
//! in a parent's `/K`) or by a class of the root's `/ClassMap`, and the
//! catalog's `MarkInfo /UserProperties` flag, which then claims attributes
//! that are gone (Table 321). An attribute object's revision number, the
//! integer after it in an `/A` array (§14.7.5.3), goes with it. An array
//! left with one attribute stays an array, as 2.0 permits; collapsing it
//! would reset that attribute's revision. A `/C` naming a class left empty
//! stays: an absent class is an empty class (2.0 erratum 254). The
//! elements, their `/R`, the parent tree and the content are untouched.

use std::collections::BTreeSet;

use super::actions::{Rewrite, rewrite};
use super::objects::{DiscardObject, ObjectDiscard, Strip};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::view::DocumentView;

/// Inline structure elements nested deeper are left as they are.
const MAX_DEPTH: usize = 64;

fn is_user_properties(view: &DocumentView<'_>, value: &Object) -> bool {
    view.resolve(value)
        .as_dict()
        .and_then(|d| d.get(b"O"))
        .and_then(Object::as_name)
        .is_some_and(|o| o.as_bytes() == b"UserProperties")
}

/// An attribute entry with its user properties gone: `None` when it holds
/// none, `Some(None)` when nothing else is left.
fn without(view: &DocumentView<'_>, value: &Object) -> Option<Option<Object>> {
    if is_user_properties(view, value) {
        return Some(None);
    }
    let Object::Array(items) = view.resolve(value) else {
        return None;
    };
    let mut kept = Vec::new();
    let mut dropped = false;
    let mut skip_revision = false;
    for item in items {
        if skip_revision && matches!(item, Object::Integer(_)) {
            skip_revision = false;
            continue;
        }
        skip_revision = is_user_properties(view, item);
        if skip_revision {
            dropped = true;
        } else {
            kept.push(item.clone());
        }
    }
    if !dropped {
        return None;
    }
    Some((!kept.is_empty()).then_some(Object::Array(kept)))
}

/// The class map's own rewrites, under `owner` (the root when inline).
fn class_rewrites(view: &DocumentView<'_>, owner: ObjId, map: &Dict, inline: bool) -> Vec<Rewrite> {
    let mut out = Vec::new();
    for (class, before) in map.iter() {
        if let Some(value) = without(view, before) {
            let mut path = Vec::new();
            if inline {
                path.push(b"ClassMap".to_vec());
            }
            path.push(class.as_bytes().to_vec());
            out.push(rewrite(owner, path, before, value));
        }
    }
    out
}

/// Every object holding user properties, as one [`ObjectDiscard`] each
/// (`bytes` zero; the session measures it), when `wanted` selects them and
/// the structure tree itself is not being discarded.
pub(crate) fn object_data_carriers(
    view: &DocumentView<'_>,
    live: &BTreeSet<ObjId>,
    catalog: ObjId,
    cat: &Dict,
    wanted: impl Fn(DiscardObject) -> bool,
) -> Vec<ObjectDiscard> {
    let Some(root) = cat.get(b"StructTreeRoot") else {
        return Vec::new();
    };
    if !wanted(DiscardObject::ObjectData) || wanted(DiscardObject::Tags) {
        return Vec::new();
    }
    let item = |owner: ObjId, location: String, strips, rewrites| ObjectDiscard {
        object: DiscardObject::ObjectData,
        owner,
        location,
        bytes: 0,
        strips,
        rewrites,
    };
    let mut out = Vec::new();
    let root_id = root.as_reference();
    let class_map = view
        .resolve(root)
        .as_dict()
        .and_then(|r| r.get(b"ClassMap"));
    if let (Some(root_id), Some(map)) = (root_id, class_map) {
        let (owner, inline) = match map.as_reference() {
            Some(r) => (r, false),
            None => (root_id, true),
        };
        if let Some(dict) = view.resolve(map).as_dict() {
            let rewrites = class_rewrites(view, owner, dict, inline);
            if !rewrites.is_empty() {
                out.push(item(owner, "class map".into(), Vec::new(), rewrites));
            }
        }
    }
    for &id in live {
        let Some(dict) = view.value(id).and_then(Object::as_dict) else {
            continue;
        };
        let rewrites = element_rewrites(view, id, dict);
        if !rewrites.is_empty() {
            let location = format!("structure element {} {}", id.num, id.generation);
            out.push(item(id, location, Vec::new(), rewrites));
        }
    }
    if let Some(strip) = flag_strip(view, catalog, cat) {
        out.push(item(catalog, "catalog".into(), vec![strip], Vec::new()));
    }
    out
}

/// A structure element's (or the root's) own `/A`, and its `/K` when a
/// child written inline in it carries user properties. A child that is its
/// own object is visited on its own.
fn element_rewrites(view: &DocumentView<'_>, id: ObjId, dict: &Dict) -> Vec<Rewrite> {
    let root = dict
        .get(b"Type")
        .and_then(Object::as_name)
        .is_some_and(|t| t.as_bytes() == b"StructTreeRoot");
    if dict.get(b"S").is_none() && !root {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (key, value) in [
        (
            b"A".as_slice(),
            dict.get(b"A").and_then(|a| without(view, a)),
        ),
        (
            b"K",
            dict.get(b"K")
                .and_then(|k| direct_kids(view, k, 0).map(Some)),
        ),
    ] {
        if let (Some(before), Some(value)) = (dict.get(key), value) {
            out.push(rewrite(id, vec![key.to_vec()], before, value));
        }
    }
    out
}

/// `/K` with its inline structure elements' user properties gone, or `None`
/// when none holds any.
fn direct_kids(view: &DocumentView<'_>, kids: &Object, depth: usize) -> Option<Object> {
    let Object::Array(items) = kids else {
        return direct_kid(view, kids, depth);
    };
    let mut changed = false;
    let new = items
        .iter()
        .map(|item| match direct_kid(view, item, depth) {
            Some(n) => {
                changed = true;
                n
            }
            None => item.clone(),
        })
        .collect();
    changed.then_some(Object::Array(new))
}

fn direct_kid(view: &DocumentView<'_>, kid: &Object, depth: usize) -> Option<Object> {
    let Object::Dict(d) = kid else {
        return None;
    };
    if depth > MAX_DEPTH || d.get(b"S").is_none() {
        return None;
    }
    let mut copy: Option<Dict> = None;
    if let Some(value) = d.get(b"A").and_then(|a| without(view, a)) {
        let c = copy.get_or_insert_with(|| d.clone());
        match value {
            Some(v) => c.insert(Name::from(b"A".as_slice()), v),
            None => {
                c.remove(b"A");
            }
        }
    }
    if let Some(k) = d.get(b"K").and_then(|k| direct_kids(view, k, depth + 1)) {
        copy.get_or_insert_with(|| d.clone())
            .insert(Name::from(b"K".as_slice()), k);
    }
    copy.map(Object::Dict)
}

/// The catalog's `MarkInfo /UserProperties` flag, inline or in its own
/// object.
fn flag_strip(view: &DocumentView<'_>, catalog: ObjId, cat: &Dict) -> Option<Strip> {
    let info = cat.get(b"MarkInfo")?;
    view.resolve(info).as_dict()?.get(b"UserProperties")?;
    Some(match info.as_reference() {
        Some(r) => Strip {
            owner: r,
            path: vec![b"UserProperties"],
        },
        None => Strip {
            owner: catalog,
            path: vec![b"MarkInfo", b"UserProperties"],
        },
    })
}
