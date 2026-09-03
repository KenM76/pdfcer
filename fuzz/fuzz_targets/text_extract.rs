//! Fuzz target: text extraction (`pdfcer_core::text_extract`,
//! `pdfcer_core::textstring`).
//!
//! Pass 4's untrusted-input surface, driven from three directions over
//! the same bytes. `ARCHITECTURE.md` §10.2 requires a fuzz target for
//! any new code that touches untrusted-input parsing, and extraction
//! adds three distinct parsers plus a stateful walk.
//!
//! ## 1. `ToUnicodeCMap::parse` — the new parser
//!
//! The sharpest surface of the three, and the reason this target exists
//! separately from `content_and_filters`. A `ToUnicode` CMap is a
//! PostScript-subset grammar with **attacker-controlled arithmetic** in
//! four independent places, every one of which ISO 32000-1 §9.10.3
//! specifies with a `shall` and **no stated recovery**:
//!
//! - **The form-B last-byte increment.** `<lo> <hi> <dst>` adds
//!   `code − lo` to the final byte of `dst`. The clause bounds that
//!   addition (`last ≤ 255 − (hi − lo)`) and then says the result past
//!   the bound "is undefined" — so the bound is exactly the kind of
//!   thing an implementation forgets to check, and `hi − lo` is a
//!   64K-wide attacker-chosen span.
//! - **Source-code width.** `<41>` and `<0041>` are the same numeric
//!   value at different byte widths; a code string may be 0 to many
//!   bytes long, and the shift loop that assembles it is a classic
//!   overflow site.
//! - **Form-C array cardinality.** `m` must equal `hi − lo + 1`; a file
//!   may declare a range of 65,536 and supply two elements, or the
//!   reverse.
//! - **UTF-16BE destination decoding.** Odd lengths and lone surrogates
//!   are both syntactically legal `ToUnicode` content (§9.10.3 N3
//!   records that the standard states no validity rule at all).
//!
//! On top of that sit the R25 guards — `MAX_BF_ENTRIES`,
//! `MAX_BF_RANGES`, `MAX_DST_BYTES`, `MAX_CMAP_TOKENS` — whose whole
//! purpose is to bound work on hostile input. libFuzzer's
//! `-rss_limit_mb` and `-timeout` are what turn a regression in any of
//! them into a reported crash rather than a slow afternoon.
//!
//! ## 2. `decode_text_string` — the second decode path
//!
//! §7.9.2.2's BOM-or-PDFDocEncoding discriminator, the surrogate
//! pairing, and the in-band language escape (U+001B … U+001B), whose
//! recognizer indexes forward by up to five code units from a position
//! chosen by the input.
//!
//! ## 3. `extract_document` over a whole fuzzed PDF
//!
//! The end-to-end walk: content tokenization, the marked-content stack,
//! form-XObject recursion with its depth and cycle guards, the text
//! matrix arithmetic, font resolution through the §9.10.2 ladder, and
//! the derived-layout pass. Reached only when the bytes happen to load
//! as a document — rare from raw input, but `load_document`'s corpus
//! and the committed `fixtures/synthetic/text/*.pdf` seeds make it
//! reachable, and it is the only way to fuzz the *stateful* part.
//!
//! ## Invariants asserted
//!
//! 1. **No panic, ever** — the crate's panic-free policy (decision 001
//!    §6.1 item 5). Every call returns a value or a structured error.
//! 2. **Every glyph's text range is in bounds and on a char boundary**
//!    of its own run. This is the invariant the entire per-glyph
//!    provenance model rests on: a caller slicing `run.text` by
//!    `text_start..text_start+text_len` is the *intended* use, so a
//!    range that is out of bounds or splits a UTF-8 sequence would
//!    panic in consumer code rather than here. Asserting it inside the
//!    fuzz target moves that panic to where it can be found.
//! 3. **`sourced_text()` is never longer than `plain_text()`** — the
//!    two differ only by pdfcer's insertions, so a violation would mean
//!    the "friendly" accessor is silently dropping file content.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::text_extract::cmap::ToUnicodeCMap;
use pdfcer_core::text_extract::{self, ExtractOptions};
use pdfcer_core::textstring::{decode_text_string, decode_utf16be_bytes, encode_text_string};

fuzz_target!(|data: &[u8]| {
    // --- 1. The ToUnicode CMap parser -----------------------------------
    let cmap = ToUnicodeCMap::parse(data);
    // Probe a spread of codes across the 1-, 2- and 3-byte spaces. Every
    // lookup must terminate and must not panic, including inside the
    // form-B increment arithmetic.
    for code in [0u32, 1, 0x20, 0x41, 0xFF, 0x100, 0x3A51, 0xFFFF, 0x1_0000] {
        let _ = cmap.lookup(code);
    }
    // Also probe codes taken FROM the input, so the fuzzer can steer
    // lookups onto ranges it just declared rather than only onto the
    // fixed set above.
    for pair in data.chunks_exact(2).take(64) {
        let hi = u32::from(pair.first().copied().unwrap_or(0));
        let lo = u32::from(pair.get(1).copied().unwrap_or(0));
        let _ = cmap.lookup((hi << 8) | lo);
    }
    let _ = cmap.stats();
    let _ = cmap.codespace_widths();

    // --- 2. Text-string decoding ----------------------------------------
    let decoded = decode_text_string(data);
    let _ = decode_utf16be_bytes(data);
    // Encode-then-decode must round-trip any string the decoder produced.
    // This is a real invariant and not a tautology: the encoder picks
    // PDFDocEncoding when every character fits, and Annex D.3's 24
    // undefined codes make that choice easy to get subtly wrong.
    let reencoded = encode_text_string(&decoded.text);
    assert_eq!(
        decode_text_string(&reencoded).text,
        decoded.text,
        "text-string encode/decode round trip"
    );

    // --- 3. Whole-document extraction ------------------------------------
    if let Ok(doc) = Document::from_bytes(data.to_vec()) {
        let options = ExtractOptions::default();
        if let Ok(extracted) = text_extract::extract_document(&doc, &options) {
            for page in &extracted.pages {
                for run in &page.runs {
                    let mut expected_start = 0usize;
                    for glyph in &run.glyphs {
                        let start = glyph.text_start as usize;
                        let end = start + glyph.text_len as usize;
                        assert!(end <= run.text.len(), "glyph range past the run's text");
                        assert!(
                            run.text.is_char_boundary(start),
                            "glyph range splits a UTF-8 sequence"
                        );
                        assert!(
                            run.text.is_char_boundary(end),
                            "glyph range splits a UTF-8 sequence"
                        );
                        assert_eq!(start, expected_start, "glyph ranges must tile the run");
                        expected_start = end;
                    }
                }
            }
            let plain = extracted.plain_text();
            let sourced = extracted.sourced_text();
            assert!(
                sourced.chars().count() <= plain.chars().count(),
                "sourced text cannot exceed plain text — the two differ only by pdfcer's insertions"
            );
        }
    }
});
