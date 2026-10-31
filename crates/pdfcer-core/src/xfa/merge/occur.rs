//! Instances from data: a subform with an `occur` element gets one instance
//! per data group it matches, up to `occur@max`, and at least `occur@min`
//! (XFA 3.3 Ch.9 pp.339-347). Matching is greedy (p.346): each instance
//! takes the next unbound group its name finds, scope matches included.
//!
//! Instances replace the template subform in its parent: instance `k` sits
//! at the first instance's child index plus `k`, and later siblings shift
//! by the number of extra instances. Each instance's `occur@initial` is set
//! to 1 so layout lays it once. A subform with no instance stays in place
//! with `occur@initial` 0, unmerged, so layout lays nothing for it.

use std::collections::HashSet;

use super::normal::{Found, Search};
use super::{MergeWarningKind, Merger};
use crate::xfa::template::{ContainerKind, XfaNode};

/// The most subform instances one merge creates, nested ones included.
pub(super) const MAX_INSTANCES: usize = 100_000;

impl Merger<'_> {
    /// Whether `child` repeats from data: a by-name subform with an
    /// explicit `occur` element, merged against data.
    pub(super) fn repeats(&self, child: &XfaNode) -> bool {
        self.has_data
            && child.tag == "subform"
            && child.child("occur").is_some()
            && self
                .bind_name(child, Some(ContainerKind::Subform))
                .is_some()
    }

    /// Append `child`'s instances to `out`, each merged in `scope`.
    pub(super) fn instances(
        &mut self,
        out: &mut Vec<XfaNode>,
        child: XfaNode,
        path: &mut Vec<usize>,
        scope: &[usize],
        pending: &[String],
    ) {
        let name = self
            .bind_name(&child, Some(ContainerKind::Subform))
            .map(|(n, _)| n)
            .unwrap_or_default();
        let Some(occur) = child.occur() else {
            self.place(out, child, path, scope, pending);
            return;
        };
        let max = occur.max.unwrap_or(u32::MAX);
        let mut made = 0u32;
        while made < max {
            let hit = self.peek(scope, &name, &self.bound_nodes);
            if (!hit && made >= occur.min) || self.instances_made >= MAX_INSTANCES {
                if self.instances_made >= MAX_INSTANCES {
                    self.warn(path, MergeWarningKind::InstanceLimit, &name);
                }
                break;
            }
            let mut copy = child.clone();
            set_initial(&mut copy, 1);
            self.place(out, copy, path, scope, pending);
            self.instances_made += 1;
            made += 1;
        }
        if made == 0 {
            let mut skipped = child;
            set_initial(&mut skipped, 0);
            out.push(skipped);
        }
        path.push(out.len().saturating_sub(1));
        for _ in 0..self.excess(scope, &name) {
            self.warn(path, MergeWarningKind::ExcessData, &name);
        }
        path.pop();
    }

    /// Push `child` onto `out` and merge it there.
    pub(super) fn place(
        &mut self,
        out: &mut Vec<XfaNode>,
        child: XfaNode,
        path: &mut Vec<usize>,
        scope: &[usize],
        pending: &[String],
    ) {
        out.push(child);
        path.push(out.len() - 1);
        if let Some(node) = out.last_mut() {
            self.container(node, path, scope, pending);
        }
        path.pop();
    }

    /// Whether a subform named `name` would bind a data group from `scope`.
    fn peek(&self, scope: &[usize], name: &str, bound: &HashSet<Vec<usize>>) -> bool {
        let search = Search {
            root: self.root,
            record: self.record,
            bound,
            direct_only: self.direct_only,
        };
        matches!(search.find(scope, name, false), Found::Hit(_))
    }

    /// How many more groups `name` matches from `scope` once its instances
    /// are spent: data the form leaves unbound.
    fn excess(&self, scope: &[usize], name: &str) -> usize {
        let mut bound = self.bound_nodes.clone();
        let mut n = 0;
        while n < MAX_INSTANCES {
            let search = Search {
                root: self.root,
                record: self.record,
                bound: &bound,
                direct_only: self.direct_only,
            };
            let Found::Hit(at) = search.find(scope, name, false) else {
                break;
            };
            bound.insert(at);
            n += 1;
        }
        n
    }
}

/// Set `node`'s `occur@initial`, adding the attribute if it is absent.
fn set_initial(node: &mut XfaNode, n: u32) {
    let Some(occur) = node.children.iter_mut().find(|c| c.tag == "occur") else {
        return;
    };
    let value = n.to_string();
    match occur.attributes.iter_mut().find(|(k, _)| k == "initial") {
        Some((_, v)) => *v = value,
        None => occur.attributes.push(("initial".to_owned(), value)),
    }
}
