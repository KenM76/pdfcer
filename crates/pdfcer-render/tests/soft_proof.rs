//! Soft proofing the separation preview against a CMYK output profile.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)] // Tests fail loudly.

use pdfcer_core::document::Document;
use pdfcer_render::{
    RenderOptions, SeparationPreview, SoftProof, SoftProofError, render_page_with_view,
};

fn icc(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../fixtures/synthetic/icc-rgb/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

/// A 40×20 page: 100% black on the left half, nothing on the right.
fn black_and_blank() -> Document {
    let content = "0 0 0 1 k 0 0 20 20 re f";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 40 20] /Contents 4 0 R >>".to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f\r\n");
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    Document::from_bytes(buf).unwrap()
}

/// The `(black, blank)` pixels of [`black_and_blank`] under `proof`.
fn proofed(proof: Option<SoftProof>) -> ([u8; 3], [u8; 3]) {
    let doc = black_and_blank();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    let options = RenderOptions::default()
        .with_separation_preview(Some(SeparationPreview::new().with_soft_proof(proof)));
    let out = render_page_with_view(&doc.view(), &pages[0], 1.0, &options).unwrap();
    let px = |x: u32| {
        let p = out.pixmap.pixel(x, 10).unwrap();
        [p.red(), p.green(), p.blue()]
    };
    (px(10), px(30))
}

#[test]
fn a_cmyk_output_profile_is_accepted() {
    let proof = SoftProof::from_bytes(icc("dest-cmyk.icc")).unwrap();
    assert!(!proof.simulates_black_ink());
    assert!(proof.clone().with_paper_colour(true).simulates_black_ink());
}

#[test]
fn profiles_that_cannot_proof_are_refused() {
    assert!(matches!(
        SoftProof::from_bytes(icc("swap-rg.icc")),
        Err(SoftProofError::NotCmyk { .. })
    ));
    assert!(matches!(
        SoftProof::from_bytes(b"not a profile".to_vec()),
        Err(SoftProofError::Unreadable(_))
    ));
    assert!(matches!(
        SoftProof::from_bytes(vec![0; pdfcer_render::MAX_SOFT_PROOF_PROFILE_BYTES + 1]),
        Err(SoftProofError::TooLarge { .. })
    ));
}

/// `profile` with its media white point (`wtpt`, ICC.1:2022 §9.2.36)
/// replaced by `xyz`: the same press on a different paper.
fn with_media_white(mut profile: Vec<u8>, xyz: [f64; 3]) -> Vec<u8> {
    let be = |at: usize| u32::from_be_bytes(profile[at..at + 4].try_into().unwrap()) as usize;
    let count = be(128);
    let entry = (0..count)
        .map(|i| 132 + 12 * i)
        .find(|&e| &profile[e..e + 4] == b"wtpt")
        .expect("a wtpt tag");
    let at = be(entry + 4) + 8;
    for (i, v) in xyz.iter().enumerate() {
        #[allow(clippy::cast_possible_truncation)]
        let fixed = (v * 65536.0).round() as i32;
        profile[at + 4 * i..at + 4 * i + 4].copy_from_slice(&fixed.to_be_bytes());
    }
    profile
}

#[test]
fn the_proof_profile_decides_how_black_ink_looks() {
    let proof = SoftProof::from_bytes(icc("v4-cmyk-mab-lab.icc")).unwrap();
    let (plain, _) = proofed(None);
    let (through_profile, _) = proofed(Some(proof));
    assert_ne!(plain, through_profile);
}

#[test]
fn paper_colour_shows_the_measured_paper() {
    let cream = with_media_white(icc("v4-cmyk-mab-lab.icc"), [0.90, 0.92, 0.65]);
    let proof = SoftProof::from_bytes(cream).unwrap();
    let (_, relative) = proofed(Some(proof.clone().with_black_ink(true)));
    let (_, paper) = proofed(Some(proof.with_paper_colour(true)));
    assert!(paper[2] + 20 < relative[2], "{paper:?} vs {relative:?}");
}

#[test]
fn a_refused_black_compensation_is_reported() {
    let proof = SoftProof::from_bytes(icc("v4-cmyk-mab-lab.icc")).unwrap();
    assert!(!proof.compensates_black());
    assert_eq!(
        proofed(Some(proof.clone())),
        proofed(Some(proof.with_black_ink(true)))
    );
}
