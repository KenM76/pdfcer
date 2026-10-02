//! Abandoning a gesture of several verbs: [`EditSession::checkpoint`] and
//! [`EditSession::rollback`].
//!
//! Every command on the undo and redo stacks carries a serial drawn from one
//! process-wide counter, so a checkpoint finds its place in the history by
//! identity rather than by depth. Depth alone cannot tell a command pushed
//! after the checkpoint from one the depth bound evicted, nor notice a fold
//! or an undo that crossed it.

use std::collections::VecDeque;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{Command, EditSession};

/// How many commands the depth bound evicted most recently are kept, so a
/// rollback can put them back. A gesture longer than this, run at full
/// depth, loses the excess from the bottom of the history (reported in
/// [`Rollback::history_lost`]); the document itself is always restored.
pub(super) const EVICTED_TAIL: usize = 16;

static SERIAL: AtomicU64 = AtomicU64::new(0);

/// A fresh serial, greater than every serial issued before it.
pub(super) fn next_serial() -> u64 {
    SERIAL.fetch_add(1, Ordering::Relaxed) + 1
}

fn current_serial() -> u64 {
    SERIAL.load(Ordering::Relaxed)
}

/// A command on the undo or redo stack.
#[derive(Debug, Clone)]
pub(super) struct Entry {
    pub(super) serial: u64,
    pub(super) command: Command,
}

impl Entry {
    /// Wrap `command` with a fresh serial, as it goes onto the undo stack.
    pub(super) fn new(command: Command) -> Self {
        Self {
            serial: next_serial(),
            command,
        }
    }
}

impl Deref for Entry {
    type Target = Command;
    fn deref(&self) -> &Command {
        &self.command
    }
}

impl DerefMut for Entry {
    fn deref_mut(&mut self) -> &mut Command {
        &mut self.command
    }
}

/// The session's history at one moment, for [`EditSession::rollback`].
///
/// Holds a copy of the redo stack, so it costs what that stack costs. It
/// belongs to the session that took it.
#[derive(Debug, Clone)]
#[must_use = "a checkpoint does nothing until it is passed to rollback"]
pub struct Checkpoint {
    session: u64,
    top: Option<u64>,
    mark: u64,
    depth: usize,
    evictions: u64,
    redo: Vec<Entry>,
}

/// What [`EditSession::rollback`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Rollback {
    /// Commands undone: those pushed since the checkpoint, after any folding.
    pub undone: usize,
    /// Commands from before the checkpoint that the undo-depth bound
    /// evicted during the gesture and could not be put back. The document
    /// is restored regardless; only that much of the oldest undo history is
    /// gone. Zero unless the gesture ran at full depth for more than
    /// sixteen commands.
    pub history_lost: usize,
}

/// Why [`EditSession::rollback`] refused. A refusal changes nothing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CheckpointError {
    /// The checkpoint was taken on another session.
    #[error("the checkpoint was taken on another edit session")]
    ForeignSession,
    /// The history below the checkpoint changed after it was taken: a
    /// command from before it was undone, redone over, or folded together
    /// with a later one.
    #[error(
        "the edit history before the checkpoint changed after it was taken \
         (an undo, redo or fold crossed it), so it cannot be restored"
    )]
    HistoryChanged,
    /// More commands were pushed after the checkpoint than the undo-depth
    /// bound holds, so the oldest of them can no longer be undone.
    #[error(
        "{evicted} command(s) pushed after the checkpoint were evicted by the \
         undo-depth bound and can no longer be undone"
    )]
    GestureEvicted {
        /// How many post-checkpoint commands were evicted.
        evicted: usize,
    },
}

impl EditSession {
    /// Record the history as it is now, so a gesture of several verbs that
    /// fails part-way can be abandoned with [`Self::rollback`].
    ///
    /// ```
    /// # use pdfcer_core::edit::EditSession;
    /// # fn gesture(session: &mut EditSession, step: impl Fn(&mut EditSession) -> Result<(), String>) {
    /// let before = session.checkpoint();
    /// if step(session).and_then(|()| step(session)).is_err() {
    ///     session.rollback(before).expect("nothing else touched the history");
    /// }
    /// # }
    /// ```
    pub fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            session: self.session_serial,
            top: self.undo.last().map(|e| e.serial),
            mark: current_serial(),
            depth: self.undo.len(),
            evictions: self.evictions,
            redo: self.redo.clone(),
        }
    }

    /// Return the document, the undo stack and the redo stack to exactly
    /// what they were at `checkpoint`.
    ///
    /// The commands pushed since are undone **without** going onto the redo
    /// stack, so Redo cannot re-apply part of an abandoned gesture, and the
    /// redo stack the first of those commands cleared comes back. Commands
    /// the gesture folded with [`Self::coalesce_last`] are undone as one.
    ///
    /// # Errors
    ///
    /// Refused, with nothing changed, when the checkpoint belongs to another
    /// session ([`CheckpointError::ForeignSession`]), when history from
    /// before it was undone or folded ([`CheckpointError::HistoryChanged`]),
    /// or when the gesture pushed more commands than the undo-depth bound
    /// keeps ([`CheckpointError::GestureEvicted`]).
    pub fn rollback(&mut self, checkpoint: Checkpoint) -> Result<Rollback, CheckpointError> {
        if checkpoint.session != self.session_serial {
            return Err(CheckpointError::ForeignSession);
        }
        let evicted = usize::try_from(self.evictions.saturating_sub(checkpoint.evictions))
            .unwrap_or(usize::MAX);
        if evicted > checkpoint.depth {
            return Err(CheckpointError::GestureEvicted {
                evicted: evicted - checkpoint.depth,
            });
        }
        let newer = self.pushed_since(&checkpoint);
        let below = self.undo.len() - newer;
        let top_now = below
            .checked_sub(1)
            .and_then(|i| self.undo.get(i))
            .map(|e| e.serial);
        let expected = if evicted >= checkpoint.depth {
            None
        } else {
            checkpoint.top
        };
        if top_now != expected {
            return Err(CheckpointError::HistoryChanged);
        }
        for _ in 0..newer {
            self.revert_last();
        }
        self.redo = checkpoint.redo;
        let restored = self.restore_evicted(evicted);
        Ok(Rollback {
            undone: newer,
            history_lost: evicted - restored,
        })
    }

    /// How many commands on the undo stack were pushed after `checkpoint`.
    pub(super) fn pushed_since(&self, checkpoint: &Checkpoint) -> usize {
        self.undo
            .iter()
            .rev()
            .take_while(|e| e.serial > checkpoint.mark)
            .count()
    }

    /// Undo a verb's own partial work after it failed. When [`Self::rollback`]
    /// refuses (a gesture of more than [`super::MAX_UNDO_DEPTH`] commands),
    /// everything still undoable from the gesture is reverted instead.
    pub(super) fn abandon(&mut self, checkpoint: Checkpoint) {
        if let Err(err) = self.rollback(checkpoint.clone()) {
            debug_assert!(
                matches!(err, CheckpointError::GestureEvicted { .. }),
                "a verb's own rollback refused: {err}"
            );
            for _ in 0..self.pushed_since(&checkpoint) {
                self.revert_last();
            }
            self.redo = checkpoint.redo;
        }
    }

    /// Record a command the depth bound dropped from the bottom of the undo
    /// stack.
    pub(super) fn note_eviction(&mut self, entry: Entry) {
        self.evictions += 1;
        self.evicted_tail.push_back(entry);
        if self.evicted_tail.len() > EVICTED_TAIL {
            self.evicted_tail.pop_front();
        }
    }

    /// Put back up to `count` of the most recently evicted commands at the
    /// bottom of the undo stack, oldest first; returns how many.
    fn restore_evicted(&mut self, count: usize) -> usize {
        let k = count.min(self.evicted_tail.len());
        let back: VecDeque<Entry> = self.evicted_tail.split_off(self.evicted_tail.len() - k);
        self.evictions -= k as u64;
        let rest = std::mem::take(&mut self.undo);
        self.undo = back.into_iter().chain(rest).collect();
        k
    }
}
