//! pdfcer 3D model decoding: the PRC format (Adobe PRC 8137, the text
//! ISO 14739-1 standardised), read side.
//!
//! [`PrcFile::parse`] walks the container: the uncompressed file header,
//! every file structure's header and its five zlib sections, and the model
//! file section, each inflated under [`MAX_INFLATED_BYTES`].
//!
//! Sources, cited per item as `[WD clause]` (ISO/TC171/SC2 N570, the PRC 8137
//! working draft) and `[PRCRS file]` (the MIT `prc-rs` reader, a permissive
//! code inference); the register is the spec RAG's
//! `threed/prc__8137__sources_provenance.md`, and decision 169 governs their
//! use. No GUI, network or thread dependency; builds for
//! `wasm32-unknown-unknown`.

#![forbid(unsafe_code)]
// Panic-free: this crate reads untrusted input, so a reachable panic is a
// denial-of-service bug. Tests opt out per module.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod acof;
mod arrays;
pub mod bits;
mod container;
mod error;
mod export;
mod schema;
mod tess;
#[cfg(test)]
mod testw;

pub use container::{
    FileStructure, MAX_FILE_STRUCTURES, MAX_INFLATED_BYTES, PRC_READER_VERSION, PrcFile, PrcHeader,
    SectionKind, UniqueId,
};
pub use error::PrcError;
pub use export::{to_obj, to_stl};
pub use schema::Schema;
pub use tess::{Tessellation, TriangleMesh};
