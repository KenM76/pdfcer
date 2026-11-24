//! Fuzz target: PDF/UA validation (`pdfcer_core::preflight::validate_pdfua`).
//! Untrusted input: structure trees with cycles and role-map loops,
//! self-drawing forms, field `/Parent` chains, hostile XMP and annotations.
//!
//! Invariants: no panic and termination for both parts; every finding's
//! rule id is an inventory row of the part checked; the person and
//! not-checked lists never overlap.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::preflight::{PdfuaPart, requirement, validate_pdfua};

fuzz_target!(|data: &[u8]| {
    let Ok(doc) = Document::from_bytes(data.to_vec()) else {
        return;
    };
    let view = doc.view();
    for part in [PdfuaPart::Ua1, PdfuaPart::Ua2] {
        let report = validate_pdfua(&view, part);
        for f in &report.findings {
            let row = requirement(&f.rule_id).expect("finding names an inventory row");
            assert_eq!(row.part, part, "{}", f.rule_id);
        }
        for r in &report.not_checked {
            assert!(
                report.findings.iter().all(|f| f.rule_id != r.id),
                "{}",
                r.id
            );
        }
    }
});
