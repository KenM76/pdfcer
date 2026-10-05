//! The one test every editing verb applies to an encrypted document.

use crate::crypto::PermissionBit;
use crate::document::Document;

/// Whether an edit governed by any of `bits` is refused on `doc`.
///
/// A plain document is never refused. An encrypted one is refused when its
/// save could not be appended (RC4, which pdfcer never writes) or when the
/// password that opened it grants none of `bits` (ISO 32000-2 Table 22; the
/// owner password grants every bit).
pub(crate) fn forbids(doc: &Document, bits: &[PermissionBit]) -> bool {
    doc.encryption()
        .is_some_and(|e| e.edit_refusal(bits).is_some())
}
