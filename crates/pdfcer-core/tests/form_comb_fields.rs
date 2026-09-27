//! A comb text field (§12.7.4.3 Table 228 bit 25) is drawn one character per
//! `/MaxLen` cell, on every route that builds its appearance.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, FieldEdit, NewTextField};
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::Object;
use pdfcer_core::page_tree::Rect;

/// 200 pt wide: five cells of 40 pt.
const BOX: Rect = Rect {
    llx: 40.0,
    lly: 700.0,
    urx: 240.0,
    ury: 720.0,
};

fn session_with(spec: &NewTextField) -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/minimal.pdf");
    let mut s = EditSession::new(Document::load(&path).expect("load minimal.pdf"));
    s.add_text_field(spec).expect("author the field");
    s
}

fn ap_bytes(s: &EditSession) -> String {
    let g = s.graph();
    let f = forms::parse_acroform(&g)
        .expect("an AcroForm")
        .fields
        .into_iter()
        .find(|f| f.fully_qualified_name == "c")
        .expect("the field");
    let w = &f.widgets[0];
    let Some(Object::Dict(ap)) = g
        .resolved(w.id)
        .as_dict()
        .and_then(|d| d.get(b"AP"))
        .map(|o| g.resolve(o).clone())
    else {
        panic!("no /AP");
    };
    let Some(Object::Stream(st)) = ap.get(b"N").map(|o| g.resolve(o).clone()) else {
        panic!("no /AP /N stream");
    };
    String::from_utf8_lossy(s.view().slice(st.data_span).unwrap_or_default()).into_owned()
}

/// Each of A, B, C is shown on its own, and — Helvetica's A/B/C all being
/// 667/1000 or 722/1000 wide — each step moves one cell less half the
/// width difference.
fn assert_per_cell(ap: &str) {
    for ch in ["(A) Tj", "(B) Tj", "(C) Tj"] {
        assert!(ap.contains(ch), "{ch} not shown on its own:\n{ap}");
    }
    assert!(!ap.contains("(ABC)"), "drawn as one run:\n{ap}");
    // A and B share a width, so the A->B step is exactly one 40 pt cell.
    assert!(ap.contains("40 0 Td"), "cells are not 40 pt apart:\n{ap}");
}

/// The first `Td` after `(A) Tj` moves `dx` along the baseline.
fn assert_step(ap: &str, dx: f64) {
    let after = ap.split("(A) Tj").nth(1).expect("(A) shown");
    let td = after
        .lines()
        .find(|l| l.ends_with(" Td"))
        .expect("a Td after (A)");
    let got: f64 = td.split(' ').next().unwrap().parse().unwrap();
    assert!(
        (got - dx).abs() < 1e-6,
        "step {got}, want {dx}:
{ap}"
    );
}

#[test]
fn a_comb_fill_draws_one_character_per_cell() {
    let mut s = session_with(
        &NewTextField::new(0, "c", BOX)
            .with_max_len(5)
            .with_comb(true)
            .declining_tooltip(),
    );
    s.fill_text_field("c", "ABC").expect("fill");
    assert_per_cell(&ap_bytes(&s));
}

#[test]
fn a_comb_field_created_with_a_value_is_drawn_per_cell() {
    let s = session_with(
        &NewTextField::new(0, "c", BOX)
            .with_value("ABC")
            .with_max_len(5)
            .with_comb(true)
            .declining_tooltip(),
    );
    assert_per_cell(&ap_bytes(&s));
}

#[test]
fn turning_comb_on_redraws_per_cell_and_off_redraws_as_a_run() {
    let mut s = session_with(
        &NewTextField::new(0, "c", BOX)
            .with_max_len(5)
            .declining_tooltip(),
    );
    s.fill_text_field("c", "ABC").expect("fill");
    assert!(ap_bytes(&s).contains("(ABC) Tj"), "plain field not one run");

    s.edit_field("c", &FieldEdit::new().with_comb(true))
        .expect("comb on");
    assert_per_cell(&ap_bytes(&s));

    s.edit_field("c", &FieldEdit::new().with_comb(false))
        .expect("comb off");
    assert!(ap_bytes(&s).contains("(ABC) Tj"), "comb off not one run");
}

#[test]
fn changing_max_len_resizes_the_cells() {
    let mut s = session_with(
        &NewTextField::new(0, "c", BOX)
            .with_max_len(5)
            .with_comb(true)
            .declining_tooltip(),
    );
    s.fill_text_field("c", "AB").expect("fill");
    s.edit_field("c", &FieldEdit::new().with_max_len(Some(4)))
        .expect("max_len 4");
    // 200 / 4 = 50 pt cells.
    let ap = ap_bytes(&s);
    assert_step(&ap, 50.0);
}

#[test]
fn a_value_past_max_len_is_stored_whole_drawn_to_the_limit_and_disclosed() {
    let mut s = session_with(
        &NewTextField::new(0, "c", BOX)
            .with_max_len(5)
            .with_comb(true)
            .declining_tooltip(),
    );
    let out = s.fill_text_field("c", "ABCDEFG").expect("fill");
    assert_eq!(out.exceeds_max_len, Some(5), "over-length not disclosed");
    assert_eq!(
        out.unencodable_chars, 0,
        "overflow miscounted as unencodable"
    );
    let ap = ap_bytes(&s);
    assert!(ap.contains("(E) Tj"), "fifth cell not drawn:\n{ap}");
    assert!(!ap.contains("(F) Tj"), "drawn past the last cell:\n{ap}");

    let fits = s.fill_text_field("c", "ABCDE").expect("fill");
    assert_eq!(fits.exceeds_max_len, None, "an exact fit was flagged");
}
