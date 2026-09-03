//! Fuzz target: the **DCTDecode adapter**
//! (`pdfcer_core::image_codec::decode_image` over a `/DCTDecode` dict).
//!
//! ## Fuzz the adapter, not the vendor's core
//!
//! `zune-jpeg` already carries its own cargo-fuzz targets and picks up
//! transitive OSS-Fuzz coverage through the `image-rs` project, so
//! throwing bytes at its Huffman decoder here would mostly re-run
//! someone else's campaign. pdfcer's bugs will be in **the glue**
//! (`docs/decisions/005-image-codecs.md` §6.5), and that is what this
//! target aims at:
//!
//! 1. **The APP14 / SOF pre-sniff.** A hand-written marker walk over
//!    attacker-controlled bytes: 2-byte big-endian segment lengths that
//!    can claim more than the buffer holds, lengths below the 2 they
//!    must include, `0xFF` fill runs, standalone markers with no length
//!    field, and a payload offset (11) read for the Adobe transform
//!    byte. Every one of those is an out-of-bounds or infinite-loop
//!    candidate reachable from raw input.
//! 2. **Geometry reconciliation.** The image dictionary's `/Width`,
//!    `/Height` and `/BitsPerComponent` are fuzzed *independently* of
//!    the codestream's, because the interesting failures live in the
//!    disagreement — the caller keeps the dictionary's numbers for
//!    placement and the codestream's for the row stride, and a
//!    multiplication of two attacker-chosen values is where overflow
//!    lives.
//! 3. **Buffer sizing.** `output_buffer_size()` is the *decoder's own
//!    claim* about how much memory it wants; the adapter checks it
//!    against `MAX_IMAGE_SAMPLE_BYTES` before allocating. A regression
//!    that trusts the claim shows up here as libFuzzer's `-rss_limit_mb`
//!    firing.
//! 4. **The colour-routing table** of §4.1, including the YCCK → CMYK
//!    inverse's `chunks_exact_mut(4)` over a buffer whose length the
//!    decoder chose.
//!
//! ## Invariant asserted
//!
//! For ANY input, `decode_image` returns `Ok(_)` or a structured
//! `ImageCodecError` — never a panic, never an abort, never an
//! unbounded allocation. The ceilings documented in
//! `pdfcer_core::image_codec` bound memory, so libFuzzer's default
//! `-rss_limit_mb` and `-timeout` turn any regression into a reported
//! crash rather than a hung machine.
//!
//! Seed corpora come from `fixtures/synthetic/` only, never from a
//! downloaded real-world PDF (`docs/LEGAL.md` §5).
//!
//! ## Why the first bytes steer the dictionary
//!
//! The dictionary is derived from a short prefix of the same input so
//! that libFuzzer's mutation feedback can steer *both* halves of the
//! problem — codestream and dictionary — with one corpus. Taking the
//! prefix rather than a separate `Arbitrary` struct keeps the remaining
//! bytes a contiguous, mutable codestream, which is what lets a seeded
//! JPEG survive mutation as a recognizable JPEG.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::image_codec::decode_image;
use pdfcer_core::object::{Dict, Name, Object};

/// The smallest well-formed document `decode_image` will take a
/// `&Document` from.
///
/// The DCT path never reads the file body — `doc` exists to resolve
/// indirect parameter values and, from Pass 2.2, `/JBIG2Globals` — so a
/// two-object catalog is enough, and building it once per run keeps the
/// fuzzer's time in the code under test.
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

fuzz_target!(|data: &[u8]| {
    let doc = empty_document();

    // First four bytes steer the dictionary; the rest is the
    // codestream. A shorter input still exercises the codestream path
    // with the defaults.
    let (control, codestream) = data.split_at(data.len().min(4));
    let byte = |i: usize| i64::from(control.get(i).copied().unwrap_or(0));

    let mut dict = Dict::new();
    dict.insert(
        Name::from(b"Filter"),
        Object::Name(Name::from(b"DCTDecode")),
    );
    // Deliberately allowed to disagree with the codestream, to be zero,
    // and to be absurd: Table 89 calls that an inconsistent image
    // dictionary, and real files do it anyway.
    dict.insert(Name::from(b"Width"), Object::Integer(byte(0) * 257));
    dict.insert(Name::from(b"Height"), Object::Integer(byte(1) * 257));
    dict.insert(Name::from(b"BitsPerComponent"), Object::Integer(byte(2)));

    // `/ColorTransform` is the second level of Table 13's precedence
    // chain, so it must be fuzzed alongside the APP14 marker in the
    // codestream — including with values outside the 0..1 the table
    // defines.
    let mut parms = Dict::new();
    parms.insert(
        Name::from(b"ColorTransform"),
        Object::Integer(byte(3) - 128),
    );
    dict.insert(Name::from(b"DecodeParms"), Object::Dict(parms));

    // Both origins: §8.9.7's inline rules are a different code path,
    // and `DCT` is legal inline while `JBIG2Decode`/`JPXDecode` are not.
    let _ = decode_image(&doc, &dict, codestream, false);
    let _ = decode_image(&doc, &dict, codestream, true);

    // The same bytes routed as an ASCII85-armoured codestream, which is
    // a real and legal chain shape and exercises the byte-stream
    // PREFIX + terminal-codec split rather than the codec alone.
    let mut chained = Dict::new();
    chained.insert(
        Name::from(b"Filter"),
        Object::Array(vec![
            Object::Name(Name::from(b"ASCII85Decode")),
            Object::Name(Name::from(b"DCTDecode")),
        ]),
    );
    let _ = decode_image(&doc, &chained, codestream, false);

    // And as one of the codecs that is recognized but not implemented,
    // so the routing/rejection path stays panic-free too.
    for name in [&b"CCITTFaxDecode"[..], b"JBIG2Decode", b"JPXDecode"] {
        let mut other = Dict::new();
        other.insert(Name::from(b"Filter"), Object::Name(Name::from(name)));
        let _ = decode_image(&doc, &other, codestream, false);
        let _ = decode_image(&doc, &other, codestream, true);
    }
});
