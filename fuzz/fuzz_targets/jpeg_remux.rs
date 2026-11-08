//! Fuzz target: `pdfcer_jpeg_remux::interleave` over untrusted JPEG bytes.
//!
//! It must never panic. When it rewrites a stream, the output must be
//! one it would not rewrite again: a single interleaved scan.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(Some(out)) = pdfcer_jpeg_remux::interleave(data, 1 << 16) {
        assert_eq!(pdfcer_jpeg_remux::interleave(&out, 1 << 16), Ok(None));
    }
});
