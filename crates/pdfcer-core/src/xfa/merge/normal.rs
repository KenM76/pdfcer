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
//! (p.191).

use std::collections::HashSet;

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
