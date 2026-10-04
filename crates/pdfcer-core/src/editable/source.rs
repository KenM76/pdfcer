//! The read surface [`super::export`] and [`super::fingerprint`] need, so both
//! accept a [`Document`] or an open [`EditSession`] with its unsaved edits.

use crate::PdfVersion;
use crate::document::Document;
use crate::edit::EditSession;
use crate::object::{ObjId, Object, Stream};
use crate::writer::serialize;

pub(crate) mod sealed {
    pub trait Sealed {}
}

/// A document whose objects can be enumerated and whose streams can be read:
/// a [`Document`], or an [`EditSession`] as the operator currently has it.
///
/// Sealed: implemented for exactly those two.
pub trait EditableSource: sealed::Sealed {
    /// Every live object id, ascending.
    fn object_ids(&self) -> Vec<ObjId>;
    /// The current value of `id`, or `None` when it is not live.
    fn object(&self, id: ObjId) -> Option<&Object>;
    /// `stream`'s stored (still encoded) bytes, or `None` when its span lies
    /// outside this source.
    fn stream_bytes(&self, stream: &Stream) -> Option<&[u8]>;
    /// The document's version.
    fn pdf_version(&self) -> PdfVersion;
    /// The current trailer entry `key`.
    fn trailer_entry(&self, key: &[u8]) -> Option<&Object>;
    /// Whether the document is encrypted.
    fn is_encrypted(&self) -> bool;
}

impl sealed::Sealed for Document {}

impl EditableSource for Document {
    fn object_ids(&self) -> Vec<ObjId> {
        let mut ids: Vec<ObjId> = self.objects().map(|o| o.id).collect();
        ids.sort_unstable_by_key(|i| (i.num, i.generation));
        ids
    }

    fn object(&self, id: ObjId) -> Option<&Object> {
        self.get(id).map(|o| &o.value)
    }

    fn stream_bytes(&self, stream: &Stream) -> Option<&[u8]> {
        serialize::stream_data(stream, self.bytes())
    }

    fn pdf_version(&self) -> PdfVersion {
        self.version()
    }

    fn trailer_entry(&self, key: &[u8]) -> Option<&Object> {
        self.trailer().get(key)
    }

    fn is_encrypted(&self) -> bool {
        self.encryption().is_some()
    }
}

impl sealed::Sealed for EditSession {}
