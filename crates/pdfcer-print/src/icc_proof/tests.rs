#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use super::*;
use crate::icc_encode::{D50, matrix_trc_rgb, mluc_tag, para_tag, profile, xyz_tag};

const CMYK: &[u8] = include_bytes!("../../../../fixtures/synthetic/icc-rgb/dest-cmyk.icc");

fn out(colour: &ProofColour, rgb: [u8; 3]) -> [u8; 3] {
    let mut px = [rgb[0], rgb[1], rgb[2], 255];
    colour.compile().unwrap().apply(&mut px);
    [px[0], px[1], px[2]]
}

/// An sRGB-primaries output profile on tinted paper: its media white is a
/// cream, and only ICC-absolute colorimetric shows it.
fn cream_paper() -> Vec<u8> {
    let srgb = icccer_cmm::builtin::srgb().matrix();
    let mut p = matrix_trc_rgb(b"prtr", "cream paper", srgb.rows, &para_tag(0, &[2.2]));
    let wtpt = xyz_tag([0.90, 0.93, 0.62]);
    let at = p
        .windows(20)
        .position(|w| w == xyz_tag(D50).as_slice())
        .unwrap();
    p[at..at + 20].copy_from_slice(&wtpt);
    p
}

#[test]
fn a_cmyk_press_profile_is_accepted_as_a_proof() {
    let proof = ProofProfile::from_bytes(CMYK).unwrap();
    assert_eq!(proof.space(), ProfileSpace::Cmyk);
    assert!(matches!(
        PrinterProfile::from_bytes(CMYK),
        Err(ProfileError::Cmyk)
    ));
}

/// The press leg is applied: saturated sRGB green is outside a CMYK gamut and
/// comes back duller. White still prints as paper white.
#[test]
fn a_cmyk_proof_dulls_out_of_gamut_green_and_keeps_white() {
    let colour = ProofColour::new(ProofProfile::from_bytes(CMYK).unwrap());
    let g = out(&colour, [0, 255, 0]);
    assert!(g[0] > 20 || g[2] > 20, "green unchanged: {g:?}");
    assert!(g[1] < 250, "green unchanged: {g:?}");
    let w = out(&colour, [255, 255, 255]);
    assert!(w.iter().all(|&c| c >= 250), "white moved: {w:?}");
}

#[test]
fn simulating_paper_prints_its_tint_and_relative_does_not() {
    let proof = ProofProfile::from_bytes(&cream_paper()).unwrap();
    let mut colour = ProofColour::new(proof);
    let relative = out(&colour, [255, 255, 255]);
    assert!(relative.iter().all(|&c| c >= 250), "{relative:?}");
    colour.simulate_paper = true;
    let paper = out(&colour, [255, 255, 255]);
    assert!(
        paper[2] + 20 < paper[0],
        "cream paper should print yellowish: {paper:?}"
    );
}

#[test]
fn a_grey_printer_after_a_proof_writes_equal_channels() {
    let grey = profile(
        b"prtr",
        b"GRAY",
        &[
            (b"desc", mluc_tag("grey")),
            (b"wtpt", xyz_tag(D50)),
            (b"kTRC", para_tag(0, &[2.2])),
        ],
    );
    let mut colour = ProofColour::new(ProofProfile::from_bytes(CMYK).unwrap());
    colour.printer = Some(PrinterProfile::from_bytes(&grey).unwrap());
    let px = out(&colour, [200, 40, 90]);
    assert_eq!(px[0], px[1]);
    assert_eq!(px[1], px[2]);
}

#[test]
fn unusable_proof_profiles_are_named_errors() {
    let srgb = icccer_cmm::builtin::srgb().matrix();
    let scanner = matrix_trc_rgb(b"scnr", "scanner", srgb.rows, &para_tag(0, &[2.2]));
    assert!(matches!(
        ProofProfile::from_bytes(&scanner),
        Err(ProfileError::UnsupportedClass(c)) if c == "scnr"
    ));
    assert!(matches!(
        ProofProfile::from_bytes(b"not a profile"),
        Err(ProfileError::NotAProfile(_))
    ));
}

/// `dest-cmyk.icc`'s black point is outside icccer's estimation subset, so
/// asking for BPC builds the chain without it and says so.
#[test]
fn an_unestimable_black_point_is_reported_not_fatal() {
    let colour = ProofColour::new(ProofProfile::from_bytes(CMYK).unwrap());
    assert!(colour.compile().unwrap().black_point_compensation_refused());
    let cream = ProofProfile::from_bytes(&cream_paper()).unwrap();
    let plain = ProofColour::new(cream);
    assert!(!plain.compile().unwrap().black_point_compensation_refused());
}
