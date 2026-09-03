//! Fuzz target 14: FDF / XFDF form-data parsing
//! (`pdfcer_core::fdf`, ISO 32000-1 §12.7.7 + the XFDF XML subset; Pass 7.1).
//!
//! Feeds arbitrary bytes to BOTH form-data readers — [`FormData::parse_fdf`]
//! (the COS-object FDF reader, which reuses pdfcer's own `Parser`) and
//! [`FormData::parse_xfdf`] (the scoped hand-rolled XML reader) — plus a
//! round-trip through the serializers. This exercises the untrusted-input
//! surface the Pass 7.1 brief calls out:
//!
//! - **malformed FDF** — no `/FDF` dictionary, a `/FDF` value that is not a
//!   dictionary, a `/Fields` that is not an array, a field `/T` that is not a
//!   string, a `/V` of an unexpected COS type, a `/Kids` cycle (bounded by
//!   the COS parser's nesting guard);
//! - **malformed XFDF** — unbalanced or unterminated tags, unterminated
//!   attribute strings, deeply nested `<field>` elements (bounded by
//!   `MAX_XML_DEPTH`), stray `&`, unterminated `&…;` entities, numeric
//!   character references with out-of-range code points;
//! - **huge field arrays** — a fan-out of `<field>`/`/Fields` entries.
//!
//! Invariant asserted (the crate's panic-free policy): for ANY input, both
//! parsers return normally — `Ok(FormData)` or an `FdfError` — and never
//! panic, abort, or loop. A successful parse is round-tripped back through
//! `to_fdf`/`to_xfdf` and re-parsed to exercise the serializers on
//! parser-derived data.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::fdf::FormData;

fuzz_target!(|data: &[u8]| {
    // FDF reader: never panics; a successful parse re-serializes + re-parses.
    if let Ok(form) = FormData::parse_fdf(data) {
        let bytes = form.to_fdf(None);
        let _ = FormData::parse_fdf(&bytes);
    }
    // XFDF reader (the scoped hand-rolled XML): same contract.
    if let Ok(form) = FormData::parse_xfdf(data) {
        let bytes = form.to_xfdf(None);
        let _ = FormData::parse_xfdf(&bytes);
    }
});
