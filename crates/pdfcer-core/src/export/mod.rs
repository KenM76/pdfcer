//! # export — pdfcer's document model written to formats other than PDF
//!
//! Everything under here is a WRITE path out of pdfcer's own model into a
//! foreign format. It is deliberately separate from `writer`, which owns
//! PDF-to-PDF serialisation and the round-trip/minimal-diff invariant
//! (`ARCHITECTURE.md` §5): those rules are about not disturbing a document
//! pdfcer did not change, and they have no meaning when the output is not a
//! PDF at all.
//!
//! Same GUI-core separation as the rest of `pdfcer-core`: no windowing
//! dependency ever reaches here, so the shells and the eventual WASM fork
//! all consume these through the same API.

pub mod dxf;
