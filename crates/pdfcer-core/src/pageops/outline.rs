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
    /// The external file this entry opens, when it opens one: a `/Launch`
    /// or `/GoToR` file name, plus the 0-based page inside it a `/GoToR`
    /// names.
    ///
    /// Read during collection and resolved in [`build`] against the other
    /// sources' file names — see [`relink`].
    external: Option<ExternalLink>,
    /// The OUTPUT object number this entry was re-pointed to, once a
    /// cross-file link has been matched to a source in this merge.
    ///
    /// Distinct from `destination`, which holds a page id local to the
    /// entry's OWN source. A re-pointed entry targets a page belonging to
    /// a *different* source, so it cannot be expressed in those terms:
    /// two sources may legitimately use the same object number, which is
    /// exactly why `page_map` is keyed by `(source, id)`. This field
    /// short-circuits that lookup with the answer already resolved.
    relinked_to: Option<u32>,
    children: Vec<Item>,
}

/// A bookmark that opens **another file** — the shape a table-of-contents
/// PDF is made of (§12.6.4.5 `/Launch`, §12.6.4.3 `/GoToR`).
#[derive(Debug, Clone)]
struct ExternalLink {
    /// The file specification's name, as bytes.
    file: Vec<u8>,
    /// The 0-based page index inside that file, when the action names one.
    /// `/GoToR` can; `/Launch` cannot, and gets `None` — which resolves to
    /// the target file's first page.
    page: Option<usize>,
}

impl Item {
    /// Whether this entry, or anything under it, targets a copied page.
    ///
    /// A container whose only value is holding kept children is itself
    /// kept — dropping it would reparent its children to the root and
    /// destroy the hierarchy the operator can see.
    fn is_kept(&self, kept_pages: &HashSet<ObjId>) -> bool {
        self.relinked_to.is_some()
            || self
                .destination
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

    // ★ RE-POINT CROSS-FILE BOOKMARKS FIRST, then prune. A table-of-
    // contents entry that opens `chapter1.pdf` has no page in its OWN
    // source, so pruning would discard it before anything had a chance to
    // notice that `chapter1.pdf` is sitting in this very merge. Order is
    // the whole fix.
    //
    // Each source's output page numbers, in page order, so a `/GoToR` can
    // land on the page it names rather than only on the file's first.
    let mut source_pages: Vec<Vec<u32>> = vec![Vec::new(); sources.len()];
    for (position, (src, id)) in selected.iter().enumerate() {
        if let Some(number) = page_numbers.get(position) {
            if let Some(pages) = source_pages.get_mut(*src) {
                pages.push(*number);
            }
            let _ = id;
        }
    }
    let source_first_page: Vec<Option<u32>> =
        source_pages.iter().map(|p| p.first().copied()).collect();

    let mut roots = roots;
    if !options.source_files.is_empty() {
        for (_, items) in &mut roots {
            relink(
                items,
                &options.source_files,
                &source_first_page,
                &source_pages,
                report,
            );
        }
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
                        // A generated per-source heading points at a page
                        // of its own source; it is never a cross-file link.
                        external: None,
                        relinked_to: None,
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
            // Only when there is no local page: an entry that navigates
            // within its own file is not a cross-file link, whatever else
            // its action dictionary carries.
            external: if page.is_none() {
                read_external_link(graph, dict)
            } else {
                None
            },
            relinked_to: None,
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

/// Read a `/Launch` or `/GoToR` action's target file, and the page inside
/// it when one is named.
///
/// These are the two action types that name another **file** (§12.6.4.5
/// and §12.6.4.3). Every other action — `/URI`, `/JavaScript`, `/Named` —
/// names nothing this merge can re-point, and is left alone.
///
/// The file specification is resolved through
/// [`crate::outline::file_spec_bytes`], which prefers `/UF` over `/F`: the
/// same resolver `list-outline` uses, so the name matched here is exactly
/// the name an operator was shown. `/Launch`'s deprecated `/Win` `/F`
/// bare-path form is honoured too, for the old files a long-lived table of
/// contents is actually made of.
fn read_external_link<G: crate::graph::ObjectGraph + ?Sized>(
    graph: &G,
    item: &Dict,
) -> Option<ExternalLink> {
    let action = graph.resolve(item.get(b"A")?).as_dict()?;
    let subtype = graph
        .resolve(action.get(b"S")?)
        .as_name()?
        .as_bytes()
        .to_vec();
    match subtype.as_slice() {
        b"Launch" => Some(ExternalLink {
            file: crate::outline::read_launch_file(graph, action)?,
            // §12.6.4.5 launches an application on a file; it names no
            // page, so the merge lands on the file's first one.
            page: None,
        }),
        b"GoToR" => Some(ExternalLink {
            file: crate::outline::file_spec_bytes(graph, action.get(b"F")?)?,
            // Table 199's remote destination is an array whose FIRST
            // element is a 0-based page NUMBER (not a reference — the
            // reference form cannot name a page of another file). Any
            // other shape, including a named destination belonging to the
            // target file's own namespace, resolves to the first page
            // rather than to a guess.
            page: graph
                .resolve(action.get(b"D").unwrap_or(&Object::Null))
                .as_array()
                .and_then(|a| a.first())
                .and_then(Object::as_number)
                .and_then(|n| usize::try_from(n as i64).ok()),
        }),
        _ => None,
    }
}

/// Re-point every cross-file bookmark whose target file is **also one of
/// the sources being merged**.
///
/// # ★ Why this exists
///
/// A table-of-contents PDF's bookmarks open the other PDFs in the folder.
/// Merging those files into one document makes every one of those targets
/// vanish: the bookmark still says *open `chapter1.pdf`*, and there is no
/// longer a `chapter1.pdf`. Before this, pdfcer dropped all of them —
/// honestly (`outline_dropped=4`, disclosed on the merge report) but
/// completely, so the operator's own bookmark titles were lost and only
/// the per-source headings pdfcer generates itself remained.
///
/// The merge already knows which file each source came from, so the
/// mapping the operator would otherwise have to build by hand is
/// available for free at exactly the moment it is needed.
///
/// # Matching
///
/// By file NAME, case-insensitively, comparing the last path component
/// only. A `/Launch` names `chapter1.pdf`; the operator merged
/// `C:\work\chapter1.pdf`. Requiring those to be equal would answer
/// "never". Case-insensitivity follows the platform the operator is on and
/// the fact that a PDF file specification carries no case rule of its own.
///
/// An unmatched link is left unresolved and the entry prunes as before —
/// a bookmark pointing at a file that was NOT merged is genuinely dead,
/// and inventing a destination for it would be worse than dropping it.
fn relink(
    items: &mut [Item],
    source_files: &[Vec<u8>],
    source_first_page: &[Option<u32>],
    source_pages: &[Vec<u32>],
    report: &mut AssembleReport,
) {
    for item in items {
        if let Some(link) = &item.external
            && let Some(target) = source_files
                .iter()
                .position(|name| same_file(name, &link.file))
        {
            // The page inside the target file, when the action named one
            // and that page exists; otherwise the file's first page.
            let number = link
                .page
                .and_then(|index| source_pages.get(target).and_then(|p| p.get(index)).copied())
                .or_else(|| source_first_page.get(target).copied().flatten());
            if let Some(number) = number {
                item.relinked_to = Some(number);
                report.outline_items_relinked += 1;
            }
        }
        relink(
            &mut item.children,
            source_files,
            source_first_page,
            source_pages,
            report,
        );
    }
}

/// Whether a file specification names the same file as a merge source,
/// comparing the last path component case-insensitively.
///
/// Both separators are treated as separators regardless of platform: a PDF
/// authored on Windows carries backslashes, and the same document opened
/// on Linux must still match.
fn same_file(source: &[u8], spec: &[u8]) -> bool {
    fn base(bytes: &[u8]) -> Vec<u8> {
        let cut = bytes
            .iter()
            .rposition(|b| *b == b'/' || *b == b'\\')
            .map_or(0, |p| p + 1);
        bytes.get(cut..).unwrap_or(bytes).to_ascii_lowercase()
    }
    !source.is_empty() && base(source) == base(spec)
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
            external: item.external.clone(),
            relinked_to: item.relinked_to,
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
        // A re-pointed cross-file link resolves to an OUTPUT object
        // number directly; it cannot go through `page_map`, whose key is
        // (source, source-local id) and whose answer would be about the
        // wrong file. Checked first, because an entry that has both is one
        // whose own page also survived and whose link is redundant.
        if let Some(number) = item.relinked_to {
            dict.insert(
                Name::from(b"Dest"),
                Object::Array(vec![
                    Object::Reference(ObjId::new(number, 0)),
                    Object::Name(Name::from(b"Fit")),
                ]),
            );
        } else if let Some((page, array)) = &item.destination
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
