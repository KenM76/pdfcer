#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::pipeline_tests::{AT_100, N, doc, plan};
use super::ssim::structural_similarity as ssim;
use crate::filters::flate;
use crate::image_codec::decode_image_view;
use crate::jpeg_write::{Chroma, JpegOptions, encode};
use crate::object::{ObjId, Object};
use crate::optimize::{ImagePatch, JpegQuality, OptimizePlan, SsimScore, StreamAction};
use crate::space_audit::tests::stream;

/// A `components`-channel `N` x `N` raster with smooth ramps and a hard
/// edge, so quality changes the score.
fn picture(components: usize) -> Vec<u8> {
    (0..N as usize * N as usize * components)
        .map(|i| {
            let (p, c) = (i / components, i % components);
            let (x, y) = (p % N as usize, p / N as usize);
            let edge = if (x / 37 + y / 41) % 2 == 0 { 0 } else { 90 };
            ((x + 2 * y + 40 * c + edge) % 256) as u8
        })
        .collect()
}

fn flate_image(cs: &str, samples: &[u8]) -> Vec<u8> {
    stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width {N} /Height {N} /ColorSpace {cs} \
             /BitsPerComponent 8 /Filter /FlateDecode"
        ),
        &flate::encode_with_level(samples, 0),
    )
}

fn quality_of(p: &OptimizePlan) -> JpegQuality {
    let c = p
        .changes
        .iter()
        .find(|c| c.id.num == 5)
        .unwrap_or_else(|| panic!("{:?}", p.skipped));
    c.image.as_ref().unwrap().jpeg.unwrap()
}

/// The applied image's samples, decoded.
fn written(s: crate::edit::EditSession, p: &OptimizePlan) -> Vec<u8> {
    let mut s = s;
    s.apply_optimize(p).unwrap();
    let Some(Object::Stream(st)) = s.value(ObjId::new(5, 0)).cloned() else {
        panic!()
    };
    let raw = s.view().slice(st.data_span).unwrap().to_vec();
    decode_image_view(&s.view(), &st.dict, &raw, false)
        .unwrap()
        .samples
}

#[test]
fn the_written_image_meets_the_target_at_a_quality_below_the_ceiling() {
    for (cs, n) in [("/DeviceGray", 1), ("/DeviceRGB", 3), ("/DeviceCMYK", 4)] {
        let px = picture(n);
        let (s, p) = plan(
            doc(AT_100, &[flate_image(cs, &px)]),
            &[("image(encode=jpeg,ssim=0.97)", None)],
        );
        let q = quality_of(&p);
        let pick = q.ssim.unwrap();
        assert_eq!(
            (q.asked, pick.target),
            (100, SsimScore::parse("0.97").unwrap())
        );
        assert!(pick.met && pick.achieved >= pick.target, "{cs}: {pick:?}");
        assert!(q.used < 100 && pick.trials > 1, "{cs}: {q:?}");
        let back = written(s, &p);
        let score = ssim(&px, &back, N, N, n as u32).unwrap();
        assert_eq!(SsimScore::from_f64_floor(score), pick.achieved, "{cs}");
    }
}

#[test]
fn a_lower_target_never_picks_a_higher_quality() {
    let px = picture(3);
    let used = |t: &str| {
        let action = format!("image(encode=jpeg,ssim={t})");
        let (_, p) = plan(
            doc(AT_100, &[flate_image("/DeviceRGB", &px)]),
            &[(action.as_str(), None)],
        );
        quality_of(&p).used
    };
    let (hi, lo) = (used("0.99"), used("0.9"));
    assert!(lo <= hi, "{lo} > {hi}");
}

#[test]
fn a_set_quality_is_the_ceiling_and_a_missed_target_is_reported() {
    let px = picture(1);
    let (_, p) = plan(
        doc(AT_100, &[flate_image("/DeviceGray", &px)]),
        &[("image(encode=jpeg,quality=30,ssim=0.9999)", None)],
    );
    let q = quality_of(&p);
    let pick = q.ssim.unwrap();
    assert_eq!((q.asked, q.used, pick.met, pick.trials), (30, 30, false, 1));
    assert!(pick.achieved < pick.target);
}

#[test]
fn a_jpeg_source_is_searched_below_its_own_quality() {
    let px = picture(1);
    let opts = JpegOptions {
        quality: 92,
        chroma: Chroma::Auto,
        progressive: false,
        optimize_huffman: true,
    };
    let src = stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width {N} /Height {N} /ColorSpace /DeviceGray \
             /BitsPerComponent 8 /Filter /DCTDecode"
        ),
        &encode(&px, N, N, 1, opts).unwrap(),
    );
    let (_, p) = plan(
        doc(AT_100, &[src]),
        &[("image(encode=jpeg,ssim=0.95)", None)],
    );
    let q = quality_of(&p);
    assert_eq!((q.asked, q.source_estimate), (100, Some(92)));
    assert!(q.used < 92 && q.ssim.unwrap().met, "{q:?}");
}

#[test]
fn ssim_parses_prints_and_folds() {
    let action = StreamAction::parse("image(encode=jpeg, ssim=0.980)").unwrap();
    assert_eq!(action.to_string(), "image(encode=jpeg,ssim=0.98)");
    let StreamAction::Image(patch) = action else {
        panic!()
    };
    assert_eq!(patch.ssim.map(SsimScore::ten_thousandths), Some(9800));
    for bad in ["image(ssim=0)", "image(ssim=1.5)", "image(ssim=high)"] {
        let e = ImagePatch::parse(bad).unwrap_err();
        assert!(e.starts_with("image(): bad ssim"), "{bad}: {e}");
    }
}
