//! Fuzz target: the Acrobat trust-store (`%PPKLITE-` address book) reader
//! (`pdfcer_core::trust_store::load_from_bytes`, `Pass 10.2`).
//!
//! The reader opens attacker-controlled bytes as a COS file through the same
//! tokenizer/xref path as a PDF (via `Document::from_cos_bytes`, header sniff
//! relaxed to `%PPKLITE-`/`%FDF-`) and decodes each `/Cert` literal string as
//! DER X.509. That is the whole untrusted-input surface of the feature: a
//! malformed address book, a truncated cert, a cyclic/oversized `/Entries`
//! array, a nonsense `/Trust`. The one invariant is the crate's panic-free
//! policy (X5): NO input may panic, hang, or over-allocate — a bad store is a
//! named error or an empty/partial anchor set, never a crash.
//!
//! The `MAX_ENTRIES` ceiling in the reader bounds the walk; this target
//! confirms the bound holds and that DER decode of arbitrary `/Cert` bytes
//! (delegated to the Pass 10.1 `cms` decoder) is itself panic-free.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::trust_store;

fuzz_target!(|data: &[u8]| {
    // Raw bytes as a would-be trust store. Any Ok set is walked so the
    // anchor-extraction path (source/trust/policy/cert decode) is exercised
    // too; any Err is a named refusal. Neither may panic.
    if let Ok(set) = trust_store::load_from_bytes(data.to_vec()) {
        let _ = set.counts();
        for a in &set.anchors {
            let _ = a.has_source("AATL");
            let _ = a.serial_hex.len();
        }
    }
});
