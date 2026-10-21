//! A `/FontFile2` program cut by [`GLYPH_ZERO_RESUBSETTER`] draws the page
//! exactly as the whole program did, and the saved file is smaller, for a
//! simple and a composite font, for two fonts sharing one program, and for a symbolic font.

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
    for (name, fonts, kept) in [
        ("simple.pdf", 1, 5),
        ("type0-identity.pdf", 1, 2),
        ("shared-program.pdf", 2, 5),
        ("symbolic.pdf", 1, 2),
    ] {
        cut_draws_the_same(name, fonts, kept);
    }
}

/// `name` cut once for its `fonts` users, keeping `kept` glyphs.
fn cut_draws_the_same(name: &str, fonts: usize, kept: u32) {
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
    let t = &cut.targets[0];
    assert_eq!((t.fonts.len(), t.glyphs_kept), (fonts, kept), "{name}");
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

fn plan_pair(merge: bool) -> (EditSession, pdfcer_core::optimize::OptimizePlan) {
    let s = EditSession::new(Document::from_bytes(fixture("pair.pdf")).unwrap());
    let mut request = ResubsetFonts::new(&GLYPH_ZERO_RESUBSETTER);
    if !merge {
        request = request.without_merge();
    }
    let mut settings = OptimizeSettings::new();
    settings.resubset = Some(request);
    let plan = s.plan_optimize(&settings).unwrap();
    (s, plan)
}

/// A face held as a simple and a composite font, each with its own copy of
/// the program, becomes one program both fonts draw from, the page drawing
/// exactly as before; undo brings both copies back.
#[test]
fn a_face_held_twice_merges_into_one_program() {
    let original = fixture("pair.pdf");
    let (mut s, plan) = plan_pair(true);
    let before = pixels(&s);
    let cut = plan.resubset.as_ref().unwrap();
    assert_eq!(cut.targets.len(), 1, "refused {:?}", cut.refused);
    let t = &cut.targets[0];
    assert_eq!((t.merged.len(), t.fonts.len()), (1, 2), "{t:?}");
    assert_eq!(t.glyphs_kept, 3, "notdef, A and C");
    let gone = t.merged[0];
    s.apply_optimize(&plan).unwrap();
    assert!(s.value(gone).is_none(), "the merged-away copy is deleted");
    let (file, _) = s
        .save_optimized(&plan, &SaveOptions::default(), &RebuildOptions::none())
        .unwrap();
    let OptimizedFile::Smaller(bytes) = file else {
        panic!("expected a smaller file, got {file:?}");
    };
    assert!(bytes.len() < original.len());
    assert_eq!(
        bytes.windows(8).filter(|w| w == b"/Length1").count(),
        1,
        "one program left"
    );
    let reopened = EditSession::new(Document::from_bytes(bytes).unwrap());
    assert!(
        pixels(&reopened) == before,
        "the merged font draws differently"
    );
    s.undo();
    assert!(s.value(gone).is_some(), "undo restores the copy");
    assert!(pixels(&s) == before);
}

#[test]
fn without_merge_each_copy_is_cut_on_its_own() {
    let (_, plan) = plan_pair(false);
    let cut = plan.resubset.as_ref().unwrap();
    assert_eq!(cut.targets.len(), 2, "refused {:?}", cut.refused);
    assert!(cut.targets.iter().all(|t| t.merged.is_empty()));
    assert!(cut.unmerged.is_empty());
}
