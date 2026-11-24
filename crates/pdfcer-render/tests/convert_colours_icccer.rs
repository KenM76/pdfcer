//! `IcccerConverter`, the shipped colour converter behind
//! `EditSession::convert_colours`, through a real CMYK destination.

use pdfcer_core::color::RenderingIntent;
use pdfcer_core::prepress::{ColourConverter, ColourSource};
use pdfcer_render::convert_colours::IcccerConverter;

const DEST: &[u8] = include_bytes!("../../../fixtures/synthetic/icc-rgb/dest-cmyk.icc");
const SWAP_RG: &[u8] = include_bytes!("../../../fixtures/synthetic/icc-rgb/swap-rg.icc");
const D50: [f64; 3] = [0.9642, 1.0, 0.8249];

fn convert(converter: &IcccerConverter, source: &ColourSource, input: &[f64]) -> (Vec<f64>, bool) {
    let t = converter
        .transform(source, DEST, RenderingIntent::RelativeColorimetric, false)
        .expect("builds");
    let c = t.convert(input).expect("converts");
    (c.values, c.out_of_gamut)
}

fn close(a: &[f64], b: &[f64], tol: f64) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol)
}

#[test]
fn device_rgb_reaches_four_inks_and_darker_means_more_ink() {
    let c = IcccerConverter::new();
    let (white, _) = convert(&c, &ColourSource::DeviceRgb, &[1.0, 1.0, 1.0]);
    let (grey, _) = convert(&c, &ColourSource::DeviceRgb, &[0.5, 0.5, 0.5]);
    let (black, _) = convert(&c, &ColourSource::DeviceRgb, &[0.0, 0.0, 0.0]);
    assert_eq!(white.len(), 4);
    let ink = |v: &[f64]| v.iter().sum::<f64>();
    assert!(
        ink(&white) < ink(&grey) && ink(&grey) < ink(&black),
        "{white:?} {grey:?} {black:?}"
    );
    assert!(white.iter().all(|v| (0.0..=1.0).contains(v)));
}

#[test]
fn an_embedded_profile_is_used_not_read_as_srgb() {
    let c = IcccerConverter::new();
    let icc = ColourSource::IccBased {
        profile: SWAP_RG.to_vec(),
        components: 3,
    };
    // swap-rg.icc is sRGB's colorants with red and green swapped, so its
    // pure red is sRGB's pure green.
    let (via_profile, _) = convert(&c, &icc, &[1.0, 0.0, 0.0]);
    let (green, _) = convert(&c, &ColourSource::DeviceRgb, &[0.0, 1.0, 0.0]);
    let (red, _) = convert(&c, &ColourSource::DeviceRgb, &[1.0, 0.0, 0.0]);
    assert!(
        close(&via_profile, &green, 0.02),
        "{via_profile:?} vs {green:?}"
    );
    assert!(
        !close(&via_profile, &red, 0.02),
        "{via_profile:?} vs {red:?}"
    );
}

#[test]
fn lab_white_lands_where_rgb_white_does() {
    let c = IcccerConverter::new();
    let lab = ColourSource::Lab {
        white: D50,
        range: [-100.0, 100.0, -100.0, 100.0],
    };
    let (from_lab, _) = convert(&c, &lab, &[100.0, 0.0, 0.0]);
    let (from_rgb, _) = convert(&c, &ColourSource::DeviceRgb, &[1.0, 1.0, 1.0]);
    assert!(
        close(&from_lab, &from_rgb, 0.02),
        "{from_lab:?} vs {from_rgb:?}"
    );
    let cal = ColourSource::CalGray {
        white: D50,
        gamma: 1.0,
    };
    let (dark, _) = convert(&c, &cal, &[0.1]);
    assert!(dark.iter().sum::<f64>() > from_lab.iter().sum::<f64>());
}

#[test]
fn the_gamut_tolerance_decides_out_of_gamut() {
    let strict = IcccerConverter::new().with_gamut_tolerance(0.0);
    let lax = IcccerConverter::new().with_gamut_tolerance(1.0e9);
    let blue = [0.0, 0.0, 1.0];
    assert!(convert(&strict, &ColourSource::DeviceRgb, &blue).1);
    assert!(!convert(&lax, &ColourSource::DeviceRgb, &blue).1);
}

#[test]
fn notes_and_refusals_are_named() {
    let c = IcccerConverter::new();
    let t = c
        .transform(
            &ColourSource::DeviceRgb,
            DEST,
            RenderingIntent::Perceptual,
            true,
        )
        .expect("builds");
    assert!(t.note().expect("a note").contains("sRGB"));
    assert!(
        c.transform(
            &ColourSource::DeviceRgb,
            &[0; 40],
            RenderingIntent::Perceptual,
            true
        )
        .is_err()
    );
    let bad = ColourSource::IccBased {
        profile: vec![1, 2, 3],
        components: 3,
    };
    let err = c
        .transform(&bad, DEST, RenderingIntent::Perceptual, false)
        .err()
        .expect("refused");
    assert!(err.message().contains("embedded profile"), "{err}");
}
