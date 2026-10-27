//! Running several verbs as one undo entry, folded as each command commits.
//!
//! Folding after the fact (`coalesce_last`) counts entries off the stack, so
//! it cannot tell this gesture's commands from the oldest history once the
//! depth bound starts evicting. Folding inside `commit`, before the bound is
//! applied, keeps the gesture at one entry throughout: a gesture of any
//! length evicts at most one old entry, the same as a single verb.

use super::checkpoint::Entry;
use super::{CommandKind, EditSession};

/// The gesture in progress: commands with a serial above `mark` are its own.
#[derive(Debug, Clone, Copy)]
pub(super) struct Group {
    mark: u64,
    kind: CommandKind,
}

impl EditSession {
    /// Run `f`, folding every command it commits into ONE undo entry labelled
    /// `kind`. Nothing committed leaves the stack unchanged. A nested call
    /// joins the outer group.
    pub(crate) fn grouped<R>(&mut self, kind: CommandKind, f: impl FnOnce(&mut Self) -> R) -> R {
        if self.undo_group.is_some() {
            return f(self);
        }
        let mark = self.undo.last().map_or(0, |e: &Entry| e.serial);
        self.undo_group = Some(Group { mark, kind });
        let out = f(self);
        self.undo_group = None;
        out
    }

    /// The serial of the newest undo entry, 0 when there is none. Serials
    /// only grow, so a change across a call means it committed something.
    pub(crate) fn undo_top_serial(&self) -> u64 {
        self.undo.last().map_or(0, |e: &Entry| e.serial)
    }

    /// Called by `commit` right after a push: merge the new top entry into
    /// the group's entry beneath it, or relabel it when it is the first.
    pub(super) fn fold_into_group(&mut self) {
        let Some(Group { mark, kind }) = self.undo_group else {
            return;
        };
        let own = self
            .undo
            .iter()
            .rev()
            .take_while(|e| e.serial > mark)
            .count();
        let _ = self.coalesce_top(own, kind);
    }

    /// Inside a group every command is already folded into one entry, so a
    /// verb's own fold must not run: it counts entries off the stack and
    /// would merge older history into the group.
    pub(super) fn in_undo_group(&self) -> bool {
        self.undo_group.is_some()
    }
}
