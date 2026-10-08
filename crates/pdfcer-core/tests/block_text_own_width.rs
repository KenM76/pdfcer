//! A paragraph rewritten with its own text keeps its own lines at the
//! default wrap width (pdfcer-gui request G162).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::text_edit::{AddTextRequest, BlockEditOptions};

fn session_with(text: &str) -> EditSession {
    let doc = Document::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/addtext/plain.pdf"),
    )
    .expect("load plain.pdf");
    let mut s = EditSession::new(doc);
    s.add_text(
        &AddTextRequest::new(0, (100.0, 400.0), text)
            .with_size(12.0)
            .with_box(100.0, 360.0, 300.0, 60.0),
    )
    .expect("add_text");
    s
}

/// The block index of the added paragraph: the one whose text starts with
/// `first`.
fn block_of(s: &EditSession, first: &str) -> usize {
    (0..8)
        .find(|&b| {
            s.edit_block_text_preview(0, b, first, &BlockEditOptions::default())
                .is_ok_and(|p| p.report.lines_before == 2)
        })
        .expect("a two-line block")
}

#[test]
fn a_rewrite_with_one_more_character_keeps_two_lines() {
    let mut s = session_with("Notes\nsecond line here");
    let b = block_of(&s, "Notes");
    let r = s
        .edit_block_text(
            0,
            b,
            "Notesx\nsecond line here",
            &BlockEditOptions::default(),
        )
        .expect("edit");
    assert_eq!((r.lines_before, r.lines_after), (2, 2), "{r:?}");
}

#[test]
fn a_rewrite_with_the_same_text_keeps_its_lines() {
    let mut s = session_with("Notes\nsecond line here");
    let b = block_of(&s, "Notes");
    let r = s
        .edit_block_text(
            0,
            b,
            "Notes\nsecond line here",
            &BlockEditOptions::default(),
        )
        .expect("edit");
    assert_eq!((r.lines_before, r.lines_after), (2, 2), "{r:?}");
}
