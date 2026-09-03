//! Fuzz target: cross-reference RECOVERY parse → recover → re-load
//! (decision 013 Pass B).
//!
//! `Document::from_bytes` already routes an unparseable-xref file through
//! rebuild-by-scan recovery, so `load_document` fuzzes the *entry* to this
//! path for free. This target asserts the recovery-specific invariants that
//! only fire once a document has actually been recovered:
//!
//! 1. **Recovery never panics and always terminates.** For ANY input,
//!    `from_bytes` returns `Ok(Document)` or a structured `Err(DocError)`
//!    (including the fail-clean `DocError::Recovery(_)` refusals). The
//!    O(n) single-pass scan + `MAX_XREF_ENTRIES` cap + no-catalog refusal
//!    bound the work (R25).
//! 2. **A recovered document REFUSES incremental save by name** — the
//!    recovered-base rule (decision 013 §9). Its base cross-reference was
//!    invalid, so an incremental append is structurally impossible; the
//!    guard returns `WriteError::RecoveredBaseForbidsIncremental`
//!    unconditionally, so anything else (an `Ok`, or a different error) is
//!    a regression and panics here.
//! 3. **`save_full` on a recovered document terminates**, and whatever
//!    bytes it produces re-load without panicking — the full-rewrite path
//!    must never emit a document pdfcer cannot at least attempt to read
//!    back.
//!
//! A non-recovered load is out of scope here (covered by `load_document` /
//! `writer_roundtrip`) and returns early.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::writer::{DirtySet, SaveOptions, WriteError, save_full, save_incremental};

fuzz_target!(|data: &[u8]| {
    // Invariant 1: from_bytes is total — never panics, always terminates.
    let Ok(doc) = Document::from_bytes(data.to_vec()) else {
        return;
    };
    // Only recovered documents exercise the Pass B invariants.
    if !doc.loaded_via_recovery() {
        return;
    }

    // Invariant 2: incremental save is refused BY NAME on a recovered doc.
    match save_incremental(&doc, &DirtySet::empty(), &SaveOptions::identity()) {
        Err(WriteError::RecoveredBaseForbidsIncremental) => {}
        other => panic!("recovered document did not refuse incremental save by name: {other:?}"),
    }

    // Invariant 3: save_full terminates; whatever it produces re-loads
    // without panicking (it may itself be Ok or a clean Err).
    if let Ok((bytes, _report)) = save_full(&doc, &DirtySet::empty(), &SaveOptions::identity()) {
        let _ = Document::from_bytes(bytes);
    }
});
