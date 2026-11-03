//! Instances from data: a subform with an `occur` element gets one instance
//! per data group it matches, up to `occur@max`, and at least `occur@min`
//! (XFA 3.3 Ch.9 pp.339-347). Matching is greedy (p.346): each instance
//! takes the next unbound group its name finds, scope matches included. A
//! subform bound by `dataRef` re-resolves its reference per instance, so a
//! `[*]` step takes the next unbound match (p.108, p.349); a reference that
//! names one node yields one instance beyond `occur@min`.
//!
//! Instances replace the template subform in its parent: instance `k` sits
//! at the first instance's child index plus `k`, and later siblings shift
//! by the number of extra instances. Each instance's `occur@initial` is set
//! to 1 so layout lays it once. A subform with no instance stays in place
//! with `occur@initial` 0, unmerged, so layout lays nothing for it.

use std::collections::HashSet;

use super::normal::{Found, Search};
use super::{MergeWarning, MergeWarningKind, Merger};
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
            && (self
                .bind_name(child, Some(ContainerKind::Subform))
                .is_some()
                || self.data_ref(child).is_some())
    }

    /// What one more instance would bind: by name, the next
    /// unbound group `name` finds; by `dataRef`, the group its reference
    /// resolves to, unless already bound (a `[*]` step takes the first
    /// unbound match, p.108).
    fn next_hit(
        &self,
        (reference, name): (Option<&str>, &str),
        scope: &[usize],
        bound: &HashSet<Vec<usize>>,
    ) -> Option<Vec<usize>> {
        match reference {
            Some(reference) => self
                .lookup_in(reference, scope, false, bound)
                .ok()
                .filter(|at| !bound.contains(at)),
            None => {
                let search = Search {
                    root: self.root,
                    record: self.record,
                    bound,
                    direct_only: self.direct_only,
                };
                match search.find(scope, name, false) {
                    Found::Hit(at) => Some(at),
                    _ => None,
                }
            }
        }
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
        let reference = self.data_ref(&child);
        let target = (reference.as_deref(), name.as_str());
        let Some(occur) = child.occur() else {
            self.place(out, child, path, scope, pending);
            return;
        };
        let max = occur.max.unwrap_or(u32::MAX);
        let mut made = 0u32;
        while made < max {
            let hit = self.next_hit(target, scope, &self.bound_nodes).is_some();
            if (!hit && made >= occur.min) || self.instances_made >= MAX_INSTANCES {
                if self.instances_made >= MAX_INSTANCES {
                    self.warn_target(path, MergeWarningKind::InstanceLimit, target);
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
            // A reference that cannot resolve at all says why, not silence.
            if let Some(r) = target.0
                && let Err(kind) = self.lookup(r, scope, false)
                && kind != MergeWarningKind::Unresolved
            {
                path.push(out.len());
                self.warn_target(path, kind, target);
                path.pop();
            }
            let mut skipped = child;
            set_initial(&mut skipped, 0);
            out.push(skipped);
        }
        path.push(out.len().saturating_sub(1));
        for _ in 0..self.excess(target, scope) {
            self.warn_target(path, MergeWarningKind::ExcessData, target);
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

    /// Warn about the subform `target` names: its reference when bound
    /// by `dataRef`, else its name.
    fn warn_target(
        &mut self,
        path: &[usize],
        kind: MergeWarningKind,
        target: (Option<&str>, &str),
    ) {
        self.warnings.push(MergeWarning {
            path: path.to_vec(),
            kind,
            reference: target.0.unwrap_or(target.1).to_owned(),
            by_name: target.0.is_none(),
        });
    }

    /// How many more groups `target` matches from `scope` once its
    /// instances are spent: data the form leaves unbound.
    fn excess(&self, target: (Option<&str>, &str), scope: &[usize]) -> usize {
        let mut bound = self.bound_nodes.clone();
        let mut n = 0;
        while n < MAX_INSTANCES {
            let Some(at) = self.next_hit(target, scope, &bound) else {
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
