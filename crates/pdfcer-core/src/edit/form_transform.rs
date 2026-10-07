//! Resize, rotate, skew or mirror objects inside a form XObject (pdfcer-gui
//! request G144): the form-scoped twin of `transform_objects`.

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
}
