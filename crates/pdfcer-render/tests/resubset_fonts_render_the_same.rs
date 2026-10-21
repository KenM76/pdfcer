//! A `/FontFile2` program cut by [`GLYPH_ZERO_RESUBSETTER`] draws the page
//! exactly as the whole program did, and the saved file is smaller, for a
//! simple and a composite font.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::optimize::resubset::ResubsetFonts;
use pdfcer_core::optimize::{OptimizeSettings, OptimizedFile, RebuildOptions};
use pdfcer_core::writer::SaveOptions;
use pdfcer_render::font::resubset::GLYPH_ZERO_RESUBSETTER;
use pdfcer_render::render_page_view;

/// A fixture from `tools/gen-resubset-fixtures.py`.
fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/synthetic/resubset")
            .join(name),
    )
    .unwrap()
}

fn pixels(s: &EditSession) -> Vec<u8> {
    let page = s.pages().unwrap()[0].clone();
    render_page_view(&s.view(), &page, 2.0)
        .unwrap()
        .pixmap
        .data()
        .to_vec()
}

#[test]
fn a_cut_font_draws_the_page_unchanged_in_a_smaller_file() {
    for name in ["simple.pdf", "type0-identity.pdf"] {
        cut_draws_the_same(name);
    }
}

fn cut_draws_the_same(name: &str) {
    let original = fixture(name);
    let mut s = EditSession::new(Document::from_bytes(original.clone()).unwrap());
    let before = pixels(&s);
    assert!(
        before
            .chunks(4)
            .filter(|p| p[..3] != [255, 255, 255])
            .count()
            > 500,
        "{name}: the fixture paints its text"
    );
    let mut settings = OptimizeSettings::new();
    settings.resubset = Some(ResubsetFonts::new(&GLYPH_ZERO_RESUBSETTER));
    let plan = s.plan_optimize(&settings).unwrap();
    let cut = plan.resubset.as_ref().expect("a re-subset plan");
    assert_eq!(cut.targets.len(), 1, "{name}: refused {:?}", cut.refused);
    s.apply_optimize(&plan).unwrap();
    let (file, _) = s
        .save_optimized(&plan, &SaveOptions::default(), &RebuildOptions::none())
        .unwrap();
    let OptimizedFile::Smaller(bytes) = file else {
        panic!("{name}: expected a smaller file, got {file:?}");
    };
    assert!(bytes.len() < original.len());
    let reopened = EditSession::new(Document::from_bytes(bytes).unwrap());
    assert!(
        pixels(&reopened) == before,
        "{name}: the cut font draws differently"
    );
}
