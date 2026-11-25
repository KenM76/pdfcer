//! Associative anchors (pdfcer-gui request G332): a ce dimension's point
//! follows a centre mark, centre line or circle through every edit of it,
//! in the same undo entry, and is detached, not moved, when it is deleted.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::dimension::{
    Anchor, AnchorOutcome, AnchorRole, AnchorState, ArcCondition, CentreMarkSpec, DEFAULT_GROUP_ID,
    DimensionEnd, DimensionId, DimensionKind, FitCircle, Resolved, Unsolvable, serialize_model,
    sidecar_version,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::object::Object;
use pdfcer_core::pageops::{self, DocumentView};
use pdfcer_core::vector::{AxisConstraint, CeDimensionTarget, Point};
use pdfcer_core::writer::SaveOptions;
use std::path::Path;

const C: Point = Point::new(300.0, 300.0);
const R: f64 = 50.0;

fn hello() -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/hello.pdf");
    EditSession::new(Document::load(&path).expect("load fixture"))
}

fn mark() -> DimensionKind {
    DimensionKind::CentreMark {
        fit: FitCircle {
            center: C,
            radius: R,
            residual: 0.0,
        },
        mark: CentreMarkSpec::default(),
    }
}

fn linear(a: Point, b: Point, constraint: AxisConstraint) -> DimensionKind {
    DimensionKind::Linear {
        a,
        b,
        constraint,
        offset: 0.0,
        text_along: 0.0,
        extension_gap: [None; 2],
    }
}

fn centre_line() -> DimensionKind {
    DimensionKind::CentreLine {
        a: Point::new(400.0, 100.0),
        b: Point::new(400.0, 500.0),
    }
}

/// A session with `target` and a linear ce dimension from `a` to (250, 300).
fn pair(
    target: DimensionKind,
    a: Point,
    c: AxisConstraint,
) -> (EditSession, DimensionId, DimensionId) {
    let mut s = hello();
    let (_, t) = s.add_dimension(0, DEFAULT_GROUP_ID, target).unwrap();
    let kind = linear(a, Point::new(250.0, 300.0), c);
    let (_, d) = s.add_dimension(0, DEFAULT_GROUP_ID, kind).unwrap();
    (s, t, d)
}

fn point(s: &EditSession, id: DimensionId, i: usize) -> Point {
    s.dimension_model()
        .dimension(id)
        .unwrap()
        .kind
        .anchor_point(i)
        .unwrap()
}

fn close(p: Point, q: Point) -> bool {
    p.distance(q) < 1e-9
}

fn bind(
    s: &mut EditSession,
    d: DimensionId,
    i: usize,
    t: DimensionId,
    role: AnchorRole,
) -> AnchorOutcome {
    s.set_dimension_anchor(d, i, Some(Anchor::new(t, role)))
        .unwrap()
}

fn save(s: &EditSession) -> Vec<u8> {
    s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0
}

/// The decoded normal appearance of `id`'s annotation, as saved.
fn ap(s: &EditSession, id: DimensionId) -> Vec<u8> {
    let annot = s.dimension_model().dimension(id).unwrap().annot.unwrap();
    let doc = Document::from_bytes(save(s)).unwrap();
    let Object::Dict(dict) = doc.get(annot).unwrap().value.clone() else {
        panic!("not a dict");
    };
    let Object::Dict(ap) = doc.resolve(dict.get(b"AP").unwrap()) else {
        panic!("no /AP");
    };
    let Object::Stream(st) = doc.resolve(ap.get(b"N").unwrap()) else {
        panic!("no /N");
    };
    let raw = st.data_span.slice(doc.bytes()).unwrap();
    pdfcer_core::filters::decode_stream(&st.dict, raw).unwrap()
}

#[test]
fn a_centre_anchor_solves_at_once_and_reports_the_new_value() {
    let (mut s, t, d) = pair(mark(), Point::new(100.0, 300.0), AxisConstraint::Aligned);
    let out = bind(&mut s, d, 1, t, AnchorRole::MarkCentre);
    assert!(close(point(&s, d, 1), C));
    assert_eq!(out.state, Some(AnchorState::Solved));
    assert_ne!(out.before_label, out.after_label);
    let anchors = s.dimension_anchors(d);
    assert_eq!(anchors.len(), 1);
    assert_eq!(anchors[0].0, 1);
    assert_eq!(anchors[0].1, Anchor::new(t, AnchorRole::MarkCentre));
    assert_eq!(s.anchor_dependents(t), vec![d]);
}

/// Would catch: a move of the target leaving its dependent measuring
/// empty paper, or the follow landing in a second undo entry.
#[test]
fn moving_the_target_moves_the_dependent_in_one_undo_step() {
    let (mut s, t, d) = pair(mark(), Point::new(100.0, 300.0), AxisConstraint::Aligned);
    bind(&mut s, d, 1, t, AnchorRole::MarkCentre);
    let depth = s.undo_depth();
    let drawn = ap(&s, d);
    s.move_dimension(t, 40.0, 0.0).unwrap();
    assert_eq!(s.undo_depth(), depth + 1);
    assert_ne!(
        ap(&s, d),
        drawn,
        "the dependent is redrawn, not only re-stored"
    );
    assert!(close(point(&s, d, 1), Point::new(340.0, 300.0)));
    let followed = s.last_anchor_resolutions();
    assert_eq!(followed.len(), 1);
    assert_eq!(followed[0].id, d);
    assert!(!followed[0].detached);
    assert_ne!(followed[0].before_label, followed[0].after_label);
    s.undo().unwrap();
    assert!(close(point(&s, d, 1), C));
    assert!(s.last_anchor_resolutions().is_empty());
    s.redo().unwrap();
    assert!(close(point(&s, d, 1), Point::new(340.0, 300.0)));
}

#[test]
fn nearest_and_farthest_rim_points_measure_from_the_other_end() {
    let (mut s, t, d) = pair(mark(), Point::new(100.0, 300.0), AxisConstraint::Aligned);
    bind(&mut s, d, 1, t, AnchorRole::Rim(ArcCondition::Min));
    assert!(close(point(&s, d, 1), Point::new(C.x - R, C.y)));
    bind(&mut s, d, 1, t, AnchorRole::Rim(ArcCondition::Max));
    assert!(close(point(&s, d, 1), Point::new(C.x + R, C.y)));
    bind(&mut s, d, 1, t, AnchorRole::Rim(ArcCondition::Centre));
    assert!(close(point(&s, d, 1), C));
    assert_eq!(s.dimension_anchors(d).len(), 1, "one anchor per point");
}

/// A horizontal ce dimension to a circle measures along x, as a CAD
/// package does: the rim point is level with the centre, not on the line to
/// the other end.
#[test]
fn a_constrained_rim_point_is_taken_along_the_constraint_axis() {
    let (mut s, t, d) = pair(mark(), Point::new(100.0, 200.0), AxisConstraint::Horizontal);
    bind(&mut s, d, 1, t, AnchorRole::Rim(ArcCondition::Min));
    assert!(close(point(&s, d, 1), Point::new(C.x - R, C.y)));
}

#[test]
fn the_other_end_on_the_centre_is_unsolvable_and_keeps_the_point() {
    let (mut s, t, d) = pair(mark(), C, AxisConstraint::Aligned);
    let before = point(&s, d, 1);
    let out = bind(&mut s, d, 1, t, AnchorRole::Rim(ArcCondition::Min));
    assert_eq!(
        out.state,
        Some(AnchorState::Unsolvable(Unsolvable::OtherEndOnCentre))
    );
    assert!(close(point(&s, d, 1), before));
    assert_eq!(
        s.dimension_anchors(d)[0].2,
        AnchorState::Unsolvable(Unsolvable::OtherEndOnCentre)
    );
}

#[test]
fn centre_line_ends_midpoint_and_axis() {
    let (mut s, t, d) = pair(
        centre_line(),
        Point::new(100.0, 250.0),
        AxisConstraint::Aligned,
    );
    bind(&mut s, d, 1, t, AnchorRole::LineEnd(DimensionEnd::A));
    assert!(close(point(&s, d, 1), Point::new(400.0, 100.0)));
    bind(&mut s, d, 1, t, AnchorRole::LineEnd(DimensionEnd::B));
    assert!(close(point(&s, d, 1), Point::new(400.0, 500.0)));
    bind(&mut s, d, 1, t, AnchorRole::LineMidpoint);
    assert!(close(point(&s, d, 1), Point::new(400.0, 300.0)));
    bind(&mut s, d, 1, t, AnchorRole::LineAxis);
    assert!(close(point(&s, d, 1), Point::new(400.0, 250.0)));
}

/// Would catch: a vertex drag or rotation of a centre line re-solving
/// nothing (only `move_dimension` hooked).
#[test]
fn a_vertex_drag_and_a_rotation_of_the_target_are_followed() {
    let (mut s, t, d) = pair(
        centre_line(),
        Point::new(100.0, 250.0),
        AxisConstraint::Aligned,
    );
    bind(&mut s, d, 1, t, AnchorRole::LineEnd(DimensionEnd::A));
    s.move_dimension_vertex(t, 0, 10.0, 20.0).unwrap();
    assert!(close(point(&s, d, 1), Point::new(410.0, 120.0)));
    assert_eq!(s.last_anchor_resolutions().len(), 1);
    s.rotate_dimension(t, (410.0, 300.0), 180.0).unwrap();
    let DimensionKind::CentreLine { a, .. } = s.dimension_model().dimension(t).unwrap().kind else {
        panic!("not a centre line");
    };
    assert!(close(point(&s, d, 1), a), "{a:?}");
    assert_eq!(s.last_anchor_resolutions().len(), 1);
}

/// Dragging an anchored point off its target is the operator detaching it.
#[test]
fn dragging_the_anchored_point_itself_detaches_it() {
    let (mut s, t, d) = pair(mark(), Point::new(100.0, 300.0), AxisConstraint::Aligned);
    bind(&mut s, d, 1, t, AnchorRole::MarkCentre);
    s.move_dimension_vertex(d, 1, 5.0, 0.0).unwrap();
    assert!(close(point(&s, d, 1), Point::new(305.0, 300.0)));
    assert!(s.dimension_anchors(d).is_empty());
    s.move_dimension(t, 40.0, 0.0).unwrap();
    assert!(close(point(&s, d, 1), Point::new(305.0, 300.0)));
}

/// Moving the dependent as a whole keeps its anchored end on the target.
#[test]
fn moving_the_dependent_keeps_its_anchored_end_on_the_target() {
    let (mut s, t, d) = pair(mark(), Point::new(100.0, 300.0), AxisConstraint::Aligned);
    bind(&mut s, d, 1, t, AnchorRole::MarkCentre);
    s.move_dimension(d, 0.0, 30.0).unwrap();
    assert!(close(point(&s, d, 0), Point::new(100.0, 330.0)));
    assert!(close(point(&s, d, 1), C));
}

/// Would catch: a delete refused, a dangling anchor left behind, or the
/// dependent moved.
#[test]
fn deleting_the_target_detaches_and_undo_restores_both() {
    let (mut s, t, d) = pair(mark(), Point::new(100.0, 300.0), AxisConstraint::Aligned);
    bind(&mut s, d, 1, t, AnchorRole::MarkCentre);
    let label = s.dimension_display(d).unwrap().text;
    s.delete_dimension(t).unwrap();
    assert!(close(point(&s, d, 1), C));
    assert!(s.dimension_anchors(d).is_empty());
    assert_eq!(s.dimension_display(d).unwrap().text, label);
    let report = s.last_anchor_resolutions();
    assert_eq!(report.len(), 1);
    assert!(report[0].detached);
    assert_eq!(report[0].after_label, label);
    s.undo().unwrap();
    assert!(s.dimension_model().dimension(t).is_some());
    assert_eq!(s.dimension_anchors(d).len(), 1);
}

#[test]
fn clearing_an_anchor_leaves_the_point() {
    let (mut s, t, d) = pair(mark(), Point::new(100.0, 300.0), AxisConstraint::Aligned);
    bind(&mut s, d, 1, t, AnchorRole::MarkCentre);
    let out = s.set_dimension_anchor(d, 1, None).unwrap();
    assert_eq!(out.state, None);
    assert_eq!(out.before_label, out.after_label);
    assert!(close(point(&s, d, 1), C));
    assert!(s.anchor_dependents(t).is_empty());
}

#[test]
fn refusals_change_nothing() {
    let (mut s, t, d) = pair(mark(), Point::new(100.0, 300.0), AxisConstraint::Aligned);
    let (_, cl) = s.add_dimension(0, DEFAULT_GROUP_ID, centre_line()).unwrap();
    let depth = s.undo_depth();
    let set = |s: &mut EditSession, id, p, a: Anchor| s.set_dimension_anchor(id, p, Some(a));
    let centre = Anchor::new(t, AnchorRole::MarkCentre);
    assert!(matches!(
        set(&mut s, d, 2, centre),
        Err(EditError::AnchorPointOutOfRange {
            point: 2,
            count: 2,
            ..
        })
    ));
    assert!(matches!(
        set(&mut s, d, 1, Anchor::new(cl, AnchorRole::MarkCentre)),
        Err(EditError::AnchorRoleNotOnTarget { .. })
    ));
    assert!(matches!(
        set(
            &mut s,
            d,
            1,
            Anchor::new(DimensionId(99), AnchorRole::MarkCentre)
        ),
        Err(EditError::DimensionNotFound { id: 99 })
    ));
    assert!(matches!(
        set(&mut s, d, 1, Anchor::new(d, AnchorRole::MarkCentre)),
        Err(EditError::AnchorCycle { .. })
    ));
    // A centre mark has no anchorable point of its own.
    assert!(matches!(
        set(&mut s, t, 0, Anchor::new(cl, AnchorRole::LineMidpoint)),
        Err(EditError::AnchorPointOutOfRange { count: 0, .. })
    ));
    assert_eq!(s.undo_depth(), depth);
}

#[test]
fn a_rim_condition_needs_a_linear_dependent() {
    let mut s = hello();
    let (_, t) = s.add_dimension(0, DEFAULT_GROUP_ID, mark()).unwrap();
    let perim = DimensionKind::Perimeter {
        points: vec![Point::new(100.0, 100.0), Point::new(200.0, 100.0)],
        closed: false,
        area: false,
        offset: 0.0,
        text_along: 0.0,
    };
    let (_, d) = s.add_dimension(0, DEFAULT_GROUP_ID, perim).unwrap();
    let rim = Anchor::new(t, AnchorRole::Rim(ArcCondition::Max));
    assert!(matches!(
        s.set_dimension_anchor(d, 0, Some(rim)),
        Err(EditError::AnchorConditionNeedsLinear { .. })
    ));
    let centre = Anchor::new(t, AnchorRole::MarkCentre);
    s.set_dimension_anchor(d, 1, Some(centre)).unwrap();
    assert!(close(point(&s, d, 1), C));
}

/// Authoring and binding are one undo entry, and the ce dimension is
/// placed already solved.
#[test]
fn add_dimension_anchored_is_one_undo_step() {
    let mut s = hello();
    let (_, t) = s.add_dimension(0, DEFAULT_GROUP_ID, mark()).unwrap();
    let depth = s.undo_depth();
    let kind = linear(
        Point::new(100.0, 300.0),
        Point::new(290.0, 300.0),
        AxisConstraint::Aligned,
    );
    let anchors = [(1, Anchor::new(t, AnchorRole::Rim(ArcCondition::Max)))];
    let (_, d) = s
        .add_dimension_anchored(0, DEFAULT_GROUP_ID, kind, &anchors)
        .unwrap();
    assert_eq!(s.undo_depth(), depth + 1);
    assert!(close(point(&s, d, 1), Point::new(C.x + R, C.y)));
    assert_eq!(s.dimension_anchors(d).len(), 1);
    s.undo().unwrap();
    assert!(s.dimension_model().dimension(d).is_none());
    assert!(s.dimension_model().dimension(t).is_some());
}

#[test]
fn add_dimension_anchored_refuses_before_placing() {
    let mut s = hello();
    let (_, t) = s.add_dimension(0, DEFAULT_GROUP_ID, mark()).unwrap();
    let depth = s.undo_depth();
    let kind = linear(
        Point::new(100.0, 300.0),
        Point::new(290.0, 300.0),
        AxisConstraint::Aligned,
    );
    let anchors = [(5, Anchor::new(t, AnchorRole::MarkCentre))];
    assert!(matches!(
        s.add_dimension_anchored(0, DEFAULT_GROUP_ID, kind, &anchors),
        Err(EditError::AnchorPointOutOfRange { .. })
    ));
    assert_eq!(s.undo_depth(), depth);
    assert_eq!(s.dimension_model().dimensions().len(), 1);
}

#[test]
fn anchors_survive_save_at_sidecar_version_11() {
    let (mut s, t, d) = pair(mark(), Point::new(100.0, 300.0), AxisConstraint::Aligned);
    bind(&mut s, d, 1, t, AnchorRole::Rim(ArcCondition::Max));
    let mut back = EditSession::new(Document::from_bytes(save(&s)).unwrap());
    let model = back.dimension_model();
    assert_eq!(sidecar_version(&serialize_model(&model)), Some(11));
    assert_eq!(
        back.dimension_anchors(d),
        vec![(
            1,
            Anchor::new(t, AnchorRole::Rim(ArcCondition::Max)),
            AnchorState::Solved
        )]
    );
    back.move_dimension(t, 0.0, 10.0).unwrap();
    // Farthest from the other end (100, 300), across the moved centre.
    let (c, a) = (Point::new(C.x, C.y + 10.0), Point::new(100.0, 300.0));
    let k = R / a.distance(c);
    let far = Point::new(c.x - k * (a.x - c.x), c.y - k * (a.y - c.y));
    assert!(close(point(&back, d, 1), far));
}

/// Would catch: an unanchored document bumped to version 11, locking an
/// older build out of a file it reads perfectly.
#[test]
fn a_document_without_anchors_stays_below_version_11() {
    let (s, _, _) = pair(mark(), Point::new(100.0, 300.0), AxisConstraint::Aligned);
    let v = sidecar_version(&serialize_model(&s.dimension_model())).unwrap();
    assert!(v < 11, "{v}");
}

/// Would catch: a merged dependent still pointing at the source's ids,
/// which name a different ce dimension in the destination.
#[test]
fn a_merge_renumbers_anchor_targets() {
    let (mut src, t, d) = pair(mark(), Point::new(100.0, 300.0), AxisConstraint::Aligned);
    bind(&mut src, d, 1, t, AnchorRole::MarkCentre);
    let source = Document::from_bytes(save(&src)).unwrap();
    let (mut s, _, _) = pair(centre_line(), Point::new(1.0, 1.0), AxisConstraint::Aligned);
    let view = DocumentView::new(&source, source.bytes(), source.version());
    let out = s
        .merge_document(&view, pageops::InsertPosition::End)
        .unwrap();
    assert_eq!(out.ce_dimension_anchors_detached, 0);
    let model = s.dimension_model();
    let dep = model
        .dimensions()
        .iter()
        .find(|r| !r.anchors.is_empty())
        .expect("the merged dependent");
    let target = model.dimension(dep.anchors[0].1.target).unwrap();
    assert!(matches!(target.kind, DimensionKind::CentreMark { .. }));
    assert_ne!(
        dep.anchors[0].1.target, t,
        "renumbered past the destination's own"
    );
}

/// The `#[non_exhaustive]` anchor types, built the way a consumer must.
#[test]
fn anchor_types_are_built_through_their_constructors_outside_the_crate() {
    let a = Anchor::new(DimensionId(4), AnchorRole::LineAxis);
    assert_eq!((a.target, a.role), (DimensionId(4), AnchorRole::LineAxis));
    for role in AnchorRole::ALL {
        assert_eq!(AnchorRole::parse(role.token()), Some(role));
    }
    assert_eq!(AnchorRole::parse("rim-sideways"), None);
    let r = Resolved::new(DimensionId(2), "1".into(), "2".into());
    assert!(!r.detached);
    let r = Resolved::detached(DimensionId(2), "3".into());
    assert!(r.detached && r.before_label == r.after_label);
    let o = AnchorOutcome::new("a".into(), "b".into(), Some(AnchorState::Solved));
    assert_eq!(o.after_label, "b");
    let t = CeDimensionTarget::new(DimensionId(1), true).with_anchor(AnchorRole::MarkCentre);
    assert_eq!(
        t.anchor,
        Some(Anchor::new(DimensionId(1), AnchorRole::MarkCentre))
    );
    assert!(!Unsolvable::AxisParallel.to_string().is_empty());
}
