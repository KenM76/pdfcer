//! Fuzz target: arbitrary bytes → `Document` → first page → EMF export
//! (`Pass 248.4`).
//!
//! The metafile writer walks the export recording and serialises integer
//! records from float geometry, replays arbitrary op subsets into scratch
//! pixmaps, and crops them by their painted box. Every one of those is a
//! place an adversarial page can push a value out of range, so the writer
//! gets its own target beside `export_svg`.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_render::RenderOptions;
use pdfcer_render::emf::{EmfOptions, export_emf, walk_records};

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
    if let Ok(out) = export_emf(
        &doc,
        page,
        &RenderOptions::default(),
        &EmfOptions::default().with_raster_dpi(72.0),
    ) {
        // Whatever the input, the output is a well-formed record chain.
        assert!(walk_records(&out.emf).is_some());
    }
});
