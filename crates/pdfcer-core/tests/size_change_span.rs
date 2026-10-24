//! A line whose show operators differ only in size is one run for the
//! matcher (request G228): an edit inside one piece is that piece's edit,
//! and an edit across the size change refuses by name. The composite
//! cases pin that a two-byte (`Identity-H`) run tracks its position across
//! show operators, which joining them needs.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::text_edit::{
    EditError, EditOptions, EditRequest, FormatOptions, FormatRequest, NotFoundReason,
};
use pdfcer_core::text_extract::{ExtractOptions, extract_page};
use pdfcer_core::writer::SaveOptions;

use crate::helvetica_page::build_pdf;

const LINE: &str = "Scaled label text";

/// The G228 fixture: one Helvetica line at `1 Tf` under an 18-pt `Tm`, with
/// "label" restyled to 24 pt by `format_text`, which splits the operator.
fn restyled() -> EditSession {
    let content = format!("BT /F1 1 Tf 18 0 0 18 72 700 Tm ({LINE}) Tj ET");
    let mut s = EditSession::new(Document::from_bytes(build_pdf(&content)).unwrap());
    s.format_text(
        &FormatRequest::new(0, "label").drawn_size(24.0),
        &FormatOptions::default(),
    )
    .unwrap();
    s
}

fn preview(s: &EditSession, find: &str, replace: &str) -> Result<usize, EditError> {
    s.edit_text_preview(
        &EditRequest::find_replace(0, find, replace),
        &EditOptions::default(),
    )
    .map(|p| p.glyphs.len())
}

/// `(text, drawn size)` of every glyph on page 0 of the saved session.
fn drawn(s: &EditSession) -> Vec<(char, f64)> {
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    let page = extract_page(&doc, &pages[0], 0, &ExtractOptions::default()).unwrap();
    let mut out = Vec::new();
    for r in &page.runs {
        for g in &r.glyphs {
            let at = g.text_start as usize..(g.text_start + g.text_len) as usize;
            for c in r.text[at].chars() {
                out.push((c, f64::from(g.size)));
            }
        }
    }
    out
}

fn size_of(glyphs: &[(char, f64)], c: char) -> f64 {
    glyphs.iter().find(|g| g.0 == c).unwrap().1
}

#[test]
fn the_restyled_line_previews_in_its_own_font() {
    let s = restyled();
    assert!(preview(&s, LINE, LINE).unwrap() >= 1);
}

#[test]
fn an_edit_inside_the_unrestyled_piece_keeps_the_restyled_word() {
    let mut s = restyled();
    s.edit_text(
        &EditRequest::find_replace(0, LINE, "Scaled label texts"),
        &EditOptions::default(),
    )
    .unwrap();
    let glyphs = drawn(&s);
    let text: String = glyphs.iter().map(|g| g.0).collect();
    assert_eq!(text.replace(' ', ""), "Scaledlabeltexts");
    assert!((size_of(&glyphs, 'b') - 24.0).abs() < 1e-3);
    assert!((size_of(&glyphs, 'S') - 18.0).abs() < 1e-3);
    assert!((size_of(&glyphs, 'x') - 18.0).abs() < 1e-3);
}

#[test]
fn an_edit_across_the_size_change_refuses_as_a_split_run() {
    let s = restyled();
    let err = preview(&s, LINE, "Scaled lab-text").unwrap_err();
    assert!(
        matches!(
            err,
            EditError::NoMatch {
                reason: NotFoundReason::SplitRun { operators: 2 },
                ..
            }
        ),
        "{err:?}"
    );
}

/// `composite-editable.pdf`'s two-byte "ABC" line with "B" restyled to 36 pt.
fn composite_restyled() -> EditSession {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/synthetic/text/composite-editable.pdf"
    );
    let bytes = std::fs::read(path).unwrap();
    let mut s = EditSession::new(Document::from_bytes(bytes).unwrap());
    s.format_text(
        &FormatRequest::new(0, "B").drawn_size(36.0),
        &FormatOptions::default(),
    )
    .unwrap();
    s
}

#[test]
fn a_restyled_composite_line_previews_and_edits_inside_one_piece() {
    let s = composite_restyled();
    assert!(preview(&s, "ABC", "ABC").unwrap() >= 1);
    assert!(preview(&s, "ABC", "ABA").unwrap() >= 1);
}

#[test]
fn an_edit_across_a_composite_size_change_refuses_as_a_split_run() {
    let s = composite_restyled();
    let err = preview(&s, "ABC", "CBA").unwrap_err();
    assert!(
        matches!(
            err,
            EditError::NoMatch {
                reason: NotFoundReason::SplitRun { operators: 3 },
                ..
            }
        ),
        "{err:?}"
    );
}
