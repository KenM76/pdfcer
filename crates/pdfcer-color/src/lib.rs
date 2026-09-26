//! pdfcer colour: device colour-space conversion (ISO 32000-1 8.6.4), the calibrated CMYK table and rendering intents (8.6.5.8).
//!
//! `pdfcer-core` re-exports every module at its old path
//! (`docs/ARCHITECTURE.md` §3). No GUI or network dependency; builds for
//! `wasm32-unknown-unknown`.

#![forbid(unsafe_code)]

pub mod color;
