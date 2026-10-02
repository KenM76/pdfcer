//! Importing a GIF and placing it on a page (GIF89a → §8.9.5 image XObject).
//!
//! The fixtures under `fixtures/synthetic/gif/` are written by Pillow's GIF
//! encoder from formulas in `gen-gif-fixtures.py`; the oracles here re-derive
//! those formulas. Colours are compared after resolving each index through
//! the imported lookup table, so the test does not depend on how the encoder
//! numbered its palette.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, NewImage};
use pdfcer_core::filters::flate;
use pdfcer_core::image_import::{
    self, ImageFormat, ImageImportError, ImportColorSpace, ImportedImage, RecompressReason,
};
use pdfcer_core::page_tree::Rect;
use std::path::{Path, PathBuf};

const FOUR: [[u8; 3]; 4] = [[200, 30, 30], [30, 160, 60], [40, 60, 220], [250, 250, 250]];

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/gif")
        .join(rel)
}

fn gif_bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).unwrap_or_else(|e| panic!("read fixture gif/{name}: {e}"))
}

fn imported(name: &str) -> ImportedImage {
    image_import::import(&gif_bytes(name)).unwrap_or_else(|e| panic!("import gif/{name}: {e}"))
}

/// Every pixel's RGB, resolved through the imported `/Indexed` lookup.
fn rgb(img: &ImportedImage) -> Vec<[u8; 3]> {
    let ImportColorSpace::Indexed { lookup, .. } = &img.color_space else {
        panic!("a GIF imports as /Indexed, got {:?}", img.color_space);
    };
    flate::decode(&img.data, None)
        .unwrap()
        .iter()
        .map(|&i| {
            let at = usize::from(i) * 3;
            [lookup[at], lookup[at + 1], lookup[at + 2]]
        })
        .collect()
}

fn mask(img: &ImportedImage) -> Vec<u8> {
    let m = img.soft_mask.as_ref().expect("an /SMask");
    assert_eq!(
        (m.width, m.height, m.bits_per_component),
        (img.width, img.height, 8)
    );
    flate::decode(&m.data, None).unwrap()
}

fn session() -> EditSession {
    EditSession::new(
        Document::load(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/synthetic/dimension/plain-base.pdf"),
        )
        .expect("load fixture"),
    )
}

fn rect(llx: f64, lly: f64, urx: f64, ury: f64) -> Rect {
    Rect { llx, lly, urx, ury }
}

#[test]
fn a_two_colour_gif_with_a_transparent_index_imports_with_a_clear_soft_mask() {
    let img = imported("two-colour-transparent.gif");
    assert_eq!(img.format, ImageFormat::Gif);
    assert_eq!((img.width, img.height), (8, 6));
    let expected: Vec<u8> = (0..6)
        .flat_map(|y| (0..8).map(move |x| if (x + y) % 2 == 1 { 0 } else { 255 }))
        .collect();
    assert_eq!(mask(&img), expected, "clear exactly where the index was");
    let colours = rgb(&img);
    for (c, a) in colours.iter().zip(&expected) {
        if *a == 255 {
            assert_eq!(*c, [0, 0, 0], "the opaque squares are black");
        }
    }
    assert!(img.notes.alpha_to_soft_mask);
    assert_eq!(
        img.notes.recompressed,
        Some(RecompressReason::SourceCodecNotReusable)
    );
}

#[test]
fn an_interlaced_gif_lands_in_display_order() {
    let img = imported("interlaced.gif");
    assert_eq!((img.width, img.height), (32, 20));
    let expected: Vec<[u8; 3]> = (0..20)
        .flat_map(|y| (0..32).map(move |x| FOUR[(y + x / 8) % 4]))
        .collect();
    assert_eq!(rgb(&img), expected);
    assert!(img.soft_mask.is_none(), "an opaque GIF needs no mask");
}

#[test]
fn an_animated_gif_places_frame_one_and_counts_the_rest() {
    let img = imported("animated-3-frames.gif");
    assert_eq!(img.notes.gif_frames_ignored, 2);
    assert!(
        rgb(&img).iter().all(|c| *c == FOUR[0]),
        "frame one is index 0"
    );
}

#[test]
fn placing_an_animated_transparent_gif_discloses_both() {
    let mut s = session();
    for (name, frames, smask) in [
        ("animated-3-frames.gif", 2, false),
        ("two-colour-transparent.gif", 0, true),
    ] {
        let img = imported(name);
        let out = s
            .add_image(&NewImage::new(0, rect(10.0, 10.0, 90.0, 90.0), &img))
            .unwrap_or_else(|e| panic!("place {name}: {e}"));
        let d = &out.disclosures;
        assert_eq!(d.gif_frames_ignored, frames, "{name}");
        assert_eq!(d.soft_mask_written, smask, "{name}");
        assert_eq!(out.soft_mask_id.is_some(), smask, "{name}");
        assert!(d.any(), "{name}: a re-compression is always worth saying");
    }
}

#[test]
fn a_truncated_gif_is_refused_as_corrupt() {
    let err = image_import::import(&gif_bytes("truncated.gif")).unwrap_err();
    assert!(matches!(err, ImageImportError::Corrupt { .. }), "{err:?}");
    assert!(err.to_string().contains("damaged or truncated"), "{err}");
}

#[test]
fn every_truncation_of_every_fixture_is_diagnosed_not_panicked() {
    for name in [
        "two-colour-transparent.gif",
        "interlaced.gif",
        "animated-3-frames.gif",
    ] {
        let full = gif_bytes(name);
        for n in 0..full.len() {
            match image_import::import(&full[..n]) {
                Ok(_)
                | Err(ImageImportError::Corrupt { .. })
                | Err(ImageImportError::NotAnImage) => {}
                Err(other) => panic!("{name} cut to {n}: {other:?}"),
            }
        }
    }
}

#[test]
fn a_corrupted_code_stream_is_refused_not_guessed() {
    let mut bytes = gif_bytes("interlaced.gif");
    // Image descriptor at 13 + 12-byte global table; then min code size and
    // the first data sub-block. Set every data byte to all-ones codes.
    let data_start = 13 + 12 + 10 + 2;
    let len = usize::from(bytes[data_start - 1]);
    for b in &mut bytes[data_start..data_start + len] {
        *b = 0xFF;
    }
    assert!(matches!(
        image_import::import(&bytes),
        Err(ImageImportError::Corrupt { .. })
    ));
}
