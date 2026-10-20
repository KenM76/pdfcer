//! Fuzz target: the space audit (`pdfcer_core::space_audit`).
//!
//! Any input that loads is audited. The audit scans the raw file bytes
//! between objects, splits object-stream containers among their members
//! and walks page content and nested forms for image placements, so it
//! must never panic, and its categories must sum to the file length.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::space_audit::audit;

fuzz_target!(|data: &[u8]| {
    if let Ok(doc) = Document::from_bytes(data.to_vec()) {
        let report = audit(&doc);
        let total: u64 = report.categories.iter().map(|c| c.bytes).sum();
        assert_eq!(total, report.file_bytes);
    }
});
