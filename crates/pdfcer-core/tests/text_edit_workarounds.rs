//! Decision 175: the opt-in text-edit workarounds. Off by default, every
//! refusal names the workaround on offer; on, the exact fix is used where one
//! exists, else the run is retyped, and the report says which.
//!
//! Fixtures: `workaround-*.pdf` (`tools/gen-workaround-fixtures.py`) and
//! `cross-object-word.pdf` (`tools/gen-cross-object-fixtures.py`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::content::ContentStream;
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree;
use pdfcer_core::span::ByteSpan;
use pdfcer_core::text_edit::{
    EditError, EditOptions, EditOutcome, EditRequest, NotFoundReason, UnsupportedCause, Workaround,
    WorkaroundPolicy, edit_text,
};
use pdfcer_core::text_extract::{ExtractOptions, extract_page};

fn bytes(name: &str) -> Vec<u8> {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/text")
        .join(name);
    std::fs::read(path).expect("fixture")
}

fn doc(name: &str) -> Document {
    Document::from_bytes(bytes(name)).expect("fixture parses")
}

fn apply() -> EditOptions {
    EditOptions::default().with_workarounds(WorkaroundPolicy::Apply)
}

fn applied(name: &str, req: &EditRequest) -> EditOutcome {
    edit_text(&doc(name), req, &apply()).expect("the workaround applies")
}

fn refused(name: &str, req: &EditRequest) -> EditError {
    edit_text(&doc(name), req, &EditOptions::default()).expect_err("the exact edit refuses")
}

fn used(out: &EditOutcome) -> Workaround {
    out.report
        .workaround
        .as_ref()
        .expect("a workaround")
        .workaround
}

/// Every glyph on the baseline `y`, in content order: (character, x).
fn line(doc_bytes: &[u8], y: f32) -> Vec<(String, f32)> {
    let d = Document::from_bytes(doc_bytes.to_vec()).expect("re-parses");
    let pages = page_tree::pages(&d).expect("pages");
    let text = extract_page(&d, &pages[0], 0, &ExtractOptions::default()).expect("text");
    let mut out = Vec::new();
    for run in &text.runs {
        for g in &run.glyphs {
            if (g.y - y).abs() < 0.5 {
                let s = g.text_start as usize;
                let ch = run.text.get(s..s + g.text_len as usize).unwrap_or("");
                out.push((ch.to_owned(), g.x));
            }
        }
    }
    out
}

fn line_text(doc_bytes: &[u8], y: f32) -> String {
    line(doc_bytes, y).into_iter().map(|(c, _)| c).collect()
}

/// The newest page content stream's bytes.
fn content(doc_bytes: &[u8]) -> Vec<u8> {
    let d = Document::from_bytes(doc_bytes.to_vec()).expect("re-parses");
    let pages = page_tree::pages(&d).expect("pages");
    ContentStream::from_page(&d.view(), &pages[0])
        .expect("parses")
        .buf
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn every_refusal_names_the_workaround_on_offer() {
    let quote = refused(
        "workaround-quote.pdf",
        &EditRequest::find_replace(0, "Quoted", "Quote2"),
    );
    assert!(matches!(
        quote,
        EditError::Unsupported(UnsupportedCause::QuoteOperator)
    ));
    assert_eq!(quote.workaround(), Some(Workaround::RewriteQuoteOperator));
    assert!(
        quote.to_string().contains("a workaround is on offer"),
        "{quote}"
    );

    let seam = refused(
        "workaround-seam.pdf",
        &EditRequest::find_replace(0, "Hello", "Howdy"),
    );
    assert!(
        matches!(
            &seam,
            EditError::NoMatch {
                reason: NotFoundReason::SplitRun { operators: 2 },
                ..
            }
        ),
        "{seam:?}"
    );
    assert_eq!(seam.workaround(), Some(Workaround::Retype));

    let objects = refused(
        "cross-object-word.pdf",
        &EditRequest::find_replace(0, "Left-Hand", "Right-Hand"),
    );
    assert_eq!(objects.workaround(), Some(Workaround::JoinTextObjects));
}

#[test]
fn a_quote_operator_is_rewritten_exactly() {
    let out = applied(
        "workaround-quote.pdf",
        &EditRequest::find_replace(0, "Quoted", "Quote2"),
    );
    assert_eq!(used(&out), Workaround::RewriteQuoteOperator);
    assert!(
        out.report.disclosures[0].starts_with("workaround (rewrite-quote-operator, exact)"),
        "{:?}",
        out.report.disclosures
    );
    let glyphs = line(&out.bytes, 686.0);
    assert!(line_text(&out.bytes, 686.0).starts_with("Quote2 line"));
    assert!((glyphs[0].1 - 72.0).abs() < 0.01, "T* still moves the line");

    // `"`: its word and character spacing survive the rewrite (§9.4.3).
    let out = applied(
        "workaround-quote.pdf",
        &EditRequest::find_replace(0, "Double", "Triple"),
    );
    let glyphs = line(&out.bytes, 672.0);
    assert_eq!(line_text(&out.bytes, 672.0), "Triple line");
    let t_then_r = glyphs[1].1 - glyphs[0].1;
    assert!(
        (t_then_r - (611.0 * 12.0 / 1000.0 + 1.0)).abs() < 0.01,
        "Tc 1 kept: {t_then_r}"
    );
}

#[test]
fn a_match_across_text_objects_is_joined_and_the_tail_follows() {
    let out = applied(
        "cross-object-word.pdf",
        &EditRequest::find_replace(0, "Left-Hand", "Right-Hand"),
    );
    assert_eq!(used(&out), Workaround::JoinTextObjects);
    assert_eq!(line_text(&out.bytes, 620.0), "Right-Hand Door");
    let xs: Vec<f32> = line(&out.bytes, 620.0).iter().map(|g| g.1).collect();
    assert!(xs.windows(2).all(|w| w[1] > w[0]), "no overlap: {xs:?}");
}

#[test]
fn a_split_run_is_retyped_and_the_text_after_it_held() {
    let before = line(&bytes("workaround-seam.pdf"), 700.0);
    let world_x = before.iter().find(|g| g.0 == " ").expect("space").1;
    let out = applied(
        "workaround-seam.pdf",
        &EditRequest::find_replace(0, "Hello", "Howdy"),
    );
    assert_eq!(used(&out), Workaround::Retype);
    assert_eq!(line_text(&out.bytes, 700.0), "Howdy world");
    let after = line(&out.bytes, 700.0);
    let w = after.iter().find(|g| g.0 == " ").expect("space").1;
    assert!((w - world_x).abs() < 0.01, "held: {w} vs {world_x}");
    let c = content(&out.bytes);
    assert!(
        !contains(&c, b"(Hel)") && !contains(&c, b"(lo)"),
        "removed: {}",
        String::from_utf8_lossy(&c)
    );
    assert!(
        out.report
            .disclosures
            .iter()
            .any(|d| d.contains("compensating TJ")),
        "{:?}",
        out.report.disclosures
    );
}

#[test]
fn a_font_seam_across_objects_falls_from_join_to_retype() {
    let out = applied(
        "cross-object-word.pdf",
        &EditRequest::find_replace(0, "FrontPanel", "BackPanel"),
    );
    assert_eq!(used(&out), Workaround::Retype);
    assert_eq!(line_text(&out.bytes, 580.0), "BackPanel");
    assert!((line(&out.bytes, 580.0)[0].1 - 72.0).abs() < 0.01);
}

#[test]
fn a_vertical_run_is_retyped_horizontally_in_the_fallback_face() {
    let req = EditRequest::find_replace(0, "AB", "XY");
    assert!(matches!(
        refused("workaround-composite.pdf", &req),
        EditError::Unsupported(UnsupportedCause::VerticalWriting)
    ));
    let out = applied("workaround-composite.pdf", &req);
    assert_eq!(used(&out), Workaround::Retype);
    assert!(
        out.report.fallback.is_some(),
        "the fallback face is reported"
    );
    let glyphs = line(&out.bytes, 700.0);
    assert_eq!(line_text(&out.bytes, 700.0), "XY");
    assert!((glyphs[0].1 - 100.0).abs() < 0.01 && glyphs[1].1 > glyphs[0].1);

    // The run's own map carries "BA", but a vertical font is never retyped
    // in itself: its horizontal advances would be wrong.
    let out = applied(
        "workaround-composite.pdf",
        &EditRequest::find_replace(0, "AB", "BA"),
    );
    assert!(
        out.report.fallback.is_some(),
        "{:?}",
        out.report.disclosures
    );
    assert_eq!(line_text(&out.bytes, 700.0), "BA");
}

#[test]
fn a_composite_run_without_tounicode_is_retyped_whole() {
    let req = EditRequest::whole_operator(0, ByteSpan { start: 64, len: 13 }, "Hi");
    assert_eq!(
        refused("workaround-composite.pdf", &req).workaround(),
        Some(Workaround::Retype)
    );
    let out = applied("workaround-composite.pdf", &req);
    assert_eq!(line_text(&out.bytes, 600.0), "Hi");
    assert!((line(&out.bytes, 600.0)[0].1 - 72.0).abs() < 0.01);
}

#[test]
fn a_workaround_that_cannot_apply_names_both_refusals() {
    // The fallback face (Helvetica) has no U+2265 either.
    let err = edit_text(
        &doc("workaround-seam.pdf"),
        &EditRequest::find_replace(0, "Hello", "H\u{2265}llo"),
        &apply(),
    )
    .expect_err("nothing can set U+2265");
    let EditError::WorkaroundRefused {
        refused,
        workaround,
        ..
    } = &err
    else {
        panic!("expected WorkaroundRefused, got {err:?}");
    };
    assert_eq!(*workaround, Workaround::Retype);
    assert!(matches!(**refused, EditError::NoMatch { .. }));
    assert!(err.to_string().contains("could not be applied"), "{err}");
}

#[test]
fn preview_matches_commit_and_is_one_undo_entry() {
    for (name, find, replace) in [
        ("workaround-seam.pdf", "Hello", "Howdy"),
        ("workaround-quote.pdf", "Quoted", "Quote2"),
    ] {
        let req = EditRequest::find_replace(0, find, replace);
        let mut s = EditSession::new(doc(name));
        let preview = s.edit_text_preview(&req, &apply()).expect("previewable");
        assert!(!s.can_undo(), "a preview records no command");
        let report = s.edit_text(&req, &apply()).expect("commits");
        assert_eq!(preview.disclosures, report.disclosures);
        let text: String = preview.glyphs.iter().filter_map(|g| g.ch).collect();
        assert_eq!(text, replace);
        let again = s
            .edit_text_preview(
                &EditRequest::find_replace(0, replace, replace),
                &EditOptions::default(),
            )
            .expect("the committed run is editable");
        assert_eq!(preview.glyphs.len(), again.glyphs.len(), "{name}");
        for (a, b) in preview.glyphs.iter().zip(&again.glyphs) {
            assert_eq!(a.code, b.code);
            assert!((a.matrix[4] - b.matrix[4]).abs() < 1e-3, "{name}");
            assert!((a.matrix[5] - b.matrix[5]).abs() < 1e-3, "{name}");
        }
        assert_eq!(s.undo_depth(), 1);
        s.undo();
        assert!(!s.can_undo());
    }
}
