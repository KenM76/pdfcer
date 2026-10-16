//! Resize or rotate several annotations as ONE undo entry, keeping what
//! applied when some members refuse.
//!
//! Unlike the all-or-nothing verbs in `batch`, a selection scaled or turned
//! together is a gesture the operator already made: a fixed-size marker or a
//! foreign appearance in it should not cancel the rest. Each member runs the
//! single-target verb; the commands that applied fold into one entry and
//! every member gets its own outcome.

use super::{
    AnnotationResize, AnnotationRotate, CommandKind, EditError, EditSession, MAX_UNDO_DEPTH,
    ResizeOptions,
};
use crate::object::ObjId;

/// Per-member outcome of [`EditSession::resize_annotations`] or
/// [`EditSession::rotate_annotations`] (and their previews).
///
/// `results` is in input order with repeated ids removed (the first
/// occurrence is kept), so each annotation is transformed at most once.
/// A refused member left its annotation unchanged; the applied members are
/// one undo entry.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AnnotationBatchOutcome<R> {
    /// One `(id, outcome)` per distinct id, in input order.
    pub results: Vec<(ObjId, Result<R, EditError>)>,
}

impl<R> AnnotationBatchOutcome<R> {
    /// How many members applied.
    #[must_use]
    pub fn applied(&self) -> usize {
        self.results.iter().filter(|(_, r)| r.is_ok()).count()
    }

    /// The members that refused, with the refusal, in input order.
    pub fn refused(&self) -> impl Iterator<Item = (ObjId, &EditError)> {
        self.results
            .iter()
            .filter_map(|(id, r)| r.as_ref().err().map(|e| (*id, e)))
    }
}

impl EditSession {
    /// **Scale several annotations** about one `anchor` by `(sx, sy)` as ONE
    /// undo entry: the multi-selection form of [`Self::resize_annotation`],
    /// with that verb's rules for each member.
    ///
    /// Each target carries its own [`ResizeOptions`] (a stamp's options
    /// differ from a square's). Every member is tried. A refusal (a
    /// fixed-size marker, a foreign appearance a non-uniform scale would
    /// distort) leaves that annotation unchanged and the others applied; see
    /// [`AnnotationBatchOutcome::refused`]. When nothing applies, nothing is
    /// pushed.
    ///
    /// # Errors
    ///
    /// Only [`EditError::SelectionTooLargeForOneUndo`], for more than
    /// [`MAX_UNDO_DEPTH`] distinct ids, before anything runs. Member
    /// refusals are in the outcome, not here.
    pub fn resize_annotations(
        &mut self,
        targets: &[(ObjId, ResizeOptions)],
        anchor: (f64, f64),
        sx: f64,
        sy: f64,
    ) -> Result<AnnotationBatchOutcome<AnnotationResize>, EditError> {
        let targets = distinct(targets, |t| t.0);
        self.each_kept(&targets, CommandKind::ResizeAnnotation, |s, &(id, opts)| {
            (id, s.resize_annotation(id, anchor, sx, sy, &opts))
        })
    }

    /// **Rotate several annotations** about one `pivot` by `degrees`
    /// (counter-clockwise, default user space) as ONE undo entry: the
    /// multi-selection form of [`Self::rotate_annotation`], with that verb's
    /// rules for each member. Partial results as for
    /// [`Self::resize_annotations`].
    ///
    /// # Errors
    ///
    /// Only [`EditError::SelectionTooLargeForOneUndo`], before anything runs.
    pub fn rotate_annotations(
        &mut self,
        ids: &[ObjId],
        pivot: (f64, f64),
        degrees: f64,
    ) -> Result<AnnotationBatchOutcome<AnnotationRotate>, EditError> {
        let ids = distinct(ids, |&id| id);
        self.each_kept(&ids, CommandKind::RotateAnnotation, |s, &id| {
            (id, s.rotate_annotation(id, pivot, degrees))
        })
    }

    /// What [`Self::resize_annotations`] would report for these arguments,
    /// with the document, undo and redo left exactly as they were: the
    /// batch runs and is rolled back. A shell calls it once before a drag
    /// commits to learn which members would refuse. It costs a full run.
    ///
    /// # Errors
    ///
    /// As [`Self::resize_annotations`].
    pub fn resize_annotations_preview(
        &mut self,
        targets: &[(ObjId, ResizeOptions)],
        anchor: (f64, f64),
        sx: f64,
        sy: f64,
    ) -> Result<AnnotationBatchOutcome<AnnotationResize>, EditError> {
        let before = self.checkpoint();
        let out = self.resize_annotations(targets, anchor, sx, sy);
        self.abandon(before);
        out
    }

    /// What [`Self::rotate_annotations`] would report, with nothing changed;
    /// see [`Self::resize_annotations_preview`].
    ///
    /// # Errors
    ///
    /// As [`Self::rotate_annotations`].
    pub fn rotate_annotations_preview(
        &mut self,
        ids: &[ObjId],
        pivot: (f64, f64),
        degrees: f64,
    ) -> Result<AnnotationBatchOutcome<AnnotationRotate>, EditError> {
        let before = self.checkpoint();
        let out = self.rotate_annotations(ids, pivot, degrees);
        self.abandon(before);
        out
    }

    /// Run `step` per target, keep what applied, fold it into one entry.
    /// A member that fails after pushing is reverted on its own.
    fn each_kept<T, R>(
        &mut self,
        targets: &[T],
        kind: CommandKind,
        mut step: impl FnMut(&mut Self, &T) -> (ObjId, Result<R, EditError>),
    ) -> Result<AnnotationBatchOutcome<R>, EditError> {
        if targets.len() > MAX_UNDO_DEPTH {
            return Err(EditError::SelectionTooLargeForOneUndo {
                targets: targets.len(),
                limit: MAX_UNDO_DEPTH,
            });
        }
        let start = self.checkpoint();
        let mut results = Vec::with_capacity(targets.len());
        for target in targets {
            let member = self.checkpoint();
            let (id, r) = step(self, target);
            if r.is_err() && self.pushed_since(&member) > 0 {
                self.abandon(member);
            }
            results.push((id, r));
        }
        let pushed = self.pushed_since(&start);
        if pushed > 1 {
            let _ = self.coalesce_last(pushed, kind);
        }
        Ok(AnnotationBatchOutcome { results })
    }
}

/// `items` with later repeats of an id dropped, order kept.
fn distinct<T: Clone>(items: &[T], id: impl Fn(&T) -> ObjId) -> Vec<T> {
    let mut seen = std::collections::BTreeSet::new();
    items
        .iter()
        .filter(|t| seen.insert(id(t)))
        .cloned()
        .collect()
}
