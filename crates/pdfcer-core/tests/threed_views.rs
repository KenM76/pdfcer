//! Writing a 3D annotation's views (ISO 32000-1 §13.6.3 Table 300 `/VA`
//! `/DV`, Table 298 `/3DV`, Table 304 view and Table 305 projection
//! dictionaries). Synthetic documents only.

use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditError, EditSession};
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_core::threed::{
    MAX_3D_VIEWS, OrthoBinding, ThreeDEmbedError, ThreeDSavedView, default_3d_view, extract_3d,
    list_3d,
};
use pdfcer_core::writer::{SaveOptions, save_full};

const ANNOT: ObjId = ObjId::new(4, 0);
const STREAM: ObjId = ObjId::new(5, 0);
const BACK: [f64; 12] = [1., 0., 0., 0., 0., 1., 0., -1., 0., 0., 5., 0.];
const TOP: [f64; 12] = [1., 0., 0., 0., 1., 0., 0., 0., -1., 0., 0., 9.];

/// Page 0: a `/3D` annotation (4, `/3DD` = `three_dd`, with an old `/3DV
/// /F`) over a 6-byte model stream (5) that already holds one view and
/// `/DV 0`, a `/Square` (6), and a locked `/3D` (7).
fn doc_with(three_dd: &str) -> Document {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R 6 0 R 7 0 R] >>"
            .to_owned(),
        format!("<< /Type /Annot /Subtype /3D /Rect [0 0 100 50] /3DD {three_dd} /3DV /F >>"),
        "<< /Type /3D /Subtype /PRC /VA [<< /Type /3DView /XN (Old) >>] /DV 0 /Length 6 >>\nstream\nPRC\x08\x00\x01\nendstream"
            .to_owned(),
        "<< /Type /Annot /Subtype /Square /Rect [0 100 50 150] >>".to_owned(),
        "<< /Type /Annot /Subtype /3D /F 128 /Rect [100 0 200 50] /3DD 5 0 R >>".to_owned(),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    Document::from_bytes(buf).expect("synthetic 3D document parses")
}

fn doc() -> Document {
    doc_with("5 0 R")
}

fn saved(session: &EditSession) -> Document {
    let bytes = save_full(
        session.document(),
        &session.dirty_set(),
        &SaveOptions::identity(),
    )
    .expect("full rewrite")
    .0;
    Document::from_bytes(bytes).expect("reloads")
}

fn dict(doc: &Document, id: ObjId) -> Dict {
    match &doc.get(id).expect("object present").value {
        Object::Dict(d) => d.clone(),
        Object::Stream(s) => s.dict.clone(),
        other => panic!("{id} is not a dictionary: {other:?}"),
    }
}

fn two_views() -> Vec<ThreeDSavedView> {
    vec![
        ThreeDSavedView::new("Back", BACK)
            .with_orbit_distance(5.0)
            .with_perspective(30.0),
        ThreeDSavedView::new("Top", TOP)
            .with_orbit_distance(9.0)
            .with_orthographic(0.25, OrthoBinding::Height),
    ]
}

#[test]
fn views_round_trip_and_the_default_is_the_one_a_reader_opens_on() {
    let mut session = EditSession::new(doc());
    let outcome = session
        .set_3d_views(0, ANNOT, &two_views(), Some(1))
        .expect("writes");
    assert_eq!((outcome.annot_id, outcome.stream_id), (ANNOT, STREAM));
    assert_eq!((outcome.views_before, outcome.views_after), (1, 2));
    assert_eq!(outcome.default, Some(1));
    assert!(!outcome.shared_stream);
    assert_eq!(
        outcome.disclosures.len(),
        1,
        "the orthographic scale reading"
    );

    let back = saved(&session);
    let art = list_3d(&back);
    let first = art
        .iter()
        .find(|a| a.annot_id == Some(ANNOT))
        .expect("listed");
    assert_eq!(first.view_count, 2);
    let view = default_3d_view(&back, first).expect("a default view");
    assert_eq!(view.name, "Top");
    assert_eq!(view.camera_to_world, Some(TOP));
    assert_eq!(view.orbit_distance, Some(9.0));
    assert!(view.orthographic);
    assert_eq!(view.ortho_scale, 0.25);
    assert_eq!(view.ortho_binding, OrthoBinding::Height);
    assert_eq!(dict(&back, ANNOT).get(b"3DV"), Some(&Object::Integer(1)));
    assert_eq!(dict(&back, STREAM).get(b"DV"), Some(&Object::Integer(1)));
    assert_eq!(
        extract_3d(&back.view(), first).expect("extracts").data,
        b"PRC\x08\x00\x01",
        "the model bytes are untouched"
    );
}

#[test]
fn a_perspective_field_of_view_reads_back() {
    let mut session = EditSession::new(doc());
    session
        .set_3d_views(0, ANNOT, &two_views(), Some(0))
        .expect("writes");
    let back = saved(&session);
    let art = list_3d(&back);
    let view = default_3d_view(&back, &art[0]).expect("a default view");
    assert_eq!(view.name, "Back");
    assert!(!view.orthographic);
    assert_eq!(view.field_of_view, Some(30.0));
    let aim = view.aim(1.0).expect("a matrix");
    assert_eq!(aim.direction, [0., -1., 0.]);
}

#[test]
fn no_default_removes_the_old_annotation_choice() {
    let mut session = EditSession::new(doc());
    let outcome = session
        .set_3d_views(0, ANNOT, &two_views(), None)
        .expect("writes");
    assert_eq!(outcome.default, None);
    let back = saved(&session);
    assert!(!dict(&back, ANNOT).contains_key(b"3DV"), "old /3DV /F gone");
    assert!(!dict(&back, STREAM).contains_key(b"DV"));
    let art = list_3d(&back);
    assert_eq!(
        default_3d_view(&back, &art[0]).map(|v| v.name),
        Some("Back".to_owned()),
        "Table 300: /DV defaults to the first view"
    );
}

#[test]
fn empty_views_clear_every_view_entry() {
    let mut session = EditSession::new(doc());
    let outcome = session
        .set_3d_views(0, ANNOT, &[], Some(0))
        .expect("clears");
    assert_eq!((outcome.views_before, outcome.views_after), (1, 0));
    assert_eq!(outcome.default, None);
    let back = saved(&session);
    let stream = dict(&back, STREAM);
    assert!(!stream.contains_key(b"VA") && !stream.contains_key(b"DV"));
    assert!(!dict(&back, ANNOT).contains_key(b"3DV"));
}

#[test]
fn one_undo_restores_both_objects() {
    let original = doc();
    let mut session = EditSession::new(doc());
    session
        .set_3d_views(0, ANNOT, &two_views(), Some(1))
        .expect("writes");
    assert_eq!(session.undo(), Some(CommandKind::SetThreeDViews));
    let back = saved(&session);
    assert_eq!(dict(&back, ANNOT), dict(&original, ANNOT));
    assert_eq!(dict(&back, STREAM), dict(&original, STREAM));
}

#[test]
fn a_3d_reference_dictionary_is_followed_and_disclosed_as_shared() {
    let mut session = EditSession::new(doc_with("<< /Type /3DRef /3D 5 0 R >>"));
    let outcome = session
        .set_3d_views(0, ANNOT, &two_views()[..1], Some(0))
        .expect("writes");
    assert_eq!(outcome.stream_id, STREAM);
    assert!(outcome.shared_stream);
    assert!(outcome.disclosures.iter().any(|d| d.contains("shared")));
}

#[test]
fn an_annotation_naming_no_3d_stream_is_refused() {
    let mut session = EditSession::new(doc_with("6 0 R"));
    let err = session
        .set_3d_views(0, ANNOT, &two_views(), None)
        .expect_err("no stream");
    assert!(matches!(
        err,
        EditError::ThreeD(ThreeDEmbedError::NoThreeDStream)
    ));
}

#[test]
fn bad_views_and_bad_targets_are_refused_before_anything_is_written() {
    let mut session = EditSession::new(doc());
    let unnamed = vec![ThreeDSavedView::new(" ", BACK)];
    let mut nan = BACK;
    nan[0] = f64::NAN;
    let cases: Vec<(ObjId, Vec<ThreeDSavedView>, Option<usize>)> = vec![
        (ANNOT, unnamed, None),
        (ANNOT, vec![ThreeDSavedView::new("N", nan)], None),
        (
            ANNOT,
            vec![ThreeDSavedView::new("F", BACK).with_perspective(200.0)],
            None,
        ),
        (
            ANNOT,
            vec![ThreeDSavedView::new("O", BACK).with_orthographic(0.0, OrthoBinding::Width)],
            None,
        ),
        (ANNOT, two_views(), Some(2)),
        (
            ANNOT,
            vec![ThreeDSavedView::new("V", BACK); MAX_3D_VIEWS + 1],
            None,
        ),
        (ObjId::new(6, 0), two_views(), None),
        (ObjId::new(7, 0), two_views(), None),
    ];
    let errors: Vec<_> = cases
        .into_iter()
        .map(|(id, views, default)| {
            session
                .set_3d_views(0, id, &views, default)
                .expect_err("refused")
        })
        .collect();
    assert!(matches!(
        errors[0],
        EditError::ThreeD(ThreeDEmbedError::ViewNameEmpty { index: 0 })
    ));
    for e in &errors[1..4] {
        assert!(
            matches!(
                e,
                EditError::ThreeD(ThreeDEmbedError::ViewInvalid { index: 0, .. })
            ),
            "{e:?}"
        );
    }
    assert!(matches!(
        errors[4],
        EditError::ThreeD(ThreeDEmbedError::DefaultViewOutOfRange { index: 2, count: 2 })
    ));
    assert!(matches!(
        errors[5],
        EditError::ThreeD(ThreeDEmbedError::TooManyViews { .. })
    ));
    assert!(matches!(
        errors[6],
        EditError::ThreeD(ThreeDEmbedError::NotA3dAnnotation { .. })
    ));
    assert!(matches!(errors[7], EditError::AnnotationLocked { .. }));
    assert_eq!(session.undo(), None, "nothing was committed");
}
