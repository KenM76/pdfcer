//! Fuzz target: the **CCITTFaxDecode adapter**
//! (`pdfcer_core::image_codec::decode_image` over a `/CCITTFaxDecode`
//! dict, ISO 32000-1 §7.4.6 / ITU-T T.4 / T.6).
//!
//! ## Fuzz the PARAMETERS as well as the data
//!
//! This is the one instruction `docs/decisions/005-image-codecs.md` §6.5
//! spells out target-by-target, and it is the difference between this
//! target and every other one in this directory:
//!
//! > **Must fuzz the PARAMETERS as well as the data** — `K` (all three
//! > sign cases), `Columns`, `Rows` (including 0/absent → dict
//! > `/Height`), `EncodedByteAlign`, `EndOfBlock`, `BlackIs1`,
//! > `DamagedRowsBeforeError`. The parameter cross-product is where fax
//! > geometry bugs live. Use `arbitrary` to derive the parameter dict
//! > from the same input.
//!
//! The reasoning is specific to this codec. A JPEG carries its own
//! geometry in a SOF header, so a fuzzer that mutates the codestream
//! automatically mutates the geometry. **CCITT carries none**: `Columns`
//! and `Rows` live in `/DecodeParms`, the row stride is computed from
//! `Columns` alone, the row *bound* comes from `Rows` or falls back to
//! the image dictionary's `/Height`, and none of the three is validated
//! against the bit stream by anything in T.4 or T.6. So the arithmetic
//! that decides how much memory to allocate and where each row starts is
//! driven **entirely by dictionary integers an attacker chooses**, with
//! the codestream contributing nothing. Fuzzing bytes alone would leave
//! that whole surface untouched.
//!
//! ## What the glue does that is worth attacking
//!
//! 1. **Stride and budget arithmetic.** `ceil(Columns / 8) × rows`,
//!    where both factors are attacker-chosen, checked against
//!    `MAX_IMAGE_PIXELS` / `MAX_IMAGE_DIMENSION` /
//!    `MAX_IMAGE_SAMPLE_BYTES`. A missing `saturating_*` here is an
//!    overflow that turns an in-bounds pixel count into an out-of-bounds
//!    allocation.
//! 2. **The `Rows` fallback chain.** `/Rows` → `/Height` → a derived
//!    ceiling, each of which can be absent, zero, negative, or absurd.
//!    `hayro-ccitt` decodes ZERO rows when handed `rows: 0`, so the
//!    fallback is load-bearing rather than cosmetic.
//! 3. **The push-sink budget latch.** The vendor `Decoder` trait is
//!    infallible by signature, so the ceiling is enforced by a latched
//!    flag rather than an early return. A regression that forgets to
//!    check the latch shows up as libFuzzer's `-rss_limit_mb` firing on
//!    an EOFB-less stream.
//! 4. **`K`'s trichotomy**, including `i64::MIN`/`i64::MAX`, where a
//!    naive `k as u32` would wrap.
//! 5. **`/BlackIs1`'s polarity**, exercised in both states so a decode
//!    that only ever runs one branch cannot hide a panic in the other.
//!
//! ## Invariant asserted
//!
//! For ANY combination of parameters and bytes, `decode_image` returns
//! `Ok(_)` or a structured `ImageCodecError` — never a panic, never an
//! abort, never an unbounded allocation.
//!
//! Seed corpora come from `fixtures/synthetic/` only, never from a
//! downloaded real-world PDF (`docs/LEGAL.md` §5).

#![no_main]

use libfuzzer_sys::arbitrary::{self, Arbitrary};
use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::image_codec::decode_image;
use pdfcer_core::object::{Dict, Name, Object};

/// The Table 11 cross-product, plus the image dictionary's own geometry,
/// plus whatever bytes are left over as the codestream.
///
/// `data` is last so `Arbitrary::arbitrary_take_rest` hands it every
/// remaining byte as one contiguous slice — which is what lets a seeded
/// CCITT stream survive mutation as a recognizable CCITT stream while
/// the leading parameter bytes steer the dictionary independently.
#[derive(Debug, Arbitrary)]
struct Input<'a> {
    /// Widened to `i64` before it reaches the dictionary, so all three
    /// sign cases and both saturation edges are reachable.
    k: i32,
    /// `/Columns`, negated when `columns_negative` is set — a zero or
    /// negative width must be refused, not defaulted to 1728.
    columns: u16,
    columns_negative: bool,
    /// `/Rows`. Zero is the interesting value: Table 11's "the image's
    /// height is not predetermined".
    rows: u16,
    /// The image dictionary's own geometry, deliberately independent of
    /// the parameters above.
    dict_width: u16,
    dict_height: u16,
    end_of_line: bool,
    encoded_byte_align: bool,
    end_of_block: bool,
    black_is_1: bool,
    /// `/DamagedRowsBeforeError`, whose Table 11 applicability window
    /// (`EndOfLine` true AND `K >= 0`) selects a named diagnostic.
    damaged_rows: u8,
    /// Bitmask deciding which keys are actually written, so "absent"
    /// — and therefore every default — is reachable for each one
    /// independently. Absence is a distinct code path from presence, and
    /// eight defaults are eight of them.
    present: u16,
    data: &'a [u8],
}

/// The smallest well-formed document `decode_image` will take a
/// `&Document` from.
///
/// The CCITT path reads the document only to resolve indirect parameter
/// values, so a two-object catalog is enough. (`/JBIG2Globals`, the
/// other reason the codec layer takes a `&Document`, is the sibling
/// `image_codec_jbig2` target's business.)
fn empty_document() -> Document {
    let body = b"%PDF-1.7\n\
1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n";
    let mut buf = body.to_vec();
    let xref_at = buf.len();
    buf.extend_from_slice(
        b"xref\n0 3\n0000000000 65535 f\r\n0000000009 00000 n\r\n0000000058 00000 n\r\n",
    );
    buf.extend_from_slice(
        format!("trailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    Document::from_bytes(buf).expect("the fixed minimal document must load")
}

fuzz_target!(|input: Input<'_>| {
    let doc = empty_document();
    let has = |bit: u16| input.present & bit != 0;

    let mut parms = Dict::new();
    if has(0x0001) {
        parms.insert(Name::from(b"K"), Object::Integer(i64::from(input.k)));
    }
    if has(0x0002) {
        let columns = i64::from(input.columns);
        parms.insert(
            Name::from(b"Columns"),
            Object::Integer(if input.columns_negative {
                -columns
            } else {
                columns
            }),
        );
    }
    if has(0x0004) {
        parms.insert(Name::from(b"Rows"), Object::Integer(i64::from(input.rows)));
    }
    if has(0x0008) {
        parms.insert(Name::from(b"EndOfLine"), Object::Boolean(input.end_of_line));
    }
    if has(0x0010) {
        parms.insert(
            Name::from(b"EncodedByteAlign"),
            Object::Boolean(input.encoded_byte_align),
        );
    }
    if has(0x0020) {
        parms.insert(
            Name::from(b"EndOfBlock"),
            Object::Boolean(input.end_of_block),
        );
    }
    if has(0x0040) {
        parms.insert(Name::from(b"BlackIs1"), Object::Boolean(input.black_is_1));
    }
    if has(0x0080) {
        parms.insert(
            Name::from(b"DamagedRowsBeforeError"),
            Object::Integer(i64::from(input.damaged_rows)),
        );
    }
    // A parameter of the wrong COS type must be treated as absent, not
    // panicked on — Table 11's preamble makes these assertions about the
    // encoded data, so a malformed one has no recoverable reading other
    // than the default.
    if has(0x0100) {
        parms.insert(
            Name::from(b"Columns"),
            Object::Name(Name::from(b"not-an-integer")),
        );
    }

    let mut dict = Dict::new();
    dict.insert(
        Name::from(b"Filter"),
        Object::Name(Name::from(b"CCITTFaxDecode")),
    );
    dict.insert(Name::from(b"DecodeParms"), Object::Dict(parms));
    // Independent of `/Columns` and `/Rows` on purpose: the divergence
    // is where the geometry bugs live, and `/Height` is also the `Rows`
    // fallback, so its absence changes which branch runs.
    if has(0x0200) {
        dict.insert(
            Name::from(b"Width"),
            Object::Integer(i64::from(input.dict_width)),
        );
    }
    if has(0x0400) {
        dict.insert(
            Name::from(b"Height"),
            Object::Integer(i64::from(input.dict_height)),
        );
    }
    if has(0x0800) {
        dict.insert(Name::from(b"BitsPerComponent"), Object::Integer(1));
    }

    // Both origins: `CCF` is a legal inline abbreviation (Table 94), so
    // the inline path reaches the same decoder through different
    // dictionary normalization.
    let _ = decode_image(&doc, &dict, input.data, false);
    let _ = decode_image(&doc, &dict, input.data, true);

    // The polarity branch that the flags above may not have selected.
    // `/BlackIs1` is the one Table 11 parameter that changes what the
    // SINK writes rather than what the decoder reads, so both states are
    // forced rather than left to chance.
    let mut flipped = dict.clone();
    if let Some(Object::Dict(p)) = flipped.get(b"DecodeParms").cloned().as_mut() {
        p.insert(Name::from(b"BlackIs1"), Object::Boolean(!input.black_is_1));
        flipped.insert(Name::from(b"DecodeParms"), Object::Dict(p.clone()));
    }
    let _ = decode_image(&doc, &flipped, input.data, false);

    // The same bytes behind a byte-stream prefix — a real, legal chain
    // shape (`/Filter [/ASCII85Decode /CCITTFaxDecode]`) that exercises
    // the prefix/terminal-codec split rather than the codec alone.
    let mut chained = dict.clone();
    chained.insert(
        Name::from(b"Filter"),
        Object::Array(vec![
            Object::Name(Name::from(b"ASCII85Decode")),
            Object::Name(Name::from(b"CCITTFaxDecode")),
        ]),
    );
    let _ = decode_image(&doc, &chained, input.data, false);
});
