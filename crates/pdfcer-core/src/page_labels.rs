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
    fn appending_at_the_end_adds_only_the_source_ranges() {
        let target = vec![(0, label(b"D", Some(b"T-"), None))];
        let source = vec![(0, label(b"a", None, None))];
        let out = splice(&target, 3, &source, 2, 3);
        let starts: Vec<usize> = out.iter().map(|r| r.0).collect();
        assert_eq!(starts, vec![0, 3]);
    }
}
