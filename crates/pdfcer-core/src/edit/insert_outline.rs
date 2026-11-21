//! The bookmarks (ISO 32000-1 §12.3.3) and named destinations (§12.3.2.3)
//! an in-session insert carries for the pages it copied.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use super::{EditError, EditSession, encode_text_string, unique_dest_key};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::pageops::references::{DestinationResolver, MAX_OUTLINE_ITEMS};
use crate::pageops::{InsertOptions, InsertedOutline};
use crate::view::DocumentView;

/// Deepest source outline level read (`ARCHITECTURE.md` §10).
const MAX_DEPTH: usize = 32;

/// What the carry added, for `InsertOutcome`.
#[derive(Debug, Default)]
pub(super) struct OutlineCarry {
    pub(super) items: usize,
    pub(super) dests: usize,
    pub(super) dests_renamed: usize,
    /// Source bookmarks among [`Self::items`] (the heading excluded).
    pub(super) from_source: usize,
}

/// One source bookmark: its title, style, the destination value it used and
/// the source page that resolves to.
struct Entry {
    title: Object,
    style: Vec<(Name, Object)>,
    dest: Option<Object>,
    page: Option<ObjId>,
    children: Vec<Entry>,
}

/// The source pages that were inserted, the final key of every carried named
/// destination, and the source's destination resolver.
struct Carry<'a, 'b> {
    inserted: HashSet<ObjId>,
    keys: BTreeMap<Vec<u8>, Vec<u8>>,
    resolver: DestinationResolver,
    source: &'a DocumentView<'b>,
}

impl EditSession {
    /// Carry what `opts` asks for: named destinations naming an inserted
    /// page (and the links on the new pages that use them), then bookmarks
    /// aimed at one. Writes into `scratch`.
    pub(super) fn carry_inserted_outline(
        &mut self,
        source: &DocumentView<'_>,
        opts: &InsertOptions,
        new_page_ids: &[ObjId],
        mapping: &mut BTreeMap<ObjId, ObjId>,
        scratch: &mut BTreeMap<ObjId, Object>,
    ) -> Result<OutlineCarry, EditError> {
        let mut out = OutlineCarry::default();
        if !opts.named_destinations && opts.outline == InsertedOutline::Drop {
            return Ok(out);
        }
        let new: HashSet<ObjId> = new_page_ids.iter().copied().collect();
        let mut carry = Carry {
            inserted: mapping
                .iter()
                .filter(|(_, n)| new.contains(n))
                .map(|(s, _)| *s)
                .collect(),
            keys: BTreeMap::new(),
            resolver: DestinationResolver::new(source.graph()),
            source,
        };
        if opts.named_destinations {
            (out.dests, out.dests_renamed) = self.carry_dests(&mut carry, mapping, scratch)?;
            rewrite_links(new_page_ids, &carry.keys, scratch);
        }
        let Some(first_page) = new_page_ids.first().copied() else {
            return Ok(out);
        };
        if opts.outline == InsertedOutline::Drop {
            return Ok(out);
        }
        let entries = prune(read_outline(&carry), &carry.inserted);
        let top = self.emit(&entries, ObjId::new(0, 0), &carry, mapping, scratch)?;
        out.from_source = count(&entries);
        out.items = out.from_source;
        if let InsertedOutline::UnderParent(title) = &opts.outline {
            let parent = self.heading(title, first_page, &top, scratch)?;
            self.append_outline_items(&[parent], 1, scratch)?;
            out.items += 1;
        } else {
            self.append_outline_items(&top, out.items, scratch)?;
        }
        Ok(out)
    }

    /// Copy the source's named destinations that resolve to an inserted
    /// page, suffixing a key the target defines; `(carried, renamed)`.
    fn carry_dests(
        &mut self,
        carry: &mut Carry<'_, '_>,
        mapping: &mut BTreeMap<ObjId, ObjId>,
        scratch: &mut BTreeMap<ObjId, Object>,
    ) -> Result<(usize, usize), EditError> {
        let graph = carry.source.graph();
        let mut names: Vec<(Vec<u8>, Object)> = carry
            .resolver
            .iter()
            .filter(|(_, v)| {
                carry
                    .resolver
                    .resolve_destination(graph, v)
                    .is_some_and(|p| carry.inserted.contains(&p))
            })
            .map(|(k, v)| (k.to_vec(), v.clone()))
            .collect();
        names.sort_by(|a, b| a.0.cmp(&b.0));
        let mut taken: BTreeSet<Vec<u8>> = DestinationResolver::new(&self.graph())
            .iter()
            .map(|(k, _)| k.to_vec())
            .collect();
        let mut entries = Vec::with_capacity(names.len());
        let mut renamed = 0;
        for (key, value) in names {
            let value = self.import_value(carry.source, &value, mapping, scratch)?;
            let final_key = if taken.contains(&key) {
                renamed += 1;
                unique_dest_key(&key, &taken)
            } else {
                key.clone()
            };
            taken.insert(final_key.clone());
            carry.keys.insert(key, final_key.clone());
            entries.push((final_key, value));
        }
        let carried = entries.len();
        self.append_named_destinations(entries, scratch)?;
        Ok((carried, renamed))
    }

    /// Write one sibling chain under `parent`, every item closed; returns
    /// the new ids in order.
    fn emit(
        &mut self,
        entries: &[Entry],
        parent: ObjId,
        carry: &Carry<'_, '_>,
        mapping: &mut BTreeMap<ObjId, ObjId>,
        scratch: &mut BTreeMap<ObjId, Object>,
    ) -> Result<Vec<ObjId>, EditError> {
        let mut ids = Vec::with_capacity(entries.len());
        for _ in entries {
            ids.push(ObjId::new(self.alloc_number()?, 0));
        }
        for (i, (entry, id)) in entries.iter().zip(&ids).enumerate() {
            let mut d = Dict::new();
            d.insert(Name::from(b"Title"), entry.title.clone());
            d.insert(Name::from(b"Parent"), Object::Reference(parent));
            for (k, v) in &entry.style {
                d.insert(k.clone(), v.clone());
            }
            if let Some(prev) = i.checked_sub(1).and_then(|p| ids.get(p)) {
                d.insert(Name::from(b"Prev"), Object::Reference(*prev));
            }
            if let Some(next) = ids.get(i + 1) {
                d.insert(Name::from(b"Next"), Object::Reference(*next));
            }
            if let Some(dest) = self.destination(entry, carry, mapping, scratch)? {
                d.insert(Name::from(b"Dest"), dest);
            }
            let kids = self.emit(&entry.children, *id, carry, mapping, scratch)?;
            link_children(&mut d, &kids);
            scratch.insert(*id, Object::Dict(d));
        }
        Ok(ids)
    }

    /// The carried item's `/Dest`: its key when that named destination was
    /// carried, else the explicit array it resolves to with the page
    /// rebound, else `[page /Fit]`.
    fn destination(
        &mut self,
        entry: &Entry,
        carry: &Carry<'_, '_>,
        mapping: &mut BTreeMap<ObjId, ObjId>,
        scratch: &mut BTreeMap<ObjId, Object>,
    ) -> Result<Option<Object>, EditError> {
        let (Some(page), Some(dest)) = (entry.page, entry.dest.as_ref()) else {
            return Ok(None);
        };
        if let Some(key) = dest_key(dest).and_then(|k| carry.keys.get(k)) {
            return Ok(Some(Object::String(key.clone())));
        }
        if let Some(array) = explicit_array(carry, dest)
            && array.first().and_then(Object::as_reference) == Some(page)
        {
            return self
                .import_value(carry.source, &Object::Array(array), mapping, scratch)
                .map(Some);
        }
        let Some(new_page) = mapping.get(&page) else {
            return Ok(None);
        };
        Ok(Some(Object::Array(vec![
            Object::Reference(*new_page),
            Object::Name(Name::from(b"Fit")),
        ])))
    }

    /// The `UnderParent` heading: `title`, aimed at `first_page` with
    /// `/Fit`, closed over `kids`.
    fn heading(
        &mut self,
        title: &str,
        first_page: ObjId,
        kids: &[ObjId],
        scratch: &mut BTreeMap<ObjId, Object>,
    ) -> Result<ObjId, EditError> {
        let id = ObjId::new(self.alloc_number()?, 0);
        let mut d = Dict::new();
        d.insert(
            Name::from(b"Title"),
            Object::String(encode_text_string(title)),
        );
        d.insert(
            Name::from(b"Dest"),
            Object::Array(vec![
                Object::Reference(first_page),
                Object::Name(Name::from(b"Fit")),
            ]),
        );
        link_children(&mut d, kids);
        for kid in kids {
            if let Some(Object::Dict(k)) = scratch.get_mut(kid) {
                k.insert(Name::from(b"Parent"), Object::Reference(id));
            }
        }
        scratch.insert(id, Object::Dict(d));
        Ok(id)
    }
}

/// `/First`, `/Last` and a closed `/Count` (§12.3.3, Table 153) for `kids`.
fn link_children(d: &mut Dict, kids: &[ObjId]) {
    if let (Some(first), Some(last)) = (kids.first(), kids.last()) {
        d.insert(Name::from(b"First"), Object::Reference(*first));
        d.insert(Name::from(b"Last"), Object::Reference(*last));
        d.insert(
            Name::from(b"Count"),
            Object::Integer(-i64::try_from(kids.len()).unwrap_or(i64::MAX)),
        );
    }
}

/// The key a destination value names (§12.3.2.3): a name or a string.
fn dest_key(dest: &Object) -> Option<&[u8]> {
    match dest {
        Object::Name(n) => Some(n.as_bytes()),
        Object::String(s) => Some(s),
        _ => None,
    }
}

/// The explicit destination array `dest` stands for, through a named
/// destination and a `/D` dictionary when it is one.
fn explicit_array(carry: &Carry<'_, '_>, dest: &Object) -> Option<Vec<Object>> {
    let graph = carry.source.graph();
    let mut current = graph.resolve(dest).clone();
    for _ in 0..8 {
        current = match current {
            Object::Array(items) => return Some(items),
            Object::Dict(d) => graph.resolve(d.get(b"D")?).clone(),
            other => graph
                .resolve(carry.resolver.lookup(dest_key(&other)?)?)
                .clone(),
        };
    }
    None
}

/// The source's outline, depth- and size-capped.
fn read_outline(carry: &Carry<'_, '_>) -> Vec<Entry> {
    let graph = carry.source.graph();
    let first = graph
        .catalog_dict()
        .and_then(|c| c.get(b"Outlines"))
        .map(|o| graph.resolve(o))
        .and_then(Object::as_dict)
        .and_then(|r| r.get(b"First"))
        .and_then(Object::as_reference);
    let mut budget = MAX_OUTLINE_ITEMS;
    read_siblings(carry, first, 0, &mut budget, &mut HashSet::new())
}

/// One sibling chain and everything under it: iterative across siblings so a
/// long flat outline cannot exhaust the stack.
fn read_siblings(
    carry: &Carry<'_, '_>,
    first: Option<ObjId>,
    depth: usize,
    budget: &mut usize,
    visited: &mut HashSet<ObjId>,
) -> Vec<Entry> {
    let graph = carry.source.graph();
    let mut out = Vec::new();
    let mut current = first.filter(|_| depth <= MAX_DEPTH);
    while let Some(id) = current {
        if *budget == 0 || !visited.insert(id) {
            break;
        }
        *budget -= 1;
        let Some(d) = graph.resolved(id).as_dict() else {
            break;
        };
        out.push(Entry {
            title: d
                .get(b"Title")
                .map_or_else(|| Object::String(Vec::new()), |o| graph.resolve(o).clone()),
            style: [&b"C"[..], b"F"]
                .iter()
                .filter_map(|k| Some((Name::from(*k), graph.resolve(d.get(k)?).clone())))
                .collect(),
            dest: goto_dest(graph, d),
            page: carry.resolver.resolve_target(graph, d),
            children: read_siblings(
                carry,
                d.get(b"First").and_then(Object::as_reference),
                depth + 1,
                budget,
                visited,
            ),
        });
        current = d.get(b"Next").and_then(Object::as_reference);
    }
    out
}

/// An item's destination value: `/Dest`, or a `/GoTo` action's `/D`
/// (§12.6.4.2).
fn goto_dest<G: ObjectGraph + ?Sized>(graph: &G, d: &Dict) -> Option<Object> {
    if let Some(dest) = d.get(b"Dest") {
        return Some(graph.resolve(dest).clone());
    }
    let action = graph.resolve(d.get(b"A")?).as_dict()?;
    let goto = action
        .get(b"S")
        .and_then(Object::as_name)
        .is_some_and(|s| s.as_bytes() == b"GoTo");
    if !goto {
        return None;
    }
    action.get(b"D").map(|o| graph.resolve(o).clone())
}

/// The entries aimed at an inserted page, plus the containers holding one; a
/// kept container whose own page was not inserted loses its destination.
fn prune(entries: Vec<Entry>, inserted: &HashSet<ObjId>) -> Vec<Entry> {
    entries
        .into_iter()
        .filter_map(|mut e| {
            e.children = prune(std::mem::take(&mut e.children), inserted);
            let aimed = e.page.is_some_and(|p| inserted.contains(&p));
            if !aimed {
                e.page = None;
            }
            (aimed || !e.children.is_empty()).then_some(e)
        })
        .collect()
}

/// Items in a pruned tree.
fn count(entries: &[Entry]) -> usize {
    entries.iter().map(|e| 1 + count(&e.children)).sum()
}

/// Point every annotation on the new pages that named a carried destination
/// at its final key, as a `/Names`-tree string (§12.3.2.3).
fn rewrite_links(
    new_page_ids: &[ObjId],
    keys: &BTreeMap<Vec<u8>, Vec<u8>>,
    scratch: &mut BTreeMap<ObjId, Object>,
) {
    if keys.is_empty() {
        return;
    }
    let mut annots: Vec<ObjId> = Vec::new();
    for page in new_page_ids {
        let list = match scratch
            .get(page)
            .and_then(Object::as_dict)
            .and_then(|d| d.get(b"Annots"))
        {
            Some(Object::Array(a)) => a.clone(),
            Some(Object::Reference(r)) => scratch
                .get(r)
                .and_then(Object::as_array)
                .map(<[Object]>::to_vec)
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        annots.extend(list.iter().filter_map(Object::as_reference));
    }
    for id in annots {
        let Some(Object::Dict(mut d)) = scratch.get(&id).cloned() else {
            continue;
        };
        rekey(&mut d, b"Dest", keys);
        match d.get(b"A").cloned() {
            Some(Object::Dict(mut a)) => {
                rekey(&mut a, b"D", keys);
                d.insert(Name::from(b"A"), Object::Dict(a));
            }
            Some(Object::Reference(r)) => {
                if let Some(Object::Dict(mut a)) = scratch.get(&r).cloned() {
                    rekey(&mut a, b"D", keys);
                    scratch.insert(r, Object::Dict(a));
                }
            }
            _ => {}
        }
        scratch.insert(id, Object::Dict(d));
    }
}

/// Replace `d[key]` with its carried key when it names a carried
/// destination.
fn rekey(d: &mut Dict, key: &[u8], keys: &BTreeMap<Vec<u8>, Vec<u8>>) {
    let new = d
        .get(key)
        .and_then(dest_key)
        .and_then(|k| keys.get(k))
        .cloned();
    if let Some(new) = new {
        d.insert(Name::from(key), Object::String(new));
    }
}

/// Whether `source`'s catalog has the entry `key`.
pub(super) fn catalog_has(source: &DocumentView<'_>, key: &[u8]) -> bool {
    source
        .graph()
        .catalog_dict()
        .is_some_and(|c| c.contains_key(key))
}
