#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation
)]

use super::pipeline_tests::{AT_100, N, doc, plan};
use crate::edit::EditSession;
use crate::filters::{self, flate};
use crate::object::{Dict, ObjId, Object};
use crate::optimize::{ImagePatch, OptimizePlan, Predictor, StreamAction};
use crate::space_audit::tests::stream;

fn image(cs: &str, bpc: u8, samples: &[u8]) -> Vec<u8> {
    stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width {N} /Height {N} /ColorSpace {cs} \
             /BitsPerComponent {bpc} /Filter /FlateDecode"
        ),
        &flate::encode_with_level(samples, 0),
    )
}

/// A smooth `n`-component ramp: what prediction exists for.
fn ramp(n: usize, bytes_per: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for y in 0..N as usize {
        for x in 0..N as usize {
            for c in 0..n * bytes_per {
                out.push(((x * 3 + y * 2 + c * 40) % 256) as u8);
            }
        }
    }
    out
}

fn applied(mut s: EditSession, p: &OptimizePlan) -> (Dict, Vec<u8>, u64) {
    s.apply_optimize(p).unwrap();
    let Some(Object::Stream(st)) = s.value(ObjId::new(5, 0)).cloned() else {
        panic!()
    };
    let raw = s.view().slice(st.data_span).unwrap();
    let data = filters::decode_stream(&st.dict, raw).unwrap();
    (st.dict, data, raw.len() as u64)
}

fn parm(d: &Dict, k: &[u8]) -> Option<i64> {
    match d.get(b"DecodeParms") {
        Some(Object::Dict(p)) => p.get(k).and_then(Object::as_int),
        _ => None,
    }
}

#[test]
fn flate_output_is_png_predicted_by_default_and_decodes_exactly() {
    let rgb = ramp(3, 1);
    let (s, p) = plan(
        doc(AT_100, &[image("/DeviceRGB", 8, &rgb)]),
        &[("image(encode=flate)", None)],
    );
    let c = p.changes.iter().find(|c| c.id.num == 5).unwrap();
    assert!(!c.lossy);
    assert!(c.image.as_ref().unwrap().predicted);
    let (dict, data, len) = applied(s, &p);
    assert_eq!(parm(&dict, b"Predictor"), Some(15));
    assert_eq!(parm(&dict, b"Colors"), Some(3));
    assert_eq!(parm(&dict, b"BitsPerComponent"), Some(8));
    assert_eq!(parm(&dict, b"Columns"), Some(i64::from(N)));
    assert_eq!(data, rgb);

    let (s, p) = plan(
        doc(AT_100, &[image("/DeviceRGB", 8, &rgb)]),
        &[("image(encode=flate,predictor=off)", None)],
    );
    let (dict, data, plain) = applied(s, &p);
    assert!(dict.get(b"DecodeParms").is_none());
    assert_eq!(data, rgb);
    assert!(len < plain, "predicted {len} vs plain {plain}");
}

#[test]
fn sixteen_bit_and_one_bit_outputs_round_trip_through_the_predictor() {
    let g16 = ramp(1, 2);
    let (s, p) = plan(
        doc(AT_100, &[image("/DeviceGray", 16, &g16)]),
        &[("image(encode=flate,predictor=png)", None)],
    );
    let (dict, data, _) = applied(s, &p);
    assert_eq!(parm(&dict, b"BitsPerComponent"), Some(16));
    assert_eq!(data, g16);

    let (s, p) = plan(
        doc(AT_100, &[image("/DeviceGray", 8, &ramp(1, 1))]),
        &[("image(convert=bilevel,threshold=128,predictor=png)", None)],
    );
    let (dict, data, _) = applied(s, &p);
    assert_eq!(parm(&dict, b"BitsPerComponent"), Some(1));
    assert_eq!(parm(&dict, b"Colors"), Some(1));
    assert_eq!(data.len(), (N as usize).div_ceil(8) * N as usize);
}

#[test]
fn auto_keeps_plain_flate_when_prediction_does_not_help() {
    let mut flat = vec![0u8; (N * N) as usize];
    flat[7] = 9;
    let (s, p) = plan(
        doc(AT_100, &[image("/DeviceGray", 8, &flat)]),
        &[("image(encode=flate)", None)],
    );
    let predicted = p.changes[0].image.as_ref().unwrap().predicted;
    let (dict, data, _) = applied(s, &p);
    assert_eq!(dict.get(b"DecodeParms").is_some(), predicted);
    assert_eq!(data, flat);
}

#[test]
fn predictor_off_keeps_the_plain_recompress_path() {
    let (_, p) = plan(
        doc(AT_100, &[image("/DeviceGray", 8, &ramp(1, 1))]),
        &[("image(encode=flate-6,predictor=off)", None)],
    );
    let c = p.changes.iter().find(|c| c.id.num == 5).unwrap();
    assert_eq!(c.action, StreamAction::Recompress { level: 6 });
    assert!(c.image.is_none());
}

#[test]
fn the_predictor_key_parses_displays_and_refuses_unknown_values() {
    let StreamAction::Image(patch) = StreamAction::parse("image(predictor=png)").unwrap() else {
        panic!()
    };
    assert_eq!(patch.predictor, Some(Predictor::Png));
    assert_eq!(
        StreamAction::Image(patch).to_string(),
        "image(predictor=png)"
    );
    assert!(ImagePatch::parse("image(predictor=tiff)").is_err());
    assert_eq!(Predictor::default(), Predictor::Auto);
}
