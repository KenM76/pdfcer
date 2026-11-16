//! Dimension-group unit and number-format verbs (pdfcer-gui requests G146,
//! G278).

use super::{Command, CommandKind, EditError, EditSession};
use crate::crypto::PermissionBit;
use crate::dimension::{FractionMode, Group, GroupId, NumberFormat, ScaleState, Unit};

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
        self.add_group_set_up(name, format, |_| {})
    }

    /// [`Self::add_dimension_group_with_format`] with the group's scale set
    /// in the same command: one undo entry for "a new group at this scale".
    ///
    /// # Errors
    ///
    /// As [`Self::add_dimension_group_with_format`].
    pub fn add_dimension_group_with_scale(
        &mut self,
        name: &str,
        scale: ScaleState,
        format: NumberFormat,
    ) -> Result<GroupId, EditError> {
        self.add_group_set_up(name, format, |g| g.scale = scale)
    }

    /// Create a new group named `name` set up like `template`: its scale,
    /// number format, drafting standard and style are copied. Its ce
    /// dimensions are not; the new group is empty and visible. One undo
    /// entry.
    ///
    /// # Errors
    ///
    /// [`EditError::DimensionGroupNotFound`] for an unknown `template`, then
    /// as [`Self::add_dimension_group_with_format`].
    pub fn add_dimension_group_like(
        &mut self,
        name: &str,
        template: GroupId,
    ) -> Result<GroupId, EditError> {
        let model = self.read_dimension_model();
        let like = model
            .group(template)
            .ok_or(EditError::DimensionGroupNotFound { id: template.0 })?
            .clone();
        self.add_group_set_up(name, like.format, |g| {
            g.scale = like.scale;
            g.standard = like.standard;
            g.style = like.style;
        })
    }

    fn add_group_set_up(
        &mut self,
        name: &str,
        format: NumberFormat,
        set_up: impl FnOnce(&mut Group),
    ) -> Result<GroupId, EditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification()?;
        self.check_dimension_sidecar()?;
        let mut model = self.read_dimension_model();
        let id = model.add_group_with_format(name, format);
        if let Some(group) = model.group_mut(id) {
            set_up(group);
        }
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
