//! Subform sets in a merge with data (XFA 3.3 Ch.9 pp.349-353; slice
//! `adobe_ext__xfa_subformset_merge.md`). A set binds nothing; its members
//! match in its parent's data scope (G-SS14). By `relation`:
//!
//! - `ordered` (the default): members in template order, each by its own
//!   `occur` (G-SS1).
//! - `unordered`: members that bind data first, ordered by the first data
//!   node each binds; the rest after them in template order (G-SS2).
//! - `choice`: only the member whose first bound node comes first walking
//!   the data breadth-first; no member when none binds (p.349, G-SS7). Its
//!   `occur@min` is ignored (p.344); see [`ChoiceInstances`] (G-SS5).
//!   Without data only the first member is kept (p.349, G-SS6).
//!
//! A member "binds" when merging it binds any data node, its descendants'
//! included (G-SS3). The set itself gets `occur@min` copies, then one more
//! per copy that binds data, up to `occur@max` (pp.341, 351-352, G-SS8);
//! each copy's `occur@initial` is set to 1.

use super::Merger;
use super::occur::{MAX_INSTANCES, set_occur};
use crate::xfa::template::XfaNode;

/// How many instances the member a `choice` subform set picks gets (gap
/// G-SS5; Ch.9 p.349 says "only one will be copied").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ChoiceInstances {
    /// As many as its data and `occur@max` allow, `occur@min` ignored:
    /// "one" counts members, not instances (p.852).
    #[default]
    Occur,
    /// Exactly one.
    One,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Relation {
    Ordered,
    Unordered,
    Choice,
}

fn relation(node: &XfaNode) -> Relation {
    match node.attr("relation") {
        Some("unordered") => Relation::Unordered,
        Some("choice") => Relation::Choice,
        _ => Relation::Ordered,
    }
}

impl Merger<'_> {
    /// Append the copies of subform set `set` to `out`, each merged in
    /// `scope`.
    pub(super) fn set_copies(
        &mut self,
        out: &mut Vec<XfaNode>,
        set: XfaNode,
        path: &mut Vec<usize>,
        scope: &[usize],
        pending: &[String],
    ) {
        let has_occur = set.child("occur").is_some();
        let (min, max) = set.occur().map_or((1, Some(1)), |o| (o.min, o.max));
        let max = max.unwrap_or(u32::MAX);
        let mut made = 0u32;
        while made < max && self.instances_made < MAX_INSTANCES {
            let undo = (made >= min).then(|| self.clone());
            let before = self.bound_nodes.len();
            let mut copy = set.clone();
            if has_occur {
                set_occur(&mut copy, "initial", 1);
            }
            self.place(out, copy, path, scope, pending);
            if let Some(undo) = undo
                && self.bound_nodes.len() == before
            {
                *self = undo;
                out.pop();
                break;
            }
            self.instances_made += 1;
            made += 1;
        }
        if made == 0 {
            let mut skipped = set;
            set_occur(&mut skipped, "initial", 0);
            out.push(skipped);
        }
    }

    /// Merge the members of set `node` (at `path`) in `scope`.
    pub(super) fn set_members(
        &mut self,
        node: &mut XfaNode,
        path: &mut Vec<usize>,
        scope: &[usize],
        pending: &[String],
    ) {
        let relation = relation(node);
        let (mut members, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut node.children)
            .into_iter()
            .partition(|c| c.container_kind().is_some());
        node.children = rest;
        match relation {
            Relation::Unordered if self.has_data => {
                let mut keyed: Vec<_> = members
                    .into_iter()
                    .map(|m| (self.first_bound(&m, path, scope, pending, false), m))
                    .collect();
                keyed.sort_by(|a, b| (a.0.is_none(), &a.0).cmp(&(b.0.is_none(), &b.0)));
                members = keyed.into_iter().map(|(_, m)| m).collect();
            }
            Relation::Choice if self.has_data => {
                members = self.choose(members, path, scope, pending);
            }
            Relation::Choice => members.truncate(1),
            Relation::Ordered | Relation::Unordered => {}
        }
        for member in members {
            self.member(&mut node.children, member, path, scope, pending);
        }
    }

    /// The chosen member of a `choice` set, its `occur@min` dropped; none
    /// when no member binds.
    fn choose(
        &self,
        members: Vec<XfaNode>,
        path: &mut Vec<usize>,
        scope: &[usize],
        pending: &[String],
    ) -> Vec<XfaNode> {
        let chosen = members
            .into_iter()
            .filter_map(|m| {
                self.first_bound(&m, path, scope, pending, true)
                    .map(|k| (k, m))
            })
            .min_by(|a, b| a.0.cmp(&b.0));
        let Some((_, mut m)) = chosen else {
            return Vec::new();
        };
        if m.child("occur").is_some() {
            set_occur(&mut m, "min", 0);
            if self.opts.choice_instances == ChoiceInstances::One {
                set_occur(&mut m, "max", 1);
            }
        }
        vec![m]
    }

    /// The first data node merging `member` would bind, in data order, or
    /// breadth-first when `breadth`; `None` when it binds nothing. Merges a
    /// throwaway copy of this merger.
    fn first_bound(
        &self,
        member: &XfaNode,
        path: &mut Vec<usize>,
        scope: &[usize],
        pending: &[String],
        breadth: bool,
    ) -> Option<(usize, Vec<usize>)> {
        let mut trial = self.clone();
        let mut out = Vec::new();
        trial.member(&mut out, member.clone(), path, scope, pending);
        trial
            .bound_nodes
            .difference(&self.bound_nodes)
            .map(|at| (if breadth { at.len() } else { 0 }, at.clone()))
            .min()
    }

    /// Merge one member of a container: repeated from data, a set's copies,
    /// or placed once.
    pub(super) fn member(
        &mut self,
        out: &mut Vec<XfaNode>,
        child: XfaNode,
        path: &mut Vec<usize>,
        scope: &[usize],
        pending: &[String],
    ) {
        if child.tag == "subformSet" && self.has_data {
            self.set_copies(out, child, path, scope, pending);
        } else if self.repeats(&child) {
            self.instances(out, child, path, scope, pending);
        } else {
            self.place(out, child, path, scope, pending);
        }
    }
}
