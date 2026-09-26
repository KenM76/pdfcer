//! pdfcer fonts and text encoding: standard-14 and embedded font data
//! (`fontdata`), the font inventory (`fontinfo`), font embedding
//! (`font_embed`), PDF text strings (`textstring`, ISO 32000-1 §7.9.2),
//! variable-text layout for form appearances (`vartext`, §12.7.3.3) and line
//! breaking (`linebreak`).
//!
//! Depends only on `pdfcer-model`; `pdfcer-core` re-exports every module at
//! its old path (`docs/ARCHITECTURE.md` §3). No GUI or network dependency,
//! and it builds for `wasm32-unknown-unknown`.

// Parses untrusted font and string data, so a reachable panic is a
// denial-of-service bug: unwrap/expect/panic!/unchecked indexing are denied
// crate-wide, and test modules opt out with their own #[allow].
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

pub mod font_embed;
pub mod fontdata;
pub mod fontinfo;
pub mod linebreak;
pub mod textstring;
pub mod vartext;
