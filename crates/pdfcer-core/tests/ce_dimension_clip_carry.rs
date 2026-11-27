//! A ce dimension copied and pasted carries its whole dictionary
//! (`Pass 781.4`): its `/AP` stays the copied bytes when the destination
//! group draws the same and the placement is a translation, and is redrawn
//! and disclosed otherwise.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::annot::AnnotFlags;
use pdfcer_core::dimension::{DEFAULT_GROUP_ID, DimensionId, DimensionKind, ScaleState};
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::vector::{AxisConstraint, CLIP_VERSION, Matrix, ObjectClip, Point};
use std::path::Path;

/// Print (4) + NoZoom (8) + NoRotate (16): a key the model does not
/// describe, so only a carry of the dictionary keeps it.
const FLAGS: u32 = 4 | 8 | 16;

fn hello() -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/hello.pdf");
    EditSession::new(Document::load(&path).expect("load fixture"))
}

/// A horizontal linear ce dimension in the default group, flagged [`FLAGS`].
fn source() -> (EditSession, DimensionId) {
    let mut s = hello();
    let kind = DimensionKind::Linear {
        a: Point::new(100.0, 300.0),
        b: Point::new(250.0, 300.0),
        constraint: AxisConstraint::Horizontal,
        offset: 20.0,
        text_along: 0.0,
        extension_gap: [None; 2],
    };
    let (annot, d) = s.add_dimension(0, DEFAULT_GROUP_ID, kind).unwrap();
    s.set_annotation_flags(annot, AnnotFlags(FLAGS)).unwrap();
    (s, d)
}

fn copy(s: &EditSession, id: DimensionId) -> ObjectClip {
    let annot = annot_of(s, id);
    let page = s.page_slots().unwrap()[0].id;
    let annots = pdfcer_core::annot::page_annotations(&s.graph(), page);
    let index = annots.iter().position(|a| a.id == Some(annot)).unwrap();
    let clip = s.copy_annotations(0, &[index]).unwrap();
    assert_eq!(clip.version, CLIP_VERSION);
    ObjectClip::from_bytes(&clip.to_bytes()).unwrap()
}

fn annot_of(s: &EditSession, id: DimensionId) -> ObjId {
    s.dimension_model().dimension(id).unwrap().annot.unwrap()
}

fn key(s: &EditSession, id: DimensionId, k: &[u8]) -> Option<Object> {
    let Some(Object::Dict(d)) = s.value(annot_of(s, id)) else {
        panic!("annotation dict");
    };
    d.get(k).cloned()
}

/// The `/AP /N` stream's raw bytes, and the object holding it.
fn appearance(s: &EditSession, id: DimensionId) -> (ObjId, Vec<u8>) {
    let Some(Object::Dict(ap)) = key(s, id, b"AP") else {
        panic!("an /AP dictionary");
    };
    let n = ap.get(b"N").and_then(Object::as_reference).unwrap();
    let Some(Object::Stream(stream)) = s.value(n) else {
        panic!("an /N stream");
    };
    (n, s.graph().stream_raw(stream).unwrap().to_vec())
}

fn pasted(s: &EditSession, before: &[DimensionId]) -> DimensionId {
    let ids: Vec<_> = s
        .dimension_model()
        .dimensions()
        .iter()
        .map(|r| r.id)
        .filter(|id| !before.contains(id))
        .collect();
    assert_eq!(ids.len(), 1, "one ce dimension pasted");
    ids[0]
}

/// Would catch: a duplicate rebuilt from the model alone (the flags lost),
/// its `/AP` redrawn when nothing about the drawing changed, the sidecar
/// pointing at a stream the annotation does not show, or the paste taking
/// more than one undo.
#[test]
fn a_same_group_duplicate_keeps_the_appearance_bytes_and_every_key() {
    let (mut s, d) = source();
    let clip = copy(&s, d);
    let depth = s.undo_depth();
    let out = s
        .paste_objects(0, &clip, Matrix::translate(10.0, -10.0))
        .unwrap();
    assert_eq!(s.undo_depth(), depth + 1, "the paste is one undo entry");
    let new = pasted(&s, &[d]);

    assert_eq!(key(&s, new, b"F"), Some(Object::Integer(i64::from(FLAGS))));
    let (src_ap, src_bytes) = appearance(&s, d);
    let (new_ap, new_bytes) = appearance(&s, new);
    assert_ne!(src_ap, new_ap);
    assert_eq!(new_bytes, src_bytes, "the /AP bytes are the copied ones");
    assert_eq!(
        s.dimension_model().dimension(new).unwrap().ap,
        Some(new_ap),
        "the sidecar names the stream the annotation shows"
    );
    assert_eq!(key(&s, new, b"Contents"), key(&s, d, b"Contents"));
    assert!(
        !out.disclosures.iter().any(|m| m.contains("REDREW")),
        "{:?}",
        out.disclosures
    );

    s.undo().unwrap();
    assert_eq!(
        s.dimension_model().dimensions().len(),
        1,
        "undo removes the paste"
    );
}

/// Would catch: a clip pasted into a same-named group that measures at
/// another scale keeping the copied label (a wrong number on the drawing),
/// or the redraw going undisclosed, or the carried keys lost with it.
#[test]
fn a_group_with_another_scale_redraws_and_discloses_and_keeps_the_keys() {
    let (mut s, d) = source();
    let format = s.dimension_model().group(DEFAULT_GROUP_ID).unwrap().format;
    s.set_group_scale(
        DEFAULT_GROUP_ID,
        ScaleState::Calibrated { scale: 2.0 },
        format,
    )
    .unwrap();
    let clip = copy(&s, d);

    let mut dest = hello();
    let out = dest.paste_objects(0, &clip, Matrix::IDENTITY).unwrap();
    let new = pasted(&dest, &[]);
    assert_eq!(
        key(&dest, new, b"F"),
        Some(Object::Integer(i64::from(FLAGS)))
    );
    assert_ne!(appearance(&dest, new).1, appearance(&s, d).1);
    assert_ne!(key(&dest, new, b"Contents"), key(&s, d, b"Contents"));
    assert!(
        out.disclosures.iter().any(|m| m.contains("REDREW")),
        "{:?}",
        out.disclosures
    );
}

/// Would catch: a rotated paste planting the copied, unrotated `/AP`, or
/// the redraw losing the carried keys.
#[test]
fn a_rotated_paste_redraws_and_keeps_the_keys() {
    let (mut s, d) = source();
    let clip = copy(&s, d);
    let at = Matrix::rotate(std::f64::consts::FRAC_PI_2).about(Point::new(175.0, 300.0));
    let out = s.paste_objects(0, &clip, at).unwrap();
    let new = pasted(&s, &[d]);
    assert_eq!(key(&s, new, b"F"), Some(Object::Integer(i64::from(FLAGS))));
    assert_ne!(appearance(&s, new).1, appearance(&s, d).1);
    assert!(
        out.disclosures.iter().any(|m| m.contains("REDREW")),
        "{:?}",
        out.disclosures
    );
}
