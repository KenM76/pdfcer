//! # Rotating an annotation (`Pass 155.0`) — the third transform, and the
//! easiest of the three
//!
//! ## Why rotation is better behaved than resize, not worse
//!
//! ISO 32000-1 §12.5.5's placement matrix **A** *"scales and translates"* —
//! it cannot rotate — and §12.5.2 requires `/Rect` to be upright. So a
//! rotation cannot be expressed by moving the rectangle, the way a
//! translation and a scale can.
//!
//! It does not need to be. Step (a) transforms the appearance `BBox` **through
//! its own `/Matrix`** to *"produce a quadrilateral with arbitrary
//! orientation"*, and step (c) concatenates that `/Matrix` with **A**. The
//! rotation belongs in `/Matrix`, which the standard provides for explicitly.
//!
//! Two consequences, and both are improvements on `resize_annotation`:
//!
//! * **A foreign appearance rotates correctly.** pdfcer composes a rotation
//!   into the existing `/Matrix` rather than redrawing, so no producer's
//!   artwork is replaced. Resize has to refuse in that position; this does
//!   not.
//! * **Nothing distorts.** A rotation is an isometry — every length is
//!   preserved, including the drawn stroke. There is no stroke-width option
//!   here and no options type at all.
//!
//! ## The claims
//!
//! 1. **Four 90° turns are the identity.** The sharpest assertion available
//!    and it needs no oracle: rotate four times and every coordinate must
//!    come back to where it started. A sign error, a transposed matrix
//!    element or a pivot applied twice all fail it, and none of them is
//!    visible in a single rotation's output.
//! 2. **`/Rect` grows at 45° and does not at 90°.** The upright bounding box
//!    of a rotated rectangle is larger unless the angle is a multiple of a
//!    quarter turn. That growth is correct and is the thing most likely to be
//!    misread as a defect.
//! 3. **A point on the anchor does not move.**
//! 4. **The appearance `/Matrix` is composed, not replaced** — so a producer's
//!    existing matrix survives.
//!
//! Fixture provenance: `fixtures/synthetic/annot/PROVENANCE.md`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::annot::page_annotations;
use pdfcer_core::annot_author::{Color, MarkupSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_core::writer::SaveOptions;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(rel)).expect("load fixture"))
}

fn reload(s: &EditSession) -> Document {
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("incremental save");
    Document::from_bytes(bytes).expect("re-parse")
}

fn dict_of(doc: &Document, id: ObjId) -> Dict {
    match &doc.get(id).expect("object present").value {
        Object::Dict(d) => d.clone(),
        other => panic!("not a dictionary: {other:?}"),
    }
}

fn nums(doc: &Document, d: &Dict, key: &[u8]) -> Vec<f64> {
    match d.get(key).map(|o| doc.view().resolve(o).clone()) {
        Some(Object::Array(items)) => items
            .iter()
            .map(|o| doc.view().resolve(o).as_number().expect("numeric"))
            .collect(),
        other => panic!("key not a numeric array: {other:?}"),
    }
}

/// A triangle, because it has no rotational symmetry — a square would pass
/// several wrong implementations at 90°.
fn triangle() -> (EditSession, ObjId) {
    let mut s = session("annot/demo-annotated.pdf");
    let spec = MarkupSpec::Polygon {
        vertices: vec![(100.0, 100.0), (160.0, 100.0), (130.0, 150.0)],
        border: Some(Color::Gray(0.0)),
        interior: None,
        width: 1.0,
    };
    let id = s.add_markup(0, &spec).expect("author the polygon");
    (s, id)
}

const ANCHOR: (f64, f64) = (100.0, 100.0);

// ---------------------------------------------------------------------------
// 1. THE ORACLE-FREE CLAIM
// ---------------------------------------------------------------------------

/// ★ Four quarter turns are the identity. No reference render, no remembered
/// coordinate, no threshold — the program checks itself.
///
/// This catches what a single rotation cannot show: a sign error in the
/// matrix, a transposed element, or the pivot being applied twice all produce
/// a plausible-looking single result and a wrong fourth one.
#[test]
fn four_quarter_turns_return_every_coordinate_to_its_start() {
    let (mut s, id) = triangle();
    let before = reload(&s);
    let start_verts = nums(&before, &dict_of(&before, id), b"Vertices");
    let start_rect = nums(&before, &dict_of(&before, id), b"Rect");

    for _ in 0..4 {
        s.rotate_annotation(id, ANCHOR, 90.0).expect("quarter turn");
    }

    let after = reload(&s);
    let end_verts = nums(&after, &dict_of(&after, id), b"Vertices");
    let end_rect = nums(&after, &dict_of(&after, id), b"Rect");

    assert_eq!(start_verts.len(), end_verts.len());
    for (i, (a, b)) in start_verts.iter().zip(end_verts.iter()).enumerate() {
        assert!(
            (a - b).abs() < 1e-6,
            "vertex component {i} drifted over four quarter turns: {a} -> {b}"
        );
    }
    for (i, (a, b)) in start_rect.iter().zip(end_rect.iter()).enumerate() {
        assert!(
            (a - b).abs() < 1e-6,
            "/Rect component {i} drifted: {a} -> {b}"
        );
    }
}

// ---------------------------------------------------------------------------
// 2. THE ANCHOR HOLDS
// ---------------------------------------------------------------------------

#[test]
fn a_vertex_on_the_anchor_does_not_move() {
    let (mut s, id) = triangle();
    s.rotate_annotation(id, ANCHOR, 37.0).expect("rotate");
    let after = reload(&s);
    let v = nums(&after, &dict_of(&after, id), b"Vertices");
    assert!(
        (v[0] - 100.0).abs() < 1e-6 && (v[1] - 100.0).abs() < 1e-6,
        "the vertex sitting on the pivot moved: {:?}",
        &v[..2]
    );
}

/// 90° anticlockwise about (100, 100) maps (160, 100) to (100, 160): the
/// offset (+60, 0) becomes (0, +60). Asserted as arithmetic a reader can
/// redo in their head, rather than as a number copied from a run.
#[test]
fn a_quarter_turn_maps_the_offset_the_way_the_arithmetic_says() {
    let (mut s, id) = triangle();
    s.rotate_annotation(id, ANCHOR, 90.0).expect("rotate");
    let after = reload(&s);
    let v = nums(&after, &dict_of(&after, id), b"Vertices");
    assert!(
        (v[2] - 100.0).abs() < 1e-6 && (v[3] - 160.0).abs() < 1e-6,
        "(160,100) about (100,100) by +90 should be (100,160), got ({}, {})",
        v[2],
        v[3]
    );
}

// ---------------------------------------------------------------------------
// 3. /Rect GROWS, AND THAT IS CORRECT
// ---------------------------------------------------------------------------

/// The behaviour most likely to be reported as a bug. §12.5.2 requires
/// `/Rect` upright, so the rectangle bounding a rotated shape is larger —
/// while the artwork itself is unchanged, because rotation is an isometry.
#[test]
fn rect_grows_at_forty_five_degrees_and_not_at_ninety() {
    let area = |r: &[f64]| (r[2] - r[0]) * (r[3] - r[1]);

    let (mut s, id) = triangle();
    let start = reload(&s);
    let start_area = area(&nums(&start, &dict_of(&start, id), b"Rect"));

    s.rotate_annotation(id, ANCHOR, 45.0).expect("45");
    let at45 = reload(&s);
    let area45 = area(&nums(&at45, &dict_of(&at45, id), b"Rect"));
    assert!(
        area45 > start_area,
        "an upright box bounding a 45°-rotated shape must be larger: {start_area} -> {area45}"
    );

    let (mut s2, id2) = triangle();
    s2.rotate_annotation(id2, ANCHOR, 90.0).expect("90");
    let at90 = reload(&s2);
    let area90 = area(&nums(&at90, &dict_of(&at90, id2), b"Rect"));
    assert!(
        (area90 - start_area).abs() < 1e-6,
        "a quarter turn must not change the bounding area: {start_area} -> {area90}"
    );
}

// ---------------------------------------------------------------------------
// 4. THE APPEARANCE IS COMPOSED, NOT REPLACED
// ---------------------------------------------------------------------------

/// ★ The claim that makes this verb work on artwork pdfcer did not draw.
///
/// A rotation is written into the appearance's own `/Matrix` (§12.5.5 step a),
/// **composed with whatever was already there** — so a producer's existing
/// matrix survives and its artwork is never redrawn. `resize_annotation` has
/// to refuse a foreign appearance; this does not.
#[test]
fn the_appearance_matrix_is_composed_with_what_was_already_there() {
    let (mut s, id) = triangle();
    let out = s.rotate_annotation(id, ANCHOR, 90.0).expect("rotate");
    assert!(
        out.appearance_matrix_updated,
        "the rotation must reach the appearance, or the artwork stays put \
         while its geometry turns"
    );

    let after = reload(&s);
    let d = dict_of(&after, id);
    let ap_id = match d.get(b"AP") {
        Some(Object::Dict(ap)) => ap.get(b"N").and_then(Object::as_reference),
        _ => None,
    }
    .expect("the authored polygon has an /AP /N");
    let Object::Stream(stream) = &after.get(ap_id).expect("ap object").value else {
        panic!("the /AP /N must be a stream");
    };
    let m: Vec<f64> = match stream.dict.get(b"Matrix") {
        Some(Object::Array(items)) => items
            .iter()
            .map(|o| o.as_number().expect("numeric"))
            .collect(),
        other => panic!("no /Matrix written: {other:?}"),
    };
    // 90° anticlockwise is [0 1 -1 0 0 0]; composed with the identity the
    // authored appearance carries, that is what must appear.
    assert!(
        m[0].abs() < 1e-9 && (m[1] - 1.0).abs() < 1e-9 && (m[2] + 1.0).abs() < 1e-9,
        "expected a quarter-turn matrix, got {m:?}"
    );
}

// ---------------------------------------------------------------------------
// 5. REFUSALS AND DISCLOSURES
// ---------------------------------------------------------------------------

#[test]
fn a_non_finite_angle_is_refused_by_name() {
    let (mut s, id) = triangle();
    match s.rotate_annotation(id, ANCHOR, f64::NAN) {
        Err(EditError::ResizeFactorInvalid { axis, .. }) => assert_eq!(axis, "degrees"),
        other => panic!("NaN must be refused, got {other:?}"),
    }
}

#[test]
fn a_widget_is_refused_and_the_message_names_the_mk_r_mechanism() {
    let mut s = session("forms/demo-form.pdf");
    let slots = s.page_slots().expect("page slots");
    let id = page_annotations(&s.graph(), slots[0].id)
        .iter()
        .find_map(|a| a.id)
        .expect("a widget");
    match s.rotate_annotation(id, ANCHOR, 90.0) {
        Err(EditError::AnnotationMoveWrongVerb { why, .. }) => {
            assert!(why.contains("/MK /R"), "{why}");
        }
        other => panic!("a widget must be refused, got {other:?}"),
    }
}

/// `/RD` is four insets along `/Rect`'s own axes (Table 175). At an angle that
/// is not a multiple of 90° no axis-aligned inset expresses the rotated
/// result, so pdfcer leaves them and says so rather than inventing values.
#[test]
fn rect_differences_are_left_alone_and_reported() {
    let mut s = session("annot/rect-differences-square.pdf");
    let slots = s.page_slots().expect("page slots");
    let id = page_annotations(&s.graph(), slots[0].id)
        .first()
        .and_then(|a| a.id)
        .expect("the square");

    let before = nums(&reload(&s), &dict_of(&reload(&s), id), b"RD");
    let out = s.rotate_annotation(id, ANCHOR, 30.0).expect("rotate");
    assert!(
        out.rect_differences_untouched,
        "the omission must be disclosed, not silent"
    );
    let after = reload(&s);
    assert_eq!(nums(&after, &dict_of(&after, id), b"RD"), before);
}

/// ★★ The test that makes "composed" mean something, added after a sabotage
/// survived without it.
///
/// The test above uses an appearance whose `/Matrix` is the identity — where
/// *composing* a rotation and *replacing* the matrix produce the same six
/// numbers. Changing `post_concat` to a bare assignment left the whole suite
/// green, so the claim "composed, not replaced" was untested.
///
/// This fixture's `/AP` carries `/Matrix [2 0 0 2 0 0]` — a 2× scale a
/// producer put there deliberately. After a quarter turn the result must be
/// that scale **with** the rotation (`[0 2 -2 0 0 0]`), not the rotation
/// alone. A replacement would silently halve the artwork.
#[test]
fn a_producers_existing_matrix_survives_the_rotation() {
    let mut s = session("annot/placement-matrix-scale.pdf");
    let slots = s.page_slots().expect("page slots");
    let id = page_annotations(&s.graph(), slots[0].id)
        .first()
        .and_then(|a| a.id)
        .expect("the stamp");

    s.rotate_annotation(id, ANCHOR, 90.0).expect("rotate");

    let after = reload(&s);
    let ap_id = match dict_of(&after, id).get(b"AP") {
        Some(Object::Dict(ap)) => ap.get(b"N").and_then(Object::as_reference),
        _ => None,
    }
    .expect("/AP /N");
    let Object::Stream(stream) = &after.get(ap_id).expect("ap").value else {
        panic!("not a stream");
    };
    let m: Vec<f64> = match stream.dict.get(b"Matrix") {
        Some(Object::Array(items)) => items
            .iter()
            .map(|o| o.as_number().expect("numeric"))
            .collect(),
        other => panic!("no /Matrix: {other:?}"),
    };
    assert!(
        m[0].abs() < 1e-9 && (m[1] - 2.0).abs() < 1e-9 && (m[2] + 2.0).abs() < 1e-9,
        "the producer's 2x scale must survive the quarter turn — expected \
         [0 2 -2 0 ..], got {m:?}. A REPLACED matrix reads as [0 1 -1 0 ..] \
         and halves their artwork."
    );
}
