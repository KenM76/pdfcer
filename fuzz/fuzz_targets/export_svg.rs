//! Fuzz target: arbitrary bytes → `Document` → first page → SVG export
//! (`Pass 248.1`).
//!
//! The SVG writer consumes the renderer's export recording, which runs the
//! full content-stream interpreter over untrusted input with a recorder
//! that NEVER refuses — every operator class that the cache recorder
//! poisons on is instead rasterised into a scratch and harvested. That is a
//! new code path over hostile bytes (the scratch harvest, the per-op mask
//! wrapping, the base64/PNG encoders on recorder output), so it gets its
//! own target rather than inheriting coverage from `load_document`.
//!
//! The export scale is kept at 72 DPI so a fuzz iteration stays cheap; the
//! recording code is scale-independent.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_render::RenderOptions;
use pdfcer_render::svg::{SvgOptions, export_svg};

fuzz_target!(|data: &[u8]| {
    let Ok(doc) = Document::from_bytes(data.to_vec()) else {
        return;
    };
    let Ok(pages) = pdfcer_core::page_tree::pages(&doc) else {
        return;
    };
    let Some(page) = pages.first() else {
        return;
    };
    let _ = export_svg(
        &doc,
        page,
        &RenderOptions::default(),
        &SvgOptions::default().with_raster_dpi(72.0),
    );
});
