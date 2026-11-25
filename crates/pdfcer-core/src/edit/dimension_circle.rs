//! Showing and hiding a radius/diameter ce dimension's circle (pdfcer-gui
//! request G329).

use super::{Command, CommandKind, EditError, EditSession};
use crate::crypto::PermissionBit;
use crate::dimension::{DimensionId, DimensionKind};

impl EditSession {
    /// **Show or hide the circle of a radius/diameter ce dimension**, as one
    /// undoable command that regenerates its `/AP` and the sidecar.
    ///
    /// The leader, label and any centre mark stay; the fit, the value and
    /// the placement are untouched. A hidden circle saves and reopens hidden
    /// (sidecar `/ShowCircle false`); snap targets on it carry
    /// `drawn: false` ([`crate::vector::CeDimensionTarget`]). The circle's
    /// own stroke is a style property: [`crate::dimension::PartStroke`] in
    /// `circle` on [`crate::dimension::StyleOverrides`] /
    /// [`crate::dimension::GroupStyle`].
    ///
    /// Setting it to its current value still commits, as
    /// [`Self::set_dimension_display`] does: compare before calling to
    /// suppress a no-op press.
    ///
    /// # Errors
    ///
    /// [`EditError::DimensionNotFound`] for an unknown id;
    /// [`EditError::NotACircularDimension`] for any kind but
    /// [`DimensionKind::Circular`]; plus the encryption,
    /// enforced-certification and newer-sidecar guards.
    pub fn set_circular_circle_shown(
        &mut self,
        dimension: DimensionId,
        shown: bool,
    ) -> Result<(), EditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification()?;
        self.check_dimension_sidecar()?;

        let mut model = self.read_dimension_model();
        let Some(record) = model.dimension_mut(dimension) else {
            return Err(EditError::DimensionNotFound { id: dimension.0 });
        };
        let DimensionKind::Circular { show_circle, .. } = &mut record.kind else {
            return Err(EditError::NotACircularDimension { id: dimension.0 });
        };
        *show_circle = shown;

        let mut objects = self.regenerate_dimension_writes(&model, &[dimension])?;
        objects.push(self.catalog_dimension_write(&model)?);
        self.commit(Command {
            kind: CommandKind::SetCircularCircleShown { shown },
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        Ok(())
    }
}
