//! Changing a dimension group's unit without changing what it measures
//! (pdfcer-gui request G146).

use super::{EditError, EditSession};
use crate::dimension::{GroupId, NumberFormat, Unit};

impl EditSession {
    /// Show `group`'s ce dimensions in `unit`, keeping the calibration: the
    /// scale is re-expressed by [`crate::dimension::ScaleState::in_unit`], so a
    /// member that read 1000 mm reads 3.28 ft after a switch to decimal feet.
    ///
    /// The number format becomes `unit`'s [`Unit::default_format`] with the
    /// group's decimal marker kept (a decimal-place count chosen for mm is
    /// wrong for km, and a fraction mode is meaningless outside feet-inches);
    /// call [`Self::set_group_scale`] to set a different format. Regenerates
    /// every wired member's `/AP` and returns how many; one undo entry.
    ///
    /// # Errors
    ///
    /// [`EditError::DimensionGroupNotFound`] for an unknown group, and
    /// [`Self::set_group_scale`]'s encryption / certification refusals.
    pub fn set_group_unit(&mut self, group: GroupId, unit: Unit) -> Result<usize, EditError> {
        let model = self.read_dimension_model();
        let g = model
            .group(group)
            .ok_or(EditError::DimensionGroupNotFound { id: group.0 })?;
        let scale = g.scale.in_unit(g.format.unit, unit);
        let format = NumberFormat {
            decimal_marker: g.format.decimal_marker,
            ..unit.default_format()
        };
        self.set_group_scale(group, scale, format)
    }
}
