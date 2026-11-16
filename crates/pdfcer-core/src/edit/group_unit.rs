//! Dimension-group unit and number-format verbs (pdfcer-gui requests G146,
//! G278).

use super::{Command, CommandKind, EditError, EditSession};
use crate::crypto::PermissionBit;
use crate::dimension::{FractionMode, GroupId, NumberFormat, Unit};

impl EditSession {
    /// Show `group`'s ce dimensions in `unit`, keeping the calibration: the
    /// scale is re-expressed by [`crate::dimension::ScaleState::in_unit`], so a
    /// member that read 1000 mm reads 3.28 ft after a switch to decimal feet.
    ///
    /// The number format becomes `unit`'s [`Unit::default_format`] with the
    /// group's decimal marker kept (a decimal-place count chosen for mm is
    /// wrong for km). **A fraction survives where a fraction can be shown:**
    /// a fractional group switched to inches or feet-inches keeps its
    /// denominator and reduce setting. Call
    /// [`Self::set_group_scale`] to set a different format. Regenerates
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
        let fresh = unit.default_format();
        let fraction = match g.format.fraction {
            kept @ FractionMode::Fraction { .. }
                if matches!(unit, Unit::Inch | Unit::FeetInches) =>
            {
                kept
            }
            _ => fresh.fraction,
        };
        let format = NumberFormat {
            fraction,
            decimal_marker: g.format.decimal_marker,
            ..fresh
        };
        self.set_group_scale(group, scale, format)
    }

    /// Create a new named dimension group showing `format` (its unit is the
    /// group's unit). The group starts scale-never-set and visible; its OCG
    /// is allocated lazily on the first ce dimension. One undo entry.
    ///
    /// # Errors
    ///
    /// Encryption / enforced-certification refusals, and
    /// [`EditError::SidecarWrittenByNewerBuild`].
    pub fn add_dimension_group_with_format(
        &mut self,
        name: &str,
        format: NumberFormat,
    ) -> Result<GroupId, EditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification()?;
        self.check_dimension_sidecar()?;
        let mut model = self.read_dimension_model();
        let id = model.add_group_with_format(name, format);
        let catalog_write = self.catalog_dimension_write(&model)?;
        self.commit(Command {
            kind: CommandKind::AddDimension,
            objects: vec![catalog_write],
            removals: Vec::new(),
            trailer: None,
        });
        Ok(id)
    }
}
