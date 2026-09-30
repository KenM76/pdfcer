//! Fuzz target: embedded 3D listing and extraction (`pdfcer_core::threed`,
//! ISO 32000-1 §13.6, ISO 32000-2 §13.7).
//!
//! Loads arbitrary bytes, lists every 3D artwork, and extracts each one.
//! Invariant: never panics, never loops; the listing stays within
//! `MAX_3D_ARTWORKS`, and extraction is bounded by the filter ceiling.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::threed::{MAX_3D_ARTWORKS, extract_3d, list_3d_with_notes};

fuzz_target!(|data: &[u8]| {
    let Ok(doc) = Document::from_bytes(data.to_vec()) else {
        return;
    };
    let (found, _notes) = list_3d_with_notes(&doc);
    assert!(found.len() <= MAX_3D_ARTWORKS);
    let view = doc.view();
    for art in &found {
        if let Ok(got) = extract_3d(&view, art) {
            let _ = got.contradicts(art.declared.as_ref());
        }
    }
});
