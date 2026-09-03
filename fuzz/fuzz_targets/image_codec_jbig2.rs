//! Fuzz target: the **JBIG2Decode adapter**
//! (`pdfcer_core::image_codec::decode_image` over a `/JBIG2Decode` dict,
//! ISO 32000-1 §7.4.7 / ITU-T T.88).
//!
//! ## The globals path is the point
//!
//! `docs/decisions/005-image-codecs.md` §6.5 names what this target must
//! cover: "**includes the globals path**: valid globals, absent globals,
//! and globals that do not match the page segments."
//!
//! That instruction is aimed at the one thing here that is genuinely
//! pdfcer's own code rather than the vendor's. `hayro-jbig2` already
//! carries a `fuzz_jbig2` target over its segment parser, so throwing
//! bytes at *that* re-runs someone else's campaign. What no upstream
//! target covers is the **PDF embedding**: `/JBIG2Globals` is a stream
//! *reference*, so reaching the bytes means resolving an indirect
//! object, slicing the retained buffer by span, and running the globals
//! stream's own `/Filter` chain — three failure modes that live entirely
//! in pdfcer and none of which exist upstream.
//!
//! Splitting one fuzzer input into "globals" and "page" halves is what
//! makes the mismatched case reachable in volume. Real mismatches — a
//! symbol dictionary whose segment numbers do not line up with the text
//! region that refers to them, a globals stream from a *different*
//! document — are exactly what an attacker would construct, and they are
//! the shape most likely to find an index that is only valid when the
//! two halves agree.
//!
//! ## What the glue does that is worth attacking
//!
//! 1. **Globals resolution.** Indirect reference → stream → span slice →
//!    filter chain. A dangling reference, a `/Length` past the end of
//!    the buffer, a non-stream value and a globals stream whose own
//!    filter fails are four distinct paths, and three of them must be
//!    *tolerated* (treated as absent) rather than refused.
//! 2. **The ceiling window.** `Image::new_embedded` parses segment
//!    headers and reports the page geometry; `decode()` then allocates
//!    the full page bitmap from it. The ceiling check has exactly one
//!    correct place — between those two calls — and a page information
//!    segment claiming 0xFFFFFFFF × 0xFFFFFFFF is how a regression that
//!    moved it announces itself.
//! 3. **The push-sink budget latch**, as in the CCITT target: the vendor
//!    `Decoder` trait is infallible by signature, so the ceiling is a
//!    latched flag rather than an early return.
//! 4. **The polarity inversion**, which walks the whole sample buffer.
//!
//! ## Invariant asserted
//!
//! For ANY split of ANY input, `decode_image` returns `Ok(_)` or a
//! structured `ImageCodecError` — never a panic, never an abort, never
//! an unbounded allocation.
//!
//! Seed corpora come from `fixtures/synthetic/` only, never from a
//! downloaded real-world PDF (`docs/LEGAL.md` §5).

#![no_main]

use libfuzzer_sys::arbitrary::{self, Arbitrary};
use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::image_codec::decode_image;
use pdfcer_core::object::{Dict, Name, ObjId, Object};

/// One input, split into a globals half and a page half.
#[derive(Debug, Arbitrary)]
struct Input<'a> {
    /// Where to cut `data` into (globals, page). Taken modulo the
    /// length, so every cut point — including both degenerate ones — is
    /// reachable.
    split: u16,
    /// The image dictionary's own geometry, deliberately free to
    /// disagree with the page information segment (Table 89's "entries
    /// inconsistent with each other").
    dict_width: u16,
    dict_height: u16,
    /// Which shape `/JBIG2Globals` takes: a live reference, a dangling
    /// one, a non-stream value, or absent.
    globals_shape: u8,
    data: &'a [u8],
}

/// Build a document whose object 3 is an unfiltered stream holding
/// `globals` — the shape `/JBIG2Globals` points at.
///
/// Rebuilt per iteration because the globals bytes are fuzzer-chosen;
/// the cost is one small parse and it is what puts the *resolution*
/// path (reference → stream → span → filters) under the fuzzer rather
/// than only the decoder.
///
/// Returns `None` if the synthetic document fails to load, which can
/// happen for globals bytes that happen to contain PDF syntax the xref
/// offsets then disagree with. That is a property of the *harness*, not
/// of the code under test, so it is skipped rather than asserted on.
fn document_with_globals(globals: &[u8]) -> Option<Document> {
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (num, body) in [
        (1u32, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
    ] {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    offsets.push(buf.len());
    buf.extend_from_slice(format!("3 0 obj\n<< /Length {} >>\nstream\n", globals.len()).as_bytes());
    buf.extend_from_slice(globals);
    buf.extend_from_slice(b"\nendstream\nendobj\n");
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 4\n0000000000 65535 f\r\n");
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    Document::from_bytes(buf).ok()
}

fuzz_target!(|input: Input<'_>| {
    let at = if input.data.is_empty() {
        0
    } else {
        usize::from(input.split) % (input.data.len() + 1)
    };
    let (globals, page) = input.data.split_at(at);

    let Some(doc) = document_with_globals(globals) else {
        return;
    };

    let mut dict = Dict::new();
    dict.insert(
        Name::from(b"Filter"),
        Object::Name(Name::from(b"JBIG2Decode")),
    );
    dict.insert(
        Name::from(b"Width"),
        Object::Integer(i64::from(input.dict_width)),
    );
    dict.insert(
        Name::from(b"Height"),
        Object::Integer(i64::from(input.dict_height)),
    );

    let mut parms = Dict::new();
    match input.globals_shape % 4 {
        // The real path: a live reference to the stream built above.
        0 => {
            parms.insert(
                Name::from(b"JBIG2Globals"),
                Object::Reference(ObjId::new(3, 0)),
            );
        }
        // A dangling reference. §7.3.10 makes that resolve to null and
        // "shall not be considered an error", so it must behave as
        // absent rather than fail.
        1 => {
            parms.insert(
                Name::from(b"JBIG2Globals"),
                Object::Reference(ObjId::new(9999, 0)),
            );
        }
        // A value that is not a stream at all.
        2 => {
            parms.insert(Name::from(b"JBIG2Globals"), Object::Integer(42));
        }
        // Absent — the ordinary case for a self-contained image.
        _ => {}
    }
    dict.insert(Name::from(b"DecodeParms"), Object::Dict(parms));

    // The page half with whatever globals shape was selected …
    let _ = decode_image(&doc, &dict, page, false);
    // … the WHOLE input as one self-contained stream, which is the
    // no-globals organization and a different segment-ordering path …
    let _ = decode_image(&doc, &dict, input.data, false);
    // … and the inline path, which §7.4.7 and §8.9.7 require to be
    // refused before any byte is touched. Implementing the codec must
    // not relax the construct-level rule, and the cheapest way to keep
    // that true is to keep exercising it.
    let _ = decode_image(&doc, &dict, page, true);

    // The globals half routed as the image, so a stream that is *only*
    // globals (no page information segment) is exercised as a page —
    // one of the two "do not match" shapes §6.5 asks for.
    let _ = decode_image(&doc, &dict, globals, false);
});
