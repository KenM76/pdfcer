//! # pdfcer-core — the GUI-agnostic PDF engine, as one facade
//!
//! The editing layer ([`edit::EditSession`] and every verb behind it) lives
//! here. The layers below it are separate crates, each re-exported at its
//! `pdfcer_core::<module>` path, so a consumer depends on this crate alone
//! and never names the others (`docs/ARCHITECTURE.md` §3).
//!
//! | modules | crate |
//! |---|---|
//! | `content`, `crypto`, `document`, `filters`, `graph`, `lexer`, `linearization`, `object`, `objstm`, `page_tree`, `parser`, `recover`, `span`, `view`, `writer`, `xref`, header probing | `pdfcer-model` |
//! | `image_codec` (`ccitt`, `dct`, `jbig2`, `jpx`) | `pdfcer-image-codec` |
//! | `font_embed`, `fontdata`, `fontinfo`, `linebreak`, `textstring`, `vartext` | `pdfcer-fonts` |
//! | `trust_chain`, `trust_store` (and the private `asn1`, `cms`) | `pdfcer-pkix` |
//! | `color` | `pdfcer-color` |
//! | `text_extract`, `text_state` | `pdfcer-text` |
//! | `function` | `pdfcer-function` |
//! | everything else | this crate |
//!
//! Items marked `#[doc(hidden)]` with a "workspace-internal" comment are
//! `pub` only so a sibling crate can call them. They are not API.
//!
//! ## Load-bearing invariants
//!
//! No GUI/windowing dependency (egui, eframe, winit, wgpu) and no network
//! client, in this crate or any it re-exports; every one builds for
//! `wasm32-unknown-unknown`. CI checks `cargo tree` and the wasm32 build.

// Panic-free library policy (docs/decisions/001-oxidize-pdf-adopt-vs-build.md
// §6.1 item 5, serving docs/ARCHITECTURE.md §10's adversarial-input posture):
// pdfcer-core parses untrusted input, so a panic reachable from library code is
// a denial-of-service bug, not a style issue. `unwrap`/`expect`/`panic!` and
// unchecked indexing/slicing are DENIED crate-wide; fallible paths must return
// `Result` and bounds-dependent accesses must use `.get(..)`-style checked
// forms. Tests are exempt (a panicking test is just a failing test) via an
// `#[allow]` on each `tests` module.
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

pub mod annot;
pub mod annot_author;
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
pub mod vector;
pub mod wrapper;

// The facade: each lower crate's modules at their `pdfcer_core::` paths.
// `tests/facade_paths.rs` fails if one stops resolving.
pub use pdfcer_color::color;
pub use pdfcer_fonts::{font_embed, fontdata, fontinfo, linebreak, textstring, vartext};
pub use pdfcer_function::function;
pub use pdfcer_image_codec as image_codec;
pub use pdfcer_model::{
    HEADER_SCAN_WINDOW, PdfError, PdfVersion, probe_cos_header, probe_file, probe_header,
};
pub use pdfcer_model::{
    content, crypto, document, filters, graph, lexer, linearization, object, objstm, page_tree,
    parser, recover, span, view, writer, xref,
};
use pdfcer_pkix::{asn1, cms};
pub use pdfcer_pkix::{trust_chain, trust_store};
pub use pdfcer_text::{text_extract, text_state};
