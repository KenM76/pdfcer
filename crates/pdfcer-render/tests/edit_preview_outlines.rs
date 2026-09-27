//! `edit_preview::preview_outlines`: a typing preview drawn in the run's own
//! font, landing inside the box the core laid out.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::text_edit::{EditOptions, EditRequest};
use pdfcer_render::edit_preview::preview_outlines;
use pdfcer_render::font::{FontEnvironment, GlyphSource};

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn outlines(rel: &str) -> (pdfcer_render::edit_preview::PreviewOutlines, [f64; 4]) {
    let mut s = EditSession::new(Document::load(&fixture(rel)).expect("fixture parses"));
    let p = s
        .edit_text_preview(
            &EditRequest::find_replace(0, "teh", "the"),
            &EditOptions::default(),
        )
        .expect("previewable");
    let o = preview_outlines(&s.view(), &p, &FontEnvironment::bundled());
    (o, p.bbox)
}

fn assert_inside(o: &pdfcer_render::edit_preview::PreviewOutlines, bbox: [f64; 4]) {
    assert!(o.skipped.is_none(), "{:?}", o.skipped);
    assert_eq!(o.glyphs.len(), 3);
    for path in &o.glyphs {
        let b = path
            .as_ref()
            .expect("t, h and e all have outlines")
            .bounds();
        // Letterforms sit inside the advance-by-ascent/descent box, give or
        // take side bearings.
        let slack = 1.0;
        assert!(f64::from(b.left()) >= bbox[0] - slack, "{b:?} {bbox:?}");
        assert!(f64::from(b.right()) <= bbox[2] + slack, "{b:?} {bbox:?}");
        assert!(f64::from(b.top()) >= bbox[1] - slack, "{b:?} {bbox:?}");
        assert!(f64::from(b.bottom()) <= bbox[3] + slack, "{b:?} {bbox:?}");
    }
}

#[test]
fn an_embedded_run_previews_in_its_own_program() {
    let (o, bbox) = outlines("textedit/embedded_full.pdf");
    assert_eq!(o.source, Some(GlyphSource::Embedded));
    assert_inside(&o, bbox);
}

#[test]
fn a_non_embedded_run_previews_in_the_disclosed_substitute() {
    let (o, bbox) = outlines("textedit/nonembedded.pdf");
    assert_eq!(o.source, Some(GlyphSource::Bundled));
    assert_inside(&o, bbox);
}
