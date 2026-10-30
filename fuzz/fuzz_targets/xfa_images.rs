//! Fuzz target: the `/Names /XFAImages` name tree an XFA image `href`
//! resolves through (`pdfcer_core::xfa::XfaImages::from_view`; Pass 647.4).
//!
//! The fuzz bytes are a whole PDF, so the tree's shape, cycles, key types
//! and stream filters are all attacker-chosen. Reading must never panic and
//! must stay inside the walk caps and `MAX_XFA_IMAGE_BYTES`
//! (ARCHITECTURE.md §10).

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::xfa::{MAX_XFA_IMAGE_BYTES, XfaImages};

fuzz_target!(|data: &[u8]| {
    if let Ok(doc) = Document::from_bytes(data.to_vec()) {
        let images = XfaImages::from_view(&doc.view());
        assert!(images.len() <= MAX_XFA_IMAGE_BYTES);
    }
});
