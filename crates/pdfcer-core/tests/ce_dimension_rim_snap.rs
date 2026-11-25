//! Snap targets on the rim of a ce dimension's circle (pdfcer-gui request
//! G328): exact nearest point, quadrants, tangents, and the ce dimension each
//! one names.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::dimension::{
    CentreMarkSpec, DEFAULT_GROUP_ID, DimensionId, DimensionKind, FitCircle,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::vector::{
    CeDimensionTarget, Point, SnapCandidate, SnapConfig, SnapKind, snap_candidates_with_markups,
};
use std::collections::BTreeSet;
use std::path::Path;

const C: Point = Point::new(300.0, 300.0);
const R: f64 = 50.0;

fn hello() -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/hello.pdf");
    EditSession::new(Document::load(&path).expect("load fixture"))
}

fn fit() -> FitCircle {
    FitCircle {
        center: C,
        radius: R,
        residual: 0.0,
    }
}

fn circular() -> DimensionKind {
    DimensionKind::Circular {
        fit: fit(),
        show_diameter: true,
        leader_angle: 45.0,
        text_distance: None,
        centre_mark: None,
    }
}

fn with(kind: DimensionKind) -> (EditSession, DimensionId) {
    let mut s = hello();
    let (_, id) = s.add_dimension(0, DEFAULT_GROUP_ID, kind).unwrap();
    (s, id)
}

fn near(s: &mut EditSession, q: Point, from: Option<Point>) -> Vec<SnapCandidate> {
    let model = s.page_objects(0).unwrap();
    let all = s
        .page_snap_geometry(0, Some(&model), &BTreeSet::new())
        .unwrap();
    let cfg = SnapConfig::new(3.0).with_axes(false).with_from(from);
    snap_candidates_with_markups(q, &cfg, Some(&model), &all)
}

fn of(c: &[SnapCandidate], kind: SnapKind) -> Vec<SnapCandidate> {
    c.iter().filter(|c| c.kind == kind).copied().collect()
}

/// Would catch: the nearest rim point taken from a chord ring, which lands up
/// to a sagitta inside the circle.
#[test]
fn the_nearest_rim_point_lies_exactly_on_the_circle() {
    let (mut s, id) = with(circular());
    // 200 degrees: away from the leader at 45 and from every quadrant.
    let t = 200f64.to_radians();
    let q = Point::new(C.x + (R + 1.0) * t.cos(), C.y + (R + 1.0) * t.sin());
    let rim = of(&near(&mut s, q, None), SnapKind::RimNearest);
    let hit = rim.first().expect("a rim target");
    assert!((hit.point.distance(C) - R).abs() < 1e-9, "{hit:?}");
    assert!((hit.point.x - (C.x + R * t.cos())).abs() < 1e-9);
    assert_eq!(hit.ce_dimension, Some(CeDimensionTarget::new(id, true)));
}

#[test]
fn the_four_quadrants_are_named_quadrant_in_page_axes() {
    let (mut s, id) = with(circular());
    for (x, y) in [(R, 0.0), (0.0, R), (-R, 0.0), (0.0, -R)] {
        let p = Point::new(C.x + x, C.y + y);
        let c = near(&mut s, Point::new(p.x + 0.4, p.y - 0.4), None);
        assert_eq!(c[0].kind, SnapKind::Quadrant, "{p:?} {c:?}");
        assert!(c[0].point.distance(p) < 1e-9);
        assert_eq!(c[0].ce_dimension.map(|t| t.id), Some(id));
    }
}

/// Tangents from (400, 300) to a radius-50 circle at (300, 300) touch it at
/// 60 degrees either side of the line to the point: (325, 300 ± 43.30).
#[test]
fn tangent_points_come_from_the_previous_pick_outside_the_circle() {
    let (mut s, _) = with(circular());
    let from = Point::new(400.0, 300.0);
    let h = R * 60f64.to_radians().sin();
    for y in [C.y + h, C.y - h] {
        let p = Point::new(325.0, y);
        let c = near(&mut s, Point::new(p.x + 0.5, p.y + 0.5), Some(from));
        let tan = of(&c, SnapKind::Tangent);
        assert_eq!(tan.len(), 1, "{c:?}");
        assert!(tan[0].point.distance(p) < 1e-9, "{tan:?}");
        // At a right angle to the radius, which is what a tangent is.
        let (rx, ry) = (tan[0].point.x - C.x, tan[0].point.y - C.y);
        let (lx, ly) = (from.x - tan[0].point.x, from.y - tan[0].point.y);
        assert!((rx * lx + ry * ly).abs() < 1e-6);
    }
}

#[test]
fn no_tangent_without_a_previous_pick_or_from_inside_the_circle() {
    let (mut s, _) = with(circular());
    let q = Point::new(325.5, 343.8);
    assert!(of(&near(&mut s, q, None), SnapKind::Tangent).is_empty());
    let inside = Some(Point::new(310.0, 300.0));
    assert!(of(&near(&mut s, q, inside), SnapKind::Tangent).is_empty());
    let on_rim = Some(Point::new(C.x + R, C.y));
    assert!(of(&near(&mut s, q, on_rim), SnapKind::Tangent).is_empty());
}

/// Would catch: a centre mark's fitted circle, which is not drawn, offered as
/// if it were.
#[test]
fn a_centre_marks_fitted_circle_snaps_but_is_tagged_not_drawn() {
    let (mut s, id) = with(DimensionKind::CentreMark {
        fit: fit(),
        mark: CentreMarkSpec::default(),
    });
    let t = 200f64.to_radians();
    let q = Point::new(C.x + (R + 1.0) * t.cos(), C.y + (R + 1.0) * t.sin());
    let c = near(&mut s, q, None);
    let rim = of(&c, SnapKind::RimNearest);
    assert_eq!(rim[0].ce_dimension, Some(CeDimensionTarget::new(id, false)));
    // Its drawn centre is still drawn.
    let c = near(&mut s, Point::new(C.x + 0.5, C.y), None);
    let centre = of(&c, SnapKind::Center);
    assert_eq!(
        centre[0].ce_dimension,
        Some(CeDimensionTarget::new(id, true))
    );
}

#[test]
fn a_quadrant_outranks_a_tangent_which_outranks_a_midpoint() {
    let order = |k: SnapKind| SnapKind::all().iter().position(|x| *x == k).unwrap();
    assert!(order(SnapKind::Center) < order(SnapKind::Quadrant));
    assert!(order(SnapKind::Quadrant) < order(SnapKind::Tangent));
    assert!(order(SnapKind::Tangent) < order(SnapKind::Midpoint));
    assert!(order(SnapKind::SegmentCenterline) < order(SnapKind::RimNearest));
    assert!(order(SnapKind::RimNearest) < order(SnapKind::Axis));
}

/// `CeDimensionTarget` is `#[non_exhaustive]`: a consumer builds one through
/// `new` and reads both fields.
#[test]
fn a_ce_dimension_target_is_built_with_new_outside_the_crate() {
    let t = CeDimensionTarget::new(DimensionId(9), true);
    assert_eq!(t.id, DimensionId(9));
    assert!(t.drawn);
    let cfg = SnapConfig::new(1.0).with_from(Some(Point::new(1.0, 2.0)));
    assert_eq!(cfg.from, Some(Point::new(1.0, 2.0)));
    assert_eq!(SnapConfig::new(1.0).from, None);
}
