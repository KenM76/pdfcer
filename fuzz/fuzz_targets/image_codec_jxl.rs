//! Fuzz target: the **JXLDecode adapter**
//! (`pdfcer_core::image_codec::decode_image` over a `/JXLDecode` dict).
//!
//! `jxl-oxide` is fuzzed upstream; the target aims at pdfcer's glue: the
//! header ceilings checked before `render_frame`, the decoder's
//! `AllocTracker` budget, the channel-count cross-check, alpha splitting
//! and unpremultiplication, and `/SMaskInData` outside 0..=2.
//!
//! Invariant: for ANY input, `decode_image` returns `Ok(_)` or a
//! structured `ImageCodecError` — never a panic, abort or unbounded
//! allocation. Seeds: `fixtures_jxl.rs` constants only (`docs/LEGAL.md`
//! §5).
//!
//! ```text
//! cargo +nightly fuzz run image_codec_jxl -- \
//!     -max_total_time=300 -rss_limit_mb=4096 -report_slow_units=30
//! ```

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::image_codec::decode_image;
use pdfcer_core::object::{Dict, Name, Object};

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
    // First byte steers /SMaskInData (including values outside 0..=2);
    // the rest is the codestream, so a seeded file survives mutation.
    let (control, codestream) = data.split_at(data.len().min(1));
    let steer = i64::from(control.first().copied().unwrap_or(0));

    let mut dict = Dict::new();
    dict.insert(
        Name::from(b"Filter"),
        Object::Name(Name::from(b"JXLDecode")),
    );
    dict.insert(Name::from(b"Width"), Object::Integer(steer * 3));
    dict.insert(Name::from(b"Height"), Object::Integer(steer * 5));
    let _ = decode_image(&doc, &dict, codestream, false);

    dict.insert(Name::from(b"SMaskInData"), Object::Integer(steer % 5 - 1));
    let _ = decode_image(&doc, &dict, codestream, false);

    // Inline images may not use JXLDecode; this must refuse without decoding.
    let _ = decode_image(&doc, &dict, codestream, true);
});
