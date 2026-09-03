//! Fuzz target: the **JPXDecode adapter**
//! (`pdfcer_core::image_codec::decode_image` over a `/JPXDecode` dict).
//!
//! ## Fuzz the adapter, not the vendor's core
//!
//! `hayro-jpeg2000` carries its own `fuzz_jpeg2000` target upstream and
//! has been run against 20,000+ images scraped from real PDFs, so
//! throwing bytes at its wavelet coder here would mostly re-run someone
//! else's campaign. pdfcer's bugs will be in **the glue**
//! (`docs/decisions/005-image-codecs.md` §6.5), and that is what this
//! target aims at:
//!
//! 1. **Table 89's inverted dictionary rules.** This is the codec whose
//!    `/ColorSpace` may be absent, whose `/BitsPerComponent` must be
//!    ignored when present, and whose `/Decode` must be ignored
//!    outright. §6.5 names the shapes to reach: dicts with
//!    `/ColorSpace` present and *disagreeing* with the codestream,
//!    `/BitsPerComponent` present and wrong, and both absent. All three
//!    are steered independently of the codestream below.
//! 2. **`/SMaskInData`'s three-way split**, including values outside
//!    0..=2. The value decides whether an opacity channel is lifted out
//!    into a second allocation, and the channel count it is lifted from
//!    is attacker-controlled — so this is where an off-by-one between
//!    "colour channels" and "all channels" turns into an out-of-bounds
//!    read or a mis-sized `vec!`.
//! 3. **The planar → interleaved loop.** `hayro-jpeg2000` returns one
//!    `f32` buffer per component; the adapter interleaves them, scales
//!    each by its own declared bit depth, and clamps. The declared
//!    depth is attacker-controlled (SIZ marker, or a JP2 palette box
//!    column after palette resolution), which is why the adapter refuses
//!    anything outside 1..=31 rather than evaluating `1 << depth` — a
//!    regression there is a shift-overflow panic, and this target is
//!    what finds it.
//! 4. **The resource ceilings.** The decoder allocates one `f32` per
//!    sample per component from the codestream's *declared* geometry,
//!    before any entropy decoding — so a few dozen header bytes can ask
//!    for gigabytes. A regression that moves the ceiling check after
//!    `Image::decode` shows up here as libFuzzer's `-rss_limit_mb`
//!    firing rather than as a wrong pixel.
//! 5. **The marker pre-sniff.** `unsupported_marker_feature` is a
//!    hand-written walk over attacker-controlled bytes — two-byte
//!    segment lengths that can claim more than the buffer holds, a
//!    four-byte `Psot` tile-part length that can be zero, huge, or point
//!    backwards, and a JP2 box walk with 32- and 64-bit length fields.
//!    It runs only on the error path, which is exactly the path a
//!    fuzzer spends most of its time on.
//!
//! ## Invariant asserted
//!
//! For ANY input, `decode_image` returns `Ok(_)` or a structured
//! `ImageCodecError` — never a panic, never an abort, never an
//! unbounded allocation.
//!
//! Seed corpora come from `fixtures/synthetic/jpx/` only, never from a
//! downloaded real-world PDF (`docs/LEGAL.md` §5).
//!
//! ## Running it: raise `-report_slow_units`
//!
//! ```text
//! cargo +nightly fuzz run image_codec_jpx -- \
//!     -max_total_time=300 -rss_limit_mb=4096 -report_slow_units=30
//! ```
//!
//! libFuzzer flags any input taking over **one second** as a
//! `slow-unit`, a threshold set for parsers rather than for image
//! decoders. `MAX_IMAGE_PIXELS` legitimately admits a 32-megapixel
//! image; this target decodes each input up to four times; and the
//! sanitized, coverage-instrumented build runs roughly 20× slower than
//! the shipped one. A perfectly ordinary 17 Mpx codestream measures
//! **1.0 s natively and 22 s here** — so without the raised threshold
//! every large image is reported as a finding, which trains the reader
//! to ignore the ones that are not.
//!
//! One genuine `slow-unit` has been found and fixed: a 310-byte
//! codestream declaring a **65,536-tile grid** over a 512 × 1024 image,
//! 32 seconds of work for half a megapixel of output. The tile count is
//! independent of the pixel count, so no pixel or byte ceiling saw it;
//! `image_codec::jpx::MAX_TILES` is the guard that resulted, and the
//! input is kept as a seed.
//!
//! ## Why the first bytes steer the dictionary
//!
//! The dictionary is derived from a short prefix of the same input so
//! that libFuzzer's mutation feedback can steer *both* halves of the
//! problem — codestream and dictionary — with one corpus. Taking the
//! prefix rather than a separate `Arbitrary` struct keeps the remaining
//! bytes a contiguous, mutable codestream, which is what lets a seeded
//! JP2 file survive mutation as a recognizable JP2 file. (The CCITT
//! target uses `Arbitrary` instead, because its eight independent
//! Table 11 integers cannot be steered by a byte prefix — JPX has no
//! `/DecodeParms` at all, so four control bytes are enough here.)

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::image_codec::decode_image;
use pdfcer_core::object::{Dict, Name, Object};

/// The smallest well-formed document `decode_image` will take a
/// `&Document` from.
///
/// The JPX path never reads the file body — `JPXDecode` has no
/// `/DecodeParms` at all (Table 6), so `doc` serves only to resolve
/// indirect values in the image dictionary — but the signature requires
/// one, and building a fixed minimal document once per run keeps the
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

    // Base dictionary: the CONFORMANT shape. Table 89 makes
    // `/ColorSpace` and `/BitsPerComponent` optional for this filter, so
    // a dict with neither is the baseline rather than a degenerate case,
    // and it must be exercised on every input — not only when a control
    // byte happens to select it.
    let mut bare = Dict::new();
    bare.insert(
        Name::from(b"Filter"),
        Object::Name(Name::from(b"JPXDecode")),
    );
    // Deliberately allowed to disagree with the codestream, to be zero,
    // and to be absurd: §7.4.9 says these "shall match" the JPEG2000
    // data but gives no conflict-resolution rule, and real files
    // disagree anyway.
    bare.insert(Name::from(b"Width"), Object::Integer(byte(0) * 257));
    bare.insert(Name::from(b"Height"), Object::Integer(byte(1) * 257));
    // /SMaskInData across its whole defined range AND outside it: 0/1/2
    // are Table 89's three codes, and anything else is undefined by the
    // spec, which is precisely the input a fuzzer should be steering
    // into the adapter's fallback.
    bare.insert(Name::from(b"SMaskInData"), Object::Integer(byte(2) % 5 - 1));
    let _ = decode_image(&doc, &bare, codestream, false);

    // The same codestream under a dictionary that states BOTH of the
    // entries Table 89 tells a reader to ignore, with values chosen to
    // disagree — the §6.5 shape. `/Decode` is inverting, so a
    // regression that applies it is visible as a behavioural change and
    // not merely as an unread field.
    let mut stated = bare.clone();
    stated.insert(
        Name::from(b"ColorSpace"),
        Object::Name(Name::from(match byte(3) % 3 {
            0 => &b"DeviceGray"[..],
            1 => b"DeviceRGB",
            _ => b"DeviceCMYK",
        })),
    );
    stated.insert(Name::from(b"BitsPerComponent"), Object::Integer(byte(3)));
    stated.insert(
        Name::from(b"Decode"),
        Object::Array(vec![Object::Integer(1), Object::Integer(0)]),
    );
    let _ = decode_image(&doc, &stated, codestream, false);

    // §7.4.9 / §8.9.7: JPXDecode is forbidden in an inline image. The
    // rejection happens before any byte is touched, so this call is
    // cheap — but it is the path that must never start decoding, and a
    // regression there would otherwise only show up as a fidelity bug
    // in a file nobody has.
    let _ = decode_image(&doc, &bare, codestream, true);

    // An ASCII85-armoured JPX stream: a real and legal chain shape that
    // exercises the byte-stream PREFIX plus terminal-codec split rather
    // than the codec alone. `/Filter [/ASCII85Decode /JPXDecode]` is
    // what a producer emits when the file must stay 7-bit clean.
    let mut chained = bare.clone();
    chained.insert(
        Name::from(b"Filter"),
        Object::Array(vec![
            Object::Name(Name::from(b"ASCII85Decode")),
            Object::Name(Name::from(b"JPXDecode")),
        ]),
    );
    let _ = decode_image(&doc, &chained, codestream, false);
});
