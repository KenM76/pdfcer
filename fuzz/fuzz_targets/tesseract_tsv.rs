//! Fuzz target: Tesseract TSV output (`pdfcer_core::ocr::tesseract_tsv`).
//!
//! The TSV comes from an external program — a bundled build or whatever the
//! operator points `--model-dir` at — so it is untrusted. Invariant: for any
//! UTF-8 input `parse_tsv` returns `Ok` or `TsvError` and never panics, and
//! every word it returns has a finite, non-inverted rectangle and a
//! confidence in 0..=1.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::ocr::tesseract_tsv::parse_tsv;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(words) = parse_tsv(text) {
        for w in words {
            let r = w.rect;
            assert!(
                r.llx.is_finite() && r.lly.is_finite() && r.urx.is_finite() && r.ury.is_finite()
            );
            assert!(r.llx <= r.urx && r.lly <= r.ury);
            if let Some(c) = w.confidence {
                assert!((0.0..=1.0).contains(&c));
            }
        }
    }
});
