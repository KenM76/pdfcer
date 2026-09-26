//! pdfcer text: text extraction (ISO 32000-1 9.10), ToUnicode CMaps and the text-state model (9.3).
//!
//! `pdfcer-core` re-exports every module at its old path
//! (`docs/ARCHITECTURE.md` §3). No GUI or network dependency; builds for
//! `wasm32-unknown-unknown`.

#![forbid(unsafe_code)]
// Panic-free, as in pdfcer-core: this crate reads untrusted input, so a
// reachable panic is a denial-of-service bug. Tests opt out per module.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

pub mod text_extract;
pub mod text_state;
