#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use super::pipeline_tests::{AT_100, N, doc, plan, skip};
use crate::filters::{decode_stream, flate};
use crate::object::{ObjId, Object};
use crate::optimize::{
    ImagePatch, ImageSkip, ImageStep, KeyedResample, ResampleMethod, SettingSource, SkipReason,
};
use crate::space_audit::tests::stream;

/// A 4-bit grey image of stripes valued 0, 4 and 15, keyed on 3 to 5.
fn keyed() -> Vec<u8> {
    let nibble = |x: u32| [0u8, 4, 15][(x / 3 % 3) as usize];
    let px: Vec<u8> = (0..N * N / 2)
        .map(|i| {
            let x = i * 2 % N;
            nibble(x) << 4 | nibble(x + 1)
        })
        .collect();
    stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width {N} /Height {N} /ColorSpace /DeviceGray \
             /BitsPerComponent 4 /Mask [3 5] /Filter /FlateDecode"
        ),
        &flate::encode_with_level(&px, 9),
    )
}

#[test]
fn a_keyed_image_is_resampled_by_nearest_pixel_and_keeps_its_key() {
    let (mut s, p) = plan(
        doc(AT_100, &[keyed()]),
        &[("image(ppi=72,method=lanczos)", None)],
    );
    let c = p.changes.iter().find(|c| c.id.num == 5).unwrap();
    let img = c.image.as_ref().unwrap();
    assert_eq!((img.size_after, img.bpc), ((100, 100), (4, 4)));
    assert_eq!(img.settings.method.value, ResampleMethod::Subsample);
    assert_eq!(img.settings.method.source, SettingSource::BuiltIn);
    s.apply_optimize(&p).unwrap();
    let Some(Object::Stream(st)) = s.value(ObjId::new(5, 0)).cloned() else {
        panic!()
    };
    assert!(matches!(st.dict.get(b"Mask"), Some(Object::Array(a)) if a.len() == 2));
    let data = decode_stream(&st.dict, s.view().slice(st.data_span).unwrap()).unwrap();
    assert_eq!(data.len(), 100 * 100 / 2);
    let nibbles = data.iter().flat_map(|b| [b >> 4, b & 0xF]);
    assert!(nibbles.into_iter().all(|v| [0, 4, 15].contains(&v)));
}

#[test]
fn keyed_skip_leaves_a_keyed_image_with_the_reason() {
    let (_, p) = plan(
        doc(AT_100, &[keyed()]),
        &[("image(ppi=72,keyed=skip)", None)],
    );
    assert_eq!(
        skip(&p, 5),
        Some(SkipReason::ImageUnsupported(ImageSkip::ColourKey(
            ImageStep::Resample
        )))
    );
}

#[test]
fn keyed_parses_and_prints() {
    let p = ImagePatch::parse("image(keyed=skip)").unwrap();
    assert_eq!(p.keyed, Some(KeyedResample::Skip));
    assert_eq!(p.to_string(), "image(keyed=skip)");
    assert!(ImagePatch::parse("image(keyed=blend)").is_err());
}
