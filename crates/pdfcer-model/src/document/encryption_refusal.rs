//! Why an encrypted document refuses an edit, for a shell to word in its own terms.

use super::DocumentEncryption;
use crate::crypto::PermissionBit;

/// The cause of an editing verb's encryption refusal (every verb's
/// `DocumentEncrypted` / `Encrypted` error variant).
///
/// Each cause has one remedy: [`Rc4NotAllowed`](Self::Rc4NotAllowed) lifts
/// when the session allows edits under RC4
/// ([`Document::set_rc4_append`](crate::document::Document::set_rc4_append)
/// with [`Rc4Append::Preserve`](crate::writer::Rc4Append::Preserve));
/// [`PermissionDenied`](Self::PermissionDenied) lifts only when the document
/// is opened with its owner password.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EncryptedRefusal {
    /// The password grants the edit, but the document uses RC4 and the
    /// session has not allowed saving under it (decision 190).
    Rc4NotAllowed,
    /// The password that opened the document does not grant the permission
    /// the edit needs (ISO 32000-2 Table 22).
    /// [`DocumentEncryption::edit_refusal`] reports it in preference to
    /// [`Rc4NotAllowed`](Self::Rc4NotAllowed) when both apply, because
    /// allowing RC4 alone would not let the edit through.
    PermissionDenied,
}

impl DocumentEncryption {
    /// Why an edit governed by any of `bits` would be refused, or `None` when
    /// it is allowed. This is the test every editing verb applies, so a shell
    /// can ask before calling one.
    ///
    /// Editing verbs and the bits they need: annotation verbs need
    /// [`Annotate`](PermissionBit::Annotate); fills and form-data import need
    /// [`FillForms`](PermissionBit::FillForms) or `Annotate`; page insert,
    /// delete, rotate, reorder, labels, merge and outline items need
    /// [`Assemble`](PermissionBit::Assemble); everything else needs
    /// [`ModifyContents`](PermissionBit::ModifyContents).
    #[must_use]
    pub fn edit_refusal(&self, bits: &[PermissionBit]) -> Option<EncryptedRefusal> {
        if !bits.iter().any(|&b| self.grants(b)) {
            Some(EncryptedRefusal::PermissionDenied)
        } else if !self.appendable() {
            Some(EncryptedRefusal::Rc4NotAllowed)
        } else {
            None
        }
    }

    /// The cause of an encryption refusal a verb has already returned, when
    /// the shell does not know which permission that verb needs.
    ///
    /// A refusal under an RC4 policy that does not allow saving is reported as
    /// [`Rc4NotAllowed`](EncryptedRefusal::Rc4NotAllowed) — allowing RC4 is
    /// then necessary for every edit, though a permission refusal may remain
    /// after it. Otherwise the refusal can only have come from the password,
    /// so it is [`PermissionDenied`](EncryptedRefusal::PermissionDenied).
    /// Call it only after a refusal: it does not test whether an edit is
    /// allowed (use [`edit_refusal`](Self::edit_refusal) for that).
    #[must_use]
    pub fn refusal_cause(&self) -> EncryptedRefusal {
        if self.appendable() {
            EncryptedRefusal::PermissionDenied
        } else {
            EncryptedRefusal::Rc4NotAllowed
        }
    }
}
