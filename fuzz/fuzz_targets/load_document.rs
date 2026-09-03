//! Fuzz target: whole-document loading (`pdfcer_core::document`).
//!
//! Feeds arbitrary bytes to `Document::from_bytes`, which exercises the
//! full load pipeline end to end: the `%PDF-x.y` header probe
//! (`HEADER_SCAN_WINDOW`), `startxref` discovery, the xref chain walk
//! (classic §7.5.4 tables, §7.5.8 cross-reference **streams**, `/Prev`
//! chains, §7.5.8.4 hybrid `XRefStm` lookups), trailer parsing, the
//! eager parse of every reachable indirect object, and the resolution
//! of §7.5.7 **object streams** behind type-2 xref entries — i.e. every
//! guard the loader has (cycle detection on the xref chain, offset
//! bounds checks, object-count ceilings, `/W` and `/Index` validation,
//! `MAX_OBJSTM_OBJECTS`, and the filter layer's `MAX_DECODED_LEN`
//! ceiling, which xref streams and object streams both run through).
//!
//! Note that the PDF 1.5 paths are reached *through the same entry
//! point* — no new target is needed, and the existing corpus keeps its
//! value: any input that reaches a `startxref` pointing at an integer
//! now drives the cross-reference-stream decoder instead of stopping at
//! a refusal, so coverage of the new code comes for free.
//!
//! Invariant asserted (the crate's panic-free policy): for ANY input —
//! truncated, corrupted, adversarial, or not a PDF at all —
//! `Document::from_bytes` returns `Ok(Document)` or a structured
//! `Err(DocError)`; it never panics and never aborts. The loader's
//! documented resource ceilings must hold, so memory and time stay
//! bounded relative to input size; libFuzzer's default `-rss_limit_mb`
//! and `-timeout` turn any OOM or hang into a reported finding.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;

fuzz_target!(|data: &[u8]| {
    let _ = Document::from_bytes(data.to_vec());
});
