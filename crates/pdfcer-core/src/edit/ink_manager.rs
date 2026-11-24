//! The Ink Manager's two document edits: alias one ink to another, and
//! convert spot inks to process. Acrobat applies both only at output time;
//! here each is an undoable edit with a report of what it changed.

use super::{Command, CommandKind, EditError, EditSession, ObjectWrite};
use crate::crypto::PermissionBit;
use crate::object::{Name, Object, Stream};
use crate::prepress::{InkEditReport, InkPlan, InkSelection, InkUse, Planned};

/// Why a name cannot take part in an alias, or `None` when it can.
fn alias_refusal(from: &str, to: &str) -> Option<(String, &'static str)> {
    let reserved = |n: &str| n == "All" || n == "None";
    if from.is_empty() || to.is_empty() {
        return Some((String::new(), "an ink name cannot be empty"));
    }
    if reserved(from) || reserved(to) {
        let name = if reserved(from) { from } else { to };
        return Some((name.to_owned(), "All and None are not inks"));
    }
    if !crate::preflight::walk::is_spot_colorant(from.as_bytes()) {
        return Some((from.to_owned(), "a process ink cannot be aliased away"));
    }
    if from == to {
        return Some((from.to_owned(), "an ink cannot be aliased to itself"));
    }
    None
}

impl EditSession {
    /// Every ink the document names, as [`crate::prepress::inks`] lists
    /// them, with this session's edits applied.
    #[must_use]
    pub fn inks(&self) -> Vec<InkUse> {
        crate::prepress::inks(&self.view())
    }

    /// Print spot ink `from` as `to`: rename the colorant in every
    /// `Separation` and `DeviceN` space that names it (ISO 32000-2
    /// §8.6.6.4, §8.6.6.5), the `DeviceN` attributes keyed by it
    /// (`/Colorants`, `/MixingHints`) and any `/SeparationInfo
    /// /DeviceColorant`. `to` may be a process ink (`Black`) or another
    /// spot. Each space keeps its own alternate and tint transform, so the
    /// on-screen look is unchanged; the report discloses when the two
    /// names were defined differently.
    ///
    /// A `DeviceN` space naming both inks is refused and listed in
    /// [`InkEditReport::refused`]: it would have two components of one
    /// ink. One undo entry; nothing is committed when nothing changes.
    /// Resolves a `SpotNotAllowed` preflight finding for `from`.
    ///
    /// # Errors
    ///
    /// [`EditError::InkNameRefused`] when either name is empty, `All` or
    /// `None`, `from` is a process ink, or `from == to`;
    /// [`EditError::DocumentEncrypted`] without the modify permission; the
    /// certification gate.
    pub fn alias_ink(&mut self, from: &str, to: &str) -> Result<InkEditReport, EditError> {
        if let Some((name, reason)) = alias_refusal(from, to) {
            return Err(EditError::InkNameRefused { name, reason });
        }
        self.check_ink_edit()?;
        let plan = crate::prepress::plan_alias(&self.view(), from.as_bytes(), to.as_bytes());
        Ok(self.commit_ink_plan(plan, CommandKind::AliasInk))
    }

    /// Convert the selected spot inks to process: every colour drawn in a
    /// `Separation` or `DeviceN` space naming only selected spots becomes
    /// `DeviceCMYK`, through the space's own tint transform (ISO 32000-2
    /// §8.6.6.4, §8.6.6.5) evaluated at each tint. Content-stream colours
    /// (pages, forms, tiling patterns, annotation appearances), image
    /// samples, `Indexed` lookup tables and axial and radial shadings are
    /// converted; the spaces' own arrays are left in place.
    ///
    /// Refused, and listed in [`InkEditReport::refused`]: a space whose
    /// alternate is not `DeviceCMYK` (that is a colour conversion, which
    /// pdfcer leaves to icccer), one that also names an unselected spot, a
    /// tint transform that cannot be evaluated, an inline image, a
    /// colour-keyed or undecodable image, a mesh or function-based
    /// shading, and an uncoloured pattern painting in the ink.
    ///
    /// Overprint is kept where the result still overprints: a `DeviceCMYK`
    /// paint under overprint mode 1 (§8.6.7). Streams painting a converted
    /// ink under overprint with mode 0, or drawing a converted image or
    /// shading under overprint, are listed in
    /// [`InkEditReport::overprint_lost`]. One undo entry; nothing is
    /// committed when nothing changes.
    ///
    /// # Errors
    ///
    /// [`EditError::DocumentEncrypted`] without the modify permission; the
    /// certification gate.
    pub fn spot_to_process(
        &mut self,
        selection: &InkSelection,
    ) -> Result<InkEditReport, EditError> {
        self.check_ink_edit()?;
        let plan = crate::prepress::plan_spot_to_process(&self.view(), selection);
        Ok(self.commit_ink_plan(plan, CommandKind::SpotToProcess))
    }

    /// Refuses an ink edit on a file whose permissions forbid modifying contents.
    pub(super) fn check_ink_edit(&self) -> Result<(), EditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification()?;
        Ok(())
    }

    /// Commit `plan`'s writes as one command; the report names them.
    fn commit_ink_plan(&mut self, plan: InkPlan, kind: CommandKind) -> InkEditReport {
        let InkPlan { writes, mut report } = plan;
        let objects = self.planned_writes(writes);
        report.changed.extend(objects.iter().map(|w| w.id));
        self.commit_writes(kind, objects);
        report
    }

    /// Stage planned writes: each new stream's data, with its `/Length`.
    pub(super) fn planned_writes(
        &mut self,
        writes: std::collections::BTreeMap<crate::object::ObjId, Planned>,
    ) -> Vec<ObjectWrite> {
        let mut objects = Vec::with_capacity(writes.len());
        for (id, planned) in writes {
            let after = match planned {
                Planned::Object(o) => o,
                Planned::Stream { mut dict, data } => {
                    let len = i64::try_from(data.len()).unwrap_or(i64::MAX);
                    dict.insert(Name::from(b"Length"), Object::Integer(len));
                    let data_span = self.stage_bytes(&data);
                    Object::Stream(Stream { dict, data_span })
                }
            };
            objects.push(ObjectWrite {
                id,
                before: self.state.get(&id).cloned(),
                after: Some(after),
            });
        }
        objects
    }

    /// Commit `objects` as one command of `kind`, when there are any.
    pub(super) fn commit_writes(&mut self, kind: CommandKind, objects: Vec<ObjectWrite>) {
        if !objects.is_empty() {
            self.commit(Command {
                kind,
                objects,
                removals: Vec::new(),
                trailer: None,
            });
        }
    }
}
