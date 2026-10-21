#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::{ImageSkip, ImageStep};
use crate::document::Document;
use crate::edit::EditSession;
use crate::filters::flate;
use crate::object::{ObjId, Object};
use crate::optimize::tests::build;
use crate::optimize::{
    OptimizePlan, OptimizeSettings, OptimizedFile, SettingSource, SkipReason, StreamAction,
};
use crate::space_audit::tests::stream;
use crate::writer::SaveOptions;

pub(super) const N: u32 = 300;

/// A 300 x 300 8-bit grey noise image, Flate-compressed, with `extra` dict keys.
pub(super) fn grey(extra: &str) -> Vec<u8> {
    let mut x = 0x9E37_79B9u32;
    let px: Vec<u8> = (0..N * N)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 24) as u8
        })
        .collect();
    stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width {N} /Height {N} /ColorSpace /DeviceGray \
             /BitsPerComponent 8 /Filter /FlateDecode {extra}"
        ),
        &flate::encode_with_level(&px, 9),
    )
}

/// Objects 1-3 the document, 4 the content, 5.. `images` (`/Im5`, ...).
pub(super) fn doc(content: &str, images: &[Vec<u8>]) -> Vec<u8> {
    let names: String = (0..images.len())
        .map(|i| format!("/Im{n} {n} 0 R ", n = i + 5))
        .collect();
    let mut bodies = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Contents 4 0 R \
             /Resources << /XObject << {names}>> >> >>"
        )
        .into_bytes(),
        stream("", content.as_bytes()),
    ];
    bodies.extend_from_slice(images);
    build(&bodies)
}

/// Plan `bytes` with `actions`: `(action, None)` sets the default stream
/// action, `(action, Some(n))` overrides object `n`.
pub(super) fn plan(bytes: Vec<u8>, actions: &[(&str, Option<u32>)]) -> (EditSession, OptimizePlan) {
    let mut settings = OptimizeSettings::new();
    for (a, obj) in actions {
        let action = StreamAction::parse(a).unwrap();
        match obj {
            Some(n) => {
                settings.objects.insert(ObjId::new(*n, 0), action);
            }
            None => settings.streams = action,
        }
    }
    let session = EditSession::new(Document::from_bytes(bytes).unwrap());
    let plan = session.plan_optimize(&settings).unwrap();
    (session, plan)
}

/// Why object `n` was skipped, if it was.
pub(super) fn skip(plan: &OptimizePlan, n: u32) -> Option<SkipReason> {
    plan.skipped
        .iter()
        .find(|s| s.id.num == n)
        .map(|s| s.reason.clone())
}

pub(super) const AT_100: &str = "q 100 0 0 100 0 0 cm /Im5 Do Q";

#[test]
fn an_image_drawn_small_is_downsampled_to_the_target_and_reported() {
    let (_, plan) = plan(doc(AT_100, &[grey("")]), &[("image(ppi=72)", None)]);
    let c = plan.changes.iter().find(|c| c.id.num == 5).unwrap();
    let img = c.image.as_ref().unwrap();
    // 300 px over 100 pt is 216 ppi; 72 / 216 of 300 px is 100 px.
    assert_eq!((img.size_before, img.size_after), ((N, N), (100, 100)));
    assert_eq!(img.binding.as_ref().unwrap().page, 1);
    assert!((img.binding_ppi_after.unwrap() - 72.0).abs() < 1e-9);
    assert_eq!(img.settings.ppi.unwrap().source, SettingSource::Streams);
    assert!(c.lossy && c.after_bytes < c.before_bytes);
}

#[test]
fn the_largest_placement_binds_and_each_axis_scales_on_its_own() {
    // Drawn at 100 pt and at 200 pt: the 200 pt use is 108 ppi and binds.
    let two = "q 100 0 0 100 0 0 cm /Im5 Do Q q 200 0 0 200 0 0 cm /Im5 Do Q";
    let (_, p) = plan(doc(two, &[grey("")]), &[("image(ppi=72)", None)]);
    assert!(
        p.changes.iter().all(|c| c.id.num != 5),
        "108 ppi is not above 108"
    );
    let (_, p) = plan(doc(two, &[grey("")]), &[("image(ppi=50,above=60)", None)]);
    let img = p
        .changes
        .iter()
        .find(|c| c.id.num == 5)
        .unwrap_or_else(|| panic!("{:?}", p.skipped));
    // 300 x 50 / 108 = 138.9.
    assert_eq!(img.image.as_ref().unwrap().size_after, (139, 139));
    // Stretched: 216 ppi across, 108 ppi down; only the width shrinks.
    let tall = "q 100 0 0 200 0 0 cm /Im5 Do Q";
    let (_, p) = plan(doc(tall, &[grey("")]), &[("image(ppi=72)", None)]);
    let c = p.changes.iter().find(|c| c.id.num == 5).unwrap();
    assert_eq!(c.image.as_ref().unwrap().size_after, (100, N));
}

#[test]
fn an_object_override_turns_off_only_downsampling() {
    let (_, p) = plan(
        doc(AT_100, &[grey("")]),
        &[("image(ppi=72)", None), ("image(method=off)", Some(5))],
    );
    assert!(p.changes.iter().all(|c| c.id.num != 5));
    assert!(skip(&p, 5).is_none());
}

#[test]
fn images_this_pass_cannot_resample_are_skipped_with_a_reason() {
    let jpx = stream(
        "/Type /XObject /Subtype /Image /Width 300 /Height 300 /ColorSpace /DeviceGray \
         /BitsPerComponent 8 /Filter /JPXDecode",
        &[0xFF; 64],
    );
    let content = "q 100 0 0 100 0 0 cm /Im5 Do /Im6 Do /Im7 Do Q";
    let images = [grey(""), jpx, grey("/Mask [0 10]")];
    let (_, p) = plan(doc(content, &images), &[("image(ppi=72,keyed=skip)", None)]);
    assert!(p.changes.iter().any(|c| c.id.num == 5));
    assert_eq!(
        skip(&p, 6),
        Some(SkipReason::UnsupportedFilter("JPXDecode".into()))
    );
    assert!(
        skip(&p, 7)
            == Some(SkipReason::ImageUnsupported(ImageSkip::ColourKey(
                ImageStep::Resample
            )))
    );
    // Not drawn anywhere: no resolution to measure.
    let (_, p) = plan(doc("", &[grey("")]), &[("image(ppi=72)", None)]);
    assert_eq!(skip(&p, 5), Some(SkipReason::Unplaced));
}

#[test]
fn a_palette_image_takes_the_nearest_index_and_never_blends() {
    let idx: Vec<u8> = (0..N * N)
        .map(|i| if (i / 7) % 2 == 0 { 0 } else { 3 })
        .collect();
    let image = stream(
        &format!(
            "/Type /XObject /Subtype /Image /Width {N} /Height {N} \
             /ColorSpace [/Indexed /DeviceRGB 3 <000000FF000000FF000000FF>] \
             /BitsPerComponent 8 /Filter /FlateDecode"
        ),
        &flate::encode_with_level(&idx, 9),
    );
    let (mut s, p) = plan(
        doc(AT_100, &[image]),
        &[("image(ppi=72,method=lanczos)", None)],
    );
    assert!(p.changes.iter().any(|c| c.id.num == 5));
    s.apply_optimize(&p).unwrap();
    let Some(Object::Stream(st)) = s.value(ObjId::new(5, 0)).cloned() else {
        panic!()
    };
    let data =
        crate::filters::decode_stream(&st.dict, s.view().slice(st.data_span).unwrap()).unwrap();
    assert_eq!(data.len(), 100 * 100);
    assert!(data.iter().all(|v| *v == 0 || *v == 3));
}

#[test]
fn min_saving_keeps_an_image_that_shrinks_too_little() {
    let (_, p) = plan(
        doc(AT_100, &[grey("")]),
        &[("image(ppi=72,min-saving=99)", None)],
    );
    assert!(matches!(
        skip(&p, 5),
        Some(SkipReason::BelowMinSaving { pct: 99, .. })
    ));
}

#[test]
fn a_saved_file_carries_the_new_size_and_every_other_object_unchanged() {
    let original = doc(AT_100, &[grey("")]);
    let (mut s, p) = plan(original.clone(), &[("image(ppi=72)", None)]);
    s.apply_optimize(&p).unwrap();
    let OptimizedFile::Smaller(bytes) = s
        .save_optimized(
            &p,
            &SaveOptions::default(),
            &crate::optimize::RebuildOptions::none(),
        )
        .unwrap()
        .0
    else {
        panic!("expected a smaller file")
    };
    let after = Document::from_bytes(bytes).unwrap();
    let before = Document::from_bytes(original).unwrap();
    let Some(Object::Stream(st)) = after.get(ObjId::new(5, 0)).map(|o| o.value.clone()) else {
        panic!()
    };
    assert_eq!(st.dict.get(b"Width"), Some(&Object::Integer(100)));
    assert_eq!(st.dict.get(b"Height"), Some(&Object::Integer(100)));
    for n in 1..=4 {
        let id = ObjId::new(n, 0);
        assert_eq!(
            after.get(id).map(|o| &o.value),
            before.get(id).map(|o| &o.value)
        );
    }
}
