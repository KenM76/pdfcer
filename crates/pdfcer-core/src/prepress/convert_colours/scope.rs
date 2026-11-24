//! The objects a [`super::ConvertScope`] covers.
//!
//! Soft masks (`/SMask`, `/Mask`) and thumbnails (`/Thumb`) are never
//! entered: a soft mask's colours are luminosity or alpha (ISO 32000-2
//! §11.6.5.2), not colours to print, and a thumbnail is a preview.

use std::collections::BTreeSet;

use crate::graph::ObjectGraph;
use crate::object::{ObjId, Object};
use crate::page_tree::Page;
use crate::view::DocumentView;

/// Keys never followed.
const CUT: [&[u8]; 3] = [b"SMask", b"Mask", b"Thumb"];
/// Keys a page-scoped walk never follows: back to the page or its tree.
const UP: [&[u8]; 3] = [b"P", b"Parent", b"Dest"];
/// Page-dictionary entries a page-scoped walk follows.
const PAGE_KEYS: [&[u8]; 4] = [b"Contents", b"Resources", b"Annots", b"Group"];
/// Nesting followed inside one object.
const MAX_NESTING: usize = 64;

/// What a scope reaches.
pub(crate) struct Reach {
    /// Every object in scope, ascending.
    pub(crate) ids: BTreeSet<ObjId>,
    /// References under a cut key that were not followed.
    pub(crate) cut: usize,
}

/// Everything reachable from the trailer's `/Root` and `/Info`.
pub(crate) fn whole(view: &DocumentView<'_>) -> Reach {
    let roots: Vec<ObjId> = [b"Root".as_slice(), b"Info"]
        .iter()
        .filter_map(|k| view.trailer_entry(k).and_then(Object::as_reference))
        .collect();
    let mut walk = Walk::new(view, false);
    walk.ids.extend(roots.iter().copied());
    walk.run(roots);
    walk.into_reach()
}

/// What `pages` reach: their content, resources (inherited ones
/// included), annotations and group, never another page.
pub(crate) fn pages(view: &DocumentView<'_>, pages: &[&Page]) -> Reach {
    let mut walk = Walk::new(view, true);
    let mut start = Vec::new();
    for page in pages {
        walk.ids.insert(page.id);
        if let Some(Object::Dict(d)) = view.graph().value(page.id) {
            for key in PAGE_KEYS {
                if let Some(v) = d.get(key) {
                    walk.refs(v, 0, &mut start);
                }
            }
        }
        for (_, v) in page.resources.iter() {
            walk.refs(v, 0, &mut start);
        }
        start.extend(page.contents.iter().copied());
    }
    walk.run(start);
    walk.into_reach()
}

struct Walk<'a, 'd> {
    view: &'a DocumentView<'d>,
    paged: bool,
    ids: BTreeSet<ObjId>,
    cut: usize,
}

impl<'a, 'd> Walk<'a, 'd> {
    fn new(view: &'a DocumentView<'d>, paged: bool) -> Self {
        Self {
            view,
            paged,
            ids: BTreeSet::new(),
            cut: 0,
        }
    }

    fn into_reach(self) -> Reach {
        Reach {
            ids: self.ids,
            cut: self.cut,
        }
    }

    fn run(&mut self, mut stack: Vec<ObjId>) {
        while let Some(id) = stack.pop() {
            if self.ids.len() >= crate::edit::MAX_REACHABLE_OBJECTS {
                return;
            }
            let Some(value) = self.view.graph().value(id) else {
                continue;
            };
            if self.paged && is_page_node(value) && !self.ids.contains(&id) {
                continue;
            }
            self.ids.insert(id);
            let mut next = Vec::new();
            self.refs(value, 0, &mut next);
            stack.extend(next.into_iter().filter(|r| !self.ids.contains(r)));
        }
    }

    /// The references inside `value`, skipping cut keys.
    fn refs(&mut self, value: &Object, depth: usize, out: &mut Vec<ObjId>) {
        if depth > MAX_NESTING {
            return;
        }
        match value {
            Object::Reference(r) => {
                if !self.paged || !self.view.graph().value(*r).is_some_and(is_page_node) {
                    out.push(*r);
                }
            }
            Object::Array(items) => {
                for item in items {
                    self.refs(item, depth + 1, out);
                }
            }
            Object::Dict(d) => self.entries(d, depth, out),
            Object::Stream(s) => self.entries(&s.dict, depth, out),
            _ => {}
        }
    }

    fn entries(&mut self, d: &crate::object::Dict, depth: usize, out: &mut Vec<ObjId>) {
        for (key, v) in d.iter() {
            let key = key.as_bytes();
            if CUT.contains(&key) {
                if matches!(v, Object::Reference(_) | Object::Dict(_)) {
                    self.cut += 1;
                }
                continue;
            }
            if self.paged && UP.contains(&key) {
                continue;
            }
            self.refs(v, depth + 1, out);
        }
    }
}

fn is_page_node(value: &Object) -> bool {
    value
        .as_dict()
        .and_then(|d| d.get(b"Type"))
        .and_then(Object::as_name)
        .is_some_and(|n| matches!(n.as_bytes(), b"Page" | b"Pages"))
}
