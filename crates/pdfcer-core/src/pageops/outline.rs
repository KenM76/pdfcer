//! # Outline (bookmark) carryover for assembled documents (§12.3.3)
//!
//! ## The policy, and why it is a *policy*
//!
//! Adobe does not document what happens to bookmarks when pages are
//! extracted. `core_ops__extract_pages.md` records the evidence
//! precisely: Adobe's stated carryover list for extraction is *"all
//! content, form fields, comments, and links from the original"* —
//! **bookmarks are notably absent from that list** — and the RAG marks
//! the question a **GAP**, with *"conservative reading … is 'not
//! carried' until verified empirically"*, and then recommends pdfcer
//! decide for itself:
//!
//! > carry any outline entry whose destination page falls within the
//! > extracted set, rewritten to the new page index; drop entries
//! > pointing elsewhere. Apply the same policy consistently to Split
//! > (which is architecturally repeated Extract).
//!
//! pdfcer adopts that recommendation. It is a **documented pdfcer
//! decision**, not a parity claim, and this file is where it is written
//! down. The alternative — dropping outlines wholesale — is defensible
//! against the sources but strictly worse for the operator: extracting
//! chapter 4 of a manual and losing its sub-headings is a real loss, and
//! nothing in the format makes it necessary.
//!
//! Merge takes the **other** documented behaviour, because for merge
//! there *is* one: `core_ops__merge_combine_files.md` records that
//! Acrobat generates *"one top-level bookmark per source file/document
//! (named after the source file), with that source's own existing
//! bookmarks (if any) nested beneath"*, and that it is the default. That
//! is [`super::assemble::OutlinePolicy::PerSource`].
//!
//! Insert takes **neither**: it carries the target's outline (the target
//! *is* the document being added to) and does not import the source's,
//! which matches `core_ops__insert_pages.md`'s recommended default —
//! *"No bookmark carryover on plain Insert … bookmark-carrying insert
//! should be a distinct, explicitly-named mode if ever added."*
//!
//! ## Two simplifications, both deliberate and both recorded
//!
//! **1. Every rebuilt item is emitted closed.** §12.3.3's `/Count` is
//! *"the total number of visible outline items at all levels"* for an
//! open item, and the negative of the descendant count for a closed one.
//! Reconstructing the open/closed pattern of the source would mean
//! recomputing visible-item arithmetic across a tree that has just had
//! entries removed from its middle — arithmetic no reader validates and
//! that is therefore easy to get quietly wrong. Open/closed is a **view
//! preference, not content**; emitting everything closed is correct,
//! trivially verifiable, and costs the operator one click.
//!
//! **2. A destination reached *by name* is rewritten to an explicit
//! one.** pdfcer does not carry the `/Dests` name tree into an assembled
//! document (see [`AssembleReport::named_destinations_dropped`](
//! super::assemble::AssembleReport::named_destinations_dropped)), so a
//! carried entry that used a name would point at nothing. It is rewritten
//! to `[page /Fit]` instead. That loses the source's zoom/position
//! parameters, which is a real if minor loss, and is preferred to a
//! bookmark that silently does nothing.
//!
//! An entry whose destination was an **explicit array** keeps its array
//! verbatim with only element 0 — the page reference (§12.3.2.2) —
//! rewritten, so `/XYZ`, `/FitH` and friends survive intact. That is the
//! overwhelmingly common case.

use std::collections::{HashMap, HashSet};

use crate::object::{Dict, Name, ObjId, Object};
use crate::pageops::PageOpError;
use crate::pageops::assemble::{
    AssembleOptions, AssembleReport, Copier, DocumentView, OutlinePolicy,
};
use crate::pageops::references::{DestinationResolver, MAX_OUTLINE_ITEMS};

/// Maximum outline nesting rebuilt (pdfcer policy, `ARCHITECTURE.md` §10).
const MAX_OUTLINE_DEPTH: usize = 32;

/// One outline entry, extracted from a source and ready to be rebuilt.
#[derive(Debug, Clone)]
struct Item {
    /// The source's `/Title`, carried verbatim. Bytes, not text: §12.3.3
    /// makes it a text string (§7.9.2) whose *interpretation* is a
    /// display concern, and re-encoding it here would risk changing it.
    title: Object,
    /// The destination array to emit, page reference already resolved to
    /// a source page id in element 0 — or `None` when this entry is a
    /// pure container being kept only because a descendant is kept.
    destination: Option<(ObjId, Vec<Object>)>,
    children: Vec<Item>,
}

impl Item {
    /// Whether this entry, or anything under it, targets a copied page.
    ///
    /// A container whose only value is holding kept children is itself
    /// kept — dropping it would reparent its children to the root and
    /// destroy the hierarchy the operator can see.
    fn is_kept(&self, kept_pages: &HashSet<ObjId>) -> bool {
        self.destination
            .as_ref()
            .is_some_and(|(page, _)| kept_pages.contains(page))
            || self.children.iter().any(|child| child.is_kept(kept_pages))
    }
}

/// Build the assembled document's `/Outlines` tree and attach it to
/// `catalog`, per `options.outline`.
///
/// # Errors
///
/// [`PageOpError`] — only from the copier (an object limit); an
/// unreadable source outline yields fewer entries, never a failure. A
/// merge must not fail because one input's bookmark tree was damaged.
pub fn build(
    copier: &mut Copier,
    sources: &[DocumentView<'_>],
    selected: &[(usize, ObjId)],
    page_numbers: &[u32],
    options: &AssembleOptions,
    catalog: &mut Dict,
    report: &mut AssembleReport,
) -> Result<(), PageOpError> {
    // Source page id → the output object number its copy got. Per source,
    // because two sources can legitimately use the same object number.
    let mut page_map: HashMap<(usize, ObjId), u32> = HashMap::new();
    for (position, entry) in selected.iter().enumerate() {
        if let Some(number) = page_numbers.get(position) {
            page_map.insert(*entry, *number);
        }
    }

    let roots: Vec<(usize, Vec<Item>)> = match options.outline {
        OutlinePolicy::Drop => Vec::new(),
        OutlinePolicy::Subset => match options.catalog_from {
            Some(index) => vec![(index, collect(sources, index, report))],
            None => Vec::new(),
        },
        OutlinePolicy::PerSource => (0..sources.len())
            .map(|index| (index, collect(sources, index, report)))
            .collect(),
    };
    if roots.is_empty() {
        return Ok(());
    }

    // Filter each source's tree to the entries that survive, then emit.
    let mut top_level: Vec<(usize, Item)> = Vec::new();
    for (source_index, items) in roots {
        let kept_pages: HashSet<ObjId> = page_map
            .keys()
            .filter(|(src, _)| *src == source_index)
            .map(|(_, id)| *id)
            .collect();
        let surviving = prune(&items, &kept_pages, report);
        match options.outline {
            OutlinePolicy::PerSource => {
                // Acrobat's documented default: one top-level entry per
                // source, that source's own entries nested beneath. The
                // entry is generated even when the source contributed no
                // bookmarks, because its job is to say "these pages came
                // from that file" — which is the point of the feature.
                let title = options
                    .source_titles
                    .get(source_index)
                    .cloned()
                    .unwrap_or_default();
                let first_page = selected
                    .iter()
                    .position(|(src, _)| *src == source_index)
                    .and_then(|position| selected.get(position).copied());
                top_level.push((
                    source_index,
                    Item {
                        title: Object::String(title),
                        destination: first_page.map(|(_, id)| {
                            (id, vec![Object::Null, Object::Name(Name::from(b"Fit"))])
                        }),
                        children: surviving,
                    },
                ));
            }
            _ => top_level.extend(surviving.into_iter().map(|item| (source_index, item))),
        }
    }
    if top_level.is_empty() {
        return Ok(());
    }

    let outlines_num = copier.reserve();
    let outlines_ref = ObjId::new(outlines_num, 0);
    let emitted = emit_siblings(copier, &top_level, outlines_ref, &page_map, 0);

    let mut outlines = Dict::new();
    outlines.insert(Name::from(b"Type"), Object::Name(Name::from(b"Outlines")));
    if let (Some(first), Some(last)) = (emitted.first(), emitted.last()) {
        outlines.insert(Name::from(b"First"), Object::Reference(*first));
        outlines.insert(Name::from(b"Last"), Object::Reference(*last));
    }
    // Every emitted item is closed (module docs), so the count of
    // *visible* items is exactly the number of top-level entries.
    outlines.insert(
        Name::from(b"Count"),
        Object::Integer(i64::try_from(emitted.len()).unwrap_or(i64::MAX)),
    );
    copier.store(outlines_num, Object::Dict(outlines));
    catalog.insert(Name::from(b"Outlines"), Object::Reference(outlines_ref));
    Ok(())
}

/// Read one source's outline tree into [`Item`]s, resolving each entry's
/// destination to a page id in that source.
fn collect(
    sources: &[DocumentView<'_>],
    source_index: usize,
    report: &mut AssembleReport,
) -> Vec<Item> {
    let Some(view) = sources.get(source_index) else {
        return Vec::new();
    };
    let graph = view.graph();
    let Some(root) = graph
        .catalog_dict()
        .and_then(|catalog| catalog.get(b"Outlines").map(|o| graph.resolve(o)))
        .and_then(Object::as_dict)
    else {
        return Vec::new();
    };
    let resolver = DestinationResolver::new(graph);
    let mut budget = MAX_OUTLINE_ITEMS;
    let mut visited = HashSet::new();
    let _ = report;
    read_siblings(
        view,
        &resolver,
        root.get(b"First").and_then(Object::as_reference),
        0,
        &mut budget,
        &mut visited,
    )
}

/// Read one sibling chain and everything under it.
///
/// Iterative across siblings, recursive across levels — a flat
/// 10,000-entry outline is an ordinary document and recursing on `/Next`
/// would overflow the stack on exactly those files.
fn read_siblings(
    view: &DocumentView<'_>,
    resolver: &DestinationResolver,
    first: Option<ObjId>,
    depth: usize,
    budget: &mut usize,
    visited: &mut HashSet<ObjId>,
) -> Vec<Item> {
    let mut out = Vec::new();
    if depth > MAX_OUTLINE_DEPTH {
        return out;
    }
    let graph = view.graph();
    let mut current = first;
    while let Some(id) = current {
        if *budget == 0 || !visited.insert(id) {
            break;
        }
        *budget -= 1;
        let Some(dict) = graph.resolved(id).as_dict() else {
            break;
        };
        let page = resolver.resolve_target(graph, dict);
        // Keep the source's own destination array where there is one, so
        // /XYZ and /FitH survive; synthesize /Fit only for a name-based
        // destination we cannot carry (module docs).
        let array = dict
            .get(b"Dest")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_array)
            .map(<[Object]>::to_vec)
            .or_else(|| {
                graph
                    .resolve(dict.get(b"A")?)
                    .as_dict()?
                    .get(b"D")
                    .map(|o| graph.resolve(o))
                    .and_then(Object::as_array)
                    .map(<[Object]>::to_vec)
            })
            .unwrap_or_else(|| vec![Object::Null, Object::Name(Name::from(b"Fit"))]);

        out.push(Item {
            title: dict
                .get(b"Title")
                .map(|o| graph.resolve(o))
                .cloned()
                .unwrap_or_else(|| Object::String(Vec::new())),
            destination: page.map(|p| (p, array)),
            children: read_siblings(
                view,
                resolver,
                dict.get(b"First").and_then(Object::as_reference),
                depth + 1,
                budget,
                visited,
            ),
        });
        current = dict.get(b"Next").and_then(Object::as_reference);
    }
    out
}

/// Drop every entry that neither targets a copied page nor holds a
/// descendant that does, counting both outcomes.
fn prune(items: &[Item], kept_pages: &HashSet<ObjId>, report: &mut AssembleReport) -> Vec<Item> {
    let mut out = Vec::new();
    for item in items {
        if !item.is_kept(kept_pages) {
            // Count the whole discarded subtree, not just its root: the
            // operator lost all of it, and reporting "1 bookmark dropped"
            // for a 40-entry chapter would be true and useless.
            report.outline_items_dropped += 1 + count_all(&item.children);
            continue;
        }
        let children = prune(&item.children, kept_pages, report);
        // A kept container whose own destination left keeps its title and
        // its children, and simply has no destination. Clicking it does
        // nothing, expanding it works — which is what a chapter heading
        // whose title page was not extracted should do.
        let destination = item
            .destination
            .clone()
            .filter(|(page, _)| kept_pages.contains(page));
        report.outline_items_kept += 1;
        out.push(Item {
            title: item.title.clone(),
            destination,
            children,
        });
    }
    out
}

/// Total entries in a subtree, for the dropped count.
fn count_all(items: &[Item]) -> usize {
    items.iter().map(|item| 1 + count_all(&item.children)).sum()
}

/// Emit one sibling chain as real objects, returning their ids in order.
///
/// Object numbers are reserved for the whole chain **before** any of them
/// is written, because §12.3.3 makes the chain doubly linked: an item
/// needs its `/Next` sibling's id, which does not exist yet under a
/// naive one-pass emission.
fn emit_siblings(
    copier: &mut Copier,
    items: &[(usize, Item)],
    parent: ObjId,
    page_map: &HashMap<(usize, ObjId), u32>,
    depth: usize,
) -> Vec<ObjId> {
    if depth > MAX_OUTLINE_DEPTH {
        return Vec::new();
    }
    let ids: Vec<ObjId> = items
        .iter()
        .map(|_| ObjId::new(copier.reserve(), 0))
        .collect();

    for (position, (source_index, item)) in items.iter().enumerate() {
        let Some(id) = ids.get(position) else {
            continue;
        };
        let mut dict = Dict::new();
        dict.insert(Name::from(b"Title"), item.title.clone());
        dict.insert(Name::from(b"Parent"), Object::Reference(parent));
        if let Some(prev) = position.checked_sub(1).and_then(|p| ids.get(p)) {
            dict.insert(Name::from(b"Prev"), Object::Reference(*prev));
        }
        if let Some(next) = ids.get(position + 1) {
            dict.insert(Name::from(b"Next"), Object::Reference(*next));
        }
        if let Some((page, array)) = &item.destination
            && let Some(number) = page_map.get(&(*source_index, *page))
        {
            let mut rewritten = array.clone();
            // §12.3.2.2: element 0 of an explicit destination is the page.
            if let Some(slot) = rewritten.first_mut() {
                *slot = Object::Reference(ObjId::new(*number, 0));
            } else {
                rewritten.push(Object::Reference(ObjId::new(*number, 0)));
            }
            dict.insert(Name::from(b"Dest"), Object::Array(rewritten));
        }

        let children: Vec<(usize, Item)> = item
            .children
            .iter()
            .map(|child| (*source_index, child.clone()))
            .collect();
        if !children.is_empty() {
            let child_ids = emit_siblings(copier, &children, *id, page_map, depth + 1);
            if let (Some(first), Some(last)) = (child_ids.first(), child_ids.last()) {
                dict.insert(Name::from(b"First"), Object::Reference(*first));
                dict.insert(Name::from(b"Last"), Object::Reference(*last));
                // Negative = closed (§12.3.3), magnitude = descendants
                // that would become visible on opening it. Every item is
                // emitted closed, so that is exactly the child count.
                dict.insert(
                    Name::from(b"Count"),
                    Object::Integer(-i64::try_from(child_ids.len()).unwrap_or(i64::MAX)),
                );
            }
        }
        copier.store(id.num, Object::Dict(dict));
    }
    ids
}
