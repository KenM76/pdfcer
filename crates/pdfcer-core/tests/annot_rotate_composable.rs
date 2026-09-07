//! # `Pass 155.1` / `Pass 155.2` — rotation must COMPOSE, and its angle must
//! be readable
//!
//! Sibling of `annot_rotate.rs`, which pins `Pass 155.0`'s original claims
//! (four quarter turns are the identity, the anchor does not move, a
//! producer's `/Matrix` survives). Those all passed throughout the defect
//! below, which is exactly why this file exists: **every one of them is a
//! single-rotation claim**, and the defect only appeared on the second turn.
//!
//! ## The defect, and how it was found
//!
//! Until 2026-09-07 `rotate_annotation` derived the new `/Rect` as the
//! upright bound of the four rotated corners of the **old `/Rect`**. That is
//! correct once — after turn 1 the appearance's transformed `/BBox` bounds to
//! exactly that rectangle, so ISO 32000-1 §12.5.5 step (c)'s placement matrix
//! **A** is unit-scale and the ink is drawn at true size.
//!
//! Turn 2 bounds an **already-enlarged rectangle** while `/Matrix` has only
//! accumulated to 2θ. Step (c) *"scales and translates"* **A** so the
//! transformed `/BBox` fits `/Rect` exactly — so it scales the artwork **up**
//! to fill the surplus. Every subsequent turn compounds it.
//!
//! **The operator reported it himself**, unprompted: *"the rotate bug in the
//! review objects where the object gets larger with each enactment of the
//! tool."* `pdfcer-gui` then refuted the verb's own doc comment — *"the
//! artwork does not grow; only the rectangle that bounds it does"* — with
//! rendered pixels: on a 140 × 60 pt `/Square`, one 60° turn drew an ink box
//! of 243 × 302 device px (exactly right) while **four 15° turns drew
//! 469 × 430**, 1.93× wider and 1.42× taller.
//!
//! ## Why this file measures something sharper than pixels
//!
//! They measured in pixels because from outside the crate that is all there
//! is. From inside, the defect can be measured **at its mechanism**: step
//! (c)'s fit scale itself, which must be exactly 1.0 however many times the
//! verb runs. A pixel A/B can only say two renders disagree; the fit scale
//! says *by how much the ink was stretched*, which is the quantity the
//! operator actually saw. [`placement_scale`] recomputes §12.5.5 steps (a)
//! and (b) from the **saved file** — not from anything the verb returned — so
//! the oracle is independent of the code under test.
//!
//! ## The rules being pinned
//!
//! 1. The artwork's drawn scale is 1.0 after 1, 2, 4, 8 and 24 turns.
//! 2. *N* turns of θ/*N* produce the same rectangle as one turn of θ — the
//!    requester's own acceptance criterion — **with a positive control** that
//!    reproduces the shipped defect's arithmetic and asserts the fix differs
//!    from it, so the test cannot pass vacuously on an implementation that
//!    rotated nothing.
//! 3. All three rectangle rules are reachable and each is named in the
//!    outcome (rule 4: pdfcer chose on evidence the caller cannot see, and
//!    only two of the three compose).
//! 4. The geometry rule composes **and** preserves the border allowance.
//! 5. `Pass 155.2`: the angle reads back; a shear is refused rather than
//!    rounded; the absolute setter is idempotent and refuses rather than
//!    assuming zero.
//!
//! Fixture provenance: `fixtures/synthetic/annot/PROVENANCE.md`.
//! `no-ap-polyline.pdf` was added for rule 4 above — it is the only fixture
//! that reaches the geometry rule.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::annot::{page_annotations, rotation_degrees};
use pdfcer_core::annot_author::{Color, MarkupSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, RectDerivation};
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
/// several wrong implementations at 90°. `add_markup` bakes an `/AP`, so this
/// reaches the ARTWORK rule, which is the one the defect lived in.
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

/// ISO 32000-1 §12.5.5 steps (a) and (b), recomputed from the **saved file**:
/// the scale factor step (c) applies to the artwork.
///
/// `(1.0, 1.0)` means the ink is drawn at true size. The defect showed up
/// here as roughly `(1.9, 1.4)` after four 15° turns.
///
/// Deliberately an independent reimplementation of the two steps rather than
/// a call into the crate: an oracle that shares code with the thing it is
/// testing agrees with it by construction.
fn placement_scale(doc: &Document, id: ObjId) -> (f64, f64) {
    let d = dict_of(doc, id);
    let rect = nums(doc, &d, b"Rect");
    let ap_id = match d.get(b"AP") {
        Some(Object::Dict(ap)) => ap.get(b"N").and_then(Object::as_reference),
        _ => None,
    }
    .expect("/AP /N");
    let Object::Stream(stream) = &doc.get(ap_id).expect("ap").value else {
        panic!("not a stream");
    };
    let bbox: Vec<f64> = match stream.dict.get(b"BBox") {
        Some(Object::Array(items)) => items
            .iter()
            .map(|o| o.as_number().expect("numeric"))
            .collect(),
        other => panic!("no /BBox: {other:?}"),
    };
    let m: Vec<f64> = match stream.dict.get(b"Matrix") {
        Some(Object::Array(items)) => items
            .iter()
            .map(|o| o.as_number().expect("numeric"))
            .collect(),
        None => vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        other => panic!("bad /Matrix: {other:?}"),
    };
    let corners = [
        (bbox[0], bbox[1]),
        (bbox[2], bbox[1]),
        (bbox[2], bbox[3]),
        (bbox[0], bbox[3]),
    ];
    let (mut lo_x, mut lo_y) = (f64::INFINITY, f64::INFINITY);
    let (mut hi_x, mut hi_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (x, y) in corners {
        let (tx, ty) = (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]);
        lo_x = lo_x.min(tx);
        lo_y = lo_y.min(ty);
        hi_x = hi_x.max(tx);
        hi_y = hi_y.max(ty);
    }
    (
        (rect[2] - rect[0]) / (hi_x - lo_x),
        (rect[3] - rect[1]) / (hi_y - lo_y),
    )
}

fn rect_size(doc: &Document, id: ObjId) -> (f64, f64) {
    let r = nums(doc, &dict_of(doc, id), b"Rect");
    (r[2] - r[0], r[3] - r[1])
}

// ---------------------------------------------------------------------------
// 1. THE OPERATOR'S BUG, MEASURED AT ITS MECHANISM
// ---------------------------------------------------------------------------

/// ★★ The appearance's drawn scale stays 1.0 however many times the verb
/// runs.
///
/// 24 turns is included on purpose: the defect compounded multiplicatively,
/// so a residual error that a 4-turn test could absorb inside its tolerance
/// is unmissable by then.
#[test]
fn the_artwork_is_never_scaled_however_many_times_it_turns() {
    for turns in [1_u32, 2, 4, 8, 24] {
        let (mut s, id) = triangle();
        for _ in 0..turns {
            s.rotate_annotation(id, ANCHOR, 15.0).expect("rotate");
        }
        let after = reload(&s);
        let (sx, sy) = placement_scale(&after, id);
        assert!(
            (sx - 1.0).abs() < 1e-6 && (sy - 1.0).abs() < 1e-6,
            "after {turns} turn(s) of 15 degrees the appearance is drawn at \
             {sx:.4} x {sy:.4} of its true size, not 1.0 x 1.0. Anything but \
             1.0 means /Rect no longer matches the transformed /BBox, so \
             ISO 32000-1 12.5.5 step (c) is stretching the ink to fill it -- \
             which is what the operator reported as 'the object gets larger \
             with each enactment of the tool'."
        );
    }
}

/// The requester's own oracle, restated at rectangle level, **with a positive
/// control** and **a route assertion**.
///
/// The A/B alone would pass on an implementation that made both answers wrong
/// in the same way, and on one that never rotated anything at all. The
/// control reproduces the shipped defect's arithmetic — bound the previous
/// rectangle, four times — and asserts the fix is measurably different from
/// it, in the direction and roughly the magnitude `pdfcer-gui` reported from
/// pixels.
///
/// ★ **The route assertion was added after a sabotage survived.** Disabling
/// the artwork rule entirely left this test GREEN, because the triangle also
/// carries `/Vertices` and quietly fell through to the geometry rule — which
/// composes too, so the A/B still held. The test was measuring *"some rule
/// composes"* while its name claimed it measured the artwork one. Pinning
/// [`RectDerivation::Artwork`] is what makes the name true.
#[test]
fn four_fifteen_degree_turns_equal_one_sixty_degree_turn() {
    let (mut one, id) = triangle();
    let out = one.rotate_annotation(id, ANCHOR, 60.0).expect("rotate");
    assert_eq!(
        out.rect_derived_from,
        RectDerivation::Artwork,
        "this test is about the APPEARANCE rule; if the annotation fell \
         through to another rule the A/B below would still pass and would be \
         measuring something else"
    );
    let one = reload(&one);
    let (aw, ah) = rect_size(&one, id);

    let (mut four, id) = triangle();
    for _ in 0..4 {
        let out = four.rotate_annotation(id, ANCHOR, 15.0).expect("rotate");
        assert_eq!(out.rect_derived_from, RectDerivation::Artwork);
    }
    let four = reload(&four);
    let (bw, bh) = rect_size(&four, id);

    assert!(
        (aw - bw).abs() < 1e-6 && (ah - bh).abs() < 1e-6,
        "1 x 60 degrees gave a {aw:.4} x {ah:.4} rectangle and 4 x 15 degrees \
         gave {bw:.4} x {bh:.4}. Rotation must be composable."
    );

    let (before, id_start) = triangle();
    let before = reload(&before);
    let (mut w, mut h) = rect_size(&before, id_start);
    let (c, s) = (15_f64.to_radians().cos(), 15_f64.to_radians().sin());
    for _ in 0..4 {
        let (nw, nh) = (w * c + h * s, w * s + h * c);
        w = nw;
        h = nh;
    }
    assert!(
        w > bw * 1.2 && h > bh * 1.2,
        "the positive control did not reproduce the old defect: bounding the \
         previous rectangle four times gives {w:.4} x {h:.4}, which is not \
         more than 20% larger than the fixed {bw:.4} x {bh:.4}. If these are \
         close, this test is not measuring what it claims to."
    );
}

// ---------------------------------------------------------------------------
// 2. THE THREE RULES, ALL REACHABLE, ALL DISCLOSED
// ---------------------------------------------------------------------------

/// Rule 4: which rule produced the rectangle is named in the outcome, and
/// each of the three is actually reachable — a route nothing can reach is a
/// route nothing tests.
#[test]
fn the_rectangle_rule_used_is_named_and_all_three_are_reachable() {
    // (a) an appearance stream -> ARTWORK.
    let (mut s, id) = triangle();
    let out = s.rotate_annotation(id, ANCHOR, 30.0).expect("rotate");
    assert_eq!(out.rect_derived_from, RectDerivation::Artwork);

    // (b) /Vertices and no /AP -> GEOMETRY.
    let mut s = session("annot/no-ap-polyline.pdf");
    let id = first_annot(&s);
    let out = s.rotate_annotation(id, ANCHOR, 30.0).expect("rotate");
    assert_eq!(out.rect_derived_from, RectDerivation::Geometry);

    // (c) neither -> PREVIOUS-RECT, the one that does not compose.
    let mut s = session("annot/no-ap-circle.pdf");
    let id = first_annot(&s);
    let out = s.rotate_annotation(id, ANCHOR, 30.0).expect("rotate");
    assert_eq!(out.rect_derived_from, RectDerivation::PreviousRect);
}

fn first_annot(s: &EditSession) -> ObjId {
    let slots = s.page_slots().expect("slots");
    page_annotations(&s.graph(), slots[0].id)
        .first()
        .and_then(|a| a.id)
        .expect("an annotation")
}

/// The geometry rule composes too, and keeps the border allowance the old
/// rectangle carried.
///
/// The fixture's `/Rect` sits 5 units outside its vertex bound on every side
/// precisely so an implementation that dropped the allowance fails here
/// rather than passing quietly — a fixture whose rectangle hugged its
/// geometry could not tell the two apart.
#[test]
fn the_geometry_rule_composes_and_keeps_the_border_allowance() {
    let load = || {
        let s = session("annot/no-ap-polyline.pdf");
        let id = first_annot(&s);
        (s, id)
    };

    let (mut one, id) = load();
    one.rotate_annotation(id, ANCHOR, 60.0).expect("rotate");
    let one = reload(&one);
    let (aw, ah) = rect_size(&one, id);

    let (mut four, id) = load();
    for _ in 0..4 {
        four.rotate_annotation(id, ANCHOR, 15.0).expect("rotate");
    }
    let four = reload(&four);
    let (bw, bh) = rect_size(&four, id);

    assert!(
        (aw - bw).abs() < 1e-6 && (ah - bh).abs() < 1e-6,
        "geometry-derived rectangles must compose too: {aw:.4} x {ah:.4} \
         against {bw:.4} x {bh:.4}"
    );

    let d = dict_of(&four, id);
    let rect = nums(&four, &d, b"Rect");
    let verts = nums(&four, &d, b"Vertices");
    let (mut lo_x, mut lo_y) = (f64::INFINITY, f64::INFINITY);
    let (mut hi_x, mut hi_y) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for pair in verts.chunks_exact(2) {
        lo_x = lo_x.min(pair[0]);
        lo_y = lo_y.min(pair[1]);
        hi_x = hi_x.max(pair[0]);
        hi_y = hi_y.max(pair[1]);
    }
    for (got, want) in [
        (lo_x - rect[0], 5.0),
        (lo_y - rect[1], 5.0),
        (rect[2] - hi_x, 5.0),
        (rect[3] - hi_y, 5.0),
    ] {
        assert!(
            (got - want).abs() < 1e-6,
            "the border allowance was not preserved: {got:.4} against the \
             fixture's {want:.4}"
        );
    }
}

// ---------------------------------------------------------------------------
// 3. `Pass 155.2` — the angle can be READ, and set ABSOLUTELY
// ---------------------------------------------------------------------------

/// The read half. Nothing on `Annotation` exposed an orientation before this,
/// so a shell could not seed a properties field, draw a selection outline that
/// followed the object, or place a rotate grip.
#[test]
fn the_rotation_angle_can_be_read_back_from_the_annotation() {
    let (mut s, id) = triangle();

    let slots = s.page_slots().expect("slots");
    let before = page_annotations(&s.graph(), slots[0].id)
        .into_iter()
        .find(|a| a.id == Some(id))
        .expect("the polygon");
    assert_eq!(
        before.appearance_rotation_degrees(),
        Some(0.0),
        "an unrotated annotation reads as 0 degrees, not as None"
    );

    s.rotate_annotation(id, ANCHOR, 37.5).expect("rotate");
    let slots = s.page_slots().expect("slots");
    let after = page_annotations(&s.graph(), slots[0].id)
        .into_iter()
        .find(|a| a.id == Some(id))
        .expect("the polygon");
    let got = after.appearance_rotation_degrees().expect("an angle");
    assert!(
        (got - 37.5).abs() < 1e-6,
        "expected 37.5 degrees back, got {got}"
    );
    assert!(
        after.appearance_matrix.is_some(),
        "the raw matrix is exposed too -- a shell that must tell a rotation \
         from a shear needs the six numbers, not only the decomposition"
    );
}

// ---------------------------------------------------------------------------
// 4. CLOCKWISE. Half the number line, and until now none of it was tested.
// ---------------------------------------------------------------------------

/// ★★ A clockwise rotation reports a NEGATIVE angle, and composes.
///
/// # Why this test exists, and it is not symmetry-for-its-own-sake
///
/// `pdfcer-gui` consumed [`Annotation::appearance_rotation_degrees`] within an
/// hour of `Pass 155.2` shipping and hit a defect: their adapter's doc comment
/// claimed `[0, 360)` — carried over from a local implementation that *had*
/// normalised — while this returns a **signed** `atan2` in `(−180, 180]`. So
/// **every clockwise rotation reported itself upright**, and their selection
/// outline snapped back to axis-aligned. **3,860 of their in-process tests
/// were green**; a driven UI test caught it only because its drag happened to
/// go clockwise.
///
/// That was their stale comment, not a broken contract here. But checking
/// afterwards showed **this crate had the identical blind spot**: across both
/// rotation test files, *every* angle was positive — `15`, `22`, `30`, `37.5`,
/// `45`, `60`. **A sign error is invisible to a test that only ever turns one
/// way**, so pdfcer's own suite could not have caught the mirror of their bug.
///
/// The two assertions below are chosen to fail on the two plausible wrong
/// implementations rather than to restate the right one: a **normalising**
/// reader returns `330` where this demands `−30`, and a reader that took
/// `atan2`'s arguments in the wrong order returns `+30`.
#[test]
fn a_clockwise_rotation_reports_a_negative_angle_and_still_composes() {
    let (mut s, id) = triangle();
    s.rotate_annotation(id, ANCHOR, -30.0).expect("rotate");

    let slots = s.page_slots().expect("slots");
    let got = page_annotations(&s.graph(), slots[0].id)
        .into_iter()
        .find(|a| a.id == Some(id))
        .and_then(|a| a.appearance_rotation_degrees())
        .expect("an angle");
    assert!(
        (got + 30.0).abs() < 1e-6,
        "a 30-degree CLOCKWISE turn must read as -30.0, not {got}. \
         330 means somebody normalised into [0, 360) and broke the \
         subtraction in set_annotation_rotation; +30 means atan2's \
         arguments are the wrong way round."
    );

    // ... and the composability property is not a property of positive angles.
    let (mut one, id_a) = triangle();
    one.rotate_annotation(id_a, ANCHOR, -60.0).expect("rotate");
    let one = reload(&one);
    let (aw, ah) = rect_size(&one, id_a);

    let (mut four, id_b) = triangle();
    for _ in 0..4 {
        four.rotate_annotation(id_b, ANCHOR, -15.0).expect("rotate");
    }
    let four = reload(&four);
    let (bw, bh) = rect_size(&four, id_b);

    assert!(
        (aw - bw).abs() < 1e-6 && (ah - bh).abs() < 1e-6,
        "clockwise must compose too: 1 x -60 gave {aw:.4} x {ah:.4}, \
         4 x -15 gave {bw:.4} x {bh:.4}"
    );
}

/// The absolute setter lands on a NEGATIVE target, from a positive start.
///
/// This is the arithmetic `pdfcer-gui`'s defect would have corrupted from the
/// other end: `set_annotation_rotation` computes `wanted − current`, and a
/// normalised `current` makes that subtraction wrong by 360 for exactly the
/// clockwise cases. Starting at `+40` and asking for `−25` is a 65-degree
/// clockwise delta, and a normalising reader would compute `−25 − 335 = −360`
/// and land back where it started.
#[test]
fn an_absolute_negative_target_is_reached_from_a_positive_start() {
    let (mut s, id) = triangle();
    s.rotate_annotation(id, ANCHOR, 40.0)
        .expect("start positive");

    let out = s
        .set_annotation_rotation(id, ANCHOR, -25.0)
        .expect("set a clockwise absolute angle");
    assert!(
        (out.degrees + 65.0).abs() < 1e-6,
        "the delta applied should be -65 (from +40 to -25), not {}. \
         -360 means the current angle was read normalised.",
        out.degrees
    );

    let slots = s.page_slots().expect("slots");
    let got = page_annotations(&s.graph(), slots[0].id)
        .into_iter()
        .find(|a| a.id == Some(id))
        .and_then(|a| a.appearance_rotation_degrees())
        .expect("an angle");
    assert!((got + 25.0).abs() < 1e-6, "must land on -25.0, got {got}");
}

/// A shear, a mirror and a non-uniform scale are **not angles** and must not
/// be reported as ones, however turned the artwork looks. A properties field
/// seeded from a confident wrong number commits the invention the moment the
/// operator presses Enter.
#[test]
fn a_sheared_matrix_is_refused_rather_than_rounded_to_the_nearest_angle() {
    assert_eq!(
        rotation_degrees([1.0, 0.0, 0.5, 1.0, 0.0, 0.0]),
        None,
        "shear"
    );
    assert_eq!(
        rotation_degrees([-1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
        None,
        "mirror"
    );
    assert_eq!(
        rotation_degrees([2.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
        None,
        "non-uniform scale"
    );
    assert_eq!(
        rotation_degrees([0.0, 0.0, 0.0, 0.0, 0.0, 0.0]),
        None,
        "degenerate"
    );
    // A UNIFORM scale is allowed: a producer's 2x appearance is still turned
    // by a knowable angle, and refusing it would lose the read on every
    // annotation whose producer scaled its own artwork.
    let got = rotation_degrees([0.0, 2.0, -2.0, 0.0, 0.0, 0.0]).expect("90 degrees");
    assert!((got - 90.0).abs() < 1e-9, "got {got}");
}

/// The absolute setter is IDEMPOTENT, which a delta verb cannot be. This is
/// the property a typed properties field needs: the operator sees 45, presses
/// Enter twice, and the object does not move the second time.
#[test]
fn setting_an_absolute_angle_twice_changes_nothing_the_second_time() {
    let (mut s, id) = triangle();
    s.rotate_annotation(id, ANCHOR, 22.0)
        .expect("get it crooked first, so the setter has a real delta to find");

    s.set_annotation_rotation(id, ANCHOR, 45.0).expect("set");
    let once = reload(&s);
    let first = nums(&once, &dict_of(&once, id), b"Rect");

    let out = s
        .set_annotation_rotation(id, ANCHOR, 45.0)
        .expect("set again");
    assert!(
        out.degrees.abs() < 1e-6,
        "the second call should have applied a zero delta, not {}",
        out.degrees
    );
    let twice = reload(&s);
    let second = nums(&twice, &dict_of(&twice, id), b"Rect");
    for (a, b) in first.iter().zip(second.iter()) {
        assert!((a - b).abs() < 1e-6, "{first:?} against {second:?}");
    }

    let slots = s.page_slots().expect("slots");
    let got = page_annotations(&s.graph(), slots[0].id)
        .into_iter()
        .find(|a| a.id == Some(id))
        .and_then(|a| a.appearance_rotation_degrees())
        .expect("an angle");
    assert!(
        (got - 45.0).abs() < 1e-6,
        "the absolute setter must land on 45, not {got} -- it started at 22, \
         and 67 would mean the delta was composed against an assumed zero"
    );
}

/// It REFUSES rather than assuming zero when the current angle is unknown.
///
/// Assuming zero would turn *"type 45 on an object already at 30"* into 75 —
/// an invention committed by the act of typing, which is what rule 4 exists
/// to prevent.
#[test]
fn an_absolute_angle_is_refused_when_the_current_one_cannot_be_read() {
    let mut s = session("annot/no-ap-circle.pdf");
    let id = first_annot(&s);

    match s.set_annotation_rotation(id, ANCHOR, 45.0) {
        Err(EditError::AnnotationRotationUnreadable { subtype, why }) => {
            assert_eq!(subtype, "Circle");
            assert!(
                why.contains("no appearance stream"),
                "the refusal must name WHICH of the situations it was: {why}"
            );
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    // ... and the DELTA verb still works on the same annotation, which is
    // exactly what the refusal message tells the caller to reach for. A
    // refusal that were actually a dead end would make this line fail.
    s.rotate_annotation(id, ANCHOR, 45.0)
        .expect("the delta verb needs no starting angle");
}
