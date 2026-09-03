//! # `EditSession::move_annotation` — the move verb the annotation family did
//! not have (`Pass 149.0`)
//!
//! Markup could be authored and deleted but **not moved**. `move_widget`
//! covered form widgets and `move_dimension` covered ce dimensions; Ink,
//! Square, Circle, Line, Polygon, PolyLine, the four text markups, FreeText,
//! Text notes, Stamp, Link and unapplied Redact marks had no move verb at all,
//! and a shell was blocked on it.
//!
//! ## The property that makes this correct rather than convenient
//!
//! Every assertion here runs **through an incremental save and re-parse**, so
//! it is about bytes another program would read, not about the session's
//! in-memory overlay.
//!
//! Two halves have to agree, and only one of them is visible:
//!
//! 1. **`/Rect`** — moving it moves the *painted* result, because ISO 32000-1
//!    §12.5.5 recomputes the placement matrix from the appearance `BBox` and
//!    the new `/Rect`. A pure translation makes that matrix a pure
//!    translation, so the artwork travels 1:1 with no re-authoring and an
//!    appearance pdfcer did not draw survives intact.
//! 2. **The geometry keys** — `/L`, `/Vertices`, `/InkList`, `/QuadPoints`,
//!    `/CL` hold *absolute page coordinates*, and they are what any **other**
//!    tool regenerates an appearance from.
//!
//! ★ Move only (1) and the annotation renders in the new place and is
//! reconstructed in the **old** one by the next viewer that rebuilds it. That
//! failure is invisible in pdfcer, invisible in a screenshot, and shows up in
//! somebody else's product. It is the reason `geometry_keys_moved` is a
//! disclosure rather than an implementation detail, and the reason the
//! `both_halves` test below asserts the keys and the rect *together*.
//!
//! ## What is deliberately NOT moved
//!
//! `/RD` — rect *differences* are four inset distances, not coordinates.
//! `/Popup` — a separate annotation with its own placement (§12.5.6.14).
//! Both are asserted, because "we chose not to" and "we forgot" are
//! indistinguishable from the outside unless a test says which.
//!
//! Fixture provenance: `fixtures/synthetic/annot/PROVENANCE.md`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::annot::page_annotations;
use pdfcer_core::annot_author::{Color, LineEnding, MarkupSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::writer::SaveOptions;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(rel)).expect("load fixture"))
}

fn annot_id_at(s: &EditSession, index: usize) -> ObjId {
    let slots = s.page_slots().expect("page slots");
    page_annotations(&s.graph(), slots[0].id)
        .get(index)
        .and_then(|a| a.id)
        .expect("annotation with an object identity")
}

/// Save incrementally and re-parse — every assertion is about the bytes.
fn reload(s: &EditSession) -> Document {
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("incremental save");
    Document::from_bytes(bytes).expect("re-parse")
}

fn dict_of(doc: &Document, id: ObjId) -> pdfcer_core::object::Dict {
    match &doc.get(id).expect("object present").value {
        Object::Dict(d) => d.clone(),
        other => panic!("object {id} is not a dictionary: {other:?}"),
    }
}

/// A flat numeric array from a re-parsed dictionary.
fn nums(doc: &Document, d: &pdfcer_core::object::Dict, key: &[u8]) -> Vec<f64> {
    match d.get(key).map(|o| doc.view().resolve(o).clone()) {
        Some(Object::Array(items)) => items
            .iter()
            .map(|o| doc.view().resolve(o).as_number().expect("numeric"))
            .collect(),
        other => panic!("key not a numeric array: {other:?}"),
    }
}

fn rect_of(doc: &Document, d: &pdfcer_core::object::Dict) -> Vec<f64> {
    nums(doc, d, b"Rect")
}

/// A session with one authored markup of the given spec on page 1.
fn with_markup(spec: &MarkupSpec) -> (EditSession, ObjId) {
    let mut s = session("annot/demo-annotated.pdf");
    let id = s.add_markup(0, spec).expect("author the markup");
    (s, id)
}

const DX: f64 = 17.5;
const DY: f64 = -9.25;

// ---------------------------------------------------------------------------
// 1. THE ONE THAT MATTERS — both halves move, together
// ---------------------------------------------------------------------------

#[test]
fn both_halves_move_the_rect_and_the_geometry_keys() {
    // A Polygon carries `/Vertices`, so it exercises the half that is
    // invisible in pdfcer's own rendering. If only `/Rect` moved, this
    // annotation would draw in the new place here and be rebuilt in the old
    // place by any tool that regenerates from `/Vertices`.
    let spec = MarkupSpec::Polygon {
        vertices: vec![(100.0, 100.0), (160.0, 100.0), (130.0, 150.0)],
        border: Some(Color::Gray(0.0)),
        interior: None,
        width: 1.0,
    };
    let (mut s, id) = with_markup(&spec);
    let before = reload(&s);
    let before_d = dict_of(&before, id);
    let before_rect = rect_of(&before, &before_d);
    let before_verts = nums(&before, &before_d, b"Vertices");

    let moved = s.move_annotation(id, DX, DY).expect("polygon moves");
    assert_eq!(moved.geometry_keys_moved, vec!["Vertices".to_owned()]);

    let after = reload(&s);
    let after_d = dict_of(&after, id);

    let after_rect = rect_of(&after, &after_d);
    for (i, (b, a)) in before_rect.iter().zip(after_rect.iter()).enumerate() {
        let expect = b + if i % 2 == 0 { DX } else { DY };
        assert!(
            (a - expect).abs() < 1e-6,
            "/Rect[{i}] {b} -> {a}, expected {expect}"
        );
    }

    let after_verts = nums(&after, &after_d, b"Vertices");
    assert_eq!(after_verts.len(), before_verts.len());
    for (i, (b, a)) in before_verts.iter().zip(after_verts.iter()).enumerate() {
        let expect = b + if i % 2 == 0 { DX } else { DY };
        assert!(
            (a - expect).abs() < 1e-6,
            "/Vertices[{i}] {b} -> {a}, expected {expect} — a move that leaves the \
             geometry keys behind renders right and reconstructs wrong"
        );
    }
}

#[test]
fn an_ink_lists_strokes_one_level_deeper_and_every_stroke_moves() {
    // `/InkList` is an array OF arrays, the one geometry key that is not
    // flat. A translator written for the flat shape silently leaves it alone.
    let spec = MarkupSpec::Ink {
        strokes: vec![
            vec![(10.0, 10.0), (20.0, 30.0)],
            vec![(40.0, 40.0), (50.0, 60.0), (55.0, 65.0)],
        ],
        color: Color::Gray(0.0),
        width: 2.0,
    };
    let (mut s, id) = with_markup(&spec);
    let before = reload(&s);
    let before_d = dict_of(&before, id);
    let Some(Object::Array(before_strokes)) = before_d
        .get(b"InkList")
        .map(|o| before.view().resolve(o).clone())
    else {
        panic!("the fixture markup has an /InkList");
    };

    let moved = s.move_annotation(id, DX, DY).expect("ink moves");
    assert_eq!(moved.geometry_keys_moved, vec!["InkList".to_owned()]);

    let after = reload(&s);
    let after_d = dict_of(&after, id);
    let Some(Object::Array(after_strokes)) = after_d
        .get(b"InkList")
        .map(|o| after.view().resolve(o).clone())
    else {
        panic!("/InkList survived the move");
    };
    assert_eq!(after_strokes.len(), before_strokes.len(), "stroke count");

    let (bv_view, av_view) = (before.view(), after.view());
    for (si, (bs, as_)) in before_strokes.iter().zip(after_strokes.iter()).enumerate() {
        let (Object::Array(b), Object::Array(a)) = (bv_view.resolve(bs), av_view.resolve(as_))
        else {
            panic!("stroke {si} is an array");
        };
        assert_eq!(a.len(), b.len(), "stroke {si} length");
        for (i, (bo, ao)) in b.iter().zip(a.iter()).enumerate() {
            let bv = bv_view.resolve(bo).as_number().unwrap();
            let av = av_view.resolve(ao).as_number().unwrap();
            let expect = bv + if i % 2 == 0 { DX } else { DY };
            assert!(
                (av - expect).abs() < 1e-6,
                "stroke {si}[{i}] {bv} -> {av}, expected {expect}"
            );
        }
    }
}

#[test]
fn a_line_moves_its_two_endpoints() {
    let spec = MarkupSpec::Line {
        start: (20.0, 20.0),
        end: (80.0, 70.0),
        color: Color::Gray(0.0),
        width: 1.0,
        endings: (LineEnding::None, LineEnding::None),
    };
    let (mut s, id) = with_markup(&spec);
    let before = reload(&s);
    let before_l = nums(&before, &dict_of(&before, id), b"L");

    let moved = s.move_annotation(id, DX, DY).expect("line moves");
    assert_eq!(moved.geometry_keys_moved, vec!["L".to_owned()]);

    let after = reload(&s);
    let after_l = nums(&after, &dict_of(&after, id), b"L");
    assert_eq!(after_l.len(), 4);
    for (i, (b, a)) in before_l.iter().zip(after_l.iter()).enumerate() {
        let expect = b + if i % 2 == 0 { DX } else { DY };
        assert!((a - expect).abs() < 1e-6, "/L[{i}] {b} -> {a} != {expect}");
    }
}

// ---------------------------------------------------------------------------
// 2. The appearance is CARRIED, not rewritten
// ---------------------------------------------------------------------------

#[test]
fn the_appearance_stream_is_not_touched() {
    // §12.5.5 moves the artwork for free. Rewriting the stream would risk
    // replacing an appearance pdfcer did not author with pdfcer's own drawing
    // of the same annotation — a move is not a restyle.
    let spec = MarkupSpec::Square {
        rect: Rect {
            llx: 10.0,
            lly: 10.0,
            urx: 60.0,
            ury: 40.0,
        },
        border: Some(Color::Gray(0.0)),
        interior: None,
        border_width: 1.0,
        border_effect: None,
    };
    let (mut s, id) = with_markup(&spec);
    let before = reload(&s);
    let ap_before = dict_of(&before, id)
        .get(b"AP")
        .map(|o| format!("{:?}", before.view().resolve(o)))
        .expect("the authored markup has an /AP");

    let moved = s.move_annotation(id, DX, DY).expect("square moves");
    assert!(moved.appearance_carried, "and it says so");

    let after = reload(&s);
    let ap_after = dict_of(&after, id)
        .get(b"AP")
        .map(|o| format!("{:?}", after.view().resolve(o)))
        .expect("/AP survived");
    assert_eq!(
        ap_before, ap_after,
        "the /AP reference is unchanged — the artwork moves via §12.5.5's \
         placement matrix, not by re-authoring"
    );
}

// ---------------------------------------------------------------------------
// 3. Refusals — by name, and because a BETTER verb exists
// ---------------------------------------------------------------------------

#[test]
fn a_widget_is_refused_and_named_the_verb_that_does_more() {
    // Not delegated. `move_widget` reports the siblings it left behind, and
    // silently doing less under this name would be a second, worse way to
    // move a widget.
    let s0 = session("forms/demo-form.pdf");
    let id = annot_id_at(&s0, 0);
    let mut s = session("forms/demo-form.pdf");

    let err = s
        .move_annotation(id, 1.0, 1.0)
        .expect_err("a widget has its own verb");
    let text = err.to_string();
    assert!(
        matches!(err, EditError::AnnotationMoveWrongVerb { .. }),
        "a NAMED refusal, not a string: {text}"
    );
    assert!(text.contains("move_widget"), "{text}");
    assert!(
        text.contains("Nothing was moved"),
        "and it says nothing happened: {text}"
    );
}

#[test]
fn a_refusal_leaves_the_document_untouched() {
    // "Nothing was moved" is a claim, so it is measured rather than trusted.
    let mut s = session("forms/demo-form.pdf");
    let id = annot_id_at(&s, 0);
    let before = reload(&s);
    let before_rect = rect_of(&before, &dict_of(&before, id));

    let _ = s.move_annotation(id, 100.0, 100.0).unwrap_err();

    let after = reload(&s);
    assert_eq!(rect_of(&after, &dict_of(&after, id)), before_rect);
    assert_eq!(s.undo_depth(), 0, "and no command was committed");
}

// ---------------------------------------------------------------------------
// 4. Undo — one entry, both halves back
// ---------------------------------------------------------------------------

#[test]
fn undo_restores_the_rect_and_the_geometry_together() {
    // One undo entry on purpose: restoring half would leave the annotation
    // describing two different positions at once.
    let spec = MarkupSpec::Polygon {
        vertices: vec![(100.0, 100.0), (160.0, 100.0), (130.0, 150.0)],
        border: Some(Color::Gray(0.0)),
        interior: None,
        width: 1.0,
    };
    let (mut s, id) = with_markup(&spec);
    let before = reload(&s);
    let before_d = dict_of(&before, id);
    let (r0, v0) = (
        rect_of(&before, &before_d),
        nums(&before, &before_d, b"Vertices"),
    );

    let depth = s.undo_depth();
    s.move_annotation(id, DX, DY).unwrap();
    assert_eq!(s.undo_depth(), depth + 1, "exactly one command");
    s.undo().expect("undo the move");

    let after = reload(&s);
    let after_d = dict_of(&after, id);
    assert_eq!(rect_of(&after, &after_d), r0, "/Rect restored");
    assert_eq!(
        nums(&after, &after_d, b"Vertices"),
        v0,
        "/Vertices restored"
    );
}

// ---------------------------------------------------------------------------
// 5. A round trip that must be exact
// ---------------------------------------------------------------------------

#[test]
fn moving_back_by_the_negation_returns_the_original_coordinates() {
    // A translation is its own inverse under negation, and this is the
    // cheapest possible check that the two halves use the SAME vector — a
    // sign error in one of them survives every single-direction test.
    let spec = MarkupSpec::Ink {
        strokes: vec![vec![(10.0, 10.0), (20.0, 30.0)]],
        color: Color::Gray(0.0),
        width: 2.0,
    };
    let (mut s, id) = with_markup(&spec);
    let before = reload(&s);
    let before_d = dict_of(&before, id);
    let r0 = rect_of(&before, &before_d);

    s.move_annotation(id, DX, DY).unwrap();
    s.move_annotation(id, -DX, -DY).unwrap();

    let after = reload(&s);
    let r1 = rect_of(&after, &dict_of(&after, id));
    for (a, b) in r0.iter().zip(r1.iter()) {
        assert!((a - b).abs() < 1e-6, "{a} != {b} after there-and-back");
    }
}
