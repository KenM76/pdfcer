//! Structure destinations turned into page destinations before an optimize
//! run discards the structure tree (ISO 32000-2 §12.3.2.3), so no link,
//! bookmark or named destination is left naming a deleted element.
//!
//! A structure destination is an explicit destination whose first element
//! is a structure element rather than a page. It is rewritten to name the
//! page §12.3.2.3 resolves it to, operands unchanged: the kids are walked in
//! order, a marked-content or object reference gives its own `/Pg` or else
//! the element's, an integer MCID gives the element's `/Pg`, a structure
//! element kid is searched in turn, and with nothing found the document's
//! first page is used ("shall"). The element's own `/Pg` is consulted only
//! through its kids, as the clause words it.
//!
//! Rewritten: a bare structure destination in an outline item's or link's
//! `/Dest`, an array `/OpenAction`, a GoTo action's `/D` (written before
//! 2.0 erratum 140 made `/D` a page destination only) and a named
//! destination's value. The `/SD` of a GoTo action and of a named
//! destination dictionary goes, its `/D` kept, or built from the `/SD` when
//! missing. A GoToR or GoToE `/SD` names another file's tags and stays.

use std::collections::{BTreeMap, BTreeSet};

use super::actions::Rewrite;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::struct_page::{element_page, goto_like, is_struct_elem_dict};
use crate::view::DocumentView;

/// Guard for the name-tree walk (ARCHITECTURE §10).
const MAX_DEPTH: usize = 64;

/// What [`rewrites`] produced.
#[derive(Debug, Default)]
pub(crate) struct StructDests {
    pub(crate) rewrites: Vec<Rewrite>,
    /// Structure destinations rewritten to a page.
    pub(crate) rewritten: usize,
    /// Of those, the ones that found no page and took the first.
    pub(crate) to_first_page: usize,
}

/// The rewrites that keep every local destination working once the
/// structure tree is gone. `live` is every object reachable from the
/// trailer.
pub(crate) fn rewrites(
    view: &DocumentView<'_>,
    catalog: ObjId,
    cat: &Dict,
    live: &BTreeSet<ObjId>,
) -> StructDests {
    let first = crate::page_tree::pages_in(view)
        .ok()
        .and_then(|p| p.first().map(|p| p.id));
    let mut cx = Cx {
        view,
        first,
        memo: BTreeMap::new(),
        out: StructDests::default(),
    };
    let named = cx.named_owners(catalog, cat);
    for &id in live {
        if named.contains(&id) {
            continue;
        }
        if let Some(Object::Dict(d)) = view.value(id) {
            cx.object(id, d);
        }
    }
    cx.out
}

struct Cx<'v, 'a> {
    view: &'v DocumentView<'a>,
    first: Option<ObjId>,
    memo: BTreeMap<ObjId, Option<ObjId>>,
    out: StructDests,
}

impl Cx<'_, '_> {
    fn push(&mut self, owner: ObjId, path: Vec<Vec<u8>>, before: Object, value: Object) {
        self.out.rewrites.push(Rewrite {
            owner,
            path,
            before,
            value: Some(value),
        });
    }

    /// The destination-bearing entries of one indirect dictionary.
    fn object(&mut self, id: ObjId, d: &Dict) {
        let goto = goto_like(d);
        for (key, value) in d.iter() {
            let new = match key.as_bytes() {
                b"Dest" => self.dest(value),
                b"A" | b"Next" => self.inline_actions(value),
                b"OpenAction" => self.dest(value).or_else(|| self.inline_actions(value)),
                b"AA" => self.triggers(value),
                b"D" if goto => self.dest(value),
                _ => None,
            };
            if let Some(new) = new {
                self.push(id, vec![key.as_bytes().to_vec()], value.clone(), new);
            }
        }
        if goto
            && d.get(b"D").is_none()
            && let Some(new) = d.get(b"SD").and_then(|sd| self.dest(sd))
        {
            self.push(id, vec![b"D".to_vec()], Object::Null, new);
        }
    }

    /// An explicit destination value as a page destination, when it names
    /// a structure element. A reference to such an array is replaced by
    /// the rewritten array written inline.
    fn dest(&mut self, value: &Object) -> Option<Object> {
        let arr = match value {
            Object::Array(a) => a.as_slice(),
            Object::Reference(r) => self.view.value(*r)?.as_array()?,
            _ => return None,
        };
        let elem = arr.first()?.as_reference()?;
        if !is_struct_elem(self.view.resolve(&Object::Reference(elem))) {
            return None;
        }
        let page = match self.page_of(elem) {
            Some(p) => p,
            None => {
                let first = self.first?;
                self.out.to_first_page += 1;
                first
            }
        };
        self.out.rewritten += 1;
        let mut new = vec![Object::Reference(page)];
        new.extend(arr.iter().skip(1).cloned());
        Some(Object::Array(new))
    }

    /// Inline actions (one dictionary, or an array of them as `/Next`
    /// allows); indirect ones are live objects of their own.
    fn inline_actions(&mut self, value: &Object) -> Option<Object> {
        match value {
            Object::Dict(d) => self.action(d).map(Object::Dict),
            Object::Array(items) => {
                let mut changed = false;
                let new = items
                    .iter()
                    .map(|i| match i {
                        Object::Dict(d) => self.action(d).map_or_else(
                            || i.clone(),
                            |n| {
                                changed = true;
                                Object::Dict(n)
                            },
                        ),
                        _ => i.clone(),
                    })
                    .collect();
                changed.then_some(Object::Array(new))
            }
            _ => None,
        }
    }

    /// An inline additional-actions dictionary.
    fn triggers(&mut self, value: &Object) -> Option<Object> {
        let Object::Dict(aa) = value else {
            return None;
        };
        let mut new = aa.clone();
        let mut changed = false;
        for (trigger, action) in aa.iter() {
            if let Some(a) = self.inline_actions(action) {
                new.insert(trigger.clone(), a);
                changed = true;
            }
        }
        changed.then_some(Object::Dict(new))
    }

    /// An inline action or named-destination dictionary with its local
    /// structure references resolved, `/SD` dropped, and its chain done.
    fn action(&mut self, d: &Dict) -> Option<Dict> {
        let mut new = d.clone();
        let mut changed = false;
        if goto_like(d) {
            let d_new = match d.get(b"D") {
                Some(dv) => self.dest(dv),
                None => d.get(b"SD").and_then(|sd| self.dest(sd)),
            };
            if let Some(v) = d_new {
                new.insert(Name::from(b"D".as_slice()), v);
                changed = true;
            }
            if new.remove(b"SD").is_some() {
                changed = true;
            }
        }
        if let Some(next) = d.get(b"Next").and_then(|n| self.inline_actions(n)) {
            new.insert(Name::from(b"Next".as_slice()), next);
            changed = true;
        }
        changed.then_some(new)
    }

    /// A named destination's value: an explicit destination or a
    /// dictionary with `/D` (and `/SD`).
    fn named_value(&mut self, value: &Object) -> Option<Object> {
        match value {
            Object::Dict(d) => self.action(d).map(Object::Dict),
            _ => self.dest(value),
        }
    }

    /// Rewrite the catalog's `/Dests` and the `/Dests` name tree, and
    /// return the objects holding them, which the generic pass must skip
    /// (a destination may be named `Dest` or `A`).
    fn named_owners(&mut self, catalog: ObjId, cat: &Dict) -> BTreeSet<ObjId> {
        let mut owners = BTreeSet::new();
        match cat.get(b"Dests") {
            Some(Object::Reference(r)) => {
                owners.insert(*r);
                if let Some(Object::Dict(d)) = self.view.value(*r) {
                    for (name, value) in d.iter() {
                        if let Some(new) = self.named_value(value) {
                            self.push(*r, vec![name.as_bytes().to_vec()], value.clone(), new);
                        }
                    }
                }
            }
            Some(v @ Object::Dict(d)) => {
                if let Some(new) = self.named_map(d) {
                    self.push(catalog, vec![b"Dests".to_vec()], v.clone(), new);
                }
            }
            _ => {}
        }
        self.name_tree(catalog, cat, &mut owners);
        owners
    }

    fn named_map(&mut self, d: &Dict) -> Option<Object> {
        let mut new = d.clone();
        let mut changed = false;
        for (name, value) in d.iter() {
            if let Some(v) = self.named_value(value) {
                new.insert(name.clone(), v);
                changed = true;
            }
        }
        changed.then_some(Object::Dict(new))
    }

    /// The `/Names /Dests` tree (§7.9.6): its root may be inline in the
    /// names dictionary, every other node is indirect.
    fn name_tree(&mut self, catalog: ObjId, cat: &Dict, owners: &mut BTreeSet<ObjId>) {
        let (holder, prefix): (ObjId, Vec<Vec<u8>>) = match cat.get(b"Names") {
            Some(Object::Reference(r)) => (*r, Vec::new()),
            Some(Object::Dict(_)) => (catalog, vec![b"Names".to_vec()]),
            _ => return,
        };
        let names = self
            .view
            .resolve(cat.get(b"Names").unwrap_or(&Object::Null));
        let Some(root) = names.as_dict().and_then(|n| n.get(b"Dests")) else {
            return;
        };
        let mut todo = Vec::new();
        match root {
            Object::Reference(r) => todo.push((*r, 0)),
            Object::Dict(node) => {
                if let Some(new) = self.leaf(node) {
                    let mut root_new = node.clone();
                    root_new.insert(Name::from(b"Names".as_slice()), new);
                    let mut path = prefix;
                    path.push(b"Dests".to_vec());
                    self.push(holder, path, root.clone(), Object::Dict(root_new));
                }
                todo.extend(kids(node).into_iter().map(|k| (k, 1)));
            }
            _ => return,
        }
        while let Some((id, depth)) = todo.pop() {
            if depth > MAX_DEPTH || !owners.insert(id) {
                continue;
            }
            let Some(Object::Dict(node)) = self.view.value(id) else {
                continue;
            };
            if let (Some(new), Some(old)) = (self.leaf(node), node.get(b"Names")) {
                self.push(id, vec![b"Names".to_vec()], old.clone(), new);
            }
            todo.extend(kids(node).into_iter().map(|k| (k, depth + 1)));
        }
    }

    /// A name-tree node's `/Names` array with its values rewritten.
    fn leaf(&mut self, node: &Dict) -> Option<Object> {
        let pairs = node.get(b"Names")?.as_array()?;
        let mut new = pairs.to_vec();
        let mut changed = false;
        for v in new.iter_mut().skip(1).step_by(2) {
            if let Some(n) = self.named_value(v) {
                *v = n;
                changed = true;
            }
        }
        changed.then_some(Object::Array(new))
    }

    /// The page §12.3.2.3 gives a structure element, `None` when no kid
    /// names one.
    fn page_of(&mut self, elem: ObjId) -> Option<ObjId> {
        if let Some(&p) = self.memo.get(&elem) {
            return p;
        }
        let page = element_page(self.view, elem);
        self.memo.insert(elem, page);
        page
    }
}

fn is_struct_elem(o: &Object) -> bool {
    o.as_dict().is_some_and(is_struct_elem_dict)
}

fn kids(node: &Dict) -> Vec<ObjId> {
    node.get(b"Kids")
        .and_then(Object::as_array)
        .map(|k| k.iter().filter_map(Object::as_reference).collect())
        .unwrap_or_default()
}
