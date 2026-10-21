#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::pipeline_tests::{AT_100, N, doc, grey, plan, skip};
use crate::edit::EditSession;
use crate::filters::flate;
use crate::image_codec::decode_image_view;
use crate::jpeg_write::{Chroma, JpegOptions, encode};
use crate::object::{Dict, ObjId, Object};
use crate::optimize::{
    ImageEncode, ImageSkip, ImageStep, JpegQuality, OptimizePlan, SkipReason, estimate_jpeg_quality,
};
use crate::space_audit::tests::stream;

/// A smooth `components`-channel `N` x `N` raster, so JPEG error is small.
fn smooth(components: usize) -> Vec<u8> {
    (0..N as usize * N as usize * components)
        .map(|i| {
            let (p, c) = (i / components, i % components);
            let (x, y) = (p % N as usize, p / N as usize);
            ((x + 2 * y + 40 * c) % 256) as u8
        })
        .collect()
}

fn image(cs: &str, bpc: u8, filter: &str, extra: &str, data: &[u8]) -> Vec<u8> {
    stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width {N} /Height {N} /ColorSpace {cs} \
             /BitsPerComponent {bpc} /Filter /{filter} {extra}"
        ),
        data,
    )
}

fn jpeg(samples: &[u8], components: u8, quality: u8) -> Vec<u8> {
    let opts = JpegOptions {
        quality,
        chroma: Chroma::Auto,
        progressive: false,
        optimize_huffman: true,
    };
    encode(samples, N, N, components, opts).unwrap()
}

fn applied(mut s: EditSession, p: &OptimizePlan, n: u32) -> (Dict, Vec<u8>) {
    s.apply_optimize(p).unwrap();
    let Some(Object::Stream(st)) = s.value(ObjId::new(n, 0)).cloned() else {
        panic!()
    };
    let raw = s.view().slice(st.data_span).unwrap().to_vec();
    (st.dict, raw)
}

fn mean_error(a: &[u8], b: &[u8]) -> f64 {
    assert_eq!(a.len(), b.len());
    let sum: u64 = a
        .iter()
        .zip(b)
        .map(|(x, y)| u64::from(x.abs_diff(*y)))
        .sum();
    sum as f64 / a.len() as f64
}

#[test]
fn a_flate_image_written_as_jpeg_keeps_its_size_and_reports_the_quality() {
    let src = image(
        "/DeviceGray",
        8,
        "FlateDecode",
        "",
        &flate::encode_with_level(&smooth(1), 0),
    );
    let (s, p) = plan(
        doc(AT_100, &[src]),
        &[("image(encode=jpeg,quality=60)", None)],
    );
    let c = p.changes.iter().find(|c| c.id.num == 5).unwrap();
    let img = c.image.as_ref().unwrap();
    assert_eq!(img.encoded_as, ImageEncode::Jpeg);
    assert_eq!((img.size_before, img.size_after), ((N, N), (N, N)));
    assert_eq!(img.bpc, (8, 8));
    assert!(img.binding.is_none());
    let q = img.jpeg.unwrap();
    assert_eq!((q.asked, q.used, q.source_estimate), (60, 60, None));
    assert!(c.lossy);
    let (dict, raw) = applied(s, &p, 5);
    assert_eq!(
        dict.get(b"Filter"),
        Some(&Object::Name(b"DCTDecode".as_slice().into()))
    );
    assert!(dict.get(b"DecodeParms").is_none());
    assert_eq!(estimate_jpeg_quality(&raw), Some(60));
}

#[test]
fn a_jpeg_asked_below_its_quality_is_re_encoded_at_the_asked_quality() {
    let src = image("/DeviceGray", 8, "DCTDecode", "", &jpeg(&smooth(1), 1, 90));
    let (s, p) = plan(
        doc(AT_100, &[src]),
        &[("image(encode=jpeg,quality=20)", None)],
    );
    let c = p
        .changes
        .iter()
        .find(|c| c.id.num == 5)
        .unwrap_or_else(|| panic!("{:?}", p.skipped));
    let q: JpegQuality = c.image.as_ref().unwrap().jpeg.unwrap();
    assert_eq!((q.asked, q.used, q.source_estimate), (20, 20, Some(90)));
    let (_, raw) = applied(s, &p, 5);
    assert_eq!(estimate_jpeg_quality(&raw), Some(20));
}

#[test]
fn a_jpeg_asked_at_or_above_its_quality_is_skipped_with_both_qualities() {
    let src = image("/DeviceGray", 8, "DCTDecode", "", &jpeg(&smooth(1), 1, 50));
    let junk = image("/DeviceGray", 8, "DCTDecode", "", &[0xFF; 64]);
    let content = "q 100 0 0 100 0 0 cm /Im5 Do /Im6 Do Q";
    let (_, p) = plan(
        doc(content, &[src, junk]),
        &[("image(encode=jpeg,quality=50)", None)],
    );
    assert!(p.changes.iter().all(|c| c.id.num != 5 && c.id.num != 6));
    let left = |source_estimate| {
        Some(SkipReason::ImageUnsupported(ImageSkip::AlreadyJpeg {
            source_estimate,
            asked: 50,
        }))
    };
    assert_eq!(skip(&p, 5), left(Some(50)));
    assert_eq!(skip(&p, 6), left(None));
}

#[test]
fn a_jpeg_with_its_encoding_kept_is_left_silently() {
    let src = image("/DeviceGray", 8, "DCTDecode", "", &jpeg(&smooth(1), 1, 90));
    let (_, p) = plan(doc(AT_100, &[src]), &[("image(quality=20)", None)]);
    assert!(p.changes.iter().all(|c| c.id.num != 5));
    assert!(skip(&p, 5).is_none());
}

#[test]
fn a_downsampled_jpeg_is_written_no_better_than_its_source() {
    let src = image("/DeviceRGB", 8, "DCTDecode", "", &jpeg(&smooth(3), 3, 50));
    let (s, p) = plan(doc(AT_100, &[src]), &[("image(ppi=72,quality=90)", None)]);
    let c = p
        .changes
        .iter()
        .find(|c| c.id.num == 5)
        .unwrap_or_else(|| panic!("{:?}", p.skipped));
    let img = c.image.as_ref().unwrap();
    assert_eq!(img.size_after, (100, 100));
    let q: JpegQuality = img.jpeg.unwrap();
    assert_eq!((q.asked, q.used, q.source_estimate), (90, 50, Some(50)));
    let (dict, raw) = applied(s, &p, 5);
    assert_eq!(dict.get(b"Width"), Some(&Object::Integer(100)));
    assert_eq!(estimate_jpeg_quality(&raw), Some(50));
}

#[test]
fn cmyk_with_an_inverted_decode_keeps_its_meaning() {
    let samples = smooth(4);
    let decode = "/Decode [1 0 1 0 1 0 1 0]";
    let src = image(
        "/DeviceCMYK",
        8,
        "FlateDecode",
        decode,
        &flate::encode_with_level(&samples, 0),
    );
    let (s, p) = plan(
        doc(AT_100, &[src]),
        &[("image(encode=jpeg,quality=95)", None)],
    );
    let (dict, raw) = applied(s, &p, 5);
    assert!(matches!(dict.get(b"Decode"), Some(Object::Array(a)) if a.len() == 8));
    let session = EditSession::new(crate::document::Document::from_bytes(doc("", &[])).unwrap());
    let back = decode_image_view(&session.view(), &dict, &raw, false).unwrap();
    // The stored samples come back as they went in; /Decode still inverts them.
    assert!(mean_error(&back.samples[..samples.len()], &samples) < 3.0);
}

#[test]
fn sixteen_bit_samples_written_as_jpeg_report_the_depth_change() {
    let wide: Vec<u8> = smooth(1).iter().flat_map(|v| [*v, 0x80]).collect();
    let src = image(
        "/DeviceGray",
        16,
        "FlateDecode",
        "",
        &flate::encode_with_level(&wide, 0),
    );
    let (_, p) = plan(doc(AT_100, &[src]), &[("image(encode=jpeg)", None)]);
    let c = p.changes.iter().find(|c| c.id.num == 5).unwrap();
    let img = c.image.as_ref().unwrap();
    assert_eq!(img.bpc, (16, 8));
    assert_eq!(img.jpeg.unwrap().used, 80);
}

#[test]
fn images_jpeg_cannot_store_are_refused_by_name() {
    let mono = image(
        "/DeviceGray",
        1,
        "FlateDecode",
        "",
        &flate::encode(&[0x55; 38 * 300]),
    );
    let palette = image(
        "[/Indexed /DeviceRGB 1 <000000FFFFFF>]",
        8,
        "FlateDecode",
        "",
        &flate::encode(&[1; 300 * 300]),
    );
    let content = "q 100 0 0 100 0 0 cm /Im5 Do /Im6 Do /Im7 Do Q";
    let images = [mono, palette, grey("/Mask [0 10]")];
    let (_, p) = plan(doc(content, &images), &[("image(encode=jpeg)", None)]);
    for (n, why) in [
        (5, ImageSkip::OneBitAsJpeg),
        (6, ImageSkip::PaletteAsJpeg),
        (7, ImageSkip::ColourKey(ImageStep::Jpeg)),
    ] {
        assert!(
            skip(&p, n) == Some(SkipReason::ImageUnsupported(why)),
            "object {n}: {:?}",
            skip(&p, n)
        );
    }
}

#[test]
fn a_matte_parent_may_change_encoding_at_its_size() {
    let parent = grey("/SMask 6 0 R");
    let mask = grey("/Matte [0]");
    let (_, p) = plan(
        doc(AT_100, &[parent, mask]),
        &[("image(encode=jpeg)", None)],
    );
    assert!(skip(&p, 5).is_none(), "{:?}", skip(&p, 5));
}

#[test]
fn undecodable_jpeg_data_is_skipped_with_the_decoder_error() {
    let junk = image("/DeviceGray", 8, "DCTDecode", "", &[0xFF; 64]);
    let (_, p) = plan(doc(AT_100, &[junk]), &[("image(ppi=72)", None)]);
    assert!(matches!(skip(&p, 5), Some(SkipReason::Undecodable(_))));
}
