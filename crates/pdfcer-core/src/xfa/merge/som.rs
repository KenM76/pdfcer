//! The subset of SOM a `bind ref` uses, resolved against the Data DOM
//! (XFA 3.3 Ch.3 pp.86-121; `adobe_ext__xfa_som_data_binding.md` §§2-6,
//! gaps G-SOM1..G-SOM3).
//!
//! Supported: the `$`, `$data`, `$record`, `!data` and `xfa.datasets.data`
//! starts, unqualified names (scope-matched), `\.` escapes, and `[n]` /
//! `[*]` indexes. Refused with [`MergeWarningKind::Unsupported`]: `..`,
//! `#class`, `*` names, predicates, and relative `[+n]` / `[-n]` indexes
//! (forbidden in a qualified reference, G-SOM2).

use std::collections::HashSet;

use super::data::{DataNode, node_at};
use super::{MergeWarningKind, ScopeSearch};

/// Steps read from one reference (a pdfcer guard).
pub(super) const MAX_REF_STEPS: usize = 1024;

/// Where a reference starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Start {
    /// `$`: the current data scope.
    Scope,
    /// `$data`, `!data`, `xfa.datasets.data`: the data root.
    Data,
    /// `$record`: the top data group.
    Record,
    /// A bare name, scope-matched (p.110).
    Unqualified,
}

/// Which same-named sibling a step takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Index {
    At(usize),
    /// `[*]`: the first not yet bound (p.108).
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Step {
    pub(super) name: String,
    pub(super) index: Index,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Ref {
    pub(super) start: Start,
    pub(super) steps: Vec<Step>,
}

/// Parse `text` as a data reference.
pub(super) fn parse(text: &str) -> Result<Ref, MergeWarningKind> {
    let segments = split(text.trim())?;
    let (start, rest) = match segments.first().map(String::as_str) {
        Some("$") => (Start::Scope, 1),
        Some("$data" | "!data") => (Start::Data, 1),
        Some("$record") => (Start::Record, 1),
        Some("xfa")
            if segments.get(1).map(String::as_str) == Some("datasets")
                && segments.get(2).map(String::as_str) == Some("data") =>
        {
            (Start::Data, 3)
        }
        Some(_) => (Start::Unqualified, 0),
        None => return Err(MergeWarningKind::Unsupported),
    };
    let steps = segments
        .get(rest..)
        .unwrap_or_default()
        .iter()
        .map(|s| step(s))
        .collect::<Result<Vec<_>, _>>()?;
    if start == Start::Unqualified && steps.is_empty() {
        return Err(MergeWarningKind::Unsupported);
    }
    Ok(Ref { start, steps })
}

/// Split on unescaped dots; `\.` is a literal dot.
fn split(text: &str) -> Result<Vec<String>, MergeWarningKind> {
    let mut out = vec![String::new()];
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'.') => {
                chars.next();
                push(&mut out, '.');
            }
            '.' => {
                if out.len() >= MAX_REF_STEPS + 3 {
                    return Err(MergeWarningKind::TooLong);
                }
                out.push(String::new());
            }
            _ => push(&mut out, c),
        }
    }
    if out.iter().any(String::is_empty) {
        // `..`, a leading or trailing dot, or an empty reference.
        return Err(MergeWarningKind::Unsupported);
    }
    Ok(out)
}

fn push(out: &mut [String], c: char) {
    if let Some(last) = out.last_mut() {
        last.push(c);
    }
}

fn step(segment: &str) -> Result<Step, MergeWarningKind> {
    let (name, index) = match segment.find('[') {
        Some(open) => {
            let inner = segment
                .get(open + 1..)
                .and_then(|s| s.strip_suffix(']'))
                .ok_or(MergeWarningKind::Unsupported)?
                .trim();
            let index = if inner == "*" {
                Index::All
            } else if inner.starts_with(['+', '-']) {
                return Err(MergeWarningKind::Unsupported);
            } else {
                Index::At(inner.parse().map_err(|_| MergeWarningKind::Unsupported)?)
            };
            (segment.get(..open).unwrap_or_default(), index)
        }
        None => (segment, Index::At(0)),
    };
    let unsupported = name.is_empty()
        || name == "*"
        || name.starts_with(['#', '$', '!'])
        || name.contains(['(', ')', '[', ']']);
    if unsupported {
        return Err(MergeWarningKind::Unsupported);
    }
    Ok(Step {
        name: name.to_owned(),
        index,
    })
}

/// What resolution needs besides the reference.
pub(super) struct Context<'a> {
    pub(super) root: &'a DataNode,
    /// The path of the current data scope (`$`).
    pub(super) scope: &'a [usize],
    /// The path of `$record`.
    pub(super) record: &'a [usize],
    pub(super) bound: &'a HashSet<Vec<usize>>,
    pub(super) search: ScopeSearch,
}

/// The path of the data node `r` names, or `None`.
pub(super) fn resolve(r: &Ref, cx: &Context<'_>) -> Option<Vec<usize>> {
    match r.start {
        Start::Scope => walk(cx, cx.scope.to_vec(), &r.steps),
        Start::Data => walk(cx, Vec::new(), &r.steps),
        Start::Record => walk(cx, cx.record.to_vec(), &r.steps),
        Start::Unqualified => {
            let mut from = cx.scope.to_vec();
            loop {
                // No backtracking once the first step matches (G-SOM3).
                if first_match(cx, &from, &r.steps).is_some() {
                    return walk(cx, from, &r.steps);
                }
                if cx.search == ScopeSearch::Children || from.pop().is_none() {
                    return None;
                }
            }
        }
    }
}

fn first_match(cx: &Context<'_>, from: &[usize], steps: &[Step]) -> Option<usize> {
    let first = steps.first()?;
    pick(cx, from, first, steps.len() == 1)
}

fn walk(cx: &Context<'_>, mut path: Vec<usize>, steps: &[Step]) -> Option<Vec<usize>> {
    for (i, s) in steps.iter().enumerate() {
        let child = pick(cx, &path, s, i + 1 == steps.len())?;
        path.push(child);
    }
    Some(path)
}

/// The index of the child of the node at `path` that `step` names.
fn pick(cx: &Context<'_>, path: &[usize], step: &Step, last: bool) -> Option<usize> {
    let node = node_at(cx.root, path)?;
    let mut named = node
        .children
        .iter()
        .enumerate()
        .filter(|(_, c)| c.name == step.name)
        .map(|(i, _)| i);
    match step.index {
        Index::At(n) => named.nth(n),
        Index::All if last => {
            let mut probe = path.to_vec();
            named.find(|i| {
                probe.push(*i);
                let free = !cx.bound.contains(&probe);
                probe.pop();
                free
            })
        }
        Index::All => named.next(),
    }
}
