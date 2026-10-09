//! Folding a run of same-kind commands into one undo entry.

use super::{CommandKind, EditSession};

impl EditSession {
    /// [`Self::coalesce_last`], but only when each of the last `count` undo
    /// entries is already `kind`: a run of one verb repeated, such as one
    /// [`Self::deskew_image`] per page, folded into one undo.
    ///
    /// Returns `false` and leaves the undo stack unchanged when the stack
    /// holds fewer than `count` entries or any of the last `count` is
    /// another kind — an edit the operator made mid-run is never swallowed.
    /// `false` means every change was applied and only the grouping was
    /// declined; disclose that the run takes more than one undo.
    /// `count == 0` returns `true`.
    pub fn coalesce_last_same(&mut self, count: usize, kind: CommandKind) -> bool {
        if self.undo.len() < count || self.undo_kinds().take(count).any(|k| k != kind) {
            return false;
        }
        self.coalesce_last(count, kind)
    }
}
