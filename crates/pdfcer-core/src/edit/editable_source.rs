//! The session as an [`EditableSource`], and compiling a hand-edited export
//! back into it ([`EditSession::import_editable`]).

use super::{Command, CommandKind, EditError, EditSession, ObjectWrite, Removal};
use crate::PdfVersion;
use crate::crypto::PermissionBit;
use crate::document::Document;
use crate::editable::{EditableSource, ImportReport, diff};
use crate::object::{Name, ObjId, Object, Stream};
use crate::view::StreamSource;
use std::collections::BTreeSet;

impl EditableSource for EditSession {
    fn object_ids(&self) -> Vec<ObjId> {
        let ids: BTreeSet<(u32, u16)> = self
            .base
            .objects()
            .map(|o| o.id)
            .chain(self.state.keys().copied())
            .filter(|id| !self.deleted.contains(id))
            .map(|id| (id.num, id.generation))
            .collect();
        ids.into_iter().map(|(n, g)| ObjId::new(n, g)).collect()
    }

    fn object(&self, id: ObjId) -> Option<&Object> {
        self.value(id)
    }

    fn stream_bytes(&self, stream: &Stream) -> Option<&[u8]> {
        StreamSource::Split {
            base: self.base.bytes(),
            staged: &self.staging,
        }
        .slice(stream.data_span)
    }

    fn pdf_version(&self) -> PdfVersion {
        self.base.version()
    }

    fn trailer_entry(&self, key: &[u8]) -> Option<&Object> {
        self.trailer.get(key)
    }

    fn is_encrypted(&self) -> bool {
        self.base.encryption().is_some()
    }
}

impl EditSession {
    /// Compile a hand-edited [`crate::editable::export`] of this session back
    /// into it, as one undo entry ([`CommandKind::ImportEditable`]).
    ///
    /// The diff is [`crate::editable::import`]'s, taken against the session's
    /// current state rather than its base: objects that changed are replaced,
    /// objects the export added are created under their exported numbers, and
    /// objects it dropped are deleted. Nothing is written to disk; Save
    /// commits it like any other edit, as an incremental update by default.
    ///
    /// [`ImportReport::base`] says whether `edited` was exported from the
    /// session's current state. The edit is applied either way; refusing a
    /// [`crate::editable::ExportBase::Differs`] import is the caller's choice
    /// to make before calling.
    ///
    /// # Errors
    ///
    /// [`EditError::DocumentEncrypted`] when the document's permissions forbid
    /// modifying contents, and the certification refusal.
    ///
    /// # Examples
    ///
    /// ```
    /// # use pdfcer_core::{document::Document, edit::EditSession, editable};
    /// # fn demo(doc: Document) -> Result<(), Box<dyn std::error::Error>> {
    /// let mut session = EditSession::new(doc);
    /// let exported = editable::export(&session)?;
    /// // ... the operator edits `exported` in a text editor ...
    /// let edited = Document::from_bytes(exported)?;
    /// let report = session.import_editable(&edited)?;
    /// assert!(report.is_empty());
    /// # Ok(())
    /// # }
    /// ```
    pub fn import_editable(&mut self, edited: &Document) -> Result<ImportReport, EditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification()?;
        let found = diff(&*self, edited);
        let mut objects = Vec::new();
        for (id, value) in found.writes(edited) {
            let after = self.stage_imported(value, edited);
            objects.push(ObjectWrite {
                id,
                before: self.state.get(&id).cloned(),
                after: Some(after),
            });
        }
        let removals: Vec<Removal> = found
            .report
            .removed
            .iter()
            .map(|&id| Removal {
                id,
                was_deleted: self.deleted.contains(&id),
                is_deleted: true,
            })
            .collect();
        if let Some(top) = found.report.added.iter().map(|id| id.num).max() {
            let floor = top.checked_add(1);
            if self
                .next_number
                .is_some_and(|n| floor.is_none_or(|f| n < f))
            {
                self.next_number = floor;
            }
        }
        if !objects.is_empty() || !removals.is_empty() {
            self.commit(Command {
                kind: CommandKind::ImportEditable,
                objects,
                removals,
                trailer: None,
            });
        }
        Ok(found.report)
    }

    /// `value` from `edited`, with a stream's bytes copied into this
    /// session's staging buffer. The export dropped `/Filter`, so the bytes
    /// are literal and `/Length` is set to match.
    fn stage_imported(&mut self, value: &Object, edited: &Document) -> Object {
        let Object::Stream(s) = value else {
            return value.clone();
        };
        let data = edited.stream_bytes(s).unwrap_or(&[]);
        let mut dict = s.dict.clone();
        dict.insert(
            Name::from(b"Length"),
            Object::Integer(i64::try_from(data.len()).unwrap_or(0)),
        );
        let data_span = self.stage_bytes(data);
        Object::Stream(Stream { dict, data_span })
    }
}
