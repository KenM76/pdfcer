//! The built-in presets' image settings, end to end.
#![allow(clippy::unwrap_used)]

use super::pipeline_tests::{doc, grey};
use crate::document::Document;
use crate::edit::EditSession;
use crate::filters::flate;
use crate::optimize::{ImageEncode, OptimizePlan, Preset};
use crate::space_audit::tests::stream;

/// A 600 x 600 1-bit checkerboard, Flate-compressed.
fn mono() -> Vec<u8> {
    let row: Vec<u8> = (0..75)
        .map(|i| if i % 2 == 0 { 0xF0 } else { 0x0F })
        .collect();
    let px: Vec<u8> = (0..600).flat_map(|_| row.clone()).collect();
    stream(
        "/Type /XObject /Subtype /Image /Width 600 /Height 600 /ColorSpace /DeviceGray \
         /BitsPerComponent 1 /Filter /FlateDecode",
        &flate::encode(&px),
    )
}

/// Image 5 drawn at 50 pt (432 ppi for grey) and at 100 pt as image 6
/// (216 ppi); image 7 is 1-bit at 50 pt (864 ppi).
fn planned(preset: Preset) -> OptimizePlan {
    let content = "q 50 0 0 50 0 0 cm /Im5 Do Q q 100 0 0 100 100 100 cm /Im6 Do Q \
                   q 50 0 0 50 250 250 cm /Im7 Do Q";
    let bytes = doc(content, &[grey(""), grey(""), mono()]);
    let s = EditSession::new(Document::from_bytes(bytes).unwrap());
    s.plan_optimize(&preset.settings()).unwrap()
}

/// Object `n`'s new size and encoding, if it changed as an image.
fn image(p: &OptimizePlan, n: u32) -> Option<((u32, u32), ImageEncode)> {
    let c = p.changes.iter().find(|c| c.id.num == n)?;
    let i = c.image.as_ref()?;
    Some((i.size_after, i.encoded_as))
}

#[test]
fn lossless_and_archive_leave_every_image() {
    for preset in [Preset::Lossless, Preset::Archive] {
        let p = planned(preset);
        assert!((5..=7).all(|n| image(&p, n).is_none()), "{preset}");
    }
}

#[test]
fn standard_resamples_only_images_above_its_trigger() {
    let p = planned(Preset::Standard);
    assert_eq!(image(&p, 5).map(|i| i.0), Some((104, 104)));
    assert!(image(&p, 6).is_none(), "216 ppi is under the 225 trigger");
    assert_eq!(image(&p, 7).map(|i| i.0), Some((208, 208)));
}

#[test]
fn screen_writes_jpeg_but_keeps_one_bit_images_lossless() {
    let p = planned(Preset::Screen);
    assert!(matches!(image(&p, 5), Some(((67, 67), ImageEncode::Jpeg))));
    assert!(matches!(
        image(&p, 6),
        Some(((133, 133), ImageEncode::Jpeg))
    ));
    let (size, encode) = image(&p, 7).unwrap();
    assert_eq!(size, (208, 208));
    assert_ne!(encode, ImageEncode::Jpeg);
}

#[test]
fn print_keeps_print_resolution() {
    let p = planned(Preset::Print);
    assert!(
        (5..=7).all(|n| image(&p, n).is_none()),
        "all under the trigger"
    );
    let bytes = doc("q 25 0 0 25 0 0 cm /Im5 Do Q", &[grey("")]);
    let s = EditSession::new(Document::from_bytes(bytes).unwrap());
    let p = s.plan_optimize(&Preset::Print.settings()).unwrap();
    assert_eq!(
        image(&p, 5).map(|i| i.0),
        Some((104, 104)),
        "864 to 300 ppi"
    );
}
