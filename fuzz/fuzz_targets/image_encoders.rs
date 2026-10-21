//! Fuzz target: pdfcer's own filter encoders. Every encoder's output must
//! decode back to its input. Encoders see only pdfcer's pixels, but any
//! byte string is a valid input, so the identity must hold for all of them.
//! CCITT Group 4 and the JBIG2 generic region take their width from the
//! first two bytes and their rows from the rest, and must decode through
//! the shipped decoders (CCITT in both polarities).

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::filters::{FilterNotes, lzw, runlength};
use pdfcer_core::image_codec::{ccitt::encode_g4, decode_image, jbig2::encode_generic};
use pdfcer_core::object::{Dict, Name, Object};

/// The two-object document `decode_image` needs for direct parameters.
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

/// A 1-bit picture from fuzz bytes: width, height, the rows, and the rows
/// with their pad bits cleared (what a decoder delivers).
fn picture(data: &[u8]) -> Option<(u32, u32, &[u8], Vec<u8>)> {
    let (&[w0, w1], px) = data.split_first_chunk::<2>()?;
    let width = u32::from(u16::from_le_bytes([w0, w1]) % 2048) + 1;
    let stride = width.div_ceil(8) as usize;
    let height = (px.len() / stride).min(256);
    if height == 0 {
        return None;
    }
    let px = &px[..height * stride];
    let mut clean = px.to_vec();
    let pad = (stride * 8) as u32 - width;
    if pad > 0 {
        for row in clean.chunks_mut(stride) {
            row[stride - 1] &= 0xFFu8 << pad;
        }
    }
    Some((width, height as u32, px, clean))
}

/// An image dictionary for a 1-bit DeviceGray picture under `filter`.
fn image_dict(filter: &[u8], width: u32, height: u32) -> Dict {
    let mut dict = Dict::new();
    dict.insert(Name::from(b"Filter"), Object::Name(Name::from(filter)));
    dict.insert(Name::from(b"Width"), Object::Integer(i64::from(width)));
    dict.insert(Name::from(b"Height"), Object::Integer(i64::from(height)));
    dict.insert(Name::from(b"BitsPerComponent"), Object::Integer(1));
    dict.insert(
        Name::from(b"ColorSpace"),
        Object::Name(Name::from(b"DeviceGray")),
    );
    dict
}

fn jbig2_round_trips(data: &[u8]) {
    let Some((width, height, px, clean)) = picture(data) else {
        return;
    };
    let stream = encode_generic(px, width, height);
    let dict = image_dict(b"JBIG2Decode", width, height);
    let decoded =
        decode_image(&empty_document(), &dict, &stream, false).expect("own JBIG2 must decode");
    assert_eq!(decoded.samples, clean, "{width}x{height}");
}

fn ccitt_round_trips(data: &[u8]) {
    let Some((width, height, px, clean)) = picture(data) else {
        return;
    };
    let height = height as usize;
    let doc = empty_document();
    for one_is_black in [true, false] {
        let g4 = encode_g4(px, width, height as u32, one_is_black);
        let mut parms = Dict::new();
        parms.insert(Name::from(b"K"), Object::Integer(-1));
        parms.insert(Name::from(b"Columns"), Object::Integer(i64::from(width)));
        parms.insert(Name::from(b"Rows"), Object::Integer(height as i64));
        if one_is_black {
            parms.insert(Name::from(b"BlackIs1"), Object::Boolean(true));
        }
        let mut dict = image_dict(b"CCITTFaxDecode", width, height as u32);
        dict.insert(Name::from(b"DecodeParms"), Object::Dict(parms));
        let decoded = decode_image(&doc, &dict, &g4, false).expect("own G4 must decode");
        assert_eq!(
            decoded.samples, clean,
            "{width}x{height} one_is_black={one_is_black}"
        );
    }
}

fuzz_target!(|data: &[u8]| {
    let encoded = runlength::encode(data);
    assert!(encoded.len() <= data.len() + data.len() / 128 + 2);
    assert_eq!(runlength::decode(&encoded).ok().as_deref(), Some(data));
    let encoded = lzw::encode(data).ok();
    let mut notes = FilterNotes::default();
    let decoded = encoded.and_then(|e| lzw::decode(&e, None, &mut notes).ok());
    assert_eq!(decoded.as_deref(), Some(data));
    ccitt_round_trips(data);
    jbig2_round_trips(data);
});
