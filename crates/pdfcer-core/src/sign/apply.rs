//! The **PDF half** of signing — ISO 32000-1 §12.8.1 / §12.8.3.3 as a
//! writer sees it: the signature dictionary with its zero-filled `/Contents`
//! hole, the `/ByteRange` that reaches EOF, the digest of the two spans, the
//! back-patch that changes no byte outside the hole, and the self-check
//! afterwards. Read `iso32000__ref__signature_creation.md` (`SC-1`…`SC-8`).
//!
//! The session-side verb ([`crate::edit::EditSession::sign`]) lives in
//! `edit.rs`, because it needs the session's private object plumbing; this
//! module holds the request/report types and the **pure byte-level
//! functions** it orchestrates, each testable on its own.
//!
//! # The two-pass write (`SC-2`), and why pdfcer's writer needs no changes
//!
//! ```text
//!  session + placeholder objects ──save_incremental──▶ bytes with
//!      /Contents <000…000>  (L zeros, fixed)              a zero hole
//!      /ByteRange [0 1000000000 1000000000 1000000000]   fixed-width sentinels
//!                       │
//!            locate the hole in the appended revision (this module)
//!                       │
//!            overwrite /ByteRange in place with zero-padded 10-digit ints
//!                       │
//!            digest span1 ‖ span2  (everything except `<…>`)
//!                       │
//!            CMS SignedData ──hex──▶ zero-pad to L ──▶ overwrite the hole
//! ```
//!
//! Both patches are **same-length overwrites**, so every offset the
//! cross-reference section recorded is still right and no byte outside the
//! two patched fields moves — `SC-2` step 9's *"the only mutation after the
//! digest"* holds by construction. The `/ByteRange` sentinels are ten-digit
//! integers (`1000000000`) so that back-patching them with `%010` zero-padded
//! values never changes the token length; a leading zero is a legal PDF
//! integer (§7.3.3) and every reader parses `0000012345` as `12345`.
//!
//! # The hole's geometry (`SC-2` step 3, `SC-3`)
//!
//! With the `/Contents` token occupying `[a, b)` — `a` the offset of `<`, `b`
//! one past `>` — `/ByteRange` is `[0 a b (len − b)]`: the delimiters are
//! INSIDE the gap, and the second span reaches exactly EOF (ISO 32000-2
//! §12.8.1 hardens that to a `shall`; a short tail is what lets an attacker
//! append). [`locate_hole`] finds the token by scanning the appended
//! revision for the signature object's own `N G obj` header and then for
//! `/Contents <`, so it does not depend on how the serializer laid the
//! dictionary out.
//!
//! # Size (`SC-6`)
//!
//! [`SignRequest::reserve`] is the number of **bytes** the DER may occupy;
//! the hole is `2 × reserve` hex digits. The default, 12 KiB, fits a
//! SHA-256/RSA-4096 CAdES signature with a three-certificate chain about
//! three times over; a B-T timestamp token (later) adds 4–8 KiB and the
//! request grows the reserve for it. If the blob does not fit the write is
//! refused **by name** ([`SignApplyError::ReservationTooSmall`]) with both
//! numbers — the hole is never shrunk or grown after layout, because that
//! moves bytes.

use super::SignatureAlgorithm;
use super::cms_build::SubFilter;

/// What to sign with, and how the signature dictionary should read.
///
/// Everything optional is *authored*, not inferred: the reason, location
/// and contact are the operator's words, and the signing time is the
/// caller's PDF date string (pdfcer reads no clock — see `signing_time`).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SignRequest {
    /// Which `/SubFilter`, and therefore which flavour of CMS. Default
    /// [`SubFilter::EtsiCadesDetached`] (PAdES).
    pub sub_filter: SubFilter,
    /// The signature algorithm, or `None` for the key's own default (PKCS#1
    /// v1.5 for RSA, the curve's ECDSA for EC).
    pub algorithm: Option<SignatureAlgorithm>,
    /// `/M` — the claimed signing time as a PDF date string
    /// (`D:YYYYMMDDHHmmSSOHH'mm'`, §7.9.4). **Required by PAdES**
    /// (requirement g) and written verbatim. pdfcer reads no clock: a CLI
    /// derives it from the system clock and *says so*; a GUI passes the time
    /// it showed the operator.
    pub signing_time: String,
    /// `/Name` — the signer's name as shown; `None` omits the key and a
    /// verifier falls back to the certificate subject (Table 252 says it
    /// should anyway).
    pub name: Option<String>,
    /// `/Reason`, free text.
    pub reason: Option<String>,
    /// `/Location`, free text — the operator's words, not a resolved place.
    pub location: Option<String>,
    /// `/ContactInfo`, free text.
    pub contact_info: Option<String>,
    /// The signature field's `/T`. Must not collide with an existing field
    /// name; `None` picks `Signature1`, `Signature2`, … as Acrobat does.
    pub field_name: Option<String>,
    /// Where the widget goes: `(page_index, rect)` for a visible signature
    /// (the appearance is a plain framed text block naming the signer and
    /// the time), or `None` for an **invisible** signature — `/Rect [0 0 0 0]`
    /// on the first page, nothing drawn (`SC-4`). Invisible is the default
    /// for batch/CLI signing.
    pub visible: Option<(usize, crate::page_tree::Rect)>,
    /// Bytes reserved for the DER `SignedData` (`SC-6`). Default 12 288.
    pub reserve: usize,
}

impl Default for SignRequest {
    fn default() -> Self {
        Self {
            sub_filter: SubFilter::default(),
            algorithm: None,
            signing_time: String::new(),
            name: None,
            reason: None,
            location: None,
            contact_info: None,
            field_name: None,
            visible: None,
            reserve: 12 * 1024,
        }
    }
}

impl SignRequest {
    /// An invisible PAdES B-B request at `signing_time` (a PDF date string).
    #[must_use]
    pub fn at(signing_time: impl Into<String>) -> Self {
        Self {
            signing_time: signing_time.into(),
            ..Self::default()
        }
    }
}

/// What a signing wrote — the rule-4 disclosure the CLI prints and a GUI
/// shows off-canvas.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SignReport {
    /// The signature field's `/T`.
    pub field_name: String,
    /// The signature dictionary's object id.
    pub signature_id: crate::object::ObjId,
    /// The `/SubFilter` written.
    pub sub_filter: SubFilter,
    /// The signature algorithm used.
    pub algorithm: SignatureAlgorithm,
    /// The signer certificate's subject (`CN=…`).
    pub signer_subject: String,
    /// The signer certificate's serial, upper-case hex.
    pub signer_serial_hex: String,
    /// Certificates embedded in the CMS.
    pub certificates: usize,
    /// The `/ByteRange` written: `[0 a b c]`.
    pub byte_range: [u64; 4],
    /// The DER `SignedData`'s size and the hole's capacity, in bytes.
    pub cms_bytes: usize,
    /// Bytes reserved (the hole holds `2 × reserved` hex digits).
    pub reserved_bytes: usize,
    /// The `/M` written, verbatim.
    pub signing_time: String,
    /// The PAdES level the material present supports — always `"B-B"` from
    /// this verb (`PC-12`: never claim higher than what was embedded).
    pub pades_level: &'static str,
    /// Whether `signature_verify` re-read the output and reported
    /// `Integrity::Verified` before it was returned. Always `true` on `Ok`;
    /// present so the fact is *stated*, not assumed.
    pub self_verified: bool,
    /// Signatures that already existed in the document before this one.
    pub prior_signatures: usize,
}

/// Why a document could not be signed. Every variant is a refusal by name.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SignApplyError {
    /// `signing_time` is not a PDF date string (`D:` + at least `YYYY`).
    /// PAdES requires `/M`; pdfcer will not invent one.
    #[error(
        "the signing time {given:?} is not a PDF date string (D:YYYYMMDDHHmmSSOHH'mm'); PAdES requires /M and pdfcer reads no clock"
    )]
    SigningTimeNotPdfDate {
        /// What was given.
        given: String,
    },
    /// The document is encrypted. pdfcer's incremental writer refuses an
    /// encrypted base (`WriteError::EncryptedSaveUnsupported`), and signing
    /// must be incremental — so today an encrypted document cannot be
    /// signed at all. Acrobat gates this on the document's permission bits;
    /// pdfcer's refusal is broader and says so.
    #[error(
        "the document is encrypted; pdfcer cannot yet append an incremental update to an encrypted file, and a signature must be an incremental update"
    )]
    Encrypted,
    /// A certification signature's `/DocMDP` `/P` forbids adding a
    /// signature (`P = 1`, no changes) — or the certifying tier does not
    /// permit it.
    #[error(
        "the document is certified with /DocMDP permission {permission}, which does not allow another signature to be added"
    )]
    CertificationForbids {
        /// The `/P` value.
        permission: u8,
    },
    /// The document was loaded through cross-reference recovery, so an
    /// incremental update cannot be appended (decision 013); a full
    /// rewrite would destroy any existing signature.
    #[error(
        "the document's cross-reference table was rebuilt on load; an incremental update cannot be appended to it, and signing must be incremental"
    )]
    RecoveredBase,
    /// A deferred redaction is staged; sign after applying or cancelling it.
    #[error("a deferred redaction is pending; apply or cancel it before signing")]
    RedactionPending,
    /// The chosen field name already exists.
    #[error("a form field named {name:?} already exists; choose another signature field name")]
    FieldNameTaken {
        /// The colliding name.
        name: String,
    },
    /// `visible` named a page the document does not have.
    #[error("page {page} is out of range; the document has {count} page(s)")]
    PageOutOfRange {
        /// 0-based.
        page: usize,
        /// How many pages there are.
        count: usize,
    },
    /// The signer's key cannot do the requested algorithm.
    #[error(transparent)]
    Sign(#[from] super::SignError),
    /// The CMS could not be built.
    #[error(transparent)]
    Cms(#[from] super::cms_build::CmsBuildError),
    /// The DER did not fit the reserved hole (`SC-6`).
    #[error(
        "the signature is {needed} bytes but only {reserved} were reserved; sign again with a larger reserve — the hole cannot be grown after layout"
    )]
    ReservationTooSmall {
        /// DER size.
        needed: usize,
        /// The reservation.
        reserved: usize,
    },
    /// The serialized revision did not contain the placeholder where
    /// expected — an internal inconsistency, reported rather than patched
    /// around.
    #[error(
        "internal: the signature placeholder was not found in the serialized revision ({what})"
    )]
    PlaceholderNotFound {
        /// Which token.
        what: &'static str,
    },
    /// After back-patching, pdfcer's own verifier did not report the
    /// signature as `Verified`. Nothing is returned — a file that pdfcer
    /// itself cannot verify is not handed out as signed.
    #[error("self-verification of the freshly written signature failed: {reason}")]
    SelfVerificationFailed {
        /// The verifier's integrity verdict, rendered.
        reason: String,
    },
    /// The session could not stage or serialize (an edit-layer error).
    #[error(transparent)]
    Edit(#[from] crate::edit::EditError),
    /// The writer refused.
    #[error(transparent)]
    Write(#[from] crate::writer::WriteError),
}

/// The `/ByteRange` sentinel: ten digits, so a zero-padded real value is
/// the same width. Larger than any file pdfcer will sign (≈ 953 MiB).
pub(crate) const BYTE_RANGE_SENTINEL: i64 = 1_000_000_000;

/// Where the placeholder tokens sit in a serialized revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Hole {
    /// Offset of `<`.
    pub start: usize,
    /// One past `>`.
    pub end: usize,
    /// Offset of the first sentinel digit inside `/ByteRange [0 ` — the
    /// three sentinels follow, each `1000000000` separated by one space.
    pub byte_range_digits: usize,
}

/// Find the signature object's `/Contents` hole and `/ByteRange` sentinels
/// in `bytes`, searching from `revision_start` (the base file's length —
/// the appended revision begins there, and the base is never scanned so a
/// prior signature's identical tokens cannot be mistaken for ours).
///
/// `reserve` is the request's reservation in bytes; the hole must be
/// exactly `2 × reserve` zeros between the delimiters.
pub(crate) fn locate_hole(
    bytes: &[u8],
    revision_start: usize,
    sig_id: crate::object::ObjId,
    reserve: usize,
) -> Result<Hole, SignApplyError> {
    let tail = bytes.get(revision_start..).unwrap_or(&[]);
    let header = format!("{} {} obj", sig_id.num, sig_id.generation).into_bytes();
    let obj_at = find(tail, &header, 0).ok_or(SignApplyError::PlaceholderNotFound {
        what: "object header",
    })?;
    let contents_at = find(tail, b"/Contents <", obj_at)
        .ok_or(SignApplyError::PlaceholderNotFound { what: "/Contents" })?
        + "/Contents ".len();
    let expected_zeros = reserve * 2;
    let close = contents_at + 1 + expected_zeros;
    let hole_ok = tail
        .get(contents_at + 1..close)
        .is_some_and(|z| z.iter().all(|&b| b == b'0'))
        && tail.get(close) == Some(&b'>');
    if !hole_ok {
        return Err(SignApplyError::PlaceholderNotFound {
            what: "/Contents zero hole",
        });
    }
    let br_at = find(tail, b"/ByteRange [0 ", obj_at)
        .ok_or(SignApplyError::PlaceholderNotFound { what: "/ByteRange" })?
        + "/ByteRange [0 ".len();
    let sentinel = BYTE_RANGE_SENTINEL.to_string();
    let expected = format!("{sentinel} {sentinel} {sentinel}]");
    if tail.get(br_at..br_at + expected.len()) != Some(expected.as_bytes()) {
        return Err(SignApplyError::PlaceholderNotFound {
            what: "/ByteRange sentinels",
        });
    }
    Ok(Hole {
        start: revision_start + contents_at,
        end: revision_start + close + 1,
        byte_range_digits: revision_start + br_at,
    })
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(from);
    }
    hay.get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// Overwrite the three `/ByteRange` sentinels with the real values as
/// zero-padded ten-digit integers, in place. Returns the `[0 a b c]` written.
pub(crate) fn patch_byte_range(bytes: &mut [u8], hole: Hole) -> [u64; 4] {
    let a = hole.start as u64;
    let b = hole.end as u64;
    let c = bytes.len() as u64 - b;
    let text = format!("{a:010} {b:010} {c:010}");
    if let Some(slot) = bytes.get_mut(hole.byte_range_digits..hole.byte_range_digits + text.len()) {
        slot.copy_from_slice(text.as_bytes());
    }
    [0, a, b, c]
}

/// The digest over the two spans — everything except `[hole.start, hole.end)`.
pub(crate) fn digest_spans(bytes: &[u8], hole: Hole, algorithm: SignatureAlgorithm) -> Vec<u8> {
    let span1 = bytes.get(..hole.start).unwrap_or(&[]);
    let span2 = bytes.get(hole.end..).unwrap_or(&[]);
    algorithm.digest(&[span1, span2].concat())
}

/// Write `der` into the hole as upper-case hex, zero-padded to the hole's
/// width. Refuses (without touching `bytes`) when it does not fit.
pub(crate) fn back_patch(bytes: &mut [u8], hole: Hole, der: &[u8]) -> Result<(), SignApplyError> {
    let capacity = (hole.end - hole.start).saturating_sub(2); // minus < >
    let hex: String = der.iter().map(|b| format!("{b:02X}")).collect();
    if hex.len() > capacity {
        return Err(SignApplyError::ReservationTooSmall {
            needed: der.len(),
            reserved: capacity / 2,
        });
    }
    if let Some(slot) = bytes.get_mut(hole.start + 1..hole.start + 1 + hex.len()) {
        slot.copy_from_slice(hex.as_bytes());
    }
    Ok(())
}

/// A permissive PDF-date shape check: `D:` then at least four digits, and
/// nothing but digits, `+`, `-`, `Z`, `'` after. Not a calendar — §7.9.4's
/// grammar is what a verifier reads, and rule 4 is served by refusing an
/// obviously-not-a-date rather than by validating February.
pub(crate) fn looks_like_pdf_date(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("D:") else {
        return false;
    };
    rest.len() >= 4
        && rest.chars().take(4).all(|c| c.is_ascii_digit())
        && rest
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | 'Z' | '\''))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn fake_revision(reserve: usize) -> (Vec<u8>, usize) {
        let base = b"%PDF-1.7\nbase bytes here\n".to_vec();
        let zeros = "0".repeat(reserve * 2);
        let s = BYTE_RANGE_SENTINEL;
        let rev = format!(
            "12 0 obj\n<< /Type /Sig /Filter /Adobe.PPKLite /ByteRange [0 {s} {s} {s}] /Contents <{zeros}> >>\nendobj\nxref\n%%EOF\n"
        );
        let start = base.len();
        ([base, rev.into_bytes()].concat(), start)
    }

    #[test]
    fn the_hole_is_found_patched_and_the_byte_range_reaches_eof() {
        let (mut bytes, start) = fake_revision(8);
        let hole = locate_hole(&bytes, start, crate::object::ObjId::new(12, 0), 8).unwrap();
        assert_eq!(&bytes[hole.start..hole.start + 1], b"<");
        assert_eq!(&bytes[hole.end - 1..hole.end], b">");
        assert_eq!(hole.end - hole.start, 18);
        let len_before = bytes.len();
        let br = patch_byte_range(&mut bytes, hole);
        assert_eq!(bytes.len(), len_before, "same-length overwrite");
        assert_eq!(br[1] as usize, hole.start);
        assert_eq!(br[2] as usize, hole.end);
        assert_eq!(br[2] + br[3], bytes.len() as u64, "second span reaches EOF");
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains(&format!(
            "/ByteRange [0 {:010} {:010} {:010}]",
            br[1], br[2], br[3]
        )));
        // Back-patch 8 bytes exactly fills; 9 refuses without writing.
        let snapshot = bytes.clone();
        assert!(matches!(
            back_patch(&mut bytes, hole, &[0xAB; 9]),
            Err(SignApplyError::ReservationTooSmall {
                needed: 9,
                reserved: 8
            })
        ));
        assert_eq!(bytes, snapshot, "a refusal writes nothing");
        back_patch(&mut bytes, hole, &[0xAB, 0xCD]).unwrap();
        assert_eq!(
            &bytes[hole.start..hole.end],
            b"<ABCD000000000000>",
            "ABCD + 12 pad zeros = 16 hex digits = 8 bytes"
        );
    }

    #[test]
    fn the_digest_skips_exactly_the_hole_including_delimiters() {
        let (bytes, start) = fake_revision(2);
        let hole = locate_hole(&bytes, start, crate::object::ObjId::new(12, 0), 2).unwrap();
        let d = digest_spans(&bytes, hole, SignatureAlgorithm::RsaPkcs1v15Sha256);
        let manual = SignatureAlgorithm::RsaPkcs1v15Sha256
            .digest(&[&bytes[..hole.start], &bytes[hole.end..]].concat());
        assert_eq!(d, manual);
        // Changing a byte INSIDE the hole must not change the digest.
        let mut inside = bytes.clone();
        inside[hole.start + 1] = b'F';
        assert_eq!(
            digest_spans(&inside, hole, SignatureAlgorithm::RsaPkcs1v15Sha256),
            d
        );
        // Changing the '<' itself must — it is inside the gap, so the digest
        // is unchanged too (the delimiters are part of the excluded range).
        let mut delim = bytes.clone();
        delim[hole.start] = b'(';
        assert_eq!(
            digest_spans(&delim, hole, SignatureAlgorithm::RsaPkcs1v15Sha256),
            d
        );
        // But a byte just before it does.
        let mut before = bytes.clone();
        before[hole.start - 1] ^= 1;
        assert_ne!(
            digest_spans(&before, hole, SignatureAlgorithm::RsaPkcs1v15Sha256),
            d
        );
    }

    #[test]
    fn a_wrong_object_or_a_short_hole_is_not_found() {
        let (bytes, start) = fake_revision(4);
        assert!(matches!(
            locate_hole(&bytes, start, crate::object::ObjId::new(13, 0), 4),
            Err(SignApplyError::PlaceholderNotFound {
                what: "object header"
            })
        ));
        assert!(matches!(
            locate_hole(&bytes, start, crate::object::ObjId::new(12, 0), 5),
            Err(SignApplyError::PlaceholderNotFound {
                what: "/Contents zero hole"
            })
        ));
        // Searching from the base would find nothing because the base has no
        // such object; the scan never looks before `revision_start`.
        assert!(locate_hole(&bytes, bytes.len(), crate::object::ObjId::new(12, 0), 4).is_err());
    }

    #[test]
    fn pdf_date_shape() {
        assert!(looks_like_pdf_date("D:20260905120000Z"));
        assert!(looks_like_pdf_date("D:20260905120000-05'00'"));
        assert!(looks_like_pdf_date("D:2026"));
        assert!(!looks_like_pdf_date("2026-09-05"));
        assert!(!looks_like_pdf_date("D:202"));
        assert!(!looks_like_pdf_date("D:2026x"));
    }
}
