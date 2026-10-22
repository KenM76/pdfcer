//! The page tree in document order, with inheritable attributes pushed down.

use std::collections::BTreeSet;

use super::Graph;
use crate::object::{Dict, Name, ObjId, Object};

/// The attributes §7.7.3.4 lets a page inherit from its ancestors.
const INHERITABLE: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];

/// Deeper than any real page tree; a guard against a hostile one.
const MAX_DEPTH: usize = 256;

/// The leaf pages under the catalog's `/Pages`, in order. Every page gets
/// each inheritable attribute it lacks from its nearest ancestor, and every
/// intermediate node loses them, so no page depends on inheritance (F.3.7).
/// A node seen twice, or deeper than [`MAX_DEPTH`], is skipped.
pub(super) fn push_down(graph: &mut Graph<'_>, root: ObjId) -> Vec<ObjId> {
    let Some(top) = graph
        .dict(root)
        .and_then(|c| graph.referent(c.get(b"Pages")))
    else {
        return Vec::new();
    };
    let mut walk = Walk {
        pages: Vec::new(),
        seen: BTreeSet::new(),
    };
    walk.node(graph, top, &Dict::new(), 0);
    walk.pages
}

struct Walk {
    pages: Vec<ObjId>,
    seen: BTreeSet<ObjId>,
}

impl Walk {
    fn node(&mut self, graph: &mut Graph<'_>, id: ObjId, inherited: &Dict, depth: usize) {
        if depth > MAX_DEPTH || !self.seen.insert(id) {
            return;
        }
        let Some(dict) = graph.dict(id).cloned() else {
            return;
        };
        let is_page = dict
            .get(b"Type")
            .and_then(Object::as_name)
            .map(Name::as_bytes)
            == Some(b"Page".as_slice())
            || !dict.contains_key(b"Kids");
        if is_page {
            self.leaf(graph, id, dict, inherited);
            return;
        }
        let mut carried = inherited.clone();
        let mut node = dict;
        for key in INHERITABLE {
            if let Some(value) = node.remove(key) {
                carried.insert(Name::from(key), value);
            }
        }
        let kids: Vec<ObjId> = node
            .get(b"Kids")
            .and_then(Object::as_array)
            .unwrap_or_default()
            .iter()
            .filter_map(|k| graph.referent(Some(k)))
            .collect();
        set(graph, id, node);
        for kid in kids {
            self.node(graph, kid, &carried, depth + 1);
        }
    }

    fn leaf(&mut self, graph: &mut Graph<'_>, id: ObjId, mut page: Dict, inherited: &Dict) {
        let mut changed = false;
        for (key, value) in inherited.iter() {
            if !page.contains_key(key.as_bytes()) {
                page.insert(key.clone(), value.clone());
                changed = true;
            }
        }
        if !page.contains_key(b"Type") {
            page.insert(Name::from(b"Type"), Object::Name(Name::from(b"Page")));
            changed = true;
        }
        if changed {
            set(graph, id, page);
        }
        self.pages.push(id);
    }
}

/// Replace `id`'s dictionary, keeping it a stream if it was one.
fn set(graph: &mut Graph<'_>, id: ObjId, dict: Dict) {
    let value = match graph.value(id) {
        Some(Object::Stream(s)) => {
            let mut s = s.clone();
            s.dict = dict;
            Object::Stream(s)
        }
        _ => Object::Dict(dict),
    };
    graph.overrides.insert(id, value);
}
