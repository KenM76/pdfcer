//! Fuzz target: PDF/A validation (`pdfcer_core::preflight::validate_pdfa`).
//! Untrusted input: hostile XMP packets, ICC profile headers, deep colour
//! space and function arrays, embedded files carrying PDFs, field
//! `/Parent` chains and annotation dictionaries.
//!
//! Invariants: no panic and termination at every level; every finding's
//! rule id is a rule of that level's profile and carries its clause; the
//! verdict is never `Conforms`, because some rows are not checked yet.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::preflight::{PdfaLevel, PdfaVerdict, Profile, validate_pdfa};

fuzz_target!(|data: &[u8]| {
    let Ok(doc) = Document::from_bytes(data.to_vec()) else {
        return;
    };
    let view = doc.view();
    for level in PdfaLevel::ALL {
        let profile = Profile::pdfa(level);
        let report = validate_pdfa(&view, level);
        for f in &report.preflight.findings {
            assert!(
                profile.rules.iter().any(|r| r.id == f.rule_id),
                "{}",
                f.rule_id
            );
            assert!(report.clause_of(f).is_some(), "{}", f.rule_id);
        }
        assert_ne!(report.verdict(), PdfaVerdict::Conforms);
    }
});
