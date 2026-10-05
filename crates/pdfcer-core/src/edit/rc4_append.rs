//! Whether this session's saves may keep an RC4 document's encryption
//! (decision 190).

use super::EditSession;
use crate::crypto::PermissionBit;
use crate::document::EncryptedRefusal;
use crate::writer::Rc4Append;

impl EditSession {
    /// Choose whether edits to an RC4-encrypted document are allowed and
    /// saved under its RC4 handler ([`Rc4Append::Preserve`]) or refused
    /// ([`Rc4Append::Refuse`], the default). No effect on a plain or AES
    /// document; the password's permissions still apply.
    ///
    /// Under the default, every editing verb on an RC4 document returns
    /// [`EditError::DocumentEncrypted`](crate::edit::EditError::DocumentEncrypted)
    /// and a save returns
    /// [`WriteError::Rc4AppendRefused`](crate::writer::WriteError::Rc4AppendRefused).
    /// Under Preserve, each save reports
    /// [`SaveReport::rc4_keystream_reused`](crate::writer::SaveReport::rc4_keystream_reused),
    /// which a shell must disclose.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::document::Document;
    /// use pdfcer_core::edit::EditSession;
    /// use pdfcer_core::writer::Rc4Append;
    ///
    /// let bytes = include_bytes!("../../../../fixtures/synthetic/hello.pdf").to_vec();
    /// let mut session = EditSession::new(Document::from_bytes(bytes)?);
    /// session.set_rc4_append(Rc4Append::Preserve);
    /// assert_eq!(session.rc4_append(), None, "a plain document has no RC4 policy");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn set_rc4_append(&mut self, policy: Rc4Append) {
        self.base.set_rc4_append(policy);
    }

    /// The RC4 append policy in force, or `None` for a plain document.
    #[must_use]
    pub fn rc4_append(&self) -> Option<Rc4Append> {
        self.base.encryption().map(|e| e.rc4_append())
    }

    /// Why an edit governed by any of `bits` would be refused on this
    /// session's document, or `None` when it is allowed (a plain document is
    /// never refused). See
    /// [`DocumentEncryption::edit_refusal`](crate::document::DocumentEncryption::edit_refusal)
    /// for which verbs need which bit.
    #[must_use]
    pub fn encryption_refusal(&self, bits: &[PermissionBit]) -> Option<EncryptedRefusal> {
        self.base.encryption().and_then(|e| e.edit_refusal(bits))
    }

    /// The cause of an encryption refusal an editing verb has already
    /// returned, or `None` for a plain document. See
    /// [`DocumentEncryption::refusal_cause`](crate::document::DocumentEncryption::refusal_cause).
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::document::{Document, EncryptedRefusal};
    /// use pdfcer_core::edit::EditSession;
    ///
    /// let bytes = include_bytes!("../../../../fixtures/synthetic/encryption/enc-rc4-128.pdf");
    /// let doc = Document::from_bytes_with_password(bytes.to_vec(), Some(b"ownerpw"))?;
    /// let mut session = EditSession::new(doc);
    /// assert!(session.rotate_pages(&[0], 90).is_err());
    /// assert_eq!(session.encryption_refusal_cause(), Some(EncryptedRefusal::Rc4NotAllowed));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[must_use]
    pub fn encryption_refusal_cause(&self) -> Option<EncryptedRefusal> {
        self.base.encryption().map(|e| e.refusal_cause())
    }
}
