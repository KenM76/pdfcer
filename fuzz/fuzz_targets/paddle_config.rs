//! Fuzz target: the `character_dict` reader for a PaddleOCR `inference.yml`
//! (`pdfcer_core::ocr::paddle_config`).
//!
//! The file comes with a downloaded or operator-supplied model set, so it is
//! untrusted. Invariant: for arbitrary text, `parse_config_dictionary` returns
//! `Ok` or `Err` and never panics; on `Ok` there is at least one entry, none
//! is empty, and there are at most `MAX_ENTRIES`.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::ocr::paddle_config::{MAX_ENTRIES, parse_config_dictionary};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    // Most inputs should reach the item reader, not the missing-key refusal.
    for text in [text.to_owned(), format!("character_dict:\n{text}")] {
        if let Ok(entries) = parse_config_dictionary(&text) {
            assert!(!entries.is_empty() && entries.len() <= MAX_ENTRIES);
            assert!(entries.iter().all(|e| !e.is_empty()));
        }
    }
});
