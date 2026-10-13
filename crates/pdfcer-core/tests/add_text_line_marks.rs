//! `EditSession::add_text` writes the same line-end marks as
//! `edit_block_text`, so a paragraph pdfcer typed re-opens
//! `LineEndSource::Marked` with its typed breaks and blank lines kept
//! (Pass 574.0).
//!
//! Fixture: `fixtures/synthetic/addtext/plain.pdf` (provenance beside it).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::fontdata::Std14;
use pdfcer_core::text_edit::{AddTextRequest, BlockHit, LineEnd, LineEndSource};

fn session() -> EditSession {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/addtext/plain.pdf");
    EditSession::new(Document::load(&path).expect("fixture loads"))
}

/// Add `text` boxed at the bottom of the page, `width` wide, and re-open it.
fn typed(text: &str, width: f64) -> BlockHit {
    let mut s = session();
    s.add_text(
        &AddTextRequest::new(0, (0.0, 0.0), text)
            .with_font(Std14::Helvetica)
            .with_size(12.0)
            .with_box(72.0, 60.0, width, 120.0),
    )
    .expect("adds");
    s.block_at_point(0, 74.0, 170.0)
        .expect("ok")
        .expect("a block")
}

#[test]
fn a_break_after_a_nearly_full_line_reads_back_as_a_break() {
    let text = "A much longer first line\nx";
    let hit = typed(text, 133.0);
    assert_eq!(hit.line_end_source, LineEndSource::Marked);
    assert_eq!(hit.line_ends, [LineEnd::Break, LineEnd::Last]);
    assert_eq!(hit.line_gaps.as_deref(), Some(&[1][..]));
    assert_eq!(hit.text, text);
}

#[test]
fn wraps_breaks_and_blank_lines_all_read_back() {
    let text = "Dear reader,\n\nAlpha bravo charlie delta echo foxtrot golf hotel india.";
    let hit = typed(text, 150.0);
    assert_eq!(hit.line_end_source, LineEndSource::Marked);
    let gaps = hit.line_gaps.as_deref().expect("marked");
    assert_eq!(gaps.first(), Some(&2), "a break and one blank line");
    assert!(gaps.len() >= 2, "the body wrapped: {gaps:?}");
    assert!(
        gaps.iter().skip(1).all(|&g| g == 0),
        "the rest wrap: {gaps:?}"
    );
}

#[test]
fn point_text_opens_marked() {
    let mut s = session();
    s.add_text(&AddTextRequest::new(0, (300.0, 40.0), "one line").with_size(12.0))
        .expect("adds");
    let hit = s
        .block_at_point(0, 302.0, 43.0)
        .expect("ok")
        .expect("a block");
    assert_eq!(hit.line_end_source, LineEndSource::Marked);
    assert_eq!(hit.line_ends, [LineEnd::Last]);
}
