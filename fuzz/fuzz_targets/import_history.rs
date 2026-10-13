//! Fuzz target: `EditSession::import_history` (pdfcer-gui request G184).
//!
//! The input follows a valid header for a fixed synthetic base, so decoding
//! reaches the overlay, the command kinds and the object codec rather than
//! stopping at the digest. An accepted history is then walked through every
//! undo and redo and saved.
//!
//! Invariant (ARCHITECTURE.md §10): no panic and bounded work for any input.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::writer::SaveOptions;

const BASE: &[u8] = include_bytes!("../../fixtures/synthetic/addtext/plain.pdf");

/// Magic, version 1, the base length (LEB128) and its SHA-256: everything an
/// export of `BASE` writes before its first body field.
fn header_len() -> usize {
    let mut leb = 1;
    let mut v = BASE.len() >> 7;
    while v != 0 {
        leb += 1;
        v >>= 7;
    }
    10 + 1 + leb + 32
}

fuzz_target!(|data: &[u8]| {
    let Ok(doc) = Document::from_bytes(BASE.to_vec()) else {
        return;
    };
    let mut session = EditSession::new(doc);
    let header = session.export_history();
    let mut bytes = header[..header_len()].to_vec();
    bytes.extend_from_slice(data);
    if session.import_history(&bytes).is_err() {
        return;
    }
    let _ = session.to_incremental_bytes(&SaveOptions::identity());
    while session.undo().is_some() {}
    while session.redo().is_some() {}
    let _ = session.to_incremental_bytes(&SaveOptions::identity());
});
