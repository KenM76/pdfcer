#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use super::pipeline_tests::{AT_100, N, doc, grey, plan, skip};
use super::{ImageEncode, ImageSkip};
use crate::document::Document;
use crate::edit::EditSession;
use crate::image_codec::decode_image_view;
use crate::object::{Dict, ObjId, Object};
use crate::optimize::{OptimizePlan, OptimizedFile, RebuildOptions, SkipReason};
use crate::space_audit::tests::stream;
use crate::writer::SaveOptions;

/// `N` x `N` packed 1-bit rows: short ink dashes on a 1-valued
/// background, or the complement when `invert`.
pub(super) fn dashes(invert: bool) -> Vec<u8> {
    let stride = N.div_ceil(8) as usize;
    let mut px = vec![0u8; stride * N as usize];
    for y in 0..N {
        for x in 0..N {
            let ink = y % 12 < 2 && x % 9 < 4;
            if ink == invert {
                px[y as usize * stride + (x / 8) as usize] |= 0x80 >> (x % 8);
            }
        }
    }
    px
}

/// An `N` x `N` image XObject over `px`, with `extra` supplying the colour
/// space, depth and any `/Decode` or `/ImageMask` under test.
pub(super) fn one_bit(extra: &str, px: &[u8]) -> Vec<u8> {
    stream(
        &format!("/Type /XObject /Subtype /Image /Width {N} /Height {N} {extra}"),
        px,
    )
}

pub(super) const GREY_1: &str = "/ColorSpace /DeviceGray /BitsPerComponent 1";

/// Applies `plan`, saves, and returns object 5's dictionary and the
/// samples the shipped CCITT decoder reads back.
pub(super) fn saved(mut s: EditSession, p: &OptimizePlan) -> (Dict, Vec<u8>) {
    s.apply_optimize(p).unwrap();
    let OptimizedFile::Smaller(bytes) = s
        .save_optimized(p, &SaveOptions::default(), &RebuildOptions::none())
        .unwrap()
        .0
    else {
        panic!("expected a smaller file")
    };
    let d = Document::from_bytes(bytes).unwrap();
    let Some(Object::Stream(st)) = d.get(ObjId::new(5, 0)).map(|o| o.value.clone()) else {
        panic!()
    };
    let s = EditSession::new(d);
    let raw = s.view().slice(st.data_span).unwrap().to_vec();
    let coded = decode_image_view(&s.view(), &st.dict, &raw, false).unwrap();
    (st.dict, coded.samples)
}

fn parm<'a>(dict: &'a Dict, key: &[u8]) -> Option<&'a Object> {
    match dict.get(b"DecodeParms") {
        Some(Object::Dict(p)) => p.get(key),
        _ => None,
    }
}

#[test]
fn encode_ccitt_parses_and_prints() {
    let a = crate::optimize::StreamAction::parse("image(encode=ccitt)").unwrap();
    assert_eq!(a.to_string(), "image(encode=ccitt)");
}

#[test]
fn a_one_bit_image_is_written_as_group_4_and_decodes_to_the_same_samples() {
    let px = dashes(false);
    let (s, p) = plan(
        doc(AT_100, &[one_bit(GREY_1, &px)]),
        &[("image(encode=ccitt)", None)],
    );
    let c = p.changes.iter().find(|c| c.id.num == 5).unwrap();
    assert_eq!(c.image.as_ref().unwrap().encoded_as, ImageEncode::Ccitt);
    assert!(!c.lossy);
    let (dict, samples) = saved(s, &p);
    assert_eq!(
        dict.get(b"Filter"),
        Some(&Object::Name(b"CCITTFaxDecode"[..].into()))
    );
    assert_eq!(parm(&dict, b"K"), Some(&Object::Integer(-1)));
    assert_eq!(
        parm(&dict, b"Columns"),
        Some(&Object::Integer(i64::from(N)))
    );
    assert_eq!(parm(&dict, b"Rows"), Some(&Object::Integer(i64::from(N))));
    assert_eq!(samples, px);
}

/// Each polarity wins on one of a picture and its complement, and the
/// samples come back unchanged either way: `/BlackIs1` is chosen for
/// size and never alters what is decoded.
#[test]
fn the_smaller_polarity_is_kept_and_black_is_1_follows_it() {
    let mut seen = Vec::new();
    for invert in [false, true] {
        let px = dashes(invert);
        let (s, p) = plan(
            doc(AT_100, &[one_bit(GREY_1, &px)]),
            &[("image(encode=ccitt)", None)],
        );
        let (dict, samples) = saved(s, &p);
        assert_eq!(samples, px, "invert={invert}");
        seen.push(parm(&dict, b"BlackIs1").cloned());
    }
    // Short ink runs code smaller as CCITT white, as the codec measured.
    assert_eq!(seen, [Some(Object::Boolean(true)), None]);
}

/// An image mask keeps `/ImageMask` and its `/Decode`: the filter's
/// polarity and the mask's are independent (ISO 32000-1 §8.9.5.2 applies
/// `/Decode` after the filter).
#[test]
fn an_image_mask_keeps_its_decode_array() {
    let px = dashes(true);
    let (s, p) = plan(
        doc(AT_100, &[one_bit("/ImageMask true /Decode [1 0]", &px)]),
        &[("image(encode=ccitt)", None)],
    );
    let (dict, samples) = saved(s, &p);
    assert_eq!(dict.get(b"ImageMask"), Some(&Object::Boolean(true)));
    assert_eq!(
        dict.get(b"Decode"),
        Some(&Object::Array(vec![Object::Integer(1), Object::Integer(0)]))
    );
    assert_eq!(dict.get(b"BitsPerComponent"), None);
    assert_eq!(samples, px);
}

#[test]
fn a_grey_image_is_not_bilevel_for_a_patch_and_refused_by_name_for_an_override() {
    let (_, p) = plan(doc(AT_100, &[grey("")]), &[("image(encode=ccitt)", None)]);
    assert!(matches!(
        skip(&p, 5),
        Some(SkipReason::ImageUnsupported(ImageSkip::NotBilevel(
            ImageEncode::Ccitt
        )))
    ));
    let (_, p) = plan(
        doc(AT_100, &[grey("")]),
        &[("image(encode=ccitt)", Some(5))],
    );
    let why = skip(&p, 5).unwrap();
    assert!(matches!(
        why,
        SkipReason::ImageUnsupported(ImageSkip::NotBilevelOverride(ImageEncode::Ccitt))
    ));
    assert!(why.to_string().contains("add convert=bilevel"), "{why}");
}

#[test]
fn a_grey_image_converted_to_bilevel_is_written_as_group_4() {
    let (s, p) = plan(
        doc(AT_100, &[grey("")]),
        &[("image(convert=bilevel,encode=ccitt)", Some(5))],
    );
    let c = p.changes.iter().find(|c| c.id.num == 5).unwrap();
    let img = c.image.as_ref().unwrap();
    assert_eq!(img.encoded_as, ImageEncode::Ccitt);
    assert_eq!(img.bpc.1, 1);
    let (dict, samples) = saved(s, &p);
    assert_eq!(
        dict.get(b"Filter"),
        Some(&Object::Name(b"CCITTFaxDecode"[..].into()))
    );
    assert_eq!(samples.len(), N.div_ceil(8) as usize * N as usize);
}
