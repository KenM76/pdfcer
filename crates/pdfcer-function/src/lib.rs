//! pdfcer PDF functions: sampled, exponential, stitching and PostScript calculator functions (ISO 32000-1 7.10).
//!
//! `pdfcer-core` re-exports the module at its old path
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

pub mod function;
