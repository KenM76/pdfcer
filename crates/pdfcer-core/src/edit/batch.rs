//! Batch verbs: several widgets or annotations moved or deleted as ONE undo
//! entry, all-or-nothing.
//!
//! Each verb runs the single-target verb once per target after a
//! [`EditSession::checkpoint`]. A refusal part-way abandons the targets
//! already done (nothing reaches the redo stack) and returns that refusal;
//! success folds every command pushed into one. A batch larger than
//! [`MAX_UNDO_DEPTH`] could not fold, so it is refused before anything runs.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    AnnotationDeletion, AnnotationMove, CommandKind, EditError, EditSession, FieldDeletion,
    MAX_UNDO_DEPTH, WidgetMove,
};
use crate::annot::AnnotFlags;
use crate::dimension::DimensionId;
use crate::object::{ObjId, Object};
use crate::vector::Point;

/// What [`EditSession::move_annotations_each`] did to one target.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum AnnotationBatchMove {
    /// A markup, link or other annotation, moved by
    /// [`EditSession::move_annotation`]'s rules.
    Annotation(AnnotationMove),
    /// A ce dimension, re-placed by the drag rule: its label anchor moved by
    /// the delta and [`EditSession::place_dimension`] given the placement that
    /// point implies. What it measures does not move.
    Dimension {
        /// The ce dimension the annotation draws.
        dimension: DimensionId,
        /// The `(offset, text_along)` passed to `place_dimension`.
        placement: (f64, f64),
    },
}

impl EditSession {
    /// Run `step` once per target as one undo entry labelled `kind`, or not
    /// at all: on the first refusal everything `step` did is undone and
    /// that refusal returned.
    fn all_or_nothing<T, R>(
        &mut self,
        targets: &[T],
        kind: CommandKind,
        mut step: impl FnMut(&mut Self, &T) -> Result<Option<R>, EditError>,
    ) -> Result<Vec<R>, EditError> {
        if targets.len() > MAX_UNDO_DEPTH {
            return Err(EditError::SelectionTooLargeForOneUndo {
                targets: targets.len(),
                limit: MAX_UNDO_DEPTH,
            });
        }
        let before = self.checkpoint();
        let mut done = Vec::with_capacity(targets.len());
        for target in targets {
            match step(self, target) {
                Ok(Some(r)) => done.push(r),
                Ok(None) => {}
                Err(e) => {
                    self.abandon(before);
                    return Err(e);
                }
            }
        }
        let pushed = self.pushed_since(&before);
        let _ = self.coalesce_last(pushed, kind);
        Ok(done)
    }

    /// **Move several form-field widgets**, each by its own page-space
    /// `(dx, dy)`, as ONE undo entry — the widget counterpart of
    /// [`Self::move_objects_each`].
    ///
    /// Each entry is `(fqn, index, dx, dy)` with [`Self::move_widget`]'s
    /// meaning. Results are in input order. A widget named twice moves by
    /// the sum of its deltas.
    ///
    /// [`WidgetMove::siblings_left_behind`] counts only widgets of the field
    /// that are **not** in this batch, so moving every box of a field
    /// together discloses none left behind.
    ///
    /// # Errors
    ///
    /// Any [`Self::move_widget`] refusal, for the first target that refuses;
    /// nothing is moved. [`EditError::SelectionTooLargeForOneUndo`] for more
    /// than [`MAX_UNDO_DEPTH`] entries.
    pub fn move_widgets_each(
        &mut self,
        moves: &[(&str, usize, f64, f64)],
    ) -> Result<Vec<WidgetMove>, EditError> {
        let mut moved: BTreeMap<&str, BTreeSet<usize>> = BTreeMap::new();
        for &(fqn, index, _, _) in moves {
            moved.entry(fqn).or_default().insert(index);
        }
        self.all_or_nothing(
            moves,
            CommandKind::MoveWidget,
            |s, &(fqn, index, dx, dy)| {
                let mut m = s.move_widget(fqn, index, dx, dy)?;
                let also_moving = moved.get(fqn).map_or(0, |set| set.len() - 1);
                m.siblings_left_behind = m.siblings_left_behind.saturating_sub(also_moving);
                Ok(Some(m))
            },
        )
    }

    /// **Move several annotations**, each by its own page-space `(dx, dy)`,
    /// as ONE undo entry.
    ///
    /// A ce dimension is re-placed by the drag rule (see
    /// [`AnnotationBatchMove::Dimension`]), so it stays on what it measures;
    /// every other annotation goes through [`Self::move_annotation`].
    /// Results are in input order.
    ///
    /// # Errors
    ///
    /// The first target's refusal, with nothing moved:
    /// [`EditError::AnnotationLocked`] for a locked annotation of either
    /// kind (Table 165 bit 8), any [`Self::move_annotation`] or
    /// [`Self::place_dimension`] refusal, and
    /// [`EditError::DimensionNotFound`] for a degenerate ce dimension with
    /// no label anchor. [`EditError::SelectionTooLargeForOneUndo`] for more
    /// than [`MAX_UNDO_DEPTH`] entries.
    pub fn move_annotations_each(
        &mut self,
        moves: &[(ObjId, f64, f64)],
    ) -> Result<Vec<AnnotationBatchMove>, EditError> {
        self.all_or_nothing(moves, CommandKind::MoveAnnotation, |s, &(id, dx, dy)| {
            let Some(dimension) = s.dimension_for_annotation(id) else {
                return s
                    .move_annotation(id, dx, dy)
                    .map(|m| Some(AnnotationBatchMove::Annotation(m)));
            };
            let model = s.read_dimension_model();
            let placement = model
                .dimension(dimension)
                .and_then(|r| {
                    let at = r.kind.label_anchor()?;
                    r.kind
                        .placement_from_point(Point::new(at.x + dx, at.y + dy))
                })
                .ok_or(EditError::DimensionNotFound { id: dimension.0 })?;
            s.place_dimension(dimension, placement.0, placement.1)?;
            Ok(Some(AnnotationBatchMove::Dimension {
                dimension,
                placement,
            }))
        })
    }

    /// **Delete several annotations** as ONE undo entry, all-or-nothing.
    ///
    /// Every id is pre-flighted with [`Self::annotation_deletion_preview`]
    /// before anything is deleted. An id a cascade already took (a pop-up
    /// whose parent went first, or an id named twice) is skipped, so the
    /// result holds one [`AnnotationDeletion`] per annotation this call
    /// deleted **by name**, in input order; a cascaded one is reported by
    /// the entry that took it (for example
    /// [`AnnotationDeletion::popup_removed`]).
    ///
    /// # Errors
    ///
    /// Any [`Self::delete_annotation`] refusal, for the first id that
    /// refuses; nothing is deleted. [`EditError::SelectionTooLargeForOneUndo`]
    /// for more than [`MAX_UNDO_DEPTH`] ids.
    pub fn delete_annotations(
        &mut self,
        ids: &[ObjId],
    ) -> Result<Vec<AnnotationDeletion>, EditError> {
        if ids.len() > MAX_UNDO_DEPTH {
            return Err(EditError::SelectionTooLargeForOneUndo {
                targets: ids.len(),
                limit: MAX_UNDO_DEPTH,
            });
        }
        for &id in ids {
            self.annotation_deletion_preview(id)?;
        }
        self.all_or_nothing(ids, CommandKind::DeleteAnnotation, |s, &id| {
            if s.value(id).is_none() {
                return Ok(None);
            }
            s.delete_annotation(id).map(Some)
        })
    }

    /// **Delete several form-field boxes**, across fields, as ONE undo
    /// entry, all-or-nothing.
    ///
    /// Each entry is `(fqn, index)` with the index **as the caller selected
    /// it**, before any deletion: the verb deletes each field's boxes
    /// highest index first, so [`Self::delete_widget`]'s renumbering of later
    /// boxes never shifts a target. A box named twice is deleted once. The
    /// result has one [`FieldDeletion`] per distinct box, in input order;
    /// deleting a field's last box removes the field, as
    /// [`Self::delete_widget`] does.
    ///
    /// # Errors
    ///
    /// [`EditError::FieldNotFound`] or [`EditError::WidgetIndexOutOfRange`]
    /// for any entry, checked before anything is deleted; otherwise any
    /// [`Self::delete_widget`] refusal, with nothing deleted.
    /// [`EditError::SelectionTooLargeForOneUndo`] for more than
    /// [`MAX_UNDO_DEPTH`] boxes.
    pub fn delete_widgets(
        &mut self,
        boxes: &[(&str, usize)],
    ) -> Result<Vec<FieldDeletion>, EditError> {
        let mut distinct: Vec<(&str, usize)> = Vec::with_capacity(boxes.len());
        for &b in boxes {
            if !distinct.contains(&b) {
                distinct.push(b);
            }
        }
        for &(fqn, index) in &distinct {
            let widgets = self.deletion_preflight(fqn)?.0.widgets.len();
            if index >= widgets {
                return Err(EditError::WidgetIndexOutOfRange {
                    name: fqn.to_owned(),
                    index,
                    widgets,
                });
            }
        }
        let mut order = distinct.clone();
        order.sort_by(|a, b| a.0.cmp(b.0).then(b.1.cmp(&a.1)));
        let done = self.all_or_nothing(&order, CommandKind::DeleteFormField, |s, &(fqn, i)| {
            s.delete_widget(fqn, i).map(|d| Some(((fqn, i), d)))
        })?;
        let by_box: BTreeMap<(&str, usize), FieldDeletion> = done.into_iter().collect();
        Ok(distinct
            .iter()
            .filter_map(|b| by_box.get(b).copied())
            .collect())
    }

    /// The ce dimension annotation `annot` draws, if it draws one.
    #[must_use]
    pub fn dimension_for_annotation(&self, annot: ObjId) -> Option<DimensionId> {
        self.read_dimension_model()
            .dimensions()
            .iter()
            .find(|r| r.annot == Some(annot))
            .map(|r| r.id)
    }

    /// Refuse to move or re-place a ce dimension whose annotation is locked
    /// (Table 165 bit 8 names position among what it forbids changing).
    pub(super) fn refuse_locked_dimension(&self, annot: Option<ObjId>) -> Result<(), EditError> {
        let Some(id) = annot else {
            return Ok(());
        };
        let Some(Object::Dict(d)) = self.value(id) else {
            return Ok(());
        };
        let flags = match d.get(b"F") {
            Some(Object::Integer(f)) => u32::try_from(*f).unwrap_or(0),
            _ => 0,
        };
        if !AnnotFlags(flags).locked() {
            return Ok(());
        }
        let subtype = match d.get(b"Subtype") {
            Some(Object::Name(n)) => String::from_utf8_lossy(n.as_bytes()).into_owned(),
            _ => String::new(),
        };
        Err(EditError::AnnotationLocked { id, subtype })
    }
}
