//! Page-label number trees (ISO 32000-1 §12.4.2, Table 159; number trees
//! §7.9.7, Table 37): reading a `/PageLabels` tree into ranges, and
//! splicing two documents' ranges so every page keeps the label it
//! displayed in its own document.

use std::collections::HashSet;

use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, Object};
use crate::pageops::references::{MAX_NAME_TREE_DEPTH, MAX_NAME_TREE_NODES};

/// One label range: the page index it starts at and its label dictionary
/// (Table 159), every value direct.
pub(crate) type Range = (usize, Dict);

/// The ranges of the `/PageLabels` tree `tree`, sorted by start index, the
/// first entry for a repeated index kept. Walks `/Kids` with the name-tree
/// depth and node ceilings and a cycle guard; an entry whose key is not a
/// non-negative integer or whose value is not a dictionary is skipped.
pub(crate) fn ranges<G: ObjectGraph + ?Sized>(graph: &G, tree: &Object) -> Vec<Range> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut nodes = 0usize;
    walk(graph, tree, 0, &mut seen, &mut nodes, &mut out);
    out.sort_by_key(|r| r.0);
    out.dedup_by_key(|r| r.0);
    out
}

fn walk<G: ObjectGraph + ?Sized>(
    graph: &G,
    node: &Object,
    depth: usize,
    seen: &mut HashSet<crate::object::ObjId>,
    nodes: &mut usize,
    out: &mut Vec<Range>,
) {
    if depth > MAX_NAME_TREE_DEPTH || *nodes >= MAX_NAME_TREE_NODES {
        return;
    }
    if let Object::Reference(id) = node
        && !seen.insert(*id)
    {
        return;
    }
    *nodes += 1;
    let Some(dict) = graph.resolve(node).as_dict() else {
        return;
    };
    if let Some(nums) = dict
        .get(b"Nums")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_array)
    {
        for pair in nums.chunks_exact(2) {
            let [key, value] = pair else {
                continue;
            };
            let Some(start) = graph
                .resolve(key)
                .as_int()
                .and_then(|k| usize::try_from(k).ok())
            else {
                continue;
            };
            let Some(label) = graph.resolve(value).as_dict() else {
                continue;
            };
            out.push((start, direct_label(graph, label)));
        }
    }
    if let Some(kids) = dict
        .get(b"Kids")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_array)
    {
        for kid in kids {
            walk(graph, kid, depth + 1, seen, nodes, out);
        }
    }
}

/// The Table 159 entries of `label`, resolved to direct scalars so the
/// dictionary can be written into another document without its objects.
fn direct_label<G: ObjectGraph + ?Sized>(graph: &G, label: &Dict) -> Dict {
    let mut out = Dict::new();
    for key in [&b"Type"[..], b"S", b"P", b"St"] {
        let Some(value) = label.get(key).map(|o| graph.resolve(o)) else {
            continue;
        };
        if matches!(
            value,
            Object::Name(_) | Object::String(_) | Object::Integer(_)
        ) {
            out.insert(Name::from(key), value.clone());
        }
    }
    out
}

/// The label range a document with no `/PageLabels` tree is displayed
/// with: decimal numbers from 1. §12.4.2 states no reader rule for pages no
/// range covers (it only requires a writer to key index 0); decimal from 1
/// is universal viewer practice, and `<< /S /D >>` reproduces it exactly
/// because `/St` defaults to 1.
fn decimal_from_one() -> Dict {
    let mut d = Dict::new();
    d.insert(Name::from(b"S"), Object::Name(Name::from(b"D")));
    d
}

/// `ranges` as a document of `count` pages displays them: a missing tree,
/// or one whose first range starts past index 0, gets a decimal range at 0
/// for the pages before it.
fn as_displayed(ranges: &[Range], count: usize) -> Vec<Range> {
    let mut out = Vec::with_capacity(ranges.len() + 1);
    if ranges.first().is_none_or(|r| r.0 > 0) && count > 0 {
        out.push((0, decimal_from_one()));
    }
    out.extend(ranges.iter().filter(|r| r.0 < count.max(1)).cloned());
    out
}

/// The ranges of a document made by inserting `source`'s `source_count`
/// pages into `target`'s `target_count` at page index `at`, such that every
/// page keeps the label it displayed in its own document: target ranges
/// before `at` are kept, the source's are offset by `at`, and target ranges
/// from `at` on are shifted by `source_count` — with the range that covered
/// `at` restarted after the inserted block at the number page `at` showed.
///
/// Empty when neither document has a tree: then there is nothing to keep,
/// and writing a tree would state what viewers already display.
pub(crate) fn splice(
    target: &[Range],
    target_count: usize,
    source: &[Range],
    source_count: usize,
    at: usize,
) -> Vec<Range> {
    if (target.is_empty() && source.is_empty()) || source_count == 0 {
        return target.to_vec();
    }
    let at = at.min(target_count);
    let target = as_displayed(target, target_count);
    let source = as_displayed(source, source_count);

    let mut out: Vec<Range> = target.iter().filter(|r| r.0 < at).cloned().collect();
    out.extend(source.iter().map(|(start, d)| (start + at, d.clone())));
    if at < target_count {
        if !target.iter().any(|r| r.0 == at)
            && let Some((start, covering)) = target.iter().rev().find(|r| r.0 < at)
        {
            // A range without `/S` is prefix-only (§12.4.2: "no numeric
            // portion"), so every page shows the same label and it resumes
            // unchanged; `/St` is only meaningful beside a style.
            let mut resumed = covering.clone();
            if covering.get(b"S").is_some() {
                let first = covering.get(b"St").and_then(Object::as_int).unwrap_or(1);
                let offset = i64::try_from(at - start).unwrap_or(i64::MAX);
                resumed.insert(
                    Name::from(b"St"),
                    Object::Integer(first.saturating_add(offset)),
                );
            }
            out.push((at + source_count, resumed));
        }
        out.extend(
            target
                .iter()
                .filter(|r| r.0 >= at)
                .map(|(start, d)| (start + source_count, d.clone())),
        );
    }
    out
}

/// The ranges that give the selected `pages` of a `source_count`-page
/// document, laid out from index 0 in the order given, the labels each
/// displayed in that document. A run of consecutive pages under one range
/// stays one range; every other page starts a range whose `/St` is the
/// number it showed.
pub(crate) fn subset(source: &[Range], source_count: usize, pages: &[usize]) -> Vec<Range> {
    let shown = as_displayed(source, source_count);
    let mut out = Vec::new();
    let mut prev: Option<(usize, usize)> = None;
    for (index, &page) in pages.iter().enumerate() {
        let Some(range) = shown.iter().rposition(|r| r.0 <= page) else {
            continue;
        };
        let Some((start, label)) = shown.get(range) else {
            continue;
        };
        let styled = label.get(b"S").is_some();
        let continues = prev.is_some_and(|(r, p)| r == range && (!styled || p + 1 == page));
        prev = Some((range, page));
        if continues {
            continue;
        }
        let mut label = label.clone();
        if styled {
            let first = label.get(b"St").and_then(Object::as_int).unwrap_or(1);
            let offset = i64::try_from(page - start).unwrap_or(i64::MAX);
            label.insert(
                Name::from(b"St"),
                Object::Integer(first.saturating_add(offset)),
            );
        }
        out.push((index, label));
    }
    out
}

/// The ranges of `target` after `count` pages are inserted at index `at`,
/// with the inserted pages joining the range that covers the page before
/// them (the first range when `at` is 0) and numbering on through it; every
/// later range keeps its labels. Empty when `target` has no tree.
pub(crate) fn continue_covering(
    target: &[Range],
    target_count: usize,
    count: usize,
    at: usize,
) -> Vec<Range> {
    if target.is_empty() || count == 0 {
        return target.to_vec();
    }
    as_displayed(target, target_count)
        .into_iter()
        .map(|(start, label)| {
            if start > at || (start == at && at > 0) {
                (start + count, label)
            } else {
                (start, label)
            }
        })
        .collect()
}

/// The ranges after inserting `pages` of `source` into `target` at `at`,
/// under `policy`. The target's own pages keep their labels either way;
/// empty when neither document has a tree.
pub(crate) fn inserted(
    target: &[Range],
    target_count: usize,
    source: &[Range],
    source_count: usize,
    pages: &[usize],
    at: usize,
    policy: crate::pageops::InsertedPageLabels,
) -> Vec<Range> {
    let at = at.min(target_count);
    match policy {
        crate::pageops::InsertedPageLabels::Source => {
            if target.is_empty() && source.is_empty() {
                return Vec::new();
            }
            let own = subset(source, source_count, pages);
            splice(target, target_count, &own, pages.len(), at)
        }
        crate::pageops::InsertedPageLabels::ContinueRange => {
            continue_covering(target, target_count, pages.len(), at)
        }
    }
}

/// The ranges of `target` after the pages at `deleted` (sorted, distinct)
/// are removed from its `target_count`, under `policy`. Empty when `target`
/// is: a document without a tree is numbered from 1 before and after.
pub(crate) fn removed(
    target: &[Range],
    target_count: usize,
    deleted: &[usize],
    policy: crate::pageops::DeletedPageLabels,
) -> Vec<Range> {
    if target.is_empty() {
        return Vec::new();
    }
    match policy {
        crate::pageops::DeletedPageLabels::Renumber => {
            let shown = as_displayed(target, target_count);
            let mut out = Vec::with_capacity(shown.len());
            for (i, (start, label)) in shown.iter().enumerate() {
                let end = shown.get(i + 1).map_or(target_count, |r| r.0);
                let gone_before = deleted.partition_point(|d| d < start);
                let gone_within = deleted.partition_point(|d| *d < end) - gone_before;
                if gone_within < end.saturating_sub(*start) {
                    out.push((start - gone_before, label.clone()));
                }
            }
            out
        }
        crate::pageops::DeletedPageLabels::KeepEach => {
            let kept: Vec<usize> = (0..target_count)
                .filter(|p| deleted.binary_search(p).is_err())
                .collect();
            subset(target, target_count, &kept)
        }
    }
}

/// A single-node `/PageLabels` tree (§7.9.7: a root carrying `Nums` alone)
/// holding `ranges`.
pub(crate) fn tree(ranges: &[Range]) -> Object {
    let mut nums = Vec::with_capacity(ranges.len() * 2);
    for (start, label) in ranges {
        nums.push(Object::Integer(i64::try_from(*start).unwrap_or(i64::MAX)));
        nums.push(Object::Dict(label.clone()));
    }
    let mut root = Dict::new();
    root.insert(Name::from(b"Nums"), Object::Array(nums));
    Object::Dict(root)
}

#[cfg(test)]
// A short result panicking on an index is the failure these tests report.
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn label(style: &[u8], prefix: Option<&[u8]>, st: Option<i64>) -> Dict {
        let mut d = Dict::new();
        if !style.is_empty() {
            d.insert(Name::from(b"S"), Object::Name(Name::from(style)));
        }
        if let Some(p) = prefix {
            d.insert(Name::from(b"P"), Object::String(p.to_vec()));
        }
        if let Some(st) = st {
            d.insert(Name::from(b"St"), Object::Integer(st));
        }
        d
    }

    fn st(r: &Range) -> Option<i64> {
        r.1.get(b"St").and_then(Object::as_int)
    }

    #[test]
    fn neither_tree_writes_nothing() {
        assert!(splice(&[], 5, &[], 3, 2).is_empty());
    }

    #[test]
    fn a_mid_insert_resumes_the_covering_range_after_the_block() {
        // Target: i, ii (roman), then 1.. from page 2; 6 pages.
        let target = vec![(0, label(b"r", None, None)), (2, label(b"D", None, None))];
        // Source: A-1.. prefix range, 3 pages.
        let source = vec![(0, label(b"D", Some(b"A-"), None))];
        let out = splice(&target, 6, &source, 3, 4);
        let starts: Vec<usize> = out.iter().map(|r| r.0).collect();
        // Target page 4 showed "3"; it now sits at 7 and must still show 3.
        assert_eq!(starts, vec![0, 2, 4, 7]);
        assert_eq!(st(&out[3]), Some(3));
        assert_eq!(out[2].1.get(b"P"), Some(&Object::String(b"A-".to_vec())));
    }

    #[test]
    fn a_source_without_a_tree_keeps_its_one_based_numbers() {
        let target = vec![(0, label(b"r", None, None))];
        let out = splice(&target, 2, &[], 3, 2);
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].0, 2);
        assert_eq!(out[1].1.get(b"S"), Some(&Object::Name(Name::from(b"D"))));
        assert_eq!(st(&out[1]), None);
    }

    #[test]
    fn a_target_without_a_tree_keeps_its_physical_numbers() {
        let source = vec![(0, label(b"R", None, None))];
        let out = splice(&[], 4, &source, 2, 1);
        let starts: Vec<usize> = out.iter().map(|r| r.0).collect();
        assert_eq!(starts, vec![0, 1, 3]);
        // Target page 1 showed "2".
        assert_eq!(st(&out[2]), Some(2));
    }

    #[test]
    fn a_range_starting_at_the_insertion_point_is_shifted_not_duplicated() {
        let target = vec![
            (0, label(b"r", None, None)),
            (2, label(b"D", None, Some(5))),
        ];
        let out = splice(&target, 4, &[], 1, 2);
        let starts: Vec<usize> = out.iter().map(|r| r.0).collect();
        assert_eq!(starts, vec![0, 2, 3]);
        assert_eq!(st(&out[2]), Some(5));
    }

    #[test]
    fn a_prefix_only_range_resumes_without_gaining_a_number() {
        let target = vec![(0, label(b"", Some(b"Cover"), None))];
        let out = splice(&target, 3, &[], 1, 1);
        assert_eq!(out.len(), 3);
        assert_eq!(out[2].0, 2);
        assert_eq!(out[2].1.get(b"S"), None, "no style may be invented");
        assert_eq!(st(&out[2]), None);
    }

    #[test]
    fn a_subset_keeps_each_page_label_and_merges_runs() {
        // Source: i..iii, then A-1.. from page 3; 6 pages. Pick 1,2,4,0.
        let source = vec![
            (0, label(b"r", None, None)),
            (3, label(b"D", Some(b"A-"), None)),
        ];
        let out = subset(&source, 6, &[1, 2, 4, 0]);
        let starts: Vec<usize> = out.iter().map(|r| r.0).collect();
        assert_eq!(starts, vec![0, 2, 3]);
        assert_eq!(st(&out[0]), Some(2), "page 1 showed ii");
        assert_eq!(st(&out[1]), Some(2), "page 4 showed A-2");
        assert_eq!(out[1].1.get(b"P"), Some(&Object::String(b"A-".to_vec())));
        assert_eq!(st(&out[2]), Some(1), "page 0 showed i");
    }

    #[test]
    fn a_subset_of_a_treeless_source_keeps_its_physical_numbers() {
        let out = subset(&[], 5, &[3, 4]);
        assert_eq!(out.len(), 1);
        assert_eq!(st(&out[0]), Some(4));
    }

    #[test]
    fn inserting_a_subset_keeps_both_documents_labels() {
        use crate::pageops::InsertedPageLabels;
        let target = vec![(0, label(b"r", None, None)), (2, label(b"D", None, None))];
        let out = inserted(&target, 6, &[], 9, &[6, 7], 4, InsertedPageLabels::Source);
        let starts: Vec<usize> = out.iter().map(|r| r.0).collect();
        assert_eq!(starts, vec![0, 2, 4, 6]);
        assert_eq!(st(&out[2]), Some(7), "source page 6 showed 7");
        assert_eq!(st(&out[3]), Some(3), "target page 4 showed 3");
    }

    #[test]
    fn continue_range_numbers_through_and_shifts_later_ranges() {
        use crate::pageops::InsertedPageLabels;
        let target = vec![(0, label(b"r", None, None)), (2, label(b"D", None, None))];
        let source = vec![(0, label(b"R", None, None))];
        let out = inserted(
            &target,
            6,
            &source,
            3,
            &[0, 1],
            2,
            InsertedPageLabels::ContinueRange,
        );
        let starts: Vec<usize> = out.iter().map(|r| r.0).collect();
        // The inserted pages join i, ii as iii, iv; the decimal range moves.
        assert_eq!(starts, vec![0, 4]);
        assert_eq!(st(&out[1]), None);
    }

    #[test]
    fn neither_tree_inserts_nothing_under_either_policy() {
        use crate::pageops::InsertedPageLabels;
        for policy in [
            InsertedPageLabels::Source,
            InsertedPageLabels::ContinueRange,
        ] {
            assert!(inserted(&[], 4, &[], 3, &[0, 2], 1, policy).is_empty());
        }
    }

    #[test]
    fn appending_at_the_end_adds_only_the_source_ranges() {
        let target = vec![(0, label(b"D", Some(b"T-"), None))];
        let source = vec![(0, label(b"a", None, None))];
        let out = splice(&target, 3, &source, 2, 3);
        let starts: Vec<usize> = out.iter().map(|r| r.0).collect();
        assert_eq!(starts, vec![0, 3]);
    }

    #[test]
    fn a_delete_renumbers_each_section_from_its_own_first_page() {
        use crate::pageops::DeletedPageLabels;
        // i ii | 1 2 3 4 | A-1 A-2 ; 8 pages.
        let target = vec![
            (0, label(b"r", None, None)),
            (2, label(b"D", None, None)),
            (6, label(b"D", Some(b"A-"), None)),
        ];
        // Delete "ii" and "2": the decimal section moves back one and runs
        // 1 2 3; the appendix moves back two.
        let out = removed(&target, 8, &[1, 3], DeletedPageLabels::Renumber);
        let starts: Vec<usize> = out.iter().map(|r| r.0).collect();
        assert_eq!(starts, vec![0, 1, 4]);
        assert!(out.iter().all(|r| st(r).is_none()));
        // Delete the whole roman section: the decimal one starts the document.
        let out = removed(&target, 8, &[0, 1], DeletedPageLabels::Renumber);
        let starts: Vec<usize> = out.iter().map(|r| r.0).collect();
        assert_eq!(starts, vec![0, 4]);
        assert_eq!(out[0].1.get(b"S"), Some(&Object::Name(Name::from(b"D"))));
    }

    #[test]
    fn keep_each_leaves_every_remaining_page_its_label() {
        use crate::pageops::DeletedPageLabels;
        let target = vec![(0, label(b"r", None, None)), (2, label(b"D", None, None))];
        // i ii 1 2 3 4, delete "2": i ii 1 | 3 4.
        let out = removed(&target, 6, &[3], DeletedPageLabels::KeepEach);
        let starts: Vec<usize> = out.iter().map(|r| r.0).collect();
        assert_eq!(starts, vec![0, 2, 3]);
        assert_eq!(st(&out[2]), Some(3));
    }

    #[test]
    fn a_treeless_delete_writes_nothing_under_either_policy() {
        use crate::pageops::DeletedPageLabels;
        for policy in [DeletedPageLabels::Renumber, DeletedPageLabels::KeepEach] {
            assert!(removed(&[], 4, &[1], policy).is_empty());
        }
    }
}
