#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use super::*;
use crate::icc_encode::{D50, curv_tag, matrix_trc_rgb, mluc_tag, profile, xyz_tag};

/// A grey printer profile whose black is lifted: device 0 prints at
/// Y = 0.03, with no stored black point.
fn lifted_grey() -> Vec<u8> {
    let samples: Vec<u16> = (0..256)
        .map(|i| {
            let v = f64::from(i) / 255.0;
            let y = 0.03 + 0.97 * v.powf(2.2);
            (y * 65535.0).round() as u16
        })
        .collect();
    profile(
        b"prtr",
        b"GRAY",
        &[
            (b"desc", mluc_tag("lifted grey")),
            (b"wtpt", xyz_tag(D50)),
            (b"kTRC", curv_tag(&samples)),
        ],
    )
}

/// A wide-gamut RGB display profile (Adobe RGB-like primaries, D50 white,
/// gamma 2.2).
fn wide_rgb(class: &[u8; 4]) -> Vec<u8> {
    let white = icccer_color::xyz::Xyz {
        x: D50[0],
        y: D50[1],
        z: D50[2],
    };
    let m = icccer_cmm::builtin::rgb_to_xyz(&[(0.64, 0.33), (0.21, 0.71), (0.15, 0.06)], white)
        .unwrap();
    matrix_trc_rgb(
        class,
        "wide rgb",
        m.rows,
        &crate::icc_encode::para_tag(0, &[2.2]),
    )
}

fn grey_out(colour: &PrinterColour, v: u8) -> u8 {
    let mut px = [v, v, v, 255];
    colour.compile().unwrap().apply(&mut px);
    assert_eq!(px[0], px[1]);
    assert_eq!(px[1], px[2]);
    px[0]
}

#[test]
fn the_encoded_srgb_source_is_icccers_srgb() {
    let src = icccer_profile::Profile::parse(&crate::icc_encode::srgb_source()).unwrap();
    let chain = icccer_cmm::transform::Chain::with_destination(
        &src,
        icccer_cmm::transform::Destination::None,
        icccer_cmm::matrix_trc::Intent::MediaRelative,
    )
    .unwrap();
    for rgb in [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.2, 0.5, 0.9],
        [1.0, 1.0, 1.0],
    ] {
        let out = chain.convert(&rgb).unwrap();
        for (a, b) in rgb.iter().zip(&out) {
            assert!((a - b).abs() < 0.5 / 255.0, "{rgb:?} -> {out:?}");
        }
    }
}

#[test]
fn bpc_keeps_dark_greys_apart_and_without_it_they_clip() {
    let profile = PrinterProfile::from_bytes(&lifted_grey()).unwrap();
    assert_eq!(profile.space(), ProfileSpace::Gray);
    assert_eq!(profile.name(), "lifted grey");
    let mut colour = PrinterColour::new(profile);
    let on: Vec<u8> = [20, 40, 60].iter().map(|v| grey_out(&colour, *v)).collect();
    assert!(on[0] < on[1] && on[1] < on[2], "BPC on: {on:?}");
    colour.black_point_compensation = false;
    let off: Vec<u8> = [20, 40, 60].iter().map(|v| grey_out(&colour, *v)).collect();
    assert_eq!(off[0], off[1], "BPC off: {off:?}");
    assert!(off[2] > off[1], "BPC off: {off:?}");
}

#[test]
fn wide_gamut_green_gains_red_and_keeps_alpha() {
    let colour = PrinterColour::new(PrinterProfile::from_bytes(&wide_rgb(b"mntr")).unwrap());
    let mut px = [0, 255, 0, 255, 0, 128, 0, 128];
    let changed = apply_printer_colour(&mut px, &colour).unwrap();
    assert_eq!(changed, 2);
    assert!(px[0] > 60, "red {}", px[0]);
    assert_eq!(px[3], 255);
    assert_eq!(px[7], 128);
    // The premultiplied half-alpha pixel is the opaque result scaled by alpha.
    assert!(px[4].abs_diff(premultiply(px[0], 128)) <= 1);
}

#[test]
fn white_stays_white_through_relative_colorimetric() {
    let colour = PrinterColour::new(PrinterProfile::from_bytes(&wide_rgb(b"prtr")).unwrap());
    let mut px = [255, 255, 255, 255];
    colour.compile().unwrap().apply(&mut px);
    assert_eq!(px, [255, 255, 255, 255]);
}

#[test]
fn unusable_profiles_are_named_errors() {
    let cmyk = include_bytes!("../../../../fixtures/synthetic/icc-rgb/dest-cmyk.icc");
    assert!(matches!(
        PrinterProfile::from_bytes(cmyk),
        Err(ProfileError::Cmyk)
    ));
    assert!(matches!(
        PrinterProfile::from_bytes(&wide_rgb(b"scnr")),
        Err(ProfileError::UnsupportedClass(c)) if c == "scnr"
    ));
    assert!(matches!(
        PrinterProfile::from_bytes(b"not an icc profile at all"),
        Err(ProfileError::NotAProfile(_))
    ));
    let mut lab = wide_rgb(b"prtr");
    lab[16..20].copy_from_slice(b"Lab ");
    assert!(matches!(
        PrinterProfile::from_bytes(&lab),
        Err(ProfileError::UnsupportedSpace(s)) if s == "Lab"
    ));
}

#[test]
fn an_oversized_file_is_refused_from_its_length() {
    let dir = std::env::temp_dir().join(format!("pdfcer-x063-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("huge.icc");
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(MAX_PROFILE_BYTES + 1).unwrap();
    drop(file);
    let err = PrinterProfile::read(&path).unwrap_err();
    std::fs::remove_file(&path).unwrap();
    assert!(matches!(err, ProfileError::TooLarge { size, .. } if size == MAX_PROFILE_BYTES + 1));
}

#[test]
fn a_file_under_the_cap_reads() {
    let dir = std::env::temp_dir().join(format!("pdfcer-x063r-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("grey.icc");
    std::fs::write(&path, lifted_grey()).unwrap();
    let read = PrinterProfile::read(&path);
    std::fs::remove_file(&path).unwrap();
    assert_eq!(read.unwrap().name(), "lifted grey");
}
