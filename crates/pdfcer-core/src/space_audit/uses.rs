//! Page uses: which pages reach each object.
//!
//! A page reaches everything under its own dictionary and its inherited
//! resources (ISO 32000-2 §7.7.3.4), through any chain of references, but
//! never through `/Parent`, `/P` or another page-tree node. A link to
//! another page does not make that page's content "used" here.

use std::collections::{BTreeMap, BTreeSet};

use super::{dict_of, name_of};
use crate::document::Document;
use crate::object::{ObjId, Object};
use crate::page_tree;

/// Direct nesting depth past which a value is not scanned.
const MAX_DIRECT_DEPTH: usize = 64;

/// Every object's using pages, 1-based and ascending.
pub(super) fn page_uses(doc: &Document) -> BTreeMap<ObjId, Vec<usize>> {
    let mut uses: BTreeMap<ObjId, Vec<usize>> = BTreeMap::new();
    let Ok(pages) = page_tree::pages(doc) else {
        return uses;
    };
    for (i, page) in pages.iter().enumerate() {
        let mut seen = BTreeSet::from([page.id]);
        let mut pending = Vec::new();
        if let Some(obj) = doc.get(page.id) {
            push_refs(&obj.value, &mut pending, 0);
        }
        for value in page.resources.iter().map(|(_, v)| v) {
            push_refs(value, &mut pending, 0);
        }
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }
            let Some(obj) = doc.get(id) else {
                continue;
            };
            if is_tree_node(&obj.value) {
                continue;
            }
            push_refs(&obj.value, &mut pending, 0);
        }
        for id in seen {
            uses.entry(id).or_default().push(i + 1);
        }
    }
    uses
}

fn is_tree_node(value: &Object) -> bool {
    dict_of(value).is_some_and(|d| {
        matches!(
            name_of(d, b"Type").as_deref(),
            Some("Page" | "Pages" | "Catalog")
        )
    })
}

fn push_refs(value: &Object, out: &mut Vec<ObjId>, depth: usize) {
    if depth > MAX_DIRECT_DEPTH {
        return;
    }
    match value {
        Object::Reference(id) => out.push(*id),
        Object::Array(items) => {
            for item in items {
                push_refs(item, out, depth + 1);
            }
        }
        Object::Dict(_) | Object::Stream(_) => {
            if let Some(dict) = dict_of(value) {
                for (key, child) in dict.iter() {
                    if !matches!(key.as_bytes(), b"Parent" | b"P") {
                        push_refs(child, out, depth + 1);
                    }
                }
            }
        }
        _ => {}
    }
}
