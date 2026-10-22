//! Fuzz target: the linearizing rebuild (`pdfcer_core::writer::rebuild` with
//! `RebuildOptions::linearize`, ISO 32000-2 Annex F).
//!
//! Feeds arbitrary bytes to `Document::from_bytes`, then linearizes. The
//! page-tree walk, attribute push-down, part classification and hint-table
//! packing all run on untrusted structure (cyclic `/Kids`, a page reached
//! twice, `/Thumb` and `/Outlines` sharing page objects).
//!
//! Invariants: never panics or loops; an `Ok` output reloads, and the
//! detector reports it `Live` (its `/L` equals its length).

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::linearization::{Linearization, detect};
use pdfcer_core::writer::{RebuildOptions, SaveOptions, rebuild};

fuzz_target!(|data: &[u8]| {
    let Ok(doc) = Document::from_bytes(data.to_vec()) else {
        return;
    };
    let options = RebuildOptions {
        linearize: true,
        ..RebuildOptions::all()
    };
    let Ok((bytes, report)) = rebuild(&doc, &options, &SaveOptions::default()) else {
        return;
    };
    assert!(report.linearized);
    assert!(
        matches!(detect(&bytes), Linearization::Live { .. }),
        "output is not live-linearized"
    );
    assert!(
        Document::from_bytes(bytes).is_ok(),
        "output does not reload"
    );
});
