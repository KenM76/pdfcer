//! Normal (`match="once"`) binding by name (XFA 3.3 Ch.4 pp.181-191;
//! `adobe_ext__xfa_data_binding_normal.md` §4-§5).
//!
//! A named container binds the first not-yet-bound data node with its name
//! and a compatible type: a subform a data group, a field or exclusion group
//! a data value. The search starts among the scope's children (direct
//! match) and, under `consumeData`, widens one generation at a time up to
//! the record (scope match, fewer generations first, p.185). It never looks
//! inside another group, so a bound group's children stay with its subform
//! (p.189). Document order decides between indistinguishable fields
//! (p.191). [`Globals`] is the `match="global"` fallback.

use std::collections::{HashMap, HashSet};

use super::data::{DataNode, node_at};

/// What a by-name search found.
pub(super) enum Found {
    /// The data node's path.
    Hit(Vec<usize>),
    /// Only same-named nodes of the wrong type, which are skipped (p.181).
    WrongType,
    /// Nothing with the name.
    Miss,
}

/// The search for one container.
pub(super) struct Search<'a> {
    pub(super) root: &'a DataNode,
    pub(super) record: &'a [usize],
    pub(super) bound: &'a HashSet<Vec<usize>>,
    /// `matchTemplate` allows direct matches only (p.172).
    pub(super) direct_only: bool,
}

impl Search<'_> {
    /// The node `name` binds to from `scope`; `want_value` for a field or
    /// exclusion group.
    pub(super) fn find(&self, scope: &[usize], name: &str, want_value: bool) -> Found {
        let mut wrong = false;
        let lowest = if self.direct_only || !scope.starts_with(self.record) {
            scope.len()
        } else {
            self.record.len()
        };
        for depth in (lowest..=scope.len()).rev() {
            let Some(level) = scope.get(..depth) else {
                continue;
            };
            let Some(group) = node_at(self.root, level) else {
                continue;
            };
            for (i, child) in group.children.iter().enumerate() {
                if child.name != name {
                    continue;
                }
                if child.group == want_value {
                    wrong = true;
                    continue;
                }
                let mut at = level.to_vec();
                at.push(i);
                if !self.bound.contains(&at) {
                    return Found::Hit(at);
                }
            }
        }
        if wrong { Found::WrongType } else { Found::Miss }
    }
}

/// The `match="global"` fallback's lookups, cached by name so many copies
/// of one global name cost one walk (pp.245-247).
#[derive(Default)]
pub(super) struct Globals {
    cache: HashMap<String, Option<Vec<usize>>>,
}

impl Globals {
    /// The data value every global field named `name` shares: the first
    /// in document order. `records_only` limits it to values outside every
    /// record (the data root's own values, p.245); otherwise the whole
    /// document is searched.
    pub(super) fn find(
        &mut self,
        root: &DataNode,
        name: &str,
        records_only: bool,
    ) -> Option<Vec<usize>> {
        if let Some(hit) = self.cache.get(name) {
            return hit.clone();
        }
        let hit = if records_only {
            root.children
                .iter()
                .position(|c| !c.group && c.name == name)
                .map(|i| vec![i])
        } else {
            first_value(root, name)
        };
        self.cache.insert(name.to_owned(), hit.clone());
        hit
    }
}

/// The first data value named `name` below `root`, depth first in
/// document order, without recursion.
fn first_value(root: &DataNode, name: &str) -> Option<Vec<usize>> {
    let mut stack: Vec<(Vec<usize>, &DataNode)> = vec![(Vec::new(), root)];
    while let Some((path, node)) = stack.pop() {
        if !node.group && node.name == name && !path.is_empty() {
            return Some(path);
        }
        for (i, child) in node.children.iter().enumerate().rev() {
            let mut p = path.clone();
            p.push(i);
            stack.push((p, child));
        }
    }
    None
}
