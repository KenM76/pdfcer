//! Fuzz target: the JPEG quality estimator
//! (`pdfcer_core::optimize::estimate_jpeg_quality`), which walks the marker
//! segments of an untrusted `/DCTDecode` stream (ITU-T T.81 §B.1, §B.2.4.1).
//! Any input must return, never panic or read past the slice; a returned
//! quality is always 1 to 100.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::optimize::estimate_jpeg_quality;

fuzz_target!(|data: &[u8]| {
    if let Some(q) = estimate_jpeg_quality(data) {
        assert!((1..=100).contains(&q));
    }
    // The same bytes behind a valid SOI, so the walk gets past the first check.
    let mut framed = vec![0xFF, 0xD8];
    framed.extend_from_slice(data);
    let _ = estimate_jpeg_quality(&framed);
});
