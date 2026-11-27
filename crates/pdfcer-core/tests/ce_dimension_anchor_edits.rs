//! Associative anchors through the edits that do not name the target:
//! page transforms, copy and paste (objects and pages), and a centre-mark
//! change. Each either keeps the ce dimension following its target or
//! detaches it with a report, in the same undo entry as the edit.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::dimension::{
    Anchor, AnchorRole, AnchorState, CentreMarkSpec, DEFAULT_GROUP_ID, DimensionId, DimensionKind,
    FitCircle,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{CropBoxEdit, CropFollow, EditError, EditSession, PageBox, PageBoxEdit};
use pdfcer_core::page_tree;
use pdfcer_core::pageops::{InsertPosition, ScaleRequest};
use pdfcer_core::vector::{
    AxisConstraint, CLIP_VERSION, ClipAnnotation, Matrix, ObjectClip, Point,
};
use std::path::Path;

const C: Point = Point::new(300.0, 300.0);

fn hello() -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/hello.pdf");
    EditSession::new(Document::load(&path).expect("load fixture"))
}

fn fit() -> FitCircle {
    FitCircle {
        center: C,
        radius: 50.0,
        residual: 0.0,
    }
}

fn mark() -> DimensionKind {
    DimensionKind::CentreMark {
        fit: fit(),
        mark: CentreMarkSpec::default(),
    }
}

fn circle() -> DimensionKind {
    DimensionKind::Circular {
        fit: fit(),
        show_diameter: false,
        show_circle: true,
        leader_angle: 30.0,
        text_distance: None,
        centre_mark: Some(CentreMarkSpec::default()),
    }
}

/// `target`, then a linear ce dimension from (100, 300) whose second end is
/// anchored to the target's centre.
fn anchored(target: DimensionKind) -> (EditSession, DimensionId, DimensionId) {
    let mut s = hello();
    let (_, t) = s.add_dimension(0, DEFAULT_GROUP_ID, target).unwrap();
    let kind = DimensionKind::Linear {
        a: Point::new(100.0, 300.0),
        b: Point::new(250.0, 300.0),
        constraint: AxisConstraint::Aligned,
        offset: 0.0,
        text_along: 0.0,
        extension_gap: [None; 2],
    };
    let (_, d) = s.add_dimension(0, DEFAULT_GROUP_ID, kind).unwrap();
    s.set_dimension_anchor(d, 1, Some(Anchor::new(t, AnchorRole::MarkCentre)))
        .unwrap();
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

/// Whether `d` is anchored to `t` and solved.
fn follows(s: &EditSession, d: DimensionId, t: DimensionId) -> bool {
    s.dimension_anchors(d)
        .iter()
        .any(|(_, a, st)| a.target == t && *st == AnchorState::Solved)
}

/// The page-annotation index of each ce dimension in `ids`, page 0.
fn indices(s: &EditSession, ids: &[DimensionId]) -> Vec<usize> {
    let model = s.dimension_model();
    let page = s.page_slots().unwrap()[0].id;
    let annots = pdfcer_core::annot::page_annotations(&s.graph(), page);
    ids.iter()
        .map(|id| {
            let annot = model.dimension(*id).unwrap().annot;
            annots.iter().position(|a| a.id == annot).unwrap()
        })
        .collect()
}

/// The ce dimensions in the model that were not in `before`.
fn new_ids(s: &EditSession, before: &[DimensionId]) -> Vec<DimensionId> {
    s.dimension_model()
        .dimensions()
        .iter()
        .map(|r| r.id)
        .filter(|id| !before.contains(id))
        .collect()
}

/// Would catch: a page rotate, crop, media resize or box edit dropping or
/// breaking an anchor. None of them changes user space, so the dependent
/// must still be on the target and still follow a later move.
#[test]
fn page_rotate_crop_and_resize_keep_the_anchor_following() {
    let (mut s, t, d) = anchored(mark());
    s.rotate_pages(&[0], 90).unwrap();
    s.set_page_rotation(0, 180).unwrap();
    let crop = page_tree::Rect::from_corners(50.0, 50.0, 500.0, 700.0);
    s.set_page_boxes(&[PageBoxEdit::new(0, PageBox::Crop, CropBoxEdit::Set(crop))])
        .unwrap();
    let sheet = page_tree::Rect::from_corners(0.0, 0.0, 842.0, 1191.0);
    s.resize_pages(&[0], sheet, CropFollow::Always).unwrap();
    assert!(follows(&s, d, t));
    assert!(close(point(&s, d, 1), C));
    s.move_dimension(t, 30.0, 0.0).unwrap();
    assert!(close(point(&s, d, 1), Point::new(330.0, 300.0)));
    assert_eq!(s.last_anchor_resolutions().len(), 1);
}

/// Would catch: a scale route that rewrote a target's geometry without its
/// dependent. A page carrying ce dimensions is refused before any write.
#[test]
fn page_scale_refuses_a_page_carrying_anchored_ce_dimensions() {
    let (mut s, t, d) = anchored(mark());
    let depth = s.undo_depth();
    let err = s
        .scale_pages(&[0], &ScaleRequest::new(842.0, 1191.0))
        .unwrap_err();
    assert!(matches!(err, EditError::ScaleRefusedCeDimensions { .. }));
    assert_eq!(s.undo_depth(), depth);
    assert!(follows(&s, d, t));
}

/// Would catch: a pasted dependent left pointing at the ORIGINAL target
/// (so moving the copy's target leaves the copy behind), or the re-point
/// landing in a second undo entry.
#[test]
fn pasting_target_and_dependent_together_re_points_the_anchor() {
    let (mut s, t, d) = anchored(mark());
    let before = vec![t, d];
    let clip = s.copy_annotations(0, &indices(&s, &[t, d])).unwrap();
    assert_eq!(clip.version, CLIP_VERSION);
    let depth = s.undo_depth();
    let out = s
        .paste_objects(0, &clip, Matrix::translate(0.0, 100.0))
        .unwrap();
    assert_eq!(out.annotations_pasted, 2);
    assert_eq!(s.undo_depth(), depth + 1, "the paste is one undo entry");

    let pasted = new_ids(&s, &before);
    let model = s.dimension_model();
    let new_d = *pasted
        .iter()
        .find(|id| !model.dimension(**id).unwrap().anchors.is_empty())
        .expect("the pasted dependent");
    let new_t = *pasted.iter().find(|id| **id != new_d).unwrap();
    assert!(follows(&s, new_d, new_t));
    assert!(close(point(&s, new_d, 1), Point::new(300.0, 400.0)));
    let report = s.last_anchor_resolutions();
    assert_eq!(report.len(), 1);
    assert_eq!(report[0].id, new_d);
    assert!(!report[0].detached);

    s.move_dimension(new_t, 20.0, 0.0).unwrap();
    assert!(close(point(&s, new_d, 1), Point::new(320.0, 400.0)));
    assert!(close(point(&s, d, 1), C), "the original is not touched");

    s.undo().unwrap();
    s.undo().unwrap();
    assert!(
        new_ids(&s, &before).is_empty(),
        "undo removes the whole paste"
    );
    assert!(follows(&s, d, t));
}

/// Would catch: a dependent pasted without its target keeping an anchor to
/// the original (which would drag it back onto the source on the next
/// move), or the drop going undisclosed.
#[test]
fn pasting_only_the_dependent_drops_the_anchor_and_discloses_it() {
    let (mut s, t, d) = anchored(mark());
    let clip = s.copy_annotations(0, &indices(&s, &[d])).unwrap();
    let depth = s.undo_depth();
    let out = s
        .paste_objects(0, &clip, Matrix::translate(0.0, 100.0))
        .unwrap();
    assert_eq!(s.undo_depth(), depth + 1);
    let pasted = new_ids(&s, &[t, d]);
    assert_eq!(pasted.len(), 1);
    assert!(s.dimension_anchors(pasted[0]).is_empty());
    assert!(close(point(&s, pasted[0], 1), Point::new(300.0, 400.0)));
    let report = s.last_anchor_resolutions();
    assert_eq!(report.len(), 1);
    assert!(report[0].detached);
    assert!(
        out.disclosures.iter().any(|m| m.contains("anchor")),
        "{:?}",
        out.disclosures
    );
    s.undo().unwrap();
    assert!(new_ids(&s, &[t, d]).is_empty());
}

/// Would catch: the anchor carry lost on the wire, or a ce dimension clip
/// written below the version whose reader understands it.
#[test]
fn the_anchor_carry_survives_the_wire_and_every_ce_dimension_clip_needs_v7() {
    let (s, t, d) = anchored(mark());
    let clip = s.copy_annotations(0, &indices(&s, &[t, d])).unwrap();
    let back = ObjectClip::from_bytes(&clip.to_bytes()).unwrap();
    assert_eq!(back.annotations, clip.annotations);
    let carried = back.annotations.iter().find_map(|a| match a {
        ClipAnnotation::Dimension {
            source, anchors, ..
        } if !anchors.is_empty() => Some((*source, anchors.clone())),
        _ => None,
    });
    let (source, anchors) = carried.expect("the dependent's carry");
    assert_eq!(source, Some(d));
    assert_eq!(anchors, vec![(1, Anchor::new(t, AnchorRole::MarkCentre))]);

    let target_only = s.copy_annotations(0, &indices(&s, &[t])).unwrap();
    assert_eq!(target_only.version, CLIP_VERSION);
    let back = ObjectClip::from_bytes(&target_only.to_bytes()).unwrap();
    assert_eq!(back.annotations, target_only.annotations, "lossless at v7");
}

/// Would catch: a page copy and paste losing the anchor or leaving it on
/// the source page's target.
#[test]
fn a_page_copy_and_paste_carries_the_anchor_to_the_pasted_target() {
    let (mut s, t, d) = anchored(mark());
    let clip = s.copy_pages(&[0]).unwrap();
    s.paste_pages(&clip, InsertPosition::End).unwrap();
    let pasted = new_ids(&s, &[t, d]);
    assert_eq!(pasted.len(), 2);
    let model = s.dimension_model();
    let new_d = *pasted
        .iter()
        .find(|id| !model.dimension(**id).unwrap().anchors.is_empty())
        .unwrap();
    let new_t = *pasted.iter().find(|id| **id != new_d).unwrap();
    assert!(follows(&s, new_d, new_t));
    assert!(follows(&s, d, t));
}

/// Would catch: a centre-mark change that re-solves nothing and reports
/// nothing, or that loses the anchor when a radius/diameter mark is cleared.
#[test]
fn clearing_or_changing_a_centre_mark_re_solves_its_dependents() {
    let (mut s, t, d) = anchored(circle());
    let depth = s.undo_depth();
    s.set_centre_mark(t, None).unwrap();
    assert_eq!(s.undo_depth(), depth + 1);
    assert!(follows(&s, d, t));
    assert!(close(point(&s, d, 1), C));
    let report = s.last_anchor_resolutions();
    assert_eq!(report.len(), 1);
    assert_eq!(report[0].id, d);
    assert!(!report[0].detached);
    s.undo().unwrap();
    assert!(s.centre_mark_host(t).unwrap().mark().is_some());
    assert!(follows(&s, d, t));

    let (mut s, t, d) = anchored(mark());
    let turned = CentreMarkSpec::default().turned(45.0);
    s.set_centre_mark(t, Some(turned)).unwrap();
    assert!(follows(&s, d, t));
    assert_eq!(s.last_anchor_resolutions().len(), 1);
    s.undo().unwrap();
    assert_eq!(
        s.centre_mark_host(t).unwrap().mark(),
        Some(CentreMarkSpec::default())
    );
}

/// Flattening a ce dimension's annotation burns it into the page, so it
/// leaves the ce dimension model; a ce dimension anchored to it detaches
/// and keeps its position. Undo restores both.
#[test]
fn flattening_a_target_detaches_its_dependents() {
    let (mut s, t, d) = anchored(mark());
    let annot = s.dimension_model().dimension(t).unwrap().annot.unwrap();
    let held = point(&s, d, 1);
    let out = s.flatten_annotations(0, Some(&[annot])).unwrap();
    assert_eq!(out.ce_dimensions_flattened, 1);
    assert_eq!(out.ce_dimension_anchors_detached, 1);
    assert!(
        out.disclosures
            .iter()
            .any(|m| m.contains("detached ce dimension"))
    );
    let m = s.dimension_model();
    assert!(
        m.dimension(t).is_none(),
        "a flattened ce dimension left the model"
    );
    assert!(s.dimension_anchors(d).is_empty());
    assert_eq!(point(&s, d, 1), held);
    s.undo().unwrap();
    assert!(s.dimension_model().dimension(t).is_some());
    assert_eq!(s.dimension_anchors(d).len(), 1);
}

/// Flattening both leaves nothing to detach.
#[test]
fn flattening_a_target_with_its_dependent_detaches_nothing() {
    let (mut s, _, _) = anchored(mark());
    let out = s.flatten_annotations(0, None).unwrap();
    assert_eq!(out.ce_dimensions_flattened, 2);
    assert_eq!(out.ce_dimension_anchors_detached, 0);
    assert!(s.dimension_model().dimensions().is_empty());
}
