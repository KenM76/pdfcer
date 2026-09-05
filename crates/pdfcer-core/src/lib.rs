//! # pdfcer-core — the GUI-agnostic PDF engine
//!
//! This crate is the heart of pdfcer (docs/ARCHITECTURE.md §3, §4). Its
//! eventual scope is the full COS object model, tokenizer, cross-reference
//! parsing (classic tables and xref streams), object streams, an
//! incremental-update writer, filters, fonts, colour spaces, encryption,
//! digital-signature verification, and a content-stream interpreter that
//! emits a draw-op stream (never pixels — rasterization lives in the
//! separate `pdfcer-render` crate).
//!
//! ## Load-bearing invariant
//!
//! `pdfcer-core` **must not** depend on any GUI/windowing crate
//! (egui/eframe/winit/wgpu). This is what keeps the future web fork a
//! shell-crate swap instead of a rewrite (docs/ARCHITECTURE.md §3). CI
//! greps `cargo tree -p pdfcer-core` to enforce it. The only dependency at
//! Pass 0 is `thiserror` (a compile-time derive macro, no runtime/GUI
//! surface).
//!
//! ## Pass 0 scope (this file)
//!
//! Pass 0 is the workspace bootstrap. The only real behaviour implemented
//! here is **header probing**: given the leading bytes of a file (or a
//! path), confirm the `%PDF-` marker and extract the declared version.
//! This backs both front ends' Pass 0 acceptance bar (the GUI "Open File"
//! flow and `pdfcer inspect`) without yet standing up the tokenizer or
//! object parser — those arrive in Pass 1 (docs/ROADMAP.md).
//!
//! Deliberately **not** done here yet: validating that the rest of the
//! file is well-formed, locating the `startxref`/trailer, or reading any
//! object. A successful probe means only "this looks like a PDF and
//! declares version M.N", which is exactly the Pass 0 contract.
//!
//! ## Spec basis
//!
//! The file header is specified in ISO 32000-1:2008 §7.5.2 ("File
//! header"; see `iso32000__s__7.5.md` in the PDF-spec RAG at
//! `D:\Dev\Rag-Specialized\PDF_Spec\`): the first line of a PDF file is
//! `%PDF-` followed by a version number of the form `1.N` (PDF 1.x) —
//! ISO 32000-2 adds `2.0`. Per the spec the marker is at byte offset 0.
//!
//! **The 1024-byte tolerance window is NOT spec text.** Real-world
//! producers sometimes emit leading bytes (a UTF-8 BOM, stray whitespace)
//! before the marker, and mainstream readers — following Acrobat's
//! implementation practice — accept the marker anywhere within roughly
//! the first 1024 bytes. This probe matches that common practice rather
//! than demanding the marker at byte 0. (An earlier revision of this
//! module miscited the window as ISO 32000-2 §7.5.2; the spec-RAG build
//! of 2026-07-30 could not verify any such clause — the window is
//! empirical, and is recorded as such here and in `C:\personal_rag\pdf\`.)
//!
//! Open question deliberately deferred to the Pass 1 xref work: when the
//! header is NOT at byte 0, are the file's byte offsets (`startxref`,
//! xref entries) relative to byte 0 or to the `%PDF-` marker? The spec
//! assumes byte 0; real producers may disagree. The probe itself doesn't
//! care, but the xref parser must decide (and possibly try both).

// Panic-free library policy (docs/decisions/001-oxidize-pdf-adopt-vs-build.md
// §6.1 item 5, serving docs/ARCHITECTURE.md §10's adversarial-input posture):
// pdfcer-core parses untrusted input, so a panic reachable from library code is
// a denial-of-service bug, not a style issue. `unwrap`/`expect`/`panic!` and
// unchecked indexing/slicing are DENIED crate-wide; fallible paths must return
// `Result` and bounds-dependent accesses must use `.get(..)`-style checked
// forms. Tests are exempt (a panicking test is just a failing test) via the
// `#[allow]` on the `tests` module below.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

pub mod annot;
pub mod annot_author;
mod asn1;
pub mod attachments;
/// Build provenance — what this binary is and when it was made
/// (`Pass 101.0`; see the module's own docs for the `iccce` question).
pub mod build;
/// Unix timestamp to RFC 3339 UTC, shared with this crate's BUILD SCRIPT via
/// `include!` so the calendar arithmetic exists once and stays testable.
///
/// Private: it serves the build stamp and, since `Pass 119.0`, the
/// `/LastModified` bump a form-XObject content edit owes a `/PieceInfo`
/// holder (ISO 32000-1 14.5). Still private — a general-purpose date formatter
/// is not something `pdfcer-core` should be offering. The file's own header
/// comments carry the reasoning and the leap-year cases.
mod civil_time;
mod cms;
pub mod color;
pub mod content;
pub mod crypto;
pub mod dimension;
pub mod document;
pub mod edit;
pub mod editable;
pub mod export;
pub mod fdf;
pub mod filters;
pub mod font_embed;
pub mod font_embed_missing;
pub mod font_unembed;
pub mod fontdata;
pub mod fontinfo;
pub mod form_script;
pub mod formclip;
pub mod formcsv;
pub mod forms;
pub mod forms_author;
pub mod function;
pub mod graph;
pub mod image_codec;
pub mod image_import;
pub mod layers;
pub mod lexer;
pub mod linearization;
pub mod linebreak;
pub mod object;
pub mod objstm;
/// OCR text layers — turning recognised words into an invisible, selectable
/// layer over an untouched scan (ISO 32000-1 §9.3.6 mode 3). Engine-agnostic:
/// the recogniser is a trait, so the engine choice stays a separate decision.
pub mod ocr;
pub mod outline;
pub mod page_tree;
pub mod pageops;
pub mod paper;
pub mod parser;
pub mod recover;
pub mod redact;
mod redact_image;
mod redact_vector;
pub mod richtext;
pub mod settings;
pub mod signature;
pub mod signature_verify;
pub mod span;
pub mod structure;
pub mod text_edit;
pub mod text_extract;
pub mod text_state;
pub mod textstring;
pub mod trust_chain;
pub mod trust_store;
pub mod vartext;
pub mod vector;
pub mod view;
pub mod wrapper;
pub mod writer;
pub mod xref;

use std::fmt;
use std::path::Path;

/// The maximum number of leading bytes scanned for the `%PDF-` header.
///
/// Mainstream PDF readers accept the header marker anywhere within the
/// first 1024 bytes of the file to tolerate leading BOMs/whitespace that
/// some producers emit (see the module docs' "Spec basis" note). 1024 is
/// that conventional window; a marker not present within it is treated as
/// "not a PDF" ([`PdfError::MissingHeader`]).
///
/// This is also a cheap first line of the adversarial-input defence
/// described in docs/ARCHITECTURE.md §10: probing reads at most this many
/// bytes, so a hostile multi-gigabyte file cannot make the probe itself
/// allocate or scan without bound.
pub const HEADER_SCAN_WINDOW: usize = 1024;

/// A PDF version number as declared in the file header (`%PDF-M.N`).
///
/// This is the *declared* version from the header, not a judgement about
/// which features the file actually uses (a file can declare `1.4` yet be
/// upgraded in a later incremental update via the catalog's `/Version`
/// entry — that reconciliation is Pass 1+ work, out of scope for the Pass
/// 0 header probe).
///
/// # Examples
///
/// ```
/// use pdfcer_core::PdfVersion;
///
/// let v = PdfVersion { major: 1, minor: 7 };
/// assert_eq!(v.to_string(), "1.7");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PdfVersion {
    /// Major version digit (`1` for PDF 1.x, `2` for PDF 2.0).
    pub major: u8,
    /// Minor version digit (`0`–`7` for PDF 1.x, `0` for PDF 2.0).
    pub minor: u8,
}

impl fmt::Display for PdfVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// Errors that can arise from the Pass 0 header-probe surface.
///
/// Marked `#[non_exhaustive]` so that later Passes can add variants
/// (tokenizer errors, xref errors, decryption failures, …) without it
/// being a breaking change for downstream consumers such as `pdfcer`.
/// Consumers matching on this enum must therefore include a wildcard arm.
///
/// Follows the Rust API Guidelines (C-GOOD-ERR): every variant is
/// `Send + Sync + 'static` and the type implements [`std::error::Error`]
/// via `thiserror`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PdfError {
    /// The underlying file could not be read (not found, permission
    /// denied, an I/O failure mid-read, …). Carries the source
    /// [`std::io::Error`].
    #[error("I/O error reading PDF: {0}")]
    Io(#[from] std::io::Error),

    /// No `%PDF-` marker was found within the first
    /// [`HEADER_SCAN_WINDOW`] bytes — the input is not a PDF (or is
    /// damaged beyond recognition at its head). `searched` reports how
    /// many bytes were actually inspected (may be fewer than the window
    /// for a short file).
    #[error("not a PDF: no %PDF- header in the first {searched} bytes")]
    MissingHeader {
        /// Number of leading bytes actually inspected.
        searched: usize,
    },

    /// A `%PDF-` marker was found but the bytes after it were not a
    /// parseable `M.N` version number. `found` is a lossy-UTF-8 snapshot
    /// of the offending bytes, for diagnostics.
    #[error("malformed PDF version after %PDF- marker: {found:?}")]
    MalformedVersion {
        /// Lossy-decoded snapshot of the bytes that failed to parse.
        found: String,
    },
}

/// Probe the leading bytes of a candidate PDF and return its declared
/// version.
///
/// Scans up to the first [`HEADER_SCAN_WINDOW`] bytes of `prefix` for the
/// `%PDF-` marker (tolerating leading BOM/whitespace, per the module
/// docs), then parses the `M.N` version immediately following it.
///
/// This is a *probe*, not full validation: it says only "this looks like a
/// PDF declaring version M.N", which is the Pass 0 contract. It does not
/// read the cross-reference table, trailer, or any object.
///
/// # Errors
///
/// - [`PdfError::MissingHeader`] if no `%PDF-` marker appears within the
///   scanned window.
/// - [`PdfError::MalformedVersion`] if the marker is present but not
///   followed by a parseable `M.N` (e.g. `%PDF-` at end of input, or
///   `%PDF-x.y`).
///
/// # Examples
///
/// ```
/// use pdfcer_core::{probe_header, PdfVersion};
///
/// // A normal header at the start of the buffer.
/// let v = probe_header(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n").unwrap();
/// assert_eq!(v, PdfVersion { major: 1, minor: 7 });
///
/// // PDF 2.0, tolerated after a leading UTF-8 BOM.
/// let v = probe_header(b"\xEF\xBB\xBF%PDF-2.0\n").unwrap();
/// assert_eq!(v.to_string(), "2.0");
///
/// // Not a PDF at all.
/// assert!(probe_header(b"GIF89a").is_err());
/// ```
pub fn probe_header(prefix: &[u8]) -> Result<PdfVersion, PdfError> {
    const MARKER: &[u8] = b"%PDF-";

    // Checked-slicing forms per the crate-level `indexing_slicing` policy:
    // `.get(..N)` in place of `[..N]`, falling back to the full/empty slice
    // where the range provably cannot be exceeded anyway.
    let window = prefix.get(..HEADER_SCAN_WINDOW).unwrap_or(prefix);
    let Some(pos) = find_subslice(window, MARKER) else {
        return Err(PdfError::MissingHeader {
            searched: window.len(),
        });
    };
    // `pos` came from `find_subslice`, so `pos + MARKER.len()` is ≤
    // `window.len()` by construction; the `unwrap_or` arm is unreachable but
    // keeps the access checked rather than lint-suppressed.
    parse_version(window.get(pos + MARKER.len()..).unwrap_or(&[]))
}

/// Probe a COS-family file whose header is NOT `%PDF-` (`Pass 10.2`).
///
/// PDF, FDF (§12.7.7) and Adobe's PPKLITE address book
/// (`addressbook.acrodata`, ISO 32000-1 §7.5 object/xref grammar under a
/// `%PPKLITE-2.1` header) share the entire COS container — objects, classic
/// xref, trailer, `startxref`, `%%EOF` — and differ ONLY in the first-line
/// marker. This is [`probe_header`] with the marker parameterised: it scans the
/// same bounded window for the FIRST of `markers` that appears and parses the
/// `M.N` after it. The version token is cosmetic for a non-PDF (PPKLITE's is
/// `2.1`, unrelated to any PDF version); it is returned so the same
/// `Document` assembly path can be reused unchanged.
///
/// The offsets in a COS xref table are absolute from byte 0, so the header
/// must be parsed IN PLACE — a caller cannot substitute a `%PDF-` header of a
/// different length without invalidating every offset. That is why this is a
/// header-sniff seam, not a rewrite.
///
/// # Errors
///
/// - [`PdfError::MissingHeader`] if none of `markers` appears in the window.
/// - [`PdfError::MalformedVersion`] if a marker is present but not followed by
///   a parseable `M.N`.
pub fn probe_cos_header(prefix: &[u8], markers: &[&[u8]]) -> Result<PdfVersion, PdfError> {
    let window = prefix.get(..HEADER_SCAN_WINDOW).unwrap_or(prefix);
    for marker in markers {
        if let Some(pos) = find_subslice(window, marker) {
            return parse_version(window.get(pos + marker.len()..).unwrap_or(&[]));
        }
    }
    Err(PdfError::MissingHeader {
        searched: window.len(),
    })
}

/// Probe a PDF file on disk and return its declared version.
///
/// Opens `path`, reads at most [`HEADER_SCAN_WINDOW`] bytes, and delegates
/// to [`probe_header`]. Never reads more than the window, so it is safe to
/// call on arbitrarily large (or hostile) files — the read is bounded.
///
/// # Errors
///
/// - [`PdfError::Io`] if the file cannot be opened or read.
/// - Otherwise the same errors as [`probe_header`].
pub fn probe_file(path: &Path) -> Result<PdfVersion, PdfError> {
    use std::io::Read as _;

    let file = std::fs::File::open(path)?;
    let mut buf = Vec::with_capacity(HEADER_SCAN_WINDOW);
    // `.take(N)` bounds the read at the window size regardless of the
    // file's true length — the decompression-bomb-adjacent defence from
    // docs/ARCHITECTURE.md §10.1, applied to the probe itself.
    file.take(HEADER_SCAN_WINDOW as u64).read_to_end(&mut buf)?;
    probe_header(&buf)
}

/// Parse an `M.N` version number from the bytes immediately following the
/// `%PDF-` marker. Canonically both parts are single digits, but this
/// accepts multi-digit runs defensively and rejects anything that is not
/// `<digits> '.' <digits>`.
fn parse_version(bytes: &[u8]) -> Result<PdfVersion, PdfError> {
    // Only the first few bytes can legitimately belong to the version;
    // cap the snapshot so a malformed header can't produce a giant `found`
    // string in the error.
    let sample = bytes.get(..16).unwrap_or(bytes);

    let (Some(major), major_len) = take_u8(sample) else {
        return Err(malformed(sample));
    };
    // `major_len` counts bytes `take_u8` actually consumed from `sample`, so
    // both `get` calls below are in-bounds by construction; checked forms per
    // the crate-level `indexing_slicing` policy.
    let rest = sample.get(major_len..).unwrap_or(&[]);
    if rest.first() != Some(&b'.') {
        return Err(malformed(sample));
    }
    let (Some(minor), _) = take_u8(rest.get(1..).unwrap_or(&[])) else {
        return Err(malformed(sample));
    };
    Ok(PdfVersion { major, minor })
}

/// Consume a leading run of ASCII digits and return its value plus the
/// number of bytes consumed. Returns `(None, _)` if there was no leading
/// digit or the value did not fit in a `u8` (no legitimate PDF version
/// component exceeds a single digit, so overflow here means garbage).
fn take_u8(bytes: &[u8]) -> (Option<u8>, usize) {
    let mut value: u32 = 0;
    let mut consumed = 0;
    for &b in bytes {
        if b.is_ascii_digit() {
            value = value.saturating_mul(10).saturating_add(u32::from(b - b'0'));
            consumed += 1;
        } else {
            break;
        }
    }
    if consumed == 0 {
        (None, 0)
    } else {
        (u8::try_from(value).ok(), consumed)
    }
}

/// Build a [`PdfError::MalformedVersion`] with a lossy snapshot of the
/// offending bytes.
fn malformed(sample: &[u8]) -> PdfError {
    PdfError::MalformedVersion {
        found: String::from_utf8_lossy(sample).into_owned(),
    }
}

/// Return the index of the first occurrence of `needle` in `haystack`, or
/// `None`. A small naive substring search — the search space is bounded by
/// [`HEADER_SCAN_WINDOW`] (≤ 1 KiB) and `needle` is the 5-byte `%PDF-`
/// marker, so a specialised algorithm would be pointless here.
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
// Tests are exempt from the panic-free policy: a panicking assertion IS the
// test-failure mechanism (see the crate-level lint rationale above).
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_1_7_header() {
        let v = probe_header(b"%PDF-1.7\nrest of file").unwrap();
        assert_eq!(v, PdfVersion { major: 1, minor: 7 });
    }

    #[test]
    fn parses_pdf_2_0_header() {
        let v = probe_header(b"%PDF-2.0\n").unwrap();
        assert_eq!(v, PdfVersion { major: 2, minor: 0 });
        assert_eq!(v.to_string(), "2.0");
    }

    #[test]
    fn tolerates_leading_bom_before_marker() {
        // UTF-8 BOM (EF BB BF) preceding the header — a real producer quirk.
        let v = probe_header(b"\xEF\xBB\xBF%PDF-1.4\n").unwrap();
        assert_eq!(v, PdfVersion { major: 1, minor: 4 });
    }

    #[test]
    fn rejects_non_pdf() {
        let err = probe_header(b"GIF89a...").unwrap_err();
        assert!(matches!(err, PdfError::MissingHeader { searched } if searched == 9));
    }

    #[test]
    fn rejects_marker_without_version() {
        let err = probe_header(b"%PDF-").unwrap_err();
        assert!(matches!(err, PdfError::MalformedVersion { .. }));
    }

    #[test]
    fn rejects_marker_with_nondigit_version() {
        let err = probe_header(b"%PDF-x.y").unwrap_err();
        assert!(matches!(err, PdfError::MalformedVersion { .. }));
    }

    #[test]
    fn missing_header_searched_count_is_capped_at_window() {
        // A buffer larger than the scan window with no marker: `searched`
        // must report the window size, not the full buffer length.
        let big = vec![b'x'; HEADER_SCAN_WINDOW * 4];
        let err = probe_header(&big).unwrap_err();
        assert!(
            matches!(err, PdfError::MissingHeader { searched } if searched == HEADER_SCAN_WINDOW)
        );
    }

    #[test]
    fn version_ordering_is_sensible() {
        assert!(PdfVersion { major: 1, minor: 4 } < PdfVersion { major: 1, minor: 7 });
        assert!(PdfVersion { major: 1, minor: 7 } < PdfVersion { major: 2, minor: 0 });
    }
}
