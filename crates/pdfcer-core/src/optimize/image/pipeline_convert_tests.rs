#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::pipeline_tests::{AT_100, N, doc, plan, skip};
use crate::edit::EditSession;
use crate::filters::{self, flate};
use crate::jpeg_write::{Chroma, JpegOptions, encode};
use crate::object::{Dict, ObjId, Object};
use crate::optimize::{ImageChange, ImageConvert, ImageEncode, OptimizePlan, SkipReason};
use crate::space_audit::tests::stream;

const PX: usize = (N * N) as usize;

fn image(cs: &str, bpc: u8, extra: &str, samples: &[u8]) -> Vec<u8> {
    stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width {N} /Height {N} /ColorSpace {cs} \
             /BitsPerComponent {bpc} /Filter /FlateDecode {extra}"
        ),
        &flate::encode_with_level(samples, 0),
    )
}

/// `n`-component noise.
fn noise(n: usize) -> Vec<u8> {
    let mut x = 0x2545_F491u32;
    (0..PX * n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 24) as u8
        })
        .collect()
}

fn change(p: &OptimizePlan, n: u32) -> &ImageChange {
    p.changes
        .iter()
        .find(|c| c.id.num == n)
        .and_then(|c| c.image.as_ref())
        .unwrap_or_else(|| panic!("{:?}", p.skipped))
}

fn lossy(p: &OptimizePlan, n: u32) -> bool {
    p.changes.iter().find(|c| c.id.num == n).unwrap().lossy
}

/// Object `n` after applying `p`: its dictionary and decoded samples.
fn applied(mut s: EditSession, p: &OptimizePlan, n: u32) -> (Dict, Vec<u8>) {
    s.apply_optimize(p).unwrap();
    let Some(Object::Stream(st)) = s.value(ObjId::new(n, 0)).cloned() else {
        panic!()
    };
    let data = filters::decode_stream(&st.dict, s.view().slice(st.data_span).unwrap()).unwrap();
    (st.dict, data)
}

fn name(v: &str) -> Object {
    Object::Name(v.as_bytes().into())
}

fn unsupported(p: &OptimizePlan, n: u32) -> String {
    match skip(p, n) {
        Some(SkipReason::ImageUnsupported(w)) => w.to_string(),
        other => panic!(
            "{n}: {other:?} {:?}",
            p.changes.iter().map(|c| c.id.num).collect::<Vec<_>>()
        ),
    }
}

#[test]
fn rgb_to_gray_uses_the_spec_formula_and_reports_it() {
    let rgb = noise(3);
    let (s, p) = plan(
        doc(AT_100, &[image("/DeviceRGB", 8, "", &rgb)]),
        &[("image(convert=gray)", None)],
    );
    let c = change(&p, 5);
    let conv = c.conversion.as_ref().unwrap();
    assert_eq!(conv.to, ImageConvert::Gray);
    assert!(conv.formula.unwrap().contains("10.4.2.2"));
    assert!(!conv.not_colour_managed);
    assert_eq!(c.color_space, ("DeviceRGB".into(), "DeviceGray".into()));
    assert_eq!(c.bpc, (8, 8));
    assert!(lossy(&p, 5));
    let (dict, gray) = applied(s, &p, 5);
    assert_eq!(dict.get(b"ColorSpace"), Some(&name("DeviceGray")));
    assert_eq!(gray.len(), PX);
    for (g, px) in gray.iter().zip(rgb.chunks(3)).take(500) {
        let want = 0.3 * f64::from(px[0]) + 0.59 * f64::from(px[1]) + 0.11 * f64::from(px[2]);
        assert!((f64::from(*g) - want).abs() <= 1.0, "{g} vs {want}");
    }
}

#[test]
fn cmyk_to_gray_applies_and_drops_an_inverted_decode() {
    let cmyk = noise(4);
    let decode = "/Decode [1 0 1 0 1 0 1 0]";
    let (s, p) = plan(
        doc(AT_100, &[image("/DeviceCMYK", 8, decode, &cmyk)]),
        &[("image(convert=gray)", None)],
    );
    assert!(
        change(&p, 5)
            .conversion
            .as_ref()
            .unwrap()
            .formula
            .unwrap()
            .contains("10.4.2.3")
    );
    let (dict, gray) = applied(s, &p, 5);
    assert!(dict.get(b"Decode").is_none());
    for (g, px) in gray.iter().zip(cmyk.chunks(4)).take(500) {
        let v = |i: usize| f64::from(255 - px[i]) / 255.0;
        let ink = (0.3 * v(0) + 0.59 * v(1) + 0.11 * v(2) + v(3)).min(1.0);
        let want = (1.0 - ink) * 255.0;
        assert!((f64::from(*g) - want).abs() <= 1.0, "{g} vs {want}");
    }
}

#[test]
fn an_icc_image_converted_by_formula_says_it_is_not_colour_managed() {
    let icc = stream("/N 3", &[0; 16]);
    let images = [image("[/ICCBased 6 0 R]", 8, "", &noise(3)), icc];
    let (_, p) = plan(doc(AT_100, &images), &[("image(convert=gray)", None)]);
    let conv = change(&p, 5).conversion.clone().unwrap();
    assert!(conv.not_colour_managed);
    assert_eq!(conv.from, "ICCBased");
}

#[test]
fn bilevel_by_otsu_reports_its_threshold_and_writes_one_bit() {
    let two: Vec<u8> = (0..PX).map(|i| if i % 3 == 0 { 40 } else { 210 }).collect();
    let (s, p) = plan(
        doc(AT_100, &[image("/DeviceGray", 8, "", &two)]),
        &[("image(convert=bilevel)", None)],
    );
    let c = change(&p, 5);
    let t = c.conversion.as_ref().unwrap().threshold.unwrap();
    assert!((40..210).contains(&t), "{t}");
    assert_eq!(c.bpc, (8, 1));
    let (dict, bits) = applied(s, &p, 5);
    assert_eq!(dict.get(b"BitsPerComponent"), Some(&Object::Integer(1)));
    assert_eq!(bits.len(), (N as usize).div_ceil(8) * N as usize);
    // Row 0: pixel 0 dark, 1 and 2 light.
    assert_eq!(bits[0] >> 5, 0b011);
}

#[test]
fn bilevel_dither_and_fixed_thresholds_are_honoured() {
    let mid = vec![100u8; PX];
    let (_, p) = plan(
        doc(AT_100, &[image("/DeviceGray", 8, "", &mid)]),
        &[("image(convert=bilevel,threshold=dither)", None)],
    );
    assert_eq!(change(&p, 5).conversion.as_ref().unwrap().threshold, None);
    let (s, p) = plan(
        doc(AT_100, &[image("/DeviceGray", 8, "", &mid)]),
        &[("image(convert=bilevel,threshold=99)", None)],
    );
    assert_eq!(
        change(&p, 5).conversion.as_ref().unwrap().threshold,
        Some(99)
    );
    let (_, bits) = applied(s, &p, 5);
    assert!(bits.iter().all(|b| *b == 0xFF || *b == 0xF0), "all white");
}

#[test]
fn a_few_colour_image_becomes_an_exact_lossless_palette() {
    let colours = [[255, 0, 0], [0, 128, 0], [0, 0, 255]];
    let rgb: Vec<u8> = (0..PX).flat_map(|i| colours[i % 3]).collect();
    let (s, p) = plan(
        doc(AT_100, &[image("/DeviceRGB", 8, "", &rgb)]),
        &[("image(convert=indexed)", None)],
    );
    let c = change(&p, 5);
    let conv = c.conversion.as_ref().unwrap();
    assert!(conv.exact);
    assert_eq!(conv.colors, Some(3));
    assert_eq!(c.bpc, (8, 2));
    assert_eq!(c.color_space.1, "Indexed DeviceRGB");
    assert!(!lossy(&p, 5));
    let (dict, idx) = applied(s, &p, 5);
    let Some(Object::Array(cs)) = dict.get(b"ColorSpace") else {
        panic!()
    };
    let Object::String(lookup) = &cs[3] else {
        panic!()
    };
    assert_eq!((&cs[0], &cs[2]), (&name("Indexed"), &Object::Integer(2)));
    let stride = (N as usize * 2).div_ceil(8);
    for x in 0..N as usize {
        let i = usize::from(idx[x / 4] >> (6 - 2 * (x % 4)) & 3);
        assert_eq!(&lookup[i * 3..i * 3 + 3], &colours[x % 3]);
    }
    assert_eq!(idx.len(), stride * N as usize);
}

#[test]
fn a_photo_capped_at_sixteen_colours_is_lossy_at_four_bits() {
    let (_, p) = plan(
        doc(AT_100, &[image("/DeviceRGB", 8, "", &noise(3))]),
        &[("image(convert=indexed,colors=16)", None)],
    );
    let c = change(&p, 5);
    let conv = c.conversion.as_ref().unwrap();
    assert!(!conv.exact);
    assert_eq!(conv.colors, Some(16));
    assert_eq!(c.bpc, (8, 4));
    assert!(lossy(&p, 5));
}

#[test]
fn a_jpeg_converted_to_gray_under_keep_stays_jpeg() {
    let opts = JpegOptions {
        quality: 80,
        chroma: Chroma::Auto,
        progressive: false,
        optimize_huffman: true,
    };
    let rgb: Vec<u8> = (0..PX * 3).map(|i| ((i / 3) % 256) as u8).collect();
    let data = encode(&rgb, N, N, 3, opts).unwrap();
    let dct = stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width {N} /Height {N} /ColorSpace /DeviceRGB \
             /BitsPerComponent 8 /Filter /DCTDecode"
        ),
        &data,
    );
    let (_, p) = plan(doc(AT_100, &[dct]), &[("image(convert=gray)", None)]);
    let c = change(&p, 5);
    assert_eq!(c.encoded_as, ImageEncode::Jpeg);
    assert_eq!(c.color_space.1, "DeviceGray");
}

#[test]
fn a_bilevel_result_is_never_jpeg_even_after_a_fold() {
    let (_, p) = plan(
        doc(AT_100, &[image("/DeviceGray", 8, "", &noise(1))]),
        &[
            ("image(convert=bilevel)", None),
            ("image(encode=jpeg)", Some(5)),
        ],
    );
    assert!(unsupported(&p, 5).contains("not written as JPEG"));
}

#[test]
fn conversions_that_change_nothing_leave_the_image() {
    let gray = image("/DeviceGray", 8, "", &noise(1));
    let (_, p) = plan(doc(AT_100, &[gray]), &[("image(convert=gray)", None)]);
    assert!(p.changes.is_empty() && p.skipped.is_empty(), "{p:?}");
    let pal = image(
        "[/Indexed /DeviceRGB 1 <000000FFFFFF>]",
        8,
        "",
        &vec![0; PX],
    );
    let (_, p) = plan(doc(AT_100, &[pal]), &[("image(convert=indexed)", None)]);
    assert!(p.changes.is_empty() && p.skipped.is_empty(), "{p:?}");
}

#[test]
fn masks_keys_and_ungreyable_spaces_are_refused_by_name() {
    let content = "q 100 0 0 100 0 0 cm /Im5 Do /Im7 Do /Im8 Do /Im9 Do Q";
    let images = [
        image("/DeviceRGB", 8, "/SMask 6 0 R", &noise(3)),
        image("/DeviceRGB", 8, "", &noise(3)),
        image("/DeviceRGB", 8, "/Mask [0 10 0 10 0 10]", &noise(3)),
        image("[/Separation /Gold /DeviceCMYK 10 0 R]", 8, "", &noise(1)),
        image("[/Lab << /WhitePoint [0.95 1 1.09] >>]", 8, "", &noise(3)),
        stream("/FunctionType 2 /Domain [0 1] /N 1", b""),
    ];
    let (_, p) = plan(doc(content, &images), &[("image(convert=gray)", None)]);
    assert!(
        p.changes.iter().any(|c| c.id.num == 5),
        "the parent converts"
    );
    assert!(unsupported(&p, 6).contains("soft mask"));
    assert!(unsupported(&p, 7).contains("colour-key"));
    assert!(unsupported(&p, 8).contains("no defined grey"));
    assert!(unsupported(&p, 9).contains("Lab"));
}
