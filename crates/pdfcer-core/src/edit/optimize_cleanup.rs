//! The navigation clean-up of an optimize run: planning over the session
//! and committing through the existing delete verbs, folded into the run's
//! one undo entry. Definitions are in [`crate::optimize::cleanup`].

use std::collections::{BTreeMap, HashMap, HashSet};

use super::{Command, CommandKind, EditError, EditSession, ObjectWrite};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::optimize::OptimizeError;
use crate::optimize::cleanup::{
    BrokenBookmark, BrokenLink, BrokenTarget, CleanupOptions, CleanupPlan,
};
use crate::outline::{Destination, DestinationReader, OutlineItem, read_outline};
use crate::pageops::references::{DestinationResolver, name_tree_entries};

impl EditSession {
    /// What `options` would remove, with disclosures for what was not
    /// checked appended to `notes`.
    pub(super) fn plan_cleanup(
        &self,
        options: &CleanupOptions,
        notes: &mut Vec<String>,
    ) -> CleanupPlan {
        let mut plan = CleanupPlan::default();
        if options.is_none() {
            return plan;
        }
        if crate::signature::census(self).forbids_structural_change() {
            notes
                .push("a certification signature forbids deletions; nothing was cleaned up".into());
            return plan;
        }
        let graph = self.graph();
        if options.invalid_links || options.invalid_bookmarks {
            let reader = DestinationReader::new(&graph);
            if reader.page_tree_error().is_some() {
                notes.push(
                    "the page tree cannot be walked; links and bookmarks were not checked".into(),
                );
            } else {
                if options.invalid_links {
                    plan.links = broken_links(&graph, &reader);
                }
                if options.invalid_bookmarks {
                    plan.bookmarks = broken_bookmarks(&read_outline(&graph).items);
                }
            }
        }
        if options.unreferenced_destinations {
            plan.destinations = self.unreferenced_destinations(notes);
            if !plan.destinations.is_empty() {
                notes.push(format!(
                    "{} named destination(s) removed; a link from another file to one of them \
                     no longer resolves",
                    plan.destinations.len()
                ));
            }
        }
        plan
    }

    /// Defined named destinations that no `/Dest` or `/D` names, directly or
    /// through another named destination. Empty, with a note, when the file
    /// holds JavaScript or its destination tree could not be read whole.
    fn unreferenced_destinations(&self, notes: &mut Vec<String>) -> Vec<Vec<u8>> {
        let graph = self.graph();
        let defined = DestinationResolver::new(&graph);
        if defined.named_count() == 0 {
            return Vec::new();
        }
        if let Some(tree) = dests_tree(&graph)
            && name_tree_entries(&graph, tree).is_none()
        {
            notes.push("the named-destination tree is too deep to read whole; kept".into());
            return Vec::new();
        }
        let mut named = HashSet::new();
        let mut javascript = false;
        for id in self.live_object_ids() {
            if let Some(value) = self.value(id) {
                scan_names(value, &mut named, &mut javascript);
            }
        }
        if javascript {
            notes.push(
                "the file holds JavaScript, which can name a destination; named destinations \
                 were kept"
                    .into(),
            );
            return Vec::new();
        }
        // A named destination whose value is another name keeps that one.
        let mut todo: Vec<Vec<u8>> = named.iter().cloned().collect();
        while let Some(key) = todo.pop() {
            if let Some(next) = defined
                .lookup(&key)
                .and_then(|v| name_bytes(graph.resolve(v)))
                && named.insert(next.clone())
            {
                todo.push(next);
            }
        }
        let mut out: Vec<Vec<u8>> = defined
            .iter()
            .map(|(k, _)| k.to_vec())
            .filter(|k| !named.contains(k))
            .collect();
        out.sort();
        out
    }

    /// Commit `plan`'s removals, folding each verb's entry into one undo
    /// entry. Returns the objects removed or rewritten. On a failure every
    /// clean-up already committed is undone.
    pub(super) fn apply_cleanup(
        &mut self,
        plan: &CleanupPlan,
        fold: &mut Fold,
    ) -> Result<usize, OptimizeError> {
        let mut count = 0;
        for link in &plan.links {
            let done = self.delete_annotation(link.id).map(|_| 1);
            count += fold.after(self, link.id, done)?;
        }
        for bookmark in &plan.bookmarks {
            let done = self.delete_outline_item(bookmark.id);
            count += fold.after(self, bookmark.id, done)?;
        }
        if !plan.destinations.is_empty() {
            let catalog = self.graph().catalog_id().unwrap_or(ObjId::new(0, 0));
            let done = self.remove_named_destinations(&plan.destinations);
            count += fold.after(self, catalog, done)?;
        }
        Ok(count)
    }

    /// Delete `names` from both §12.3.2.3 namespaces as one command. The
    /// `/Names /Dests` tree is rewritten as one flat leaf, keys sorted
    /// byte-wise (§7.9.6); its old nodes become unreferenced.
    fn remove_named_destinations(&mut self, names: &[Vec<u8>]) -> Result<usize, EditError> {
        let gone: HashSet<&[u8]> = names.iter().map(Vec::as_slice).collect();
        let catalog_id = self.graph().catalog_id().ok_or(EditError::NotADictionary {
            id: ObjId::new(0, 0),
            key: "Root",
        })?;
        let Some(Object::Dict(mut catalog)) = self.value(catalog_id).cloned() else {
            return Err(EditError::NotADictionary {
                id: catalog_id,
                key: "Root",
            });
        };
        let mut writes: BTreeMap<ObjId, Object> = BTreeMap::new();
        if let Some(legacy) = catalog.get(b"Dests").cloned() {
            let pruned = self
                .deref_dict(Some(&legacy))
                .map(|d| prune_dict(&d, &gone));
            self.place(&mut catalog, b"Dests", &legacy, pruned, &mut writes);
        }
        if let Some(names_obj) = catalog.get(b"Names").cloned()
            && let Some(mut names_dict) = self.deref_dict(Some(&names_obj))
            && let Some(tree_obj) = names_dict.get(b"Dests").cloned()
        {
            let entries =
                dests_tree(&self.graph()).and_then(|t| name_tree_entries(&self.graph(), t));
            let Some(entries) = entries else {
                return Err(EditError::NameTreeUnsupported);
            };
            let leaf = flat_leaf(entries, &gone);
            let is_leaf = self
                .deref_dict(Some(&tree_obj))
                .is_some_and(|d| !d.contains_key(b"Kids"));
            if is_leaf {
                self.place(
                    &mut names_dict,
                    b"Dests",
                    &tree_obj,
                    Some(leaf),
                    &mut writes,
                );
            } else {
                names_dict.insert(Name::from(&b"Dests"[..]), Object::Dict(leaf));
            }
            let names_dict = Object::Dict(names_dict);
            match names_obj {
                Object::Reference(id) => {
                    writes.insert(id, names_dict);
                }
                _ => catalog.insert(Name::from(&b"Names"[..]), names_dict),
            }
        }
        writes.entry(catalog_id).or_insert(Object::Dict(catalog));
        let objects: Vec<ObjectWrite> = writes
            .into_iter()
            .map(|(id, after)| ObjectWrite {
                id,
                before: self.state.get(&id).cloned(),
                after: Some(after),
            })
            .collect();
        self.commit(Command {
            kind: CommandKind::Optimize { count: names.len() },
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        Ok(names.len())
    }

    /// Store `value` where `original` lived: in its own object when
    /// `original` is a reference, else directly under `key` in `holder`.
    fn place(
        &self,
        holder: &mut Dict,
        key: &[u8],
        original: &Object,
        value: Option<Dict>,
        writes: &mut BTreeMap<ObjId, Object>,
    ) {
        let Some(value) = value else { return };
        match original {
            Object::Reference(id) if matches!(self.value(*id), Some(Object::Dict(_))) => {
                writes.insert(*id, Object::Dict(value));
            }
            _ => holder.insert(Name::from(key), Object::Dict(value)),
        }
    }
}

/// Folds each committed clean-up or optimize command into the run's first,
/// counting only commands that actually committed.
pub(super) struct Fold {
    kind: CommandKind,
    started: bool,
    /// The top undo serial last seen; a different one is a new commit.
    mark: Option<u64>,
}

impl Fold {
    /// Start folding: remembers the undo entry on top now, so `finish`
    /// folds only what this run committed.
    pub(super) fn new(session: &EditSession, kind: CommandKind) -> Self {
        Self {
            kind,
            started: false,
            mark: session.undo.last().map(|e| e.serial),
        }
    }

    /// Fold what the verb that returned `result` committed; on its failure,
    /// undo the run so far and report `id`.
    fn after<T>(
        &mut self,
        session: &mut EditSession,
        id: ObjId,
        result: Result<T, EditError>,
    ) -> Result<T, OptimizeError> {
        match result {
            Ok(value) => {
                self.absorb(session);
                Ok(value)
            }
            Err(error) => {
                self.roll_back(session);
                Err(OptimizeError::CleanupFailed {
                    id,
                    reason: error.to_string(),
                })
            }
        }
    }

    /// Fold the newest undo entry into the run's, if it is new.
    pub(super) fn absorb(&mut self, session: &mut EditSession) {
        let top = session.undo.last().map(|e| e.serial);
        if top.is_none() || top == self.mark {
            return;
        }
        let folded = if self.started {
            session.coalesce_last(2, self.kind)
        } else {
            session.coalesce_last(1, self.kind)
        };
        self.started |= folded;
        self.mark = session.undo.last().map(|e| e.serial);
    }

    /// Fold the last commit and label the run's entry with `count`.
    pub(super) fn finish(&mut self, session: &mut EditSession, count: usize) {
        self.absorb(session);
        if self.started
            && let Some(last) = session.undo.last_mut()
        {
            last.command.kind = CommandKind::Optimize { count };
        }
    }

    /// Undo the run's entry, if one was committed.
    pub(super) fn roll_back(&mut self, session: &mut EditSession) {
        self.absorb(session);
        if self.started {
            session.undo();
            self.started = false;
        }
    }
}

fn dests_tree<G: ObjectGraph + ?Sized>(graph: &G) -> Option<&Dict> {
    graph
        .catalog_dict()?
        .get(b"Names")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_dict)?
        .get(b"Dests")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_dict)
}

fn prune_dict(dict: &Dict, gone: &HashSet<&[u8]>) -> Dict {
    let mut out = Dict::new();
    for (k, v) in dict.iter() {
        if !gone.contains(k.as_bytes()) {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

fn flat_leaf(entries: HashMap<Vec<u8>, Object>, gone: &HashSet<&[u8]>) -> Dict {
    let mut kept: Vec<(Vec<u8>, Object)> = entries
        .into_iter()
        .filter(|(k, _)| !gone.contains(k.as_slice()))
        .collect();
    kept.sort_by(|a, b| a.0.cmp(&b.0));
    let mut array = Vec::with_capacity(kept.len() * 2);
    for (k, v) in kept {
        array.push(Object::String(k));
        array.push(v);
    }
    let mut leaf = Dict::new();
    leaf.insert(Name::from(&b"Names"[..]), Object::Array(array));
    leaf
}

fn name_bytes(value: &Object) -> Option<Vec<u8>> {
    match value {
        Object::String(s) => Some(s.clone()),
        Object::Name(n) => Some(n.as_bytes().to_vec()),
        _ => None,
    }
}

/// Collect every name or string under a `/Dest` or `/D` key in `value`, and
/// note whether any dictionary carries `/JS` or is a JavaScript action.
fn scan_names(value: &Object, named: &mut HashSet<Vec<u8>>, javascript: &mut bool) {
    let mut stack = vec![value];
    while let Some(obj) = stack.pop() {
        let dict = match obj {
            Object::Array(items) => {
                stack.extend(items.iter());
                continue;
            }
            Object::Dict(d) => d,
            Object::Stream(s) => &s.dict,
            _ => continue,
        };
        for (k, v) in dict.iter() {
            match k.as_bytes() {
                b"Dest" | b"D" => {
                    if let Some(name) = name_bytes(v) {
                        named.insert(name);
                    }
                }
                b"JS" => *javascript = true,
                b"S" if v.as_name().is_some_and(|n| n.as_bytes() == b"JavaScript") => {
                    *javascript = true;
                }
                _ => {}
            }
            stack.push(v);
        }
    }
}

fn broken(dest: Option<&Destination>) -> Option<BrokenTarget> {
    match dest? {
        Destination::UnmappedPage { page: Some(id), .. } => Some(BrokenTarget::MissingPage(*id)),
        Destination::Named { name, .. } => Some(BrokenTarget::UndefinedName(name.clone())),
        _ => None,
    }
}

fn broken_links<G: ObjectGraph + ?Sized>(graph: &G, reader: &DestinationReader) -> Vec<BrokenLink> {
    let Ok(slots) = crate::page_tree::page_slots(graph) else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (page_index, slot) in slots.iter().enumerate() {
        let annots = graph
            .value(slot.id)
            .and_then(Object::as_dict)
            .and_then(|p| p.get(b"Annots"))
            .map(|o| graph.resolve(o))
            .and_then(Object::as_array);
        for id in annots
            .into_iter()
            .flatten()
            .filter_map(Object::as_reference)
        {
            let Some(dict) = graph.value(id).and_then(Object::as_dict) else {
                continue;
            };
            let is_link = dict
                .get(b"Subtype")
                .map(|o| graph.resolve(o))
                .and_then(Object::as_name)
                .is_some_and(|n| n.as_bytes() == b"Link");
            if is_link
                && seen.insert(id)
                && let Some(target) = broken(reader.destination(graph, dict).as_ref())
            {
                out.push(BrokenLink {
                    page_index,
                    id,
                    target,
                });
            }
        }
    }
    out
}

/// What happens to one outline item.
enum Verdict {
    /// Deleted with its subtree.
    Whole(BrokenBookmark),
    /// Kept; these items under it are deleted.
    Partial(Vec<BrokenBookmark>),
}

fn broken_bookmarks(items: &[OutlineItem]) -> Vec<BrokenBookmark> {
    let mut out = Vec::new();
    for item in items {
        match verdict(item) {
            Verdict::Whole(b) => out.push(b),
            Verdict::Partial(v) => out.extend(v),
        }
    }
    out
}

/// An item is deleted when its own destination is broken, or it has none
/// and only groups children, and every child is deleted. Recursion is
/// bounded by [`crate::outline::MAX_OUTLINE_DEPTH`].
fn verdict(item: &OutlineItem) -> Verdict {
    let mut under = Vec::new();
    let mut descendants = 0;
    let mut all_whole = true;
    for child in &item.children {
        match verdict(child) {
            Verdict::Whole(b) => {
                descendants += b.descendants + 1;
                under.push(b);
            }
            Verdict::Partial(v) => {
                all_whole = false;
                under.extend(v);
            }
        }
    }
    let target = broken(item.destination.as_ref());
    let grouping = item.destination.is_none() && !item.children.is_empty();
    if all_whole && (target.is_some() || grouping) {
        Verdict::Whole(BrokenBookmark {
            id: item.id,
            title: item.title.clone(),
            target,
            descendants,
        })
    } else {
        Verdict::Partial(under)
    }
}
