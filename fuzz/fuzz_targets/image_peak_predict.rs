//! Fuzz target: the **image peak-memory predictors**
//! (`pdfcer_core::image_codec::peak`, decision 227).
//!
//! The predictors are pdfcer's own hand-written header walkers over
//! attacker-controlled bytes: JPEG marker segments (T.81 B.1), the JPX
//! SIZ marker and JP2 box lengths (T.800 A.5.1, I.4), JBIG2 segment
//! headers with their variable-width referred-to fields (T.88 7.2), and
//! the image dictionary's geometry and `/DecodeParms`. They run before
//! any decode, so a panic or a non-terminating walk here would precede
//! every ceiling the codecs enforce.
//!
//! Invariant asserted: for ANY bytes and ANY dictionary geometry,
//! `predict_peak` and `codec_working_bytes` return without panicking.

#![no_main]

use libfuzzer_sys::arbitrary::{self, Arbitrary};
use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::image_codec::Codec;
use pdfcer_core::image_codec::peak::{codec_working_bytes, predict_peak};
use pdfcer_core::object::{Dict, Name, Object};

#[derive(Debug, Arbitrary)]
struct Input<'a> {
    filter: u8,
    width: i32,
    height: i32,
    bpc: i8,
    colour: u8,
    predictor: u8,
    columns: i32,
    data: &'a [u8],
}

const FILTERS: [&[u8]; 7] = [
    b"DCTDecode",
    b"JPXDecode",
    b"JBIG2Decode",
    b"CCITTFaxDecode",
    b"FlateDecode",
    b"JXLDecode",
    b"",
];

const COLOURS: [&[u8]; 5] = [
    b"DeviceGray",
    b"DeviceRGB",
    b"DeviceCMYK",
    b"Pattern",
    b"R0",
];

fn document() -> Option<Document> {
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (num, body) in [
        (1u32, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
    ] {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 3\n0000000000 65535 f\r\n");
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    Document::from_bytes(buf).ok()
}

fuzz_target!(|input: Input<'_>| {
    let Some(doc) = document() else {
        return;
    };
    let mut dict = Dict::new();
    let filter = FILTERS[usize::from(input.filter) % FILTERS.len()];
    if !filter.is_empty() {
        dict.insert(Name::from(b"Filter"), Object::Name(Name::from(filter)));
    }
    for (key, value) in [
        (&b"Width"[..], i64::from(input.width)),
        (b"Height", i64::from(input.height)),
        (b"BitsPerComponent", i64::from(input.bpc)),
    ] {
        dict.insert(Name::from(key), Object::Integer(value));
    }
    let colour = COLOURS[usize::from(input.colour) % COLOURS.len()];
    dict.insert(Name::from(b"ColorSpace"), Object::Name(Name::from(colour)));
    let mut parms = Dict::new();
    for (key, value) in [
        (&b"Predictor"[..], i64::from(input.predictor)),
        (b"Columns", i64::from(input.columns)),
        (b"Rows", i64::from(input.height)),
        (b"K", -1),
    ] {
        parms.insert(Name::from(key), Object::Integer(value));
    }
    dict.insert(Name::from(b"DecodeParms"), Object::Dict(parms));

    let view = doc.view();
    let _ = predict_peak(&view, &dict, input.data);
    for codec in [
        Codec::Dct,
        Codec::Ccitt,
        Codec::Jbig2,
        Codec::Jpx,
        Codec::Jxl,
    ] {
        let _ = codec_working_bytes(&view, codec, input.data, &dict);
    }
});
