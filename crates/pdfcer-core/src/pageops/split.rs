//! # Splitting one document into several
//!
//! Split is **repeated extract** — `core_ops__split_document.md` says so
//! in as many words (*"splitting is architecturally 'extract N times',
//! not a distinct mechanism"*) — so this module contains no copying logic
//! at all. It contains two things extract does not need: a way to decide
//! **where the boundaries fall**, and a way to **name the outputs**.
//!
//! ## The criteria, and which of Acrobat's three ship here
//!
//! Acrobat documents exactly three native split criteria: fixed page
//! count, maximum output file size, and top-level bookmarks. pdfcer ships
//! **page count** and **top-level bookmarks**, plus **explicit break
//! points** — which Acrobat's web variant offers as manual split markers
//! over the same page-range-partition mechanism, and which is what the
//! Pass 3.2 UI spec's "after these pages" and "at the rail selection"
//! criteria both compile down to.
//!
//! **File-size-budget splitting is deliberately not in this Pass**, and
//! is named rather than silently omitted. The RAG flags why it is not a
//! peer of the other two: *"File-size-budget splitting requires either a
//! size-estimation pass or an iterative pack-then-check loop (page/object
//! byte cost isn't known until final serialization) — flag as a genuinely
//! more complex implementation item than the other two purely
//! page-tree-driven criteria."* Every other criterion here is a pure
//! function of the page tree and runs in microseconds; a size budget
//! means serializing candidate outputs repeatedly, whose cost is
//! quadratic in parts and which needs its own decision about what to do
//! when a **single page** exceeds the budget. It gets its own slice.
//!
//! ## Naming
//!
//! A template with four placeholders — `{stem}`, `{n}`, `{start}`,
//! `{end}` — which covers both naming schemes the RAG documents
//! (sequential-numeric, e.g. `document_1.pdf`, and page-range, e.g.
//! `PagesFrom_6_To_6.pdf`) with one mechanism rather than an enum of
//! two. Bookmark-text naming is not offered: it would need sanitizing
//! arbitrary document text into a filename on three operating systems'
//! rules, which is a genuinely separate problem and one that fails
//! unpredictably rather than obviously.
//!
//! An operator-supplied template that yields duplicate names is a
//! **named refusal** ([`PageOpError::AmbiguousNames`]), not a silent
//! overwrite. That is the whole reason [`plan_split`] returns names
//! before anything is written.

use std::collections::HashSet;

use crate::object::{ObjId, Object};
use crate::pageops::PageOpError;
use crate::pageops::assemble::{AssembleReport, DocumentView};
use crate::pageops::references::{DestinationResolver, MAX_OUTLINE_ITEMS};

/// How a document is divided into parts.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SplitCriterion {
    /// A fixed number of pages per output.
    ///
    /// The final part is shorter when the total does not divide evenly —
    /// standard partition semantics, and what Acrobat does (no documented
    /// pad, no merge-remainder-into-previous).
    EveryN(usize),
    /// Explicit break points: 0-based page indices **after which** a new
    /// part begins.
    ///
    /// Covers both of the UI's remaining criteria. "Split after pages 3
    /// and 7" is `AfterPages(vec![2, 6])`; "split at the rail's current
    /// selection" is the selected indices, minus the first page (a break
    /// before page 1 would create an empty leading part).
    AfterPages(Vec<usize>),
    /// One output per **top-level** outline entry, breaking at the page
    /// each one targets.
    ///
    /// Depth-1 entries only — *"nested (child) bookmarks do NOT each
    /// become their own split boundary"*. Pages before the first
    /// top-level entry's target form a leading part rather than being
    /// dropped, which the RAG marks as an unconfirmed **GAP** in
    /// Acrobat's own behaviour; keeping them is the only option that
    /// cannot lose the operator's content.
    TopLevelBookmarks,
}

/// One planned output: its page range and the file name it will be given.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SplitPart {
    /// 0-based index of the first page of this part, in the source's
    /// document order.
    pub first_page: usize,
    /// 0-based index of the last page of this part, inclusive.
    pub last_page: usize,
    /// The file name this part will be written under, template already
    /// applied. No directory component — the destination folder is the
    /// caller's.
    pub name: String,
}

impl SplitPart {
    /// How many pages this part holds.
    #[must_use]
    pub const fn page_count(&self) -> usize {
        self.last_page.saturating_sub(self.first_page) + 1
    }

    /// The 0-based page indices this part covers.
    pub fn pages(&self) -> impl Iterator<Item = usize> + use<> {
        self.first_page..=self.last_page
    }
}

/// Work out where the parts fall and what they will be called — without
/// writing anything.
///
/// Split in two from [`split`] on purpose. The Pass 3.2 UI spec requires
/// a **live preview** of the outputs (*"nothing is written until you
/// click Split"*), and a preview that ran the real operation would be
/// both slow and a lie about what "nothing is written" means. This
/// function is what the preview calls; [`split`] calls it too, so the
/// preview and the result can never disagree.
///
/// `stem` is the source file's base name, substituted for `{stem}`.
///
/// # Errors
///
/// - [`PageOpError::PageTree`] — the source's page tree could not be
///   walked.
/// - [`PageOpError::NoPages`] — the source has none.
/// - [`PageOpError::NoSplitPoints`] — the criterion selected nothing (an
///   `EveryN(0)`, an out-of-range break-point list, or a document with no
///   top-level bookmarks).
/// - [`PageOpError::AmbiguousNames`] — the template gives two parts the
///   same name.
pub fn plan_split(
    source: &DocumentView<'_>,
    criterion: &SplitCriterion,
    template: &str,
    stem: &str,
) -> Result<Vec<SplitPart>, PageOpError> {
    let slots = crate::page_tree::page_slots(source.graph())?;
    let page_count = slots.len();
    if page_count == 0 {
        return Err(PageOpError::NoPages);
    }

    let breaks = break_points(source, criterion, &slots, page_count);
    let mut parts: Vec<SplitPart> = Vec::new();
    let mut first = 0usize;
    // `breaks` holds the index of the *last* page of each part except the
    // final one, ascending and deduplicated.
    for boundary in breaks
        .iter()
        .copied()
        .chain(std::iter::once(page_count - 1))
    {
        if boundary < first {
            continue;
        }
        parts.push(SplitPart {
            first_page: first,
            last_page: boundary.min(page_count - 1),
            name: String::new(),
        });
        first = boundary + 1;
        if first >= page_count {
            break;
        }
    }
    if parts.is_empty() {
        return Err(PageOpError::NoSplitPoints);
    }

    // A criterion that produces exactly one part covering the whole
    // document has not split anything. Refusing is more useful than
    // writing one file that is a copy of the input under a new name.
    if parts.len() == 1 && !matches!(criterion, SplitCriterion::EveryN(_)) {
        return Err(PageOpError::NoSplitPoints);
    }

    let total = parts.len();
    for (index, part) in parts.iter_mut().enumerate() {
        part.name = render_name_template(template, stem, index + 1, part, total);
    }

    // Duplicate names would mean silently overwriting an output with a
    // later one — the operator asks for eight files and gets one.
    for (i, part) in parts.iter().enumerate() {
        if let Some(j) = parts
            .iter()
            .take(i)
            .position(|earlier| earlier.name == part.name)
        {
            return Err(PageOpError::AmbiguousNames {
                first: j + 1,
                second: i + 1,
            });
        }
    }
    Ok(parts)
}

/// The 0-based index of the last page of each part except the last.
fn break_points(
    source: &DocumentView<'_>,
    criterion: &SplitCriterion,
    slots: &[crate::page_tree::PageSlot],
    page_count: usize,
) -> Vec<usize> {
    let mut breaks: Vec<usize> = match criterion {
        SplitCriterion::EveryN(0) => Vec::new(),
        SplitCriterion::EveryN(n) => (*n..page_count).step_by(*n).map(|at| at - 1).collect(),
        SplitCriterion::AfterPages(points) => points
            .iter()
            .copied()
            .filter(|at| *at + 1 < page_count)
            .collect(),
        SplitCriterion::TopLevelBookmarks => {
            let starts = bookmark_start_pages(source, slots);
            // A start page at index k means the previous part ends at
            // k-1. A start at 0 yields no break (nothing precedes it).
            starts
                .into_iter()
                .filter_map(|start| start.checked_sub(1))
                .collect()
        }
    };
    breaks.sort_unstable();
    breaks.dedup();
    breaks.retain(|at| *at + 1 < page_count);
    breaks
}

/// The 0-based page indices that depth-1 outline entries target,
/// ascending.
///
/// Only depth 1: §12.3.3's tree nests arbitrarily, and the RAG is
/// explicit that *"nested (child) bookmarks do NOT each become their own
/// split boundary — only depth-1 entries do."*
fn bookmark_start_pages(
    source: &DocumentView<'_>,
    slots: &[crate::page_tree::PageSlot],
) -> Vec<usize> {
    let graph = source.graph();
    let Some(root) = graph
        .catalog_dict()
        .and_then(|catalog| catalog.get(b"Outlines").map(|o| graph.resolve(o)))
        .and_then(Object::as_dict)
    else {
        return Vec::new();
    };
    let resolver = DestinationResolver::new(graph);
    let index_of: std::collections::HashMap<ObjId, usize> = slots
        .iter()
        .enumerate()
        .map(|(index, slot)| (slot.id, index))
        .collect();

    let mut starts: Vec<usize> = Vec::new();
    let mut visited: HashSet<ObjId> = HashSet::new();
    let mut budget = MAX_OUTLINE_ITEMS;
    // Walk the top-level sibling chain only — `walk_outline` descends, so
    // the chain is followed here rather than delegated.
    let mut current = root.get(b"First").and_then(Object::as_reference);
    while let Some(id) = current {
        if budget == 0 || !visited.insert(id) {
            break;
        }
        budget -= 1;
        let Some(dict) = graph.resolved(id).as_dict() else {
            break;
        };
        if let Some(page) = resolver.resolve_target(graph, dict)
            && let Some(index) = index_of.get(&page)
        {
            starts.push(*index);
        }
        current = dict.get(b"Next").and_then(Object::as_reference);
    }
    starts.sort_unstable();
    starts.dedup();
    starts
}

/// Substitute a naming template's placeholders.
///
/// | Placeholder | Value |
/// |---|---|
/// | `{stem}` | the source file's base name, without extension |
/// | `{n}` | this part's 1-based sequence number, zero-padded to the width of the total |
/// | `{start}` | this part's first page, **1-based** |
/// | `{end}` | this part's last page, **1-based** |
///
/// `{n}` is zero-padded so that ten or more parts sort correctly in a
/// file manager — `part_02` before `part_10`, not after it. That is a
/// small thing that is extremely annoying to discover after generating
/// 300 files.
///
/// Page numbers are 1-based here and 0-based everywhere in the engine,
/// because these appear in a **file name a human reads**. The conversion
/// happens exactly once, here, rather than at every call site.
///
/// An unknown placeholder is left alone rather than blanked: an operator
/// who typed `{page}` should see `{page}` in the result and understand
/// why, instead of getting a file called `chapter_.pdf`.
#[must_use]
pub fn render_name_template(
    template: &str,
    stem: &str,
    number: usize,
    part: &SplitPart,
    total: usize,
) -> String {
    let width = total.to_string().len();
    template
        .replace("{stem}", stem)
        .replace("{n}", &format!("{number:0width$}"))
        .replace("{start}", &(part.first_page + 1).to_string())
        .replace("{end}", &(part.last_page + 1).to_string())
}

/// The default naming template: `stem_01.pdf`, `stem_02.pdf`, …
pub const DEFAULT_NAME_TEMPLATE: &str = "{stem}_{n}.pdf";

/// Split `source` into parts, returning each part's plan and bytes.
///
/// # Errors
///
/// [`PageOpError`] — from [`plan_split`], or from the per-part
/// [`extract`](crate::pageops::extract).
pub fn split(
    source: &DocumentView<'_>,
    criterion: &SplitCriterion,
    template: &str,
    stem: &str,
) -> Result<Vec<(SplitPart, Vec<u8>, AssembleReport)>, PageOpError> {
    split_with(
        source,
        criterion,
        template,
        stem,
        crate::pageops::SeparationPolicy::default(),
    )
}

/// [`split`], with an explicit answer for preseparated page sets
/// (§14.11.4).
///
/// Split is the harshest case for a preseparated file: every part takes
/// one page, so a four-plate set is shattered into four separate
/// documents and *every* set loses members. `split` delegates here with
/// [`SeparationPolicy::Repair`](crate::pageops::SeparationPolicy::Repair),
/// which leaves each part holding a conforming one-member set that still
/// records which plate it was.
///
/// # Errors
///
/// As [`split`], plus [`PageOpError::SeparationSplit`] under
/// [`SeparationPolicy::Refuse`](crate::pageops::SeparationPolicy::Refuse)
/// — which, for this operation, refuses any preseparated input at all.
pub fn split_with(
    source: &DocumentView<'_>,
    criterion: &SplitCriterion,
    template: &str,
    stem: &str,
    separations: crate::pageops::SeparationPolicy,
) -> Result<Vec<(SplitPart, Vec<u8>, AssembleReport)>, PageOpError> {
    let parts = plan_split(source, criterion, template, stem)?;
    let mut out = Vec::with_capacity(parts.len());
    for part in parts {
        let pages: Vec<usize> = part.pages().collect();
        let (bytes, report) = crate::pageops::extract_with(source, &pages, separations)?;
        out.push((part, bytes, report));
    }
    Ok(out)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::pageops::tests_support::build_pdf;

    /// `n` pages, plus optional extra objects appended after them.
    fn doc_with_pages(n: u32, extra: &[(u32, &str)]) -> Document {
        let kids: String = (0..n).map(|i| format!("{} 0 R ", i + 3)).collect();
        let mut objects: Vec<(u32, String)> = vec![
            (
                1,
                if extra.is_empty() {
                    "<< /Type /Catalog /Pages 2 0 R >>".to_owned()
                } else {
                    "<< /Type /Catalog /Pages 2 0 R /Outlines 100 0 R >>".to_owned()
                },
            ),
            (
                2,
                format!(
                    "<< /Type /Pages /Kids [{kids}] /Count {n} \
                     /MediaBox [0 0 10 10] /Resources << >> >>"
                ),
            ),
        ];
        for i in 0..n {
            objects.push((i + 3, "<< /Type /Page /Parent 2 0 R >>".to_owned()));
        }
        objects.extend(extra.iter().map(|(n, b)| (*n, (*b).to_owned())));
        let borrowed: Vec<(u32, &str)> = objects.iter().map(|(n, b)| (*n, b.as_str())).collect();
        build_pdf(&borrowed)
    }

    fn view(doc: &Document) -> DocumentView<'_> {
        DocumentView::new(doc, doc.bytes(), doc.version())
    }

    #[test]
    fn every_n_partitions_with_a_short_final_part() {
        // 7 pages by 3 → 3 + 3 + 1. No pad, no merge-into-previous.
        let doc = doc_with_pages(7, &[]);
        let parts = plan_split(
            &view(&doc),
            &SplitCriterion::EveryN(3),
            DEFAULT_NAME_TEMPLATE,
            "doc",
        )
        .unwrap();
        assert_eq!(parts.len(), 3);
        assert_eq!((parts[0].first_page, parts[0].last_page), (0, 2));
        assert_eq!((parts[1].first_page, parts[1].last_page), (3, 5));
        assert_eq!((parts[2].first_page, parts[2].last_page), (6, 6));
        assert_eq!(parts[2].page_count(), 1);
    }

    #[test]
    fn every_n_larger_than_the_document_yields_one_part() {
        // Not a refusal: "at most N pages per file" is a legitimate
        // request that a short document satisfies in one file.
        let doc = doc_with_pages(3, &[]);
        let parts = plan_split(
            &view(&doc),
            &SplitCriterion::EveryN(10),
            DEFAULT_NAME_TEMPLATE,
            "doc",
        )
        .unwrap();
        assert_eq!(parts.len(), 1);
    }

    #[test]
    fn explicit_break_points_are_deduplicated_and_clamped() {
        let doc = doc_with_pages(5, &[]);
        let parts = plan_split(
            &view(&doc),
            // 1 twice, and 4 (the last page) which cannot start a part.
            &SplitCriterion::AfterPages(vec![1, 1, 4]),
            DEFAULT_NAME_TEMPLATE,
            "doc",
        )
        .unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!((parts[0].first_page, parts[0].last_page), (0, 1));
        assert_eq!((parts[1].first_page, parts[1].last_page), (2, 4));
    }

    #[test]
    fn break_points_that_select_nothing_are_a_named_refusal() {
        let doc = doc_with_pages(3, &[]);
        assert_eq!(
            plan_split(
                &view(&doc),
                &SplitCriterion::AfterPages(vec![2]),
                DEFAULT_NAME_TEMPLATE,
                "doc"
            )
            .unwrap_err(),
            PageOpError::NoSplitPoints
        );
    }

    #[test]
    fn top_level_bookmarks_break_at_their_target_pages() {
        // Two depth-1 entries (pages 1 and 3) and one CHILD entry
        // (page 2), which must NOT create a boundary.
        let doc = doc_with_pages(
            4,
            &[
                (100, "<< /Type /Outlines /First 101 0 R /Count 2 >>"),
                (
                    101,
                    "<< /Title (A) /Dest [3 0 R /Fit] /Next 103 0 R /First 102 0 R >>",
                ),
                (102, "<< /Title (A.1) /Dest [4 0 R /Fit] >>"),
                (103, "<< /Title (B) /Dest [5 0 R /Fit] /Prev 101 0 R >>"),
            ],
        );
        let parts = plan_split(
            &view(&doc),
            &SplitCriterion::TopLevelBookmarks,
            DEFAULT_NAME_TEMPLATE,
            "doc",
        )
        .unwrap();
        assert_eq!(parts.len(), 2, "the child bookmark must not split");
        assert_eq!((parts[0].first_page, parts[0].last_page), (0, 1));
        assert_eq!((parts[1].first_page, parts[1].last_page), (2, 3));
    }

    #[test]
    fn no_top_level_bookmarks_is_a_named_refusal() {
        let doc = doc_with_pages(4, &[]);
        assert_eq!(
            plan_split(
                &view(&doc),
                &SplitCriterion::TopLevelBookmarks,
                DEFAULT_NAME_TEMPLATE,
                "doc"
            )
            .unwrap_err(),
            PageOpError::NoSplitPoints
        );
    }

    #[test]
    fn the_name_template_pads_the_sequence_number() {
        // part_02 must sort before part_10 in a file manager.
        let doc = doc_with_pages(12, &[]);
        let parts = plan_split(
            &view(&doc),
            &SplitCriterion::EveryN(1),
            DEFAULT_NAME_TEMPLATE,
            "report",
        )
        .unwrap();
        assert_eq!(parts.len(), 12);
        assert_eq!(parts[1].name, "report_02.pdf");
        assert_eq!(parts[11].name, "report_12.pdf");
    }

    #[test]
    fn the_name_template_supports_page_range_naming() {
        let doc = doc_with_pages(4, &[]);
        let parts = plan_split(
            &view(&doc),
            &SplitCriterion::EveryN(2),
            "PagesFrom_{start}_To_{end}.pdf",
            "ignored",
        )
        .unwrap();
        assert_eq!(parts[0].name, "PagesFrom_1_To_2.pdf");
        assert_eq!(parts[1].name, "PagesFrom_3_To_4.pdf");
    }

    #[test]
    fn a_template_that_collides_is_refused_before_anything_is_written() {
        // Without this the operator asks for six files and silently gets
        // one — the last part, wearing everyone's name.
        let doc = doc_with_pages(6, &[]);
        assert_eq!(
            plan_split(
                &view(&doc),
                &SplitCriterion::EveryN(2),
                "{stem}.pdf",
                "flat"
            )
            .unwrap_err(),
            PageOpError::AmbiguousNames {
                first: 1,
                second: 2
            }
        );
    }

    #[test]
    fn an_unknown_placeholder_is_left_visible_rather_than_blanked() {
        let part = SplitPart {
            first_page: 0,
            last_page: 1,
            name: String::new(),
        };
        assert_eq!(
            render_name_template("{stem}-{page}-{n}.pdf", "x", 3, &part, 3),
            "x-{page}-3.pdf"
        );
    }

    #[test]
    fn split_produces_loadable_parts_covering_every_page_exactly_once() {
        let doc = doc_with_pages(5, &[]);
        let outputs = split(
            &view(&doc),
            &SplitCriterion::EveryN(2),
            DEFAULT_NAME_TEMPLATE,
            "doc",
        )
        .unwrap();
        assert_eq!(outputs.len(), 3);
        let mut total = 0usize;
        for (part, bytes, report) in outputs {
            let out = Document::from_bytes(bytes).unwrap();
            let pages = crate::page_tree::pages(&out).unwrap().len();
            assert_eq!(pages, part.page_count());
            assert_eq!(pages, report.pages);
            total += pages;
        }
        assert_eq!(total, 5, "every page lands in exactly one part");
    }
}
