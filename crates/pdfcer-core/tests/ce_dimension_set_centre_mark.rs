//! Adding, changing and removing the centre mark of a placed ce dimension
//! (pdfcer-gui request G330).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::dimension::{
    CentreMarkSpec, DEFAULT_GROUP_ID, DimensionId, DimensionKind, FitCircle,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{CentreMarkHost, CentreMarkOutcome, EditError, EditSession};
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::vector::{AxisConstraint, Point};
use pdfcer_core::writer::SaveOptions;
use std::path::Path;

fn hello() -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/hello.pdf");
    EditSession::new(Document::load(&path).expect("load fixture"))
}

fn fit() -> FitCircle {
    FitCircle {
        center: Point::new(300.0, 300.0),
        radius: 50.0,
        residual: 0.0,
    }
}

fn circular(text_distance: Option<f64>) -> DimensionKind {
    DimensionKind::Circular {
        fit: fit(),
        show_diameter: true,
        show_circle: true,
        leader_angle: 30.0,
        text_distance,
        centre_mark: None,
    }
}

const EXTENDED: CentreMarkSpec = CentreMarkSpec {
    extension_lines: true,
    angle: 15.0,
};

fn save(s: &EditSession) -> Vec<u8> {
    s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0
}

fn annot_of(bytes: Vec<u8>, id: ObjId) -> (pdfcer_core::object::Dict, String) {
    let doc = Document::from_bytes(bytes).unwrap();
    let Object::Dict(annot) = doc.get(id).unwrap().value.clone() else {
        panic!("not a dict");
    };
    let Object::Dict(ap) = doc.resolve(annot.get(b"AP").unwrap()) else {
        panic!("no /AP");
    };
    let Object::Stream(st) = doc.resolve(ap.get(b"N").unwrap()) else {
        panic!("no /N");
    };
    let raw = st.data_span.slice(doc.bytes()).unwrap();
    let text =
        String::from_utf8_lossy(&pdfcer_core::filters::decode_stream(&st.dict, raw).unwrap())
            .into_owned();
    (annot, text)
}

fn mark_of(s: &EditSession, id: DimensionId) -> Option<CentreMarkSpec> {
    s.centre_mark_host(id).and_then(CentreMarkHost::mark)
}

#[test]
fn a_circular_gains_a_mark_its_label_moves_and_undo_restores_both() {
    let mut s = hello();
    let (annot, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, circular(None))
        .unwrap();
    let before = annot_of(save(&s), annot).1;
    let anchor = s
        .dimension_model()
        .dimension(id)
        .unwrap()
        .kind
        .label_anchor();
    let depth = s.undo_depth();

    let outcome = s.set_centre_mark(id, Some(EXTENDED)).unwrap();
    assert!(outcome.label_moved);
    assert_eq!(s.undo_depth(), depth + 1);
    assert_ne!(
        s.dimension_model()
            .dimension(id)
            .unwrap()
            .kind
            .label_anchor(),
        anchor
    );
    let bytes = save(&s);
    let after = annot_of(bytes.clone(), annot).1;
    assert_ne!(after, before);
    let reopened = EditSession::new(Document::from_bytes(bytes).unwrap());
    assert_eq!(mark_of(&reopened, id), Some(EXTENDED));

    s.undo().unwrap();
    assert_eq!(mark_of(&s, id), None);
    assert_eq!(annot_of(save(&s), annot).1, before);
    s.redo().unwrap();
    assert_eq!(annot_of(save(&s), annot).1, after);

    let cleared = s.set_centre_mark(id, None).unwrap();
    assert!(cleared.label_moved);
    assert_eq!(mark_of(&s, id), None);
    assert_eq!(annot_of(save(&s), annot).1, before);
}

#[test]
fn a_stored_text_distance_keeps_the_label_where_it_is() {
    let mut s = hello();
    let (_, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, circular(Some(60.0)))
        .unwrap();
    let outcome = s
        .set_centre_mark(id, Some(CentreMarkSpec::default()))
        .unwrap();
    assert_eq!(outcome, CentreMarkOutcome::default());
    assert!(!outcome.label_moved);
}

#[test]
fn setting_the_current_mark_still_commits_one_entry() {
    let mut s = hello();
    let (_, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, circular(None))
        .unwrap();
    let depth = s.undo_depth();
    let outcome = s.set_centre_mark(id, None).unwrap();
    assert!(!outcome.label_moved);
    assert_eq!(s.undo_depth(), depth + 1);
}

#[test]
fn a_standalone_mark_is_replaced_ink_and_all_but_never_cleared() {
    let mut s = hello();
    let (annot, id) = s
        .add_dimension(
            0,
            DEFAULT_GROUP_ID,
            DimensionKind::CentreMark {
                fit: fit(),
                mark: CentreMarkSpec::default(),
            },
        )
        .unwrap();
    let strokes = |s: &EditSession| {
        annot_of(save(s), annot)
            .0
            .get(b"InkList")
            .and_then(Object::as_array)
            .unwrap()
            .len()
    };
    assert_eq!(strokes(&s), 2);

    let outcome = s.set_centre_mark(id, Some(EXTENDED)).unwrap();
    assert!(!outcome.label_moved);
    assert_eq!(strokes(&s), 6);
    let reopened = EditSession::new(Document::from_bytes(save(&s)).unwrap());
    assert_eq!(
        reopened.centre_mark_host(id),
        Some(CentreMarkHost::Standalone { mark: EXTENDED })
    );

    let depth = s.undo_depth();
    let bytes = save(&s);
    assert!(matches!(
        s.set_centre_mark(id, None),
        Err(EditError::CentreMarkRequired { id: 0 })
    ));
    assert_eq!(s.undo_depth(), depth);
    assert_eq!(save(&s), bytes);
    s.undo().unwrap();
    assert_eq!(strokes(&s), 2);
}

#[test]
fn other_kinds_unknown_ids_and_bad_angles_are_refused_before_writing() {
    let mut s = hello();
    let (_, linear) = s
        .add_dimension(
            0,
            DEFAULT_GROUP_ID,
            DimensionKind::Linear {
                a: Point::new(100.0, 100.0),
                b: Point::new(200.0, 100.0),
                constraint: AxisConstraint::Horizontal,
                offset: 0.0,
                text_along: 0.0,
                extension_gap: [None; 2],
            },
        )
        .unwrap();
    let (_, circ) = s
        .add_dimension(0, DEFAULT_GROUP_ID, circular(None))
        .unwrap();
    let depth = s.undo_depth();
    let bytes = save(&s);

    for mark in [None, Some(CentreMarkSpec::default())] {
        assert!(matches!(
            s.set_centre_mark(linear, mark),
            Err(EditError::NotACentreMarkHost { .. })
        ));
    }
    assert!(matches!(
        s.set_centre_mark(DimensionId(99), None),
        Err(EditError::DimensionNotFound { id: 99 })
    ));
    let bad = CentreMarkSpec {
        extension_lines: false,
        angle: f64::NAN,
    };
    assert!(matches!(
        s.set_centre_mark(circ, Some(bad)),
        Err(EditError::CentreGeometryDegenerate { .. })
    ));
    assert_eq!(s.undo_depth(), depth);
    assert_eq!(save(&s), bytes);
}

#[test]
fn the_preflight_names_each_host_and_whether_it_can_clear() {
    let mut s = hello();
    let (_, circ) = s
        .add_dimension(0, DEFAULT_GROUP_ID, circular(None))
        .unwrap();
    let (_, line) = s
        .add_dimension(
            0,
            DEFAULT_GROUP_ID,
            DimensionKind::CentreLine {
                a: Point::new(100.0, 100.0),
                b: Point::new(100.0, 200.0),
            },
        )
        .unwrap();
    let host = s.centre_mark_host(circ).unwrap();
    assert_eq!(host, CentreMarkHost::Circular { mark: None });
    assert!(host.can_clear());
    assert_eq!(host.mark(), None);
    assert_eq!(s.centre_mark_host(line), None);
    assert_eq!(s.centre_mark_host(DimensionId(99)), None);

    // Out-of-crate construction: the enum's variants build from outside, and
    // the outcome only through `Default`.
    let standalone = CentreMarkHost::Standalone { mark: EXTENDED };
    assert!(!standalone.can_clear());
    assert_eq!(standalone.mark(), Some(EXTENDED));
    assert!(!CentreMarkOutcome::default().label_moved);
}
