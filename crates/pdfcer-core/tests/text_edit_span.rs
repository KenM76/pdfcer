//! `Pass 256.0` — a text edit that spans consecutive show operators.
//!
//! The operator's own document (2026-09-05) was written ONE glyph per `Tj`
//! with a `Td` between; `edit-text --find "clien"` answered `NoMatch`
//! because no single operator held five letters. These fixtures reproduce
//! that shape with the three-glyph composite donor (`A`/`B`/`C`, 600/1000
//! wide at 48 pt = 28.8 pt each):
//!
//! - `composite-per-glyph.pdf`: `A` `B` `C` as three operators, a fourth
//!   operator `C` further along the same line (the FOLLOWER), and a second
//!   line `B` (so a growing replacement stays inside the embedded-subset
//!   floor without depending on the edited operators).
//! - `composite-tj-split.pdf`: `[<A> -20 <BC>] TJ` — a split between `TJ`
//!   ELEMENTS inside one operator.
//! - `composite-font-change.pdf`: `A` in `/F0`, `B` `C` in `/F1` — a `Tf`
//!   change mid-word that must NOT be spanned.
//!
//! The assertions are about the SAVED bytes re-read by pdfcer's own text
//! extractor and, for the positions, by the `Td` operands in the rewritten
//! content stream — because the whole point is where the glyphs after the
//! edit end up.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::Object;
use pdfcer_core::text_edit::{EditError, EditOptions, EditRequest};
use pdfcer_core::writer::SaveOptions;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/text")
        .join(name)
}

fn session(name: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(name)).unwrap())
}

/// Save incrementally, re-parse, and return (page-1 text, page-1 content stream).
fn saved(s: &EditSession) -> (String, String) {
    let (bytes, _) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    let page = &pages[0];
    let text = pdfcer_core::text_extract::extract_page(
        &doc,
        page,
        0,
        &pdfcer_core::text_extract::ExtractOptions::default(),
    )
    .unwrap();
    let joined: String = text
        .runs
        .iter()
        .map(|r| r.text.as_str())
        .collect::<Vec<_>>()
        .join("|");
    // The page's /Contents stream, raw (the fixture is unfiltered and the
    // incremental writer re-emits it unfiltered).
    let page_dict = doc
        .view()
        .resolve(&Object::Reference(page.id))
        .as_dict()
        .cloned()
        .unwrap();
    let contents = page_dict.get(b"Contents").cloned().unwrap();
    let stream = match doc.view().resolve(&contents) {
        Object::Stream(st) => st.data_span.slice(doc.bytes()).unwrap().to_vec(),
        other => panic!("contents not a stream: {other:?}"),
    };
    (joined, String::from_utf8_lossy(&stream).into_owned())
}

#[test]
fn a_find_across_three_operators_edits_as_one_run() {
    let mut s = session("composite-per-glyph.pdf");
    let req = EditRequest::find_replace(0, "ABC", "ACB");
    let r = s
        .edit_text(&req, &EditOptions::default())
        .expect("the span edits");
    assert_eq!(r.operators_spanned, 3, "A, B and C were three operators");
    assert_eq!(r.advance_delta, 0.0, "same glyph count, same advance");
    assert_eq!(
        r.followers_repositioned, 0,
        "nothing moves when the advance is unchanged"
    );
    assert!(
        r.disclosures
            .iter()
            .any(|d| d.contains("3 consecutive show operators")),
        "{:?}",
        r.disclosures
    );
    let (text, content) = saved(&s);
    assert!(text.contains("ACB"), "{text}");
    assert!(text.contains('B'), "line 2 untouched: {text}");
    // The two leading operators are emptied and kept; the last carries the run.
    assert_eq!(content.matches("() Tj").count(), 2, "{content}");
    // The last operator now carries all three codes (as a literal string).
    assert_eq!(content.matches(" Tj").count(), 5, "{content}");
    // Their Td steps are untouched when the advance did not change.
    assert_eq!(content.matches("28.8 0 Td").count(), 2, "{content}");
}

#[test]
fn a_growing_replacement_respaces_the_followers_and_keeps_the_next_line_put() {
    let mut s = session("composite-per-glyph.pdf");
    let req = EditRequest::find_replace(0, "ABC", "ABCB");
    let r = s
        .edit_text(&req, &EditOptions::default())
        .expect("the span edits");
    assert_eq!(r.operators_spanned, 3);
    assert!(
        (r.advance_delta - 28.8).abs() < 1e-3,
        "one more 600/1000 glyph at 48 pt: {}",
        r.advance_delta
    );
    // Two zeroed steps (the emptied operators' own advances) and the
    // follower's step, all rewritten.
    assert_eq!(r.followers_repositioned, 3, "{:?}", r.disclosures);
    let (text, content) = saved(&s);
    assert!(text.contains("ABCB"), "{text}");
    // The emptied operators' Td steps collapse to zero: A and B contribute no
    // advance now, so the run "ABCB" starts where "A" started.
    assert_eq!(content.matches("0 0 Td").count(), 2, "{content}");
    // The follower C: it was 57.6 past the old end of "ABC"; the run grew by
    // 28.8 and the two zeroed steps took 57.6 away, so its own step grows by
    // 86.4 to keep the same 28.8 pt gap.
    assert!(content.contains("144 0 Td"), "{content}");
    // The next line's Td undoes the chain's shift so line 2 lands where the
    // producer put it: −115.2 became −144 because the chain moved +28.8.
    assert!(content.contains("-144 -60 Td"), "{content}");
}

#[test]
fn a_split_across_tj_elements_inside_one_operator_spans_one() {
    let mut s = session("composite-tj-split.pdf");
    let req = EditRequest::find_replace(0, "AB", "BA");
    let r = s
        .edit_text(&req, &EditOptions::default())
        .expect("cross-element edit");
    assert_eq!(r.operators_spanned, 1, "one operator, two elements");
    let (text, content) = saved(&s);
    assert!(text.contains("BAC"), "{text}");
    // The kern between the consumed elements is gone with them.
    assert!(!content.contains("-20"), "{content}");
    // The −20 kern used to ADD 0.96 pt of advance (−(−20)/1000·48); it is
    // part of the old advance the replacement replaces.
    assert!((r.advance_delta + 0.96).abs() < 1e-3, "{}", r.advance_delta);
}

#[test]
fn a_font_resource_change_mid_word_is_not_spanned() {
    let mut s = session("composite-font-change.pdf");
    let req = EditRequest::find_replace(0, "ABC", "ACB");
    let err = s
        .edit_text(&req, &EditOptions::default())
        .expect_err("Tf change breaks the span");
    assert!(matches!(err, EditError::NoMatch(_)), "{err:?}");
    // But the part inside one resource still edits.
    let req = EditRequest::find_replace(0, "BC", "CB");
    let r = s
        .edit_text(&req, &EditOptions::default())
        .expect("B and C share /F1");
    assert_eq!(r.operators_spanned, 2);
}

#[test]
fn the_pin_never_spans_and_single_operator_edits_report_one() {
    let mut s = session("composite-per-glyph.pdf");
    // Pin the middle operator (B) — its span is the `<0002> Tj` token.
    let objs = s.page_objects(0).unwrap();
    let text_obj = objs
        .objects
        .iter()
        .find_map(|o| match o {
            pdfcer_core::vector::VectorObject::Text(t) => Some(t),
            _ => None,
        })
        .unwrap();
    let b_run = text_obj.runs[1].bytes;
    let mut req = EditRequest::find_replace(0, "", "C");
    req.pinned_span = Some(b_run);
    let r = s
        .edit_text(&req, &EditOptions::default())
        .expect("pinned edit");
    assert_eq!(r.operators_spanned, 1);
    let (text, _) = saved(&s);
    assert!(text.contains("ACC"), "{text}");
}
