//! A shell can build an [`AugmentRequest`] and ask an augmenter directly,
//! without an edit session (G210). `cid-subset.ttf` is the cmap-less
//! `.notdef A B C` cut of `face.ttf` (`pdfcerAugFace`, which also draws
//! D, E and É but no Z); see `tools/gen-augment-face-fixtures.py`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::text_edit::{AugmentRequest, OutlineCheck, ProgramAddressing, SubsetAugmenter};
use pdfcer_render::FontData;
use pdfcer_render::font::InstalledFaceAugmenter;

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../fixtures/synthetic/text/augment/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture; run tools/gen-augment-face-fixtures.py")
}

fn augmenter() -> InstalledFaceAugmenter {
    let mut faces = InstalledFaceAugmenter::new();
    faces.insert("pdfcerAugFace", FontData::new(fixture("face.ttf")));
    faces
}

const KNOWN: [(u32, char); 3] = [(1, 'A'), (2, 'B'), (3, 'C')];

#[test]
fn a_composite_subset_offers_the_faces_characters() {
    let program = fixture("cid-subset.ttf");
    let request = AugmentRequest::new(&program, "ABCDEF+pdfcerAugFace", &[])
        .with_addressing(ProgramAddressing::CidKeyed(&KNOWN));
    let offered = augmenter().candidates(&request);
    for c in ['D', 'E', 'É'] {
        assert!(offered.contains(&c), "{c} not offered: {offered:?}");
    }
    assert!(!offered.contains(&'Z'));
}

#[test]
fn addable_keeps_what_the_face_draws() {
    let program = fixture("cid-subset.ttf");
    let chars = ['D', 'Z'];
    let request = AugmentRequest::new(&program, "ABCDEF+pdfcerAugFace", &chars)
        .with_shown(&['A', 'B', 'C'])
        .with_outline_check(OutlineCheck::ShownOnly)
        .with_addressing(ProgramAddressing::CidKeyed(&KNOWN));
    assert_eq!(augmenter().addable(&request), ['D']);
}

#[test]
fn an_unnamed_face_offers_nothing() {
    let program = fixture("cid-subset.ttf");
    let request = AugmentRequest::new(&program, "ABCDEF+SomeOtherFace", &['D'])
        .with_addressing(ProgramAddressing::CidKeyed(&KNOWN));
    assert!(augmenter().candidates(&request).is_empty());
    assert!(augmenter().addable(&request).is_empty());
}
