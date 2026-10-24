//! Reflow and block-text edits of a block whose show operators share a
//! `BT … ET` with other labels — how CAD exporters write a drawing view's
//! notes, dimension values and balloon letters (`G226`).
//!
//! The block is lifted out: its shows are deleted in place and it is written
//! as a text object of its own after the shared one. Every other glyph must
//! keep its position, which exercises both halves of the obligation — the
//! positioning operators stay, and the text state a deleted `"` set
//! (`0.5 Tc` here) stays in force for the label after it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree;
use pdfcer_core::text_edit::{
    BlockEditError, BlockEditOptions, EditableTextModel, ReflowApplyError, ReflowRequest,
    UnsupportedCause,
};
use pdfcer_core::text_extract::{self, ExtractOptions};
use pdfcer_core::writer::SaveOptions;

use crate::block_layout::doc_from_pages;

/// A three-line note interleaved with a balloon `D` and a label `BB` in one
/// text object; the third note line is shown by `"` setting `Tc 0.5`, which
/// the `BB` after it is spaced by. `OTHER` is in a text object of its own.
const SHARED: &str = "BT /F3 10 Tf 12 TL\n\
    1 0 0 1 72 700 Tm (FRONT EDGE OF PLATE) Tj\n\
    1 0 0 1 400 694 Tm (D) Tj\n\
    1 0 0 1 72 688 Tm (SHOULD BE ABOUT) Tj\n\
    0 0.5 (HALF AN INCH) \"\n\
    1 0 0 1 400 600 Tm (BB) Tj\n\
    ET\n\
    BT /F3 10 Tf 400 500 Td (OTHER) Tj ET\n";

fn page_text(bytes: &[u8]) -> text_extract::PageText {
    let d = Document::from_bytes(bytes.to_vec()).expect("reload");
    let pages = page_tree::pages(&d).expect("pages");
    let opts = ExtractOptions::default().with_provenance(true);
    text_extract::extract_page(&d, &pages[0], 0, &opts).expect("extract")
}

/// The block whose text starts with `FRONT`.
fn note_block(bytes: &[u8]) -> usize {
    let p = page_text(bytes);
    let m = EditableTextModel::recognize(&p, &pdfcer_core::text_edit::reflow_recognition_options());
    (0..m.blocks().len())
        .find(|&b| {
            m.blocks()[b]
                .line_indices
                .first()
                .is_some_and(|&li| m.line_text(&m.lines()[li]).starts_with("FRONT"))
        })
        .expect("the note is a block")
}

/// Every glyph right of x = 340 (the labels, never the note) as
/// `(code, x, y)` in hundredths of a point.
fn labels(bytes: &[u8]) -> Vec<(u32, i64, i64)> {
    let mut out: Vec<(u32, i64, i64)> = page_text(bytes)
        .runs
        .iter()
        .flat_map(|r| r.glyphs.iter())
        .filter(|g| g.x >= 340.0)
        .map(|g| {
            (
                g.code,
                (f64::from(g.x) * 100.0).round() as i64,
                (f64::from(g.y) * 100.0).round() as i64,
            )
        })
        .collect();
    out.sort_unstable();
    out
}

fn source(content: &str) -> (Vec<u8>, EditSession) {
    let doc = doc_from_pages(&[content], 0);
    let bytes = doc.bytes().to_vec();
    (bytes, EditSession::new(doc))
}

fn saved(s: &EditSession) -> Vec<u8> {
    s.to_full_bytes(&SaveOptions::default()).expect("save").0
}

#[test]
fn a_note_sharing_a_text_object_reflows_and_every_label_stays_put() {
    let (src, mut s) = source(SHARED);
    let before = labels(&src);
    assert_eq!(before.len(), 8, "D, BB and OTHER: {before:?}");
    // `BB` is spaced by the `"`'s Tc 0.5: 6 pt advance + 0.5.
    assert!(
        before.contains(&(u32::from(b'B'), 40650, 60000)),
        "{before:?}"
    );
    let block = note_block(&src);
    let report = s
        .reflow_block(0, block, &ReflowRequest::new().with_wrap_width(250.0))
        .expect("a shared text object is lifted, not refused");
    assert!(
        report
            .disclosures
            .iter()
            .any(|d| d.contains("shared its text object")),
        "{:?}",
        report.disclosures
    );
    let out = saved(&s);
    assert_eq!(labels(&out), before, "no label moved");
    let text = page_text(&out).sourced_text();
    assert!(
        text.contains("FRONT EDGE OF PLATE SHOULD BE ABOUT"),
        "{text:?}"
    );
    assert!(text.contains("HALF AN INCH"), "{text:?}");
    // The other labels' operators are byte-verbatim.
    let content = String::from_utf8_lossy(&out).into_owned();
    assert!(content.contains("1 0 0 1 400 694 Tm (D) Tj"), "{content}");
    assert!(content.contains("1 0 0 1 400 600 Tm (BB) Tj"), "{content}");
}

#[test]
fn a_shared_note_opens_as_a_paragraph_and_commits() {
    let (src, mut s) = source(SHARED);
    let before = labels(&src);
    let block = note_block(&src);
    let text = "FRONT EDGE OF PLATE SHOULD BE\nABOUT HALF AN INCH";
    s.edit_block_text_preview(0, block, text, &BlockEditOptions::new())
        .expect("the preview no longer refuses a shared text object");
    let report = s
        .edit_block_text(0, block, text, &BlockEditOptions::new())
        .expect("edit");
    assert!(report.lines_after >= 2, "the break is kept: {report:?}");
    let out = saved(&s);
    assert_eq!(labels(&out), before, "no label moved");
    let joined: String = page_text(&out)
        .sourced_text()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    assert!(
        joined.contains("FRONTEDGEOFPLATESHOULDBEABOUTHALFANINCH"),
        "{joined:?}"
    );
}

#[test]
fn a_label_continuing_from_the_note_refuses_by_name() {
    // `X` is shown straight after the note's last line with only a rise
    // between: it starts where that line ended, so deleting the line would
    // move it.
    let content = "BT /F3 10 Tf\n\
        1 0 0 1 72 700 Tm (FRONT EDGE OF PLATE) Tj\n\
        1 0 0 1 72 688 Tm (SHOULD BE ABOUT) Tj\n\
        -80 Ts (X) Tj 0 Ts\n\
        ET\n";
    let (src, mut s) = source(content);
    let block = note_block(&src);
    let err = s
        .reflow_block(0, block, &ReflowRequest::new().with_wrap_width(250.0))
        .expect_err("would move X");
    assert!(
        matches!(
            err,
            ReflowApplyError::Unsupported(UnsupportedCause::TextObjectNotSeparable { .. })
        ),
        "{err:?}"
    );
}

#[test]
fn a_note_tagged_inside_the_shared_object_refuses_by_name() {
    let content = "BT /F3 10 Tf\n\
        /P << /MCID 0 >> BDC\n\
        1 0 0 1 72 700 Tm (FRONT EDGE OF PLATE) Tj\n\
        1 0 0 1 72 688 Tm (SHOULD BE ABOUT) Tj\n\
        EMC\n\
        1 0 0 1 400 694 Tm (D) Tj\n\
        ET\n";
    let (src, s) = source(content);
    let block = note_block(&src);
    let err = s
        .edit_block_text_preview(0, block, "FRONT", &BlockEditOptions::new())
        .expect_err("moving the text out of its tag would untag it");
    let BlockEditError::Block(ReflowApplyError::Unsupported(
        UnsupportedCause::TextObjectNotSeparable { detail },
    )) = &err
    else {
        panic!("{err:?}");
    };
    assert!(detail.contains("marked content"), "{detail}");
}

#[test]
fn a_split_note_under_two_translations_keeps_its_place() {
    // The note's first line is drawn under a translated CTM, its second in a
    // later object under the identity, with a label's object between. The
    // body goes after the last object, so it must be placed under that
    // object's CTM, not the first line's.
    let content = "q 1 0 0 1 10 20 cm\n\
        BT /F3 10 Tf 1 0 0 1 62 680 Tm (FRONT EDGE OF PLATE) Tj ET Q\n\
        BT /F3 10 Tf 1 0 0 1 400 694 Tm (D) Tj ET\n\
        BT /F3 10 Tf 1 0 0 1 72 688 Tm (SHOULD BE ABOUT) Tj ET\n";
    let (src, mut s) = source(content);
    let first_f = |bytes: &[u8]| {
        page_text(bytes)
            .runs
            .iter()
            .flat_map(|r| r.glyphs.iter())
            .find(|g| g.code == u32::from(b'F'))
            .map(|g| ((g.x * 100.0).round() as i64, (g.y * 100.0).round() as i64))
            .expect("F")
    };
    let before = first_f(&src);
    assert_eq!(before, (7200, 70000));
    let block = note_block(&src);
    s.reflow_block(0, block, &ReflowRequest::new().with_wrap_width(250.0))
        .expect("lifted");
    let out = saved(&s);
    assert_eq!(first_f(&out), before, "the note stays where it was");
    assert_eq!(labels(&out), labels(&src), "no label moved");
}
