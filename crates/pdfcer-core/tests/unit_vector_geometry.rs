//! Tests for `pdfcer_core::vector::geometry`, run against its public API.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::vector::geometry::*;

fn approx(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
}

#[test]
fn identity_maps_a_point_to_itself() {
    let p = Point::new(3.5, -7.25);
    assert!(approx(Matrix::IDENTITY.map_point(p), p));
}

#[test]
fn map_point_uses_the_row_vector_formula() {
    // Scale 2 in x, 3 in y, translate (10, 20).
    let m = Matrix::new(2.0, 0.0, 0.0, 3.0, 10.0, 20.0);
    assert!(approx(
        m.map_point(Point::new(1.0, 1.0)),
        Point::new(12.0, 23.0)
    ));
}

#[test]
fn post_concat_is_apply_self_then_other() {
    // A: translate by (5, 0). B: scale x by 2.
    let a = Matrix::translate(5.0, 0.0);
    let b = Matrix::new(2.0, 0.0, 0.0, 1.0, 0.0, 0.0);
    let composed = a.post_concat(b);
    let p = Point::new(1.0, 0.0);
    // apply A then B: (1,0) -> (6,0) -> (12,0)
    assert!(approx(composed.map_point(p), Point::new(12.0, 0.0)));
    // and it equals b.map(a.map(p)) by definition
    assert!(approx(composed.map_point(p), b.map_point(a.map_point(p))));
}

#[test]
fn post_concat_matches_pdf_cm_premultiply_semantics() {
    // A 90° rotation composed with a translation, verifying the same
    // orientation the render interpreter's `m.post_concat(ctm)` uses:
    // rotating (1,0) by +90° gives (0,1); then translate (0,10).
    let rot = Matrix::new(0.0, 1.0, -1.0, 0.0, 0.0, 0.0);
    let tr = Matrix::translate(0.0, 10.0);
    let ctm = rot.post_concat(tr);
    assert!(approx(
        ctm.map_point(Point::new(1.0, 0.0)),
        Point::new(0.0, 11.0)
    ));
}

#[test]
fn bounds_accumulate_and_ignore_non_finite() {
    let b = Bounds::EMPTY
        .union_point(Point::new(1.0, 2.0))
        .union_point(Point::new(-3.0, 5.0))
        .union_point(Point::new(f64::NAN, 0.0)); // ignored
    assert_eq!(b.min, Point::new(-3.0, 2.0));
    assert_eq!(b.max, Point::new(1.0, 5.0));
}

#[test]
fn bounds_containment_and_intersection() {
    let outer = Bounds {
        min: Point::new(0.0, 0.0),
        max: Point::new(10.0, 10.0),
    };
    let inner = Bounds {
        min: Point::new(2.0, 2.0),
        max: Point::new(4.0, 4.0),
    };
    let straddle = Bounds {
        min: Point::new(8.0, 8.0),
        max: Point::new(12.0, 12.0),
    };
    assert!(inner.contained_by(outer));
    assert!(!straddle.contained_by(outer));
    assert!(straddle.intersects(outer));
    assert!(outer.contains(Point::new(5.0, 5.0)));
    assert!(!outer.contains(Point::new(11.0, 5.0)));
}

#[test]
fn v_operator_first_control_is_the_current_point() {
    let cur = Point::new(3.0, 4.0);
    let (c1, c2, end) = cubic_from_v(cur, 10.0, 11.0, 20.0, 21.0);
    assert_eq!(c1, cur);
    assert_eq!(c2, Point::new(10.0, 11.0));
    assert_eq!(end, Point::new(20.0, 21.0));
}

#[test]
fn y_operator_second_control_is_the_endpoint() {
    let (c1, c2, end) = cubic_from_y(10.0, 11.0, 20.0, 21.0);
    assert_eq!(c1, Point::new(10.0, 11.0));
    assert_eq!(c2, Point::new(20.0, 21.0));
    assert_eq!(end, Point::new(20.0, 21.0));
    assert_eq!(c2, end);
}

#[test]
fn inverse_undoes_map_point_for_a_rotate_scale_translate() {
    // A non-trivial affine: scale (2,3), 30° shear-ish, translate (7,-4).
    let m = Matrix::new(2.0, 0.5, -0.5, 3.0, 7.0, -4.0);
    let inv = m.inverse().expect("non-singular");
    for p in [
        Point::new(0.0, 0.0),
        Point::new(1.0, 0.0),
        Point::new(-3.5, 12.25),
    ] {
        let round = inv.map_point(m.map_point(p));
        assert!(approx(round, p), "inverse must undo map_point: {round:?}");
    }
}

#[test]
fn map_vector_ignores_translation_and_matches_a_delta() {
    // A pure translation has identity linear part, so a delta is unchanged.
    let t = Matrix::translate(100.0, -50.0);
    assert!(approx(
        t.map_vector(Point::new(3.0, 4.0)),
        Point::new(3.0, 4.0)
    ));
    // Under a 2× scale a page-space delta of (10,10) is a user-space delta
    // of (5,5): inverse().map_vector recovers it.
    let m = Matrix::new(2.0, 0.0, 0.0, 2.0, 30.0, 30.0);
    let user_delta = m.inverse().unwrap().map_vector(Point::new(10.0, 10.0));
    assert!(approx(user_delta, Point::new(5.0, 5.0)));
}

#[test]
fn a_singular_matrix_has_no_inverse() {
    // Rank-deficient (both rows collinear): determinant 0.
    assert!(
        Matrix::new(1.0, 2.0, 2.0, 4.0, 0.0, 0.0)
            .inverse()
            .is_none()
    );
    // Non-finite operands never yield an inverse.
    assert!(
        Matrix::new(f64::NAN, 0.0, 0.0, 1.0, 0.0, 0.0)
            .inverse()
            .is_none()
    );
}

#[test]
fn re_corners_follow_the_spec_expansion_order() {
    let c = rect_corners(1.0, 2.0, 4.0, 3.0);
    assert_eq!(c[0], Point::new(1.0, 2.0));
    assert_eq!(c[1], Point::new(5.0, 2.0));
    assert_eq!(c[2], Point::new(5.0, 5.0));
    assert_eq!(c[3], Point::new(1.0, 5.0));
}

// -----------------------------------------------------------------
// Pass 112.0 — the scale/rotate/about constructors
//
// These are the primitives every transform verb is built on, and each
// one has a failure mode that produces a PLAUSIBLE WRONG PICTURE rather
// than an error: a sign flip is a mirror image, a composition-order
// slip is a shape that drifts a little on every drag. So the properties
// are pinned, not the coefficients.
// -----------------------------------------------------------------

fn close(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9
}

/// Rotation is COUNTER-CLOCKWISE. A sign error on `c` is a mirror image,
/// which renders perfectly and is wrong, so the direction is pinned at
/// all four quarter turns rather than one.
#[test]
fn rotation_is_counter_clockwise() {
    use std::f64::consts::{FRAC_PI_2, PI};
    let x = Point::new(1.0, 0.0);
    assert!(close(
        Matrix::rotate(FRAC_PI_2).map_point(x),
        Point::new(0.0, 1.0)
    ));
    assert!(close(
        Matrix::rotate(PI).map_point(x),
        Point::new(-1.0, 0.0)
    ));
    assert!(close(
        Matrix::rotate(3.0 * FRAC_PI_2).map_point(x),
        Point::new(0.0, -1.0)
    ));
    assert!(close(Matrix::rotate(2.0 * PI).map_point(x), x));
}

/// The defining property of `about`: the pivot does not move. It holds
/// for rotation, uniform scale, non-uniform scale and a composition of
/// them, and it is the one assertion that catches a reversed
/// `post_concat` order — which otherwise looks like a small drift.
#[test]
fn about_fixes_its_pivot_for_every_transform() {
    let pivot = Point::new(37.5, -12.25);
    for m in [
        Matrix::rotate(0.7),
        Matrix::scale(3.0, 3.0),
        Matrix::scale(2.0, 0.5),
        Matrix::rotate(-1.3).post_concat(Matrix::scale(1.5, 4.0)),
    ] {
        let moved = m.about(pivot).map_point(pivot);
        assert!(close(moved, pivot), "pivot moved to {moved:?} under {m:?}");
    }
}

/// `about` must agree with the long-hand it is shorthand for. Written as
/// an independent reference implementation rather than a restatement:
/// asserting `about` against its own formula would prove nothing.
#[test]
fn about_agrees_with_translate_transform_translate_back() {
    let pivot = Point::new(-4.0, 9.0);
    let m = Matrix::rotate(0.4).post_concat(Matrix::scale(2.0, 3.0));
    for p in [
        Point::new(0.0, 0.0),
        Point::new(1.0, 0.0),
        Point::new(-7.5, 22.0),
        pivot,
    ] {
        let via_about = m.about(pivot).map_point(p);
        // The long-hand, done in three separate steps on the POINT.
        let shifted = Point::new(p.x - pivot.x, p.y - pivot.y);
        let turned = m.map_point(shifted);
        let back = Point::new(turned.x + pivot.x, turned.y + pivot.y);
        assert!(close(via_about, back), "{p:?}: {via_about:?} vs {back:?}");
    }
}

/// A scale about a pivot moves every OTHER point by the factor, measured
/// from the pivot — the property a resize grip actually relies on.
#[test]
fn scaling_about_a_pivot_scales_distance_from_it() {
    let pivot = Point::new(100.0, 100.0);
    let m = Matrix::scale(3.0, 3.0).about(pivot);
    let p = m.map_point(Point::new(110.0, 100.0));
    assert!(close(p, Point::new(130.0, 100.0)), "got {p:?}");
}

/// Non-uniform scale must not secretly rotate: a horizontal edge stays
/// horizontal.
#[test]
fn non_uniform_scale_does_not_rotate() {
    let m = Matrix::scale(4.0, 0.25);
    let a = m.map_point(Point::new(0.0, 5.0));
    let b = m.map_point(Point::new(10.0, 5.0));
    assert!((a.y - b.y).abs() < 1e-9, "the edge tilted: {a:?} {b:?}");
    assert!((b.x - a.x - 40.0).abs() < 1e-9);
}

/// `is_invertible` answers the question a shell needs BEFORE it offers
/// a resize grip, and separately from "this object has no placement".
/// The drag-through-zero case is the one the consuming shell named.
#[test]
fn is_invertible_refuses_exactly_the_degenerate_matrices() {
    assert!(Matrix::IDENTITY.is_invertible());
    assert!(Matrix::rotate(1.0).is_invertible());
    assert!(Matrix::scale(2.0, 0.5).is_invertible());
    assert!(Matrix::translate(10.0, -3.0).is_invertible());

    // Dragged through zero — the case that must be a NAMED refusal.
    assert!(!Matrix::scale(0.0, 1.0).is_invertible());
    assert!(!Matrix::scale(1.0, 0.0).is_invertible());
    assert!(!Matrix::scale(0.0, 0.0).is_invertible());
    // Collapsed onto a line by a shear.
    assert!(!Matrix::new(1.0, 2.0, 2.0, 4.0, 0.0, 0.0).is_invertible());
    // Non-finite must never reach a `cm` operand.
    assert!(!Matrix::scale(f64::NAN, 1.0).is_invertible());
    assert!(!Matrix::scale(f64::INFINITY, 1.0).is_invertible());
    assert!(!Matrix::new(1.0, 0.0, 0.0, 1.0, f64::NAN, 0.0).is_invertible());
}

/// `is_invertible` must agree with `inverse` — two answers to one
/// question that could drift apart otherwise.
#[test]
fn is_invertible_agrees_with_inverse() {
    for m in [
        Matrix::IDENTITY,
        Matrix::rotate(0.3),
        Matrix::scale(2.0, 3.0),
        Matrix::scale(0.0, 1.0),
        Matrix::new(1.0, 2.0, 2.0, 4.0, 0.0, 0.0),
        Matrix::scale(f64::NAN, 1.0),
    ] {
        assert_eq!(
            m.is_invertible(),
            m.inverse().is_some(),
            "the predicate and the operation disagree about {m:?}"
        );
    }
}
