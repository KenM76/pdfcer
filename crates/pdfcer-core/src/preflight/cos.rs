//! Object-level reads shared by the conformance validators (PDF/X, PDF/A):
//! the catalog, names, decoded stream data and a walk over every
//! dictionary inside every live object.

use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object, Stream};
use crate::prepress::live_ids;
use crate::view::DocumentView;

/// Nesting followed inside one object.
const MAX_DEPTH: usize = 24;

/// The document catalog, if the trailer's `/Root` resolves to a dictionary.
pub(super) fn catalog<'v>(view: &'v DocumentView<'_>) -> Option<&'v Dict> {
    view.trailer_entry(b"Root")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)
}

/// `dict[key]` as a name, following one indirect reference.
pub(super) fn name<'v>(view: &'v DocumentView<'_>, dict: &'v Dict, key: &[u8]) -> Option<&'v [u8]> {
    dict.get(key)
        .map(|o| view.resolve(o))
        .and_then(Object::as_name)
        .map(|n| n.as_bytes())
}

/// A stream's decoded data, within the filter layer's output ceiling;
/// `None` when it cannot be read.
pub(super) fn decoded(view: &DocumentView<'_>, stream: &Stream) -> Option<Vec<u8>> {
    view.slice(stream.data_span)
        .and_then(|raw| crate::filters::decode_stream(&stream.dict, raw).ok())
}

/// Every dictionary inside every live object, with the object's id and
/// whether the dictionary is a stream's. Nesting is followed to a fixed
/// depth; references are not followed (each live object is visited once
/// on its own).
pub(super) fn each_dict(view: &DocumentView<'_>, f: &mut dyn FnMut(ObjId, &Dict, bool)) {
    for id in live_ids(view) {
        if let Some(value) = view.graph().value(id) {
            walk(id, value, 0, f);
        }
    }
}

fn walk(id: ObjId, obj: &Object, depth: usize, f: &mut dyn FnMut(ObjId, &Dict, bool)) {
    if depth > MAX_DEPTH {
        return;
    }
    let (dict, stream) = match obj {
        Object::Dict(d) => (d, false),
        Object::Stream(s) => (&s.dict, true),
        Object::Array(a) => {
            a.iter().for_each(|o| walk(id, o, depth + 1, f));
            return;
        }
        _ => return,
    };
    f(id, dict, stream);
    dict.iter().for_each(|(_, v)| walk(id, v, depth + 1, f));
}
