//! Fuzz target: the whole-document print questions (`pdfcer_core::prepress`).
//!
//! Any input that loads is asked both: the overprint walk follows every
//! drawn stream and the PDF/X reader scans the XMP packet and Info
//! dictionary, so neither may panic, and a found request must be complete.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::prepress::{overprint_use, pdfx_claim};

fuzz_target!(|data: &[u8]| {
    if let Ok(doc) = Document::from_bytes(data.to_vec()) {
        let view = doc.view();
        let found = overprint_use(&view);
        assert!(!found.asks() || found.is_complete());
        if let Some(claim) = pdfx_claim(&view) {
            assert!(!claim.level.is_empty() && claim.level.len() <= 64);
        }
    }
});
