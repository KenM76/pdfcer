//! Resize, rotate, skew or mirror objects inside a form XObject (pdfcer-gui
//! requests G144, G180): the form-scoped twins of `transform_objects` and
//! `transform_objects_each`.

use super::{CommandKind, EditError, EditSession, FormSurgeryOutcome};
use crate::vector::{Matrix, TransformOptions, VectorEditError};

impl EditSession {
    /// **Transform objects inside a form XObject by one page-space matrix**
    /// — [`Self::transform_objects`] addressed by
    /// [`crate::vector::PageObjects::leaves`] index.
    ///
    /// `matrix` is in page space exactly as for the page verb (compose a
    /// pivot with [`Matrix::about`]); the form is decomposed from the matrix
    /// that placed it, so the result lands where the operator dragged. Each
    /// object's bytes are wrapped in `q <cm> … Q` inside the form's stream,
    /// and `options` and the refusals (`DegenerateCtm`, `SingularTransform`)
    /// are the page verb's. All leaves must lie in one invocation of one
    /// form, as for [`Self::move_objects_in_form`]. The edit is in place:
    /// every place the form is drawn changes, and the outcome's
    /// `invocations` and `pages` say how many (decision 076). A clamped
    /// stroke width is reported in `disclosures`.
    ///
    /// # Errors
    ///
    /// [`EditError::FormLeafOutOfRange`] (also for an empty selection),
    /// [`EditError::FormLeafSelectionSpansForms`], [`EditError::VectorEdit`]
    /// with the page verb's refusals, and the encryption and certification
    /// guards — all before anything changes.
    pub fn transform_objects_in_form(
        &mut self,
        page_index: usize,
        leaf_indices: &[usize],
        matrix: Matrix,
        options: TransformOptions,
    ) -> Result<FormSurgeryOutcome, EditError> {
        let siblings = self.leaf_siblings(page_index, leaf_indices)?;
        let Some(&first) = leaf_indices.first() else {
            return Err(EditError::FormLeafOutOfRange { index: 0, count: 0 });
        };
        self.form_surgery_inner(
            CommandKind::TransformObjects,
            page_index,
            first,
            |stream, model, _| {
                let count = model.objects.len();
                let objs = siblings
                    .iter()
                    .map(|&i| {
                        model
                            .objects
                            .get(i)
                            .ok_or(VectorEditError::ObjectOutOfRange { index: i, count })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(crate::vector::plan_transform_many(
                    stream, &objs, matrix, options,
                )?)
            },
        )
    }

    /// **Transform objects inside a form XObject, each by its own page-space
    /// matrix**, as ONE undoable command — [`Self::transform_objects_each`]
    /// addressed by [`crate::vector::PageObjects::leaves`] index.
    ///
    /// `transforms` holds `(leaf_index, matrix)`; each matrix is in page space
    /// as for [`Self::transform_objects_in_form`]. Leaves, the in-place reach
    /// and `disclosures` are as there; `options` applies to the whole call.
    /// All-or-nothing: every leaf is planned before anything is written.
    ///
    /// # Errors
    ///
    /// As [`Self::transform_objects_in_form`], plus `DuplicateObjectInMove`
    /// (in-form index) for a leaf named twice and `OverlappingObjectSpans`
    /// as [`Self::transform_objects_each`].
    pub fn transform_objects_each_in_form(
        &mut self,
        page_index: usize,
        transforms: &[(usize, Matrix)],
        options: TransformOptions,
    ) -> Result<FormSurgeryOutcome, EditError> {
        let leaves: Vec<usize> = transforms.iter().map(|t| t.0).collect();
        let siblings = self.leaf_siblings(page_index, &leaves)?;
        let Some(&first) = leaves.first() else {
            return Err(EditError::FormLeafOutOfRange { index: 0, count: 0 });
        };
        self.form_surgery_inner(
            CommandKind::TransformObjects,
            page_index,
            first,
            |stream, model, _| {
                let resolved = Self::resolve_moves(model, siblings.iter().copied())?;
                let plan: Vec<_> = resolved
                    .into_iter()
                    .zip(transforms)
                    .map(|(obj, &(_, m))| (obj, m))
                    .collect();
                Ok(crate::vector::plan_transform_each(stream, &plan, options)?)
            },
        )
    }
}
