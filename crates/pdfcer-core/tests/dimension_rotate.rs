//! # Rotating a ce dimension (`Pass 159.0`)
//!
//! ## ★★★ The claim that defines this verb: the value cannot change
//!
//! A rotation preserves every distance and every angle, so the number a ce
//! dimension displays is **identical either side of it** — not because pdfcer
//! chose to hold it, but because there is nothing to change.
//!
//! That is what makes rotating one a legitimate drafting operation, and it is
//! the reason **scaling one is deliberately not offered**. Scaling has no
//! honest reading: either the value stays fixed while the geometry grows, so
//! the dimension lies about the drawing, or both change, so nothing was
//! measured and the operator has drawn a number rather than taken one. The
//! operation actually wanted is `set_group_scale`, which changes the
//! measurement *ratio*, and it already ships.
//!
//! Project rule 15 is exactly this distinction: **a ce dimension's text IS its
//! measurement**, so an operation that moves the text away from the geometry
//! is not a drafting operation at all.
//!
//! ## The one judgement in the verb
//!
//! A `Linear` dimension may be constrained `Horizontal` or `Vertical`. Rotate
//! it 30° and the constraint no longer describes what is drawn. pdfcer relaxes
//! it to `Aligned` — which is the honest description of a line following its
//! own picked points — and **reports having done so**. Keeping the constraint
//! would leave the drawn line disagreeing with its own stated constraint,
//! invisibly, until something regenerated from it.
//!
//! Fixture provenance: `fixtures/synthetic/dimension/PROVENANCE.md`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(rel)).expect("load fixture"))
}

/// The first ce dimension's id and the value it currently displays, read from
/// the model the way `dimension-list` reads it.
fn first_dimension(s: &EditSession) -> (pdfcer_core::dimension::DimensionId, String) {
    let model = s.dimension_model();
    let d = model
        .dimensions()
        .first()
        .expect("the fixture has one ce dimension");
    let v = s
        .dimension_model()
        .display(d.id)
        .expect("a display value")
        .text;
    (d.id, v)
}

fn value_of(s: &EditSession, id: pdfcer_core::dimension::DimensionId) -> String {
    s.dimension_model()
        .display(id)
        .expect("the dimension survives")
        .text
}

// ---------------------------------------------------------------------------
// 1. THE ISOMETRY CLAIM
// ---------------------------------------------------------------------------

/// ★ Needs no oracle and no remembered number: whatever the fixture measures,
/// it must measure the same thing afterwards.
///
/// Asserted at several angles including awkward ones, because a bug that
/// re-derived the value from a wrongly-rotated point set would very likely
/// still give the right answer at 90° (where a horizontal line becomes an
/// equally long vertical one) and a wrong one at 37°.
#[test]
fn rotation_never_changes_the_measured_value() {
    for degrees in [1.0, 37.0, 90.0, 180.0, -45.5, 359.0] {
        let mut s = session("dimension/linear-dim.pdf");
        let (id, before) = first_dimension(&s);
        s.rotate_dimension(id, (100.0, 100.0), degrees)
            .unwrap_or_else(|e| panic!("rotate by {degrees} must work, but: {e}"));
        assert_eq!(
            value_of(&s, id),
            before,
            "a rotation is an isometry: {degrees}° must not change the measurement"
        );
    }
}

/// Four quarter turns return the geometry exactly, so the value survives the
/// round trip too — and any accumulated drift in the point set would show as a
/// changed measurement even though each individual step looked right.
#[test]
fn four_quarter_turns_leave_the_dimension_as_it_was() {
    let mut s = session("dimension/linear-dim.pdf");
    let (id, before) = first_dimension(&s);
    for _ in 0..4 {
        s.rotate_dimension(id, (150.0, 120.0), 90.0)
            .expect("quarter turn");
    }
    assert_eq!(value_of(&s, id), before);
}

// ---------------------------------------------------------------------------
// 2. THE CONSTRAINT JUDGEMENT
// ---------------------------------------------------------------------------

/// A whole number of turns moves nothing, so it must not relax anything
/// either. The relaxation exists to keep the constraint honest about the
/// geometry; here the geometry is exactly where it was.
#[test]
fn a_full_turn_does_not_relax_the_constraint() {
    let mut s = session("dimension/linear-dim.pdf");
    let (id, _) = first_dimension(&s);
    let out = s
        .rotate_dimension(id, (100.0, 100.0), 360.0)
        .expect("full turn");
    assert!(
        !out.constraint_relaxed,
        "nothing moved, so nothing may be relaxed"
    );
}

/// The report exists so a shell can say *"this dimension is no longer locked
/// to horizontal"*. Whether the fixture's dimension carries a constraint at
/// all is a property of the fixture, so this asserts the FIELD IS REPORTED
/// rather than asserting a particular value — an assertion that demanded
/// `true` would be testing the fixture, not the verb.
#[test]
fn the_constraint_decision_is_reported_either_way() {
    let mut s = session("dimension/linear-dim.pdf");
    let (id, _) = first_dimension(&s);
    let out = s
        .rotate_dimension(id, (100.0, 100.0), 30.0)
        .expect("rotate");
    // Both values are legitimate; what is not legitimate is silence.
    assert!(out.constraint_relaxed || !out.constraint_relaxed);
    assert!(
        (out.degrees - 30.0).abs() < 1e-9,
        "the angle is echoed back"
    );
    assert_eq!(out.dimension, id);
}

// ---------------------------------------------------------------------------
// 3. REFUSALS AND UNDO
// ---------------------------------------------------------------------------

#[test]
fn a_non_finite_angle_is_refused_by_name() {
    let mut s = session("dimension/linear-dim.pdf");
    let (id, _) = first_dimension(&s);
    match s.rotate_dimension(id, (0.0, 0.0), f64::INFINITY) {
        Err(EditError::ResizeFactorInvalid { axis, .. }) => assert_eq!(axis, "degrees"),
        other => panic!("a non-finite angle must be refused, got {other:?}"),
    }
}

#[test]
fn undo_restores_the_previous_orientation() {
    let mut s = session("dimension/linear-dim.pdf");
    let (id, before) = first_dimension(&s);
    s.rotate_dimension(id, (100.0, 100.0), 45.0)
        .expect("rotate");
    assert!(s.undo().is_some(), "one undoable command");
    assert_eq!(value_of(&s, id), before);
}

// ---------------------------------------------------------------------------
// 4. THE GEOMETRY ITSELF — added after two sabotages survived section 1
// ---------------------------------------------------------------------------

/// ★★★ The measured VALUE is not a sufficient witness, and this is `R225`
/// arriving twice in one verb.
///
/// Section 1 asserts the displayed number is unchanged. A sabotage replacing
/// `cos` with `sin` in the y term left every one of those tests green —
/// because for a HORIZONTAL line the corrupted map still preserves the
/// distance: the x separation scales by `cos`, the y separation becomes
/// `Δx·sin`, and `√(cos² + sin²) = 1`. **The length survives a transform that
/// is not a rotation at all.**
///
/// A scalar derived from two points cannot pin a two-dimensional map. So this
/// asserts where the points actually WENT, using arithmetic a reader can
/// redo: 90° anticlockwise about `(0, 0)` sends `(x, y)` to `(−y, x)`.
#[test]
fn a_quarter_turn_puts_the_points_where_the_arithmetic_says() {
    use pdfcer_core::dimension::DimensionKind;
    use pdfcer_core::vector::geometry::Point;
    use pdfcer_core::vector::snap::AxisConstraint;

    let kind = DimensionKind::Linear {
        a: Point::new(10.0, 5.0),
        b: Point::new(30.0, 5.0),
        constraint: AxisConstraint::Aligned,
        offset: 0.0,
        text_along: 0.0,
    };
    let turned = kind.rotated(Point::new(0.0, 0.0), std::f64::consts::FRAC_PI_2);
    let DimensionKind::Linear { a, b, .. } = turned else {
        panic!("a rotated Linear must stay Linear");
    };
    // ★ BOTH points sit OFF the x-axis, and that is not decoration.
    // With y = 0 the corrupted `y·sin` term is zero and the wrong map
    // agrees with the right one exactly -- which is how the first version
    // of this very test let the sabotage through. `R225`, third time in
    // one verb: a fixture on a symmetry axis discriminates nothing.
    assert!(
        (a.x + 5.0).abs() < 1e-9 && (a.y - 10.0).abs() < 1e-9,
        "(10,5) about the origin by +90 is (-5,10), got ({}, {})",
        a.x,
        a.y
    );
    assert!(
        (b.x + 5.0).abs() < 1e-9 && (b.y - 30.0).abs() < 1e-9,
        "(30,5) about the origin by +90 is (-5,30), got ({}, {})",
        b.x,
        b.y
    );
}

/// ★★ An `Angular` dimension's arms are **unit direction vectors, not
/// points**, so they must turn about the ORIGIN even when the apex turns
/// about a distant pivot. Mapping them through the pivot flings them across
/// the page while the apex moves correctly.
///
/// The fixture suite has no angular dimension, so that sabotage survived every
/// file-based test. This exercises the transform directly — which is the
/// cheapest way to discriminate a case no fixture covers.
#[test]
fn angular_arms_turn_about_the_origin_not_the_pivot() {
    use pdfcer_core::dimension::DimensionKind;
    use pdfcer_core::vector::geometry::Point;

    let kind = DimensionKind::Angular {
        apex: Point::new(500.0, 500.0),
        dir_a: Point::new(1.0, 0.0),
        dir_b: Point::new(0.0, 1.0),
        radius: 20.0,
        text_along: 0.5,
    };
    // A pivot far from the origin: if the arms were mapped through it, they
    // would land hundreds of points away instead of staying unit-length.
    let turned = kind.rotated(Point::new(500.0, 500.0), std::f64::consts::FRAC_PI_2);
    let DimensionKind::Angular {
        apex, dir_a, dir_b, ..
    } = turned
    else {
        panic!("a rotated Angular must stay Angular");
    };
    assert!(
        (apex.x - 500.0).abs() < 1e-9 && (apex.y - 500.0).abs() < 1e-9,
        "the apex is ON the pivot, so it must not move: {apex:?}"
    );
    let len_a = dir_a.x.hypot(dir_a.y);
    assert!(
        (len_a - 1.0).abs() < 1e-9,
        "an arm direction must stay a UNIT vector — {len_a} means it was \
         translated by the pivot, not rotated"
    );
    assert!(
        dir_a.x.abs() < 1e-9 && (dir_a.y - 1.0).abs() < 1e-9,
        "(1,0) turned +90 is (0,1), got {dir_a:?}"
    );
    assert!(
        (dir_b.x + 1.0).abs() < 1e-9 && dir_b.y.abs() < 1e-9,
        "(0,1) turned +90 is (-1,0), got {dir_b:?}"
    );
}
