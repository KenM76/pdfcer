//! The Ink Manager's two document edits: alias one ink to another, and
//! convert spot inks to process. Acrobat applies both only at output time;
//! here each is an undoable edit with a report of what it changed.

use super::{Command, CommandKind, EditError, EditSession, ObjectWrite};
use crate::crypto::PermissionBit;
use crate::object::{Name, Object, Stream};
#[cfg(doc)]
use crate::prepress::InkUse;
use crate::prepress::{InkEditReport, InkList, InkPlan, InkSelection, Planned};

/// The longest name a reader has to accept, in bytes after `#XX` decoding
/// (ISO 32000-1 Annex C, Table C.1).
const MAX_INK_NAME: usize = 127;

/// The two names' bytes, or why one cannot take part in an alias. Names
/// are spelled as [`InkUse::name`] is: `#XX` is one byte (ISO 32000-1
/// §7.3.5).
fn alias_names(from: &str, to: &str) -> Result<(Vec<u8>, Vec<u8>), EditError> {
    let refused = |name: &str, reason| EditError::InkNameRefused {
        name: name.to_owned(),
        reason,
    };
    let decode = |n: &str| {
        crate::prepress::ink_name_bytes(n).ok_or_else(|| {
            refused(
                n,
                "a # must be followed by two hex digits; write # itself as #23",
            )
        })
    };
    let (f, t) = (decode(from)?, decode(to)?);
    if f.is_empty() || t.is_empty() {
        return Err(refused("", "an ink name cannot be empty"));
    }
    for (name, bytes) in [(from, &f), (to, &t)] {
        if bytes.len() > MAX_INK_NAME {
            return Err(refused(name, "an ink name can be at most 127 bytes"));
        }
        if bytes.contains(&0) {
            return Err(refused(name, "a name cannot hold a zero byte"));
        }
        if bytes.as_slice() == b"All" || bytes.as_slice() == b"None" {
            return Err(refused(name, "All and None are not inks"));
        }
    }
    if !crate::preflight::walk::is_spot_colorant(&f) {
        return Err(refused(from, "a process ink cannot be aliased away"));
    }
    if f == t {
        return Err(refused(from, "an ink cannot be aliased to itself"));
    }
    Ok((f, t))
}

impl EditSession {
    /// Every ink the document names, as [`crate::prepress::inks`] lists
    /// them, with this session's edits applied.
    #[must_use]
    pub fn inks(&self) -> InkList {
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
    /// Names are spelled as [`InkUse::name`] lists them: `#XX` is one byte
    /// (ISO 32000-1 §7.3.5), so `Spot#80` names byte 0x80 and `No#232` the
    /// literal `No#2`. A reference naming `from` is replaced by the new
    /// name, so no shared name object is rewritten. A `/SeparationInfo
    /// /DeviceColorant` renamed counts in [`InkEditReport::spaces`].
    ///
    /// A `DeviceN` space naming both inks is refused and listed in
    /// [`InkEditReport::refused`]: it would have two components of one
    /// ink. One undo entry; nothing is committed when nothing changes.
    /// Resolves a `SpotNotAllowed` preflight finding for `from`.
    ///
    /// # Errors
    ///
    /// [`EditError::InkNameRefused`] when either name is empty, `All` or
    /// `None`, longer than 127 bytes once decoded (ISO 32000-1 Annex C),
    /// holds a zero byte or a `#` not followed by two hex digits, `from`
    /// is a process ink, or `from == to`;
    /// [`EditError::InkAliasRefused`] when the search was cut short
    /// (nesting past 24 levels, more than 2^22 values, or a `DeviceN` space
    /// naming more than 32 colorants) or a rename would change an object a
    /// refused `DeviceN` space reaches; nothing is changed either way;
    /// [`EditError::DocumentEncrypted`] without the modify permission; the
    /// certification gate.
    pub fn alias_ink(&mut self, from: &str, to: &str) -> Result<InkEditReport, EditError> {
        let (from, to) = alias_names(from, to)?;
        self.check_ink_edit()?;
        let plan = crate::prepress::plan_alias(&self.view(), &from, &to)
            .map_err(|reason| EditError::InkAliasRefused { reason })?;
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
