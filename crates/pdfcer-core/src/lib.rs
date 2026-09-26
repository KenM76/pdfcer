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
pub mod dimension;
pub mod edit;
pub mod editable;
pub mod export;
pub mod fdf;
pub mod font_embed_missing;
pub mod font_unembed;
pub mod form_script;
pub mod formclip;
pub mod formcsv;
pub mod forms;
pub mod forms_author;
pub mod function;
pub use pdfcer_fonts::{font_embed, fontdata, fontinfo, linebreak, textstring, vartext};
pub use pdfcer_image_codec as image_codec;
pub mod image_import;
pub mod layers;
/// OCR text layers — turning recognised words into an invisible, selectable
/// layer over an untouched scan (ISO 32000-1 §9.3.6 mode 3). Engine-agnostic:
/// the recogniser is a trait, so the engine choice stays a separate decision.
pub mod ocr;
pub mod offpage;
pub mod outline;
pub mod pageops;
pub mod paper;
pub mod redact;
mod redact_image;
mod redact_vector;
pub mod richtext;
pub mod settings;
#[cfg(feature = "signing")]
pub mod sign;
pub mod signature;
pub mod signature_verify;
/// Acrobat-compatible stamp collection files -- one file per category,
/// one page per stamp, names in the catalog's `/Names` -> `/Pages` tree.
pub mod stamp_file;
pub mod structure;
pub mod text_edit;
pub mod text_extract;
pub mod text_state;
pub mod trust_chain;
pub mod trust_store;
pub mod vector;
pub mod wrapper;

pub use pdfcer_model::{
    HEADER_SCAN_WINDOW, PdfError, PdfVersion, probe_cos_header, probe_file, probe_header,
};
/// The COS model layer, re-exported from `pdfcer-model` so every
/// `pdfcer_core::<module>` path keeps resolving.
pub use pdfcer_model::{
    content, crypto, document, filters, graph, lexer, linearization, object, objstm, page_tree,
    parser, recover, span, view, writer, xref,
};
