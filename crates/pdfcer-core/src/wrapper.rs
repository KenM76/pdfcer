//! Detect an **unencrypted wrapper document** — ISO 32000-2 §7.6.7.
//!
//! # ★ The guard this exists to stop failing open
//!
//! pdfcer refuses an encrypted document by name: the parser sees `/Encrypt` in
//! the trailer and stops with
//! [`XrefErrorKind::EncryptionUnsupported`](crate::xref::XrefErrorKind::EncryptionUnsupported).
//! That guard is correct and it does not fire here.
//!
//! An unencrypted wrapper **has no `/Encrypt` at all.** It is a perfectly
//! ordinary, fully-readable PDF whose visible page is an author-written cover
//! sheet — typically "this document is protected, open it in X" — carrying
//! the real document as an *embedded encrypted payload*. ISO 32000-2 §7.6.7
//! standardises the mechanism (it generalises Microsoft's 2012 `/Wrapper` IRM
//! arrangement) precisely so that a reader without the right handler shows
//! something explanatory instead of nothing.
//!
//! So without this module pdfcer parses the file, renders the cover, reports
//! no error, and **presents the cover as the document**. Every count it gives
//! — one page, no form fields, no annotations — is true of the wrapper and
//! false of the thing the operator means by "this document". A guard failing
//! closed says "I cannot do this"; this one failed open and said nothing.
//!
//! # How it is detected
//!
//! §7.6.7 wires a wrapper together with machinery pdfcer already reads: the
//! catalog's `/AF` associated-files array names a file specification whose
//! **`/AFRelationship` is `/EncryptedPayload`**. That relationship name is the
//! marker, and it is the whole test.
//!
//! ## What is deliberately NOT the test
//!
//! **"Exactly one entry in the `EmbeddedFiles` name tree."** That sentence
//! appeared in ISO 32000-2 as printed and was **deleted by erratum**. It is
//! exactly the kind of secondary-source rule that reads as settled and is
//! not, and a detector built on it would miss any wrapper carrying a second
//! attachment.
//!
//! The `/Collection` dictionary and its `/View /H` are likewise not required
//! for detection. They control *presentation* of the payload; a wrapper is
//! still a wrapper without them, and requiring them would turn a disclosure
//! into a guess about the producer's thoroughness.
//!
//! # Edition, stated rather than blurred
//!
//! §7.6.7 is **ISO 32000-2 only**. ISO 32000-1 has no wrapper concept. pdfcer
//! detects it regardless of the file's declared version: a 1.7 file carrying
//! the marker is unusual, but the marker still means what it means, and
//! refusing to look would be a version check standing in for a content check.
//!
//! # What this module does NOT do
//!
//! It does not decrypt anything, and it does not extract the payload. It
//! answers one question — *is the visible document standing in for a
//! protected one?* — so a shell can say so instead of letting the operator
//! infer it from a cover page they may not read.

use crate::graph::ObjectGraph;
use crate::object::Object;

/// The `/AFRelationship` value that marks an encrypted payload (§7.6.7).
pub const ENCRYPTED_PAYLOAD: &[u8] = b"EncryptedPayload";

/// What pdfcer found when it looked for a wrapper.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WrapperInfo {
    /// Whether the catalog names an encrypted payload.
    pub is_wrapper: bool,
    /// The payload's file name (`/F`, or `/UF` in preference), if it has one.
    ///
    /// Reported so a disclosure can name the file the operator is *not*
    /// seeing. "This document wraps an encrypted payload" is a weaker
    /// statement than naming it.
    pub payload_name: Option<String>,
    /// How many associated files carry the encrypted-payload relationship.
    ///
    /// Normally one. More than one is not forbidden by anything sourced, and
    /// is reported rather than collapsed, because a file with several is
    /// unusual enough that an operator should hear about it.
    pub payload_count: usize,
}

/// Look for an encrypted-payload marker in the catalog's `/AF` array.
///
/// Cheap: one catalog lookup and a walk of a normally-empty array. Safe to
/// call on every document open, which is the point — a detector an operator
/// has to remember to run is a detector that does not fire on the day it
/// matters.
#[must_use]
pub fn detect<G: ObjectGraph + ?Sized>(graph: &G) -> WrapperInfo {
    let mut info = WrapperInfo::default();
    let Some(catalog) = graph.catalog_dict() else {
        return info;
    };
    let Some(af) = catalog
        .get(b"AF")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_array)
    else {
        return info;
    };
    for entry in af {
        let Some(spec) = graph.resolve(entry).as_dict() else {
            continue;
        };
        let relationship = spec
            .get(b"AFRelationship")
            .map(|o| graph.resolve(o))
            .and_then(Object::as_name);
        if relationship.map(|n| n.as_bytes()) != Some(ENCRYPTED_PAYLOAD) {
            continue;
        }
        info.is_wrapper = true;
        info.payload_count += 1;
        if info.payload_name.is_none() {
            // `/UF` in preference to `/F`: §7.11.3 makes `/UF` the Unicode
            // form, and a name shown to an operator should be the one the
            // producer intended them to read.
            info.payload_name = spec
                .get(b"UF")
                .or_else(|| spec.get(b"F"))
                .map(|o| graph.resolve(o))
                .and_then(|o| match o {
                    Object::String(s) => Some(crate::edit::decode_text_string(s).text),
                    _ => None,
                });
        }
    }
    info
}

impl WrapperInfo {
    /// The operator-facing disclosure, or `None` when there is nothing to
    /// say.
    ///
    /// Written to be read by someone who did not ask the question. The
    /// dangerous state is an operator who believes they are looking at the
    /// document, so the sentence leads with what the visible page *is*
    /// rather than with the mechanism.
    #[must_use]
    pub fn message(&self) -> Option<String> {
        if !self.is_wrapper {
            return None;
        }
        let named = self
            .payload_name
            .as_ref()
            .map_or_else(String::new, |n| format!(" ({n})"));
        let extra = if self.payload_count > 1 {
            format!(" It names {} encrypted payloads.", self.payload_count)
        } else {
            String::new()
        };
        Some(format!(
            "What you can see is a COVER PAGE, not the document. This file is an \
             unencrypted wrapper (ISO 32000-2 §7.6.7) around an encrypted payload{named} \
             that pdfcer cannot decrypt. Page counts, form fields and text extracted from \
             it describe the cover, not the real content.{extra}"
        ))
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::pageops::tests_support::build_pdf_bytes;

    fn doc_with_catalog(catalog: &str, extra: &[(u32, &str)]) -> Document {
        let mut objects: Vec<(u32, &str)> = vec![
            (1, catalog),
            (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            (3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>"),
        ];
        objects.extend_from_slice(extra);
        Document::from_bytes(build_pdf_bytes(&objects)).expect("fixture parses")
    }

    /// ★ **A wrapper is detected even though it carries no `/Encrypt`.**
    ///
    /// This is the whole point. pdfcer's encryption refusal keys on
    /// `/Encrypt` in the trailer, and a wrapper has none — it is a plainly
    /// readable PDF. Without this detector pdfcer reports success and shows
    /// the cover as the document.
    #[test]
    fn a_wrapper_is_detected_although_nothing_is_encrypted() {
        let doc = doc_with_catalog(
            "<< /Type /Catalog /Pages 2 0 R /AF [4 0 R] >>",
            &[(
                4,
                "<< /Type /Filespec /F (secret.pdf) /UF (secret.pdf) \
                 /AFRelationship /EncryptedPayload /EF << /F 5 0 R >> >>",
            )],
        );
        let info = detect(&doc);
        assert!(info.is_wrapper);
        assert_eq!(info.payload_name.as_deref(), Some("secret.pdf"));
        assert_eq!(info.payload_count, 1);

        let m = info.message().expect("a wrapper discloses itself");
        assert!(m.contains("COVER PAGE"), "{m}");
        assert!(m.contains("secret.pdf"), "the payload is named: {m}");
        assert!(
            m.contains("not the real content"),
            "and the counts are qualified: {m}"
        );
    }

    /// An ordinary attachment is NOT a wrapper. The relationship name is the
    /// test, not the presence of an associated file.
    #[test]
    fn an_ordinary_associated_file_is_not_a_wrapper() {
        let doc = doc_with_catalog(
            "<< /Type /Catalog /Pages 2 0 R /AF [4 0 R] >>",
            &[(
                4,
                "<< /Type /Filespec /F (data.csv) /AFRelationship /Data \
                 /EF << /F 5 0 R >> >>",
            )],
        );
        let info = detect(&doc);
        assert!(!info.is_wrapper);
        assert_eq!(info.message(), None, "silence when there is nothing to say");
    }

    /// A document with no `/AF` at all is not a wrapper, and asking is cheap.
    #[test]
    fn a_plain_document_is_not_a_wrapper() {
        let doc = doc_with_catalog("<< /Type /Catalog /Pages 2 0 R >>", &[]);
        assert_eq!(detect(&doc), WrapperInfo::default());
    }

    /// ★ **Detection does NOT require exactly one embedded file.**
    ///
    /// ISO 32000-2 as printed said it did; the sentence was deleted by
    /// erratum. A detector built on it would miss any wrapper that also
    /// carries an ordinary attachment — which is the shape a real one takes
    /// when the producer includes, say, a readme beside the payload.
    #[test]
    fn a_second_attachment_does_not_hide_the_wrapper() {
        let doc = doc_with_catalog(
            "<< /Type /Catalog /Pages 2 0 R /AF [4 0 R 5 0 R] >>",
            &[
                (
                    4,
                    "<< /Type /Filespec /F (readme.txt) /AFRelationship /Supplement >>",
                ),
                (
                    5,
                    "<< /Type /Filespec /F (payload.pdf) /AFRelationship /EncryptedPayload >>",
                ),
            ],
        );
        let info = detect(&doc);
        assert!(info.is_wrapper, "the ordinary attachment does not mask it");
        assert_eq!(info.payload_name.as_deref(), Some("payload.pdf"));
    }

    /// Several payloads are counted and said out loud rather than collapsed
    /// to "yes".
    #[test]
    fn several_payloads_are_counted_and_disclosed() {
        let doc = doc_with_catalog(
            "<< /Type /Catalog /Pages 2 0 R /AF [4 0 R 5 0 R] >>",
            &[
                (
                    4,
                    "<< /Type /Filespec /F (a.pdf) /AFRelationship /EncryptedPayload >>",
                ),
                (
                    5,
                    "<< /Type /Filespec /F (b.pdf) /AFRelationship /EncryptedPayload >>",
                ),
            ],
        );
        let info = detect(&doc);
        assert_eq!(info.payload_count, 2);
        assert!(
            info.message()
                .expect("discloses")
                .contains("2 encrypted payloads"),
            "an unusual shape is stated, not smoothed over"
        );
    }

    /// A payload with no name still discloses — the mechanism matters more
    /// than the file name, so a missing name must not silence the warning.
    #[test]
    fn a_nameless_payload_still_discloses() {
        let doc = doc_with_catalog(
            "<< /Type /Catalog /Pages 2 0 R /AF [4 0 R] >>",
            &[(4, "<< /Type /Filespec /AFRelationship /EncryptedPayload >>")],
        );
        let info = detect(&doc);
        assert!(info.is_wrapper);
        assert_eq!(info.payload_name, None);
        assert!(
            info.message()
                .expect("still discloses")
                .contains("COVER PAGE")
        );
    }
}
