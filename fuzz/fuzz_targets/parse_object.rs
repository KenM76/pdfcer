//! Fuzz target: the object parser (`pdfcer_core::parser`).
//!
//! Feeds arbitrary bytes to both top-level parser entry points:
//!
//! 1. `Parser::at(data, 0).parse_object()` — the direct-object parser
//!    (any §7.3 value, including `N G R` references), the entry point
//!    used for trailer dictionaries and bare-value positions.
//! 2. `Parser::at(data, 0).parse_indirect_object(&mut |_| None)` — the
//!    full `N G obj … endobj` / stream-form parser, with a no-op
//!    length resolver (as used at xref-stream bootstrap where indirect
//!    `/Length` is illegal per ISO 32000-1 §7.5.8.2).
//!
//! Invariant asserted (the crate's panic-free policy): for ANY input,
//! both calls return `Ok(_)` or a structured `Err(ParseError)` —
//! never a panic, never an abort. Resource ceilings are exercised too:
//! container nesting is bounded by the parser's `MAX_NESTING_DEPTH`
//! guard, so memory and time stay proportional to the input size (no
//! OOM, no unbounded recursion/stack overflow). libFuzzer's default
//! `-rss_limit_mb` and `-timeout` convert any regression of those
//! ceilings into a reported crash.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::parser::Parser;

fuzz_target!(|data: &[u8]| {
    // Direct-object entry point.
    let _ = Parser::at(data, 0).parse_object();
    // Indirect-object entry point with a no-op length resolver (a
    // stream whose /Length is an indirect reference resolves to None,
    // which must surface as a structured error, not a panic).
    let _ = Parser::at(data, 0).parse_indirect_object(&mut |_| None);
});
