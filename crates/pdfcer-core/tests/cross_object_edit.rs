//! A pinned spanning edit crosses text objects on one line (G074).
//!
//! Fixture: `cross-object-word.pdf` (`tools/gen-cross-object-fixtures.py`),
//! Word's shape — every fragment its own `BDC q BT … ET Q EMC` with an
//! absolute `Tm`. "Driver-Side" is written as three objects on two lines, so
//! a pin must choose; a third line holds it in ONE object as the unpinned
//! control.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::content::ContentStream;
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree;
use pdfcer_core::span::ByteSpan;
use pdfcer_core::text_edit::{
    EditError, EditOptions, EditRequest, FollowerDisposition, NotFoundReason, edit_text,
};
use pdfcer_core::text_extract::{ExtractOptions, extract_page};

/// Pins as the generator prints them: the first show operator of a line.
const LINE_700: (usize, usize) = (55, 11); // `(Driver) Tj`
const LINE_660: (usize, usize) = (358, 11); // `(Driver) Tj`
const LINE_620: (usize, usize) = (661, 9); // `(Left) Tj`
const LINE_580: (usize, usize) = (820, 10); // `(Front) Tj`, next object is /F2

/// Helvetica advances (per 1000 em) for the characters the tests lay out.
fn helvetica(c: char) -> f32 {
    match c {
        ' ' => 278.0,
        '-' => 333.0,
        'R' | 'H' | 'D' => 722.0,
        'i' | 'l' => 222.0,
        'g' | 'h' | 'a' | 'n' | 'd' | 'e' | 'L' | 'o' => 556.0,
        't' | 'f' => 278.0,
        'r' => 333.0,
        other => panic!("no width for {other:?}"),
    }
}

fn advance(s: &str) -> f32 {
    s.chars().map(helvetica).sum::<f32>() * 12.0 / 1000.0
}

fn bytes() -> Vec<u8> {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/text/cross-object-word.pdf");
    std::fs::read(path).expect("fixture")
}

fn doc() -> Document {
    Document::from_bytes(bytes()).expect("fixture parses")
}

fn pin(v: (usize, usize)) -> ByteSpan {
    ByteSpan {
        start: v.0,
        len: v.1,
    }
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

/// Counts of the structure operators in the newest page content.
fn structure(doc_bytes: &[u8]) -> [usize; 6] {
    let d = Document::from_bytes(doc_bytes.to_vec()).expect("re-parses");
    let pages = page_tree::pages(&d).expect("pages");
    let view = d.view();
    let cs = ContentStream::from_page(&view, &pages[0]).expect("parses");
    let mut n = [0; 6];
    for op in cs.operations() {
        let i = match op.operator_name(&cs.buf) {
            Some(b"BT") => 0,
            Some(b"ET") => 1,
            Some(b"q") => 2,
            Some(b"Q") => 3,
            Some(b"BDC") => 4,
            Some(b"EMC") => 5,
            _ => continue,
        };
        n[i] += 1;
    }
    n
}

fn edit(req: &EditRequest, opts: &EditOptions) -> pdfcer_core::text_edit::EditOutcome {
    edit_text(&doc(), req, opts).expect("the crossing edit succeeds")
}

#[test]
fn the_replacement_lands_in_the_first_object_and_the_others_are_emptied() {
    let req = EditRequest::spanning_from(0, pin(LINE_700), "Driver-Side", "Left-Hand");
    let out = edit(&req, &EditOptions::default());

    let glyphs = line(&out.bytes, 700.0);
    let text: String = glyphs.iter().map(|(c, _)| c.as_str()).collect();
    assert_eq!(text, "Left-Hand ", "the line reads the replacement once");
    assert!(
        (glyphs[0].1 - 72.0).abs() < 0.01,
        "starts at the first origin"
    );
    assert!(
        (glyphs[1].1 - (72.0 + advance("L"))).abs() < 0.01,
        "the replacement is laid out as one run in the first object: {glyphs:?}"
    );
    assert_eq!(
        line_text(&out.bytes, 660.0),
        "Driver-Side ",
        "the pin chose line 700; line 660 is untouched"
    );
    assert_eq!(line_text(&out.bytes, 540.0), "Driver-Side");
    assert_eq!(out.report.operators_spanned, 3);
    assert!(
        out.report
            .disclosures
            .iter()
            .any(|d| d.contains("across 3 text objects") && d.contains("2 left showing nothing")),
        "{:?}",
        out.report.disclosures
    );
}

#[test]
fn every_bt_et_q_and_marked_content_pair_survives() {
    let before = structure(&bytes());
    let req = EditRequest::spanning_from(0, pin(LINE_700), "Driver-Side", "Left-Hand");
    let out = edit(&req, &EditOptions::default());
    assert_eq!(structure(&out.bytes), before);
}

#[test]
fn the_second_occurrence_is_reachable_by_its_own_pin() {
    let req = EditRequest::spanning_from(0, pin(LINE_660), "Driver-Side", "Left-Hand");
    let out = edit(&req, &EditOptions::default());
    assert_eq!(line_text(&out.bytes, 660.0), "Left-Hand ");
    assert_eq!(line_text(&out.bytes, 700.0), "Driver-Side ");
}

#[test]
fn the_trailing_space_object_is_matchable() {
    let req = EditRequest::spanning_from(0, pin(LINE_700), "Driver-Side ", "Left-Hand");
    let out = edit(&req, &EditOptions::default());
    assert_eq!(line_text(&out.bytes, 700.0), "Left-Hand");
    assert_eq!(out.report.operators_spanned, 4);
}

#[test]
fn an_unpinned_request_does_not_cross_and_says_why() {
    let req = EditRequest::find_replace(0, "Left-Hand", "Righthand");
    match edit_text(&doc(), &req, &EditOptions::default()) {
        Err(EditError::NoMatch {
            reason: NotFoundReason::SpansTextObjects { objects: 2 },
            ..
        }) => {}
        other => panic!("expected NoMatch/SpansTextObjects, got {other:?}"),
    }
}

#[test]
fn a_font_seam_is_not_crossed_and_is_named() {
    let req = EditRequest::spanning_from(0, pin(LINE_580), "FrontPanel", "BackPanel");
    match edit_text(&doc(), &req, &EditOptions::default()) {
        Err(EditError::NoMatch {
            reason: NotFoundReason::SpansTextObjects { .. },
            ..
        }) => {}
        other => panic!("expected NoMatch/SpansTextObjects at the seam, got {other:?}"),
    }
}

/// The match ends mid-object ("-Hand Door"): the tail " Door" follows the
/// replacement under reflow, and stays put under pin.
#[test]
fn the_last_objects_tail_follows_under_reflow_and_stays_under_pin() {
    let req = EditRequest::spanning_from(0, pin(LINE_620), "Left-Hand", "Righthand");

    let out = edit(&req, &EditOptions::default());
    let glyphs = line(&out.bytes, 620.0);
    let text: String = glyphs.iter().map(|(c, _)| c.as_str()).collect();
    assert_eq!(text, "Righthand Door");
    let d = glyphs.iter().find(|(c, _)| c == "D").expect("D").1;
    assert!(
        (d - (72.0 + advance("Righthand "))).abs() < 0.01,
        "reflow: the tail follows the replacement; D at {d}"
    );

    let pinned = edit(
        &req,
        &EditOptions::default().with_disposition(FollowerDisposition::Pin),
    );
    let glyphs = line(&pinned.bytes, 620.0);
    let d = glyphs.iter().find(|(c, _)| c == "D").expect("D").1;
    assert!(
        (d - (72.0 + advance("Left-Hand "))).abs() < 0.01,
        "pin: the tail keeps its position; D at {d}"
    );
}

#[test]
fn preview_matches_commit_and_the_edit_is_one_undo_entry() {
    let req = EditRequest::spanning_from(0, pin(LINE_700), "Driver-Side", "Left-Hand");
    let mut s = EditSession::new(doc());
    let preview = s
        .edit_text_preview(&req, &EditOptions::default())
        .expect("the preview succeeds");
    let text: String = preview.glyphs.iter().filter_map(|g| g.ch).collect();
    assert_eq!(text, "Left-Hand");
    let report = s
        .edit_text(&req, &EditOptions::default())
        .expect("the commit succeeds");
    assert_eq!(preview.disclosures, report.disclosures);
    let xs: Vec<f64> = preview.glyphs.iter().map(|g| g.matrix[4]).collect();
    assert!((xs[0] - 72.0).abs() < 0.01, "{xs:?}");
    assert!(
        (xs[1] - (72.0 + f64::from(advance("L")))).abs() < 0.01,
        "{xs:?}"
    );
    assert_eq!(s.undo_depth(), 1);
    s.undo();
    assert!(!s.can_undo());
}

#[test]
fn the_run_repertoire_of_a_pinned_crossing_is_the_first_runs() {
    let s = EditSession::new(doc());
    let rep = s
        .run_repertoire(0, "Driver-Side", Some(pin(LINE_700)))
        .expect("an answer");
    assert_eq!(rep.resource, "F1");
    assert!(rep.accepts('R') && rep.accepts('-'), "{rep:?}");
}
