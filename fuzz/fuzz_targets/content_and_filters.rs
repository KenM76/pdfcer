//! Fuzz target: content-stream tokenization + filter decoding
//! (`pdfcer_core::content`, `pdfcer_core::filters`).
//!
//! Several independent consumers of the same input bytes:
//!
//! 1. `ContentStream::parse(data)` — the §7.8 content-stream
//!    operator/operand tokenizer. This also reaches the §8.9.7
//!    inline-image (`BI`/`ID`/`EI`) data-extent logic, which is the
//!    single most dangerous construct in the content grammar: there is
//!    no `/Length`, so the end of the data is found by computing a
//!    sample count or by scanning for a filter EOD / a delimited `EI`.
//! 2. `filters::decode_stream(dict, data)` with a stream dictionary
//!    declaring `/Filter /FlateDecode` and
//!    `/DecodeParms << /Predictor 12 /Columns 8 >>` — the zlib inflate
//!    path followed by the PNG-Up (predictor 12) de-filtering row
//!    arithmetic, over attacker-controlled compressed bytes. This is
//!    the classic parser-bug hotspot: inflate bombs, truncated zlib
//!    tails, rows that don't match the declared `Columns` geometry,
//!    and invalid PNG filter tags all land here.
//! 2a-bis. `BrotliDecode` over the same bytes, bare and with the PNG-Up
//!    predictor. Its ceiling matters more than the others': Brotli's
//!    worst-case expansion far exceeds deflate's.
//! 2b. `LZWDecode` over the same bytes in **both `/EarlyChange`
//!    modes**, with and without a `/Predictor`. LZW is byte-stream
//!    shaped exactly like the Flate case above, and the two modes are
//!    genuinely different decoders — they widen the code length at
//!    510/1022/2046 and at 511/1023/2047 respectively (TIFF 6.0 §13),
//!    so a bug reachable in one may be unreachable in the other. LZW's
//!    ~1365:1 best case (§7.4.4.1 NOTE 2) also makes it a
//!    decompression-bomb vector comparable to Flate, which is what the
//!    incremental `MAX_DECODED_LEN` enforcement is for and what
//!    libFuzzer's `-rss_limit_mb` checks here.
//! 3. `ASCIIHexDecode` and `ASCII85Decode` over the same bytes
//!    (ARCHITECTURE.md §10.2: "expand fuzz targets to each filter
//!    decoder as they're implemented"). Base-85 in particular has real
//!    arithmetic to get wrong — a five-digit group is accumulated into
//!    a value that can legitimately exceed `u32::MAX` (`uuuuu` is
//!    85⁵ − 1), partial final groups index a fixed-size array by a
//!    running count, and the `z` shorthand and `~>` EOD are both
//!    position-sensitive. Every one of those is an overflow or
//!    out-of-bounds candidate reachable from raw input.
//!
//! Invariant asserted (the crate's panic-free policy): for ANY input,
//! every call returns `Ok(_)` or a structured error (`ContentError` /
//! `FilterError`) — never a panic, never an abort. The filter module's
//! documented decompression ceilings bound memory, so a small
//! compressed input cannot balloon into unbounded output; libFuzzer's
//! default `-rss_limit_mb` and `-timeout` convert any regression into
//! a reported crash.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::content::ContentStream;
use pdfcer_core::filters::ascii::{decode_85, decode_hex};
use pdfcer_core::filters::decode_stream;
use pdfcer_core::object::{Dict, Name, Object};

fuzz_target!(|data: &[u8]| {
    // 1. Content-stream tokenizer.
    let _ = ContentStream::parse(data.to_vec());

    // 2. FlateDecode + Predictor-12 (PNG Up) decode of the same bytes.
    //    Columns 8 / Colors 1 / BitsPerComponent 8 (defaults) gives
    //    9-byte predictor rows (1 tag + 8 data), so valid-geometry and
    //    bad-geometry decompressed payloads are both reachable.
    let mut parms = Dict::new();
    parms.insert(Name::from(b"Predictor"), Object::Integer(12));
    parms.insert(Name::from(b"Columns"), Object::Integer(8));
    let mut dict = Dict::new();
    dict.insert(
        Name::from(b"Filter"),
        Object::Name(Name::from(b"FlateDecode")),
    );
    dict.insert(Name::from(b"DecodeParms"), Object::Dict(parms));
    let _ = decode_stream(&dict, data);

    // 2a-bis. BrotliDecode, bare and with the same PNG-Up predictor.
    //
    //     Fuzzed for the reason ARCHITECTURE.md §10.2 gives for every
    //     filter, and with one of its own: Brotli's worst-case expansion
    //     is far higher than deflate's ~1032:1, because a small window of
    //     literals can be repeated across a large window. That makes the
    //     ceiling the thing most worth attacking here, and a
    //     ceiling-crossing input must come back as
    //     `FilterError::OutputTooLarge` rather than as an allocation.
    //
    //     BOTH predictor states are driven deliberately: the extension
    //     retitles Table 8 to include Brotli, so the predictor path is
    //     shared with Flate verbatim and a Brotli-specific predictor bug
    //     would be a bug in shared code reached by a new caller.
    for predictor in [false, true] {
        let mut dict = Dict::new();
        dict.insert(
            Name::from(b"Filter"),
            Object::Name(Name::from(b"BrotliDecode")),
        );
        if predictor {
            let mut parms = Dict::new();
            parms.insert(Name::from(b"Predictor"), Object::Integer(12));
            parms.insert(Name::from(b"Columns"), Object::Integer(8));
            dict.insert(Name::from(b"DecodeParms"), Object::Dict(parms));
        }
        let _ = decode_stream(&dict, data);
    }

    // 2b. LZWDecode in BOTH /EarlyChange modes, bare and with the same
    //     PNG-Up predictor as the Flate case above. `EarlyChange`
    //     absent is the Table 8 default (1); the explicit 0 selects the
    //     other decoder entirely, so both must be driven.
    for early_change in [None, Some(0i64), Some(1i64)] {
        for predictor in [false, true] {
            let mut parms = Dict::new();
            if let Some(value) = early_change {
                parms.insert(Name::from(b"EarlyChange"), Object::Integer(value));
            }
            if predictor {
                parms.insert(Name::from(b"Predictor"), Object::Integer(12));
                parms.insert(Name::from(b"Columns"), Object::Integer(8));
            }
            let mut dict = Dict::new();
            dict.insert(
                Name::from(b"Filter"),
                Object::Name(Name::from(b"LZWDecode")),
            );
            if !parms.is_empty() {
                dict.insert(Name::from(b"DecodeParms"), Object::Dict(parms));
            }
            let _ = decode_stream(&dict, data);
        }
    }

    // 2c. RunLengthDecode — parameterless, self-limiting, and the one
    //     filter whose length arithmetic has an off-by-one trap the
    //     spec names explicitly (128 is EOD, not a 129-byte literal).
    let mut rl = Dict::new();
    rl.insert(
        Name::from(b"Filter"),
        Object::Name(Name::from(b"RunLengthDecode")),
    );
    let _ = decode_stream(&rl, data);

    // 3. The two ASCII-armouring decoders, called directly rather than
    //    through `decode_stream` so the input reaches them verbatim
    //    (no dictionary shape stands between libFuzzer and the byte
    //    loop it is trying to break).
    let _ = decode_hex(data);
    let _ = decode_85(data);
});
