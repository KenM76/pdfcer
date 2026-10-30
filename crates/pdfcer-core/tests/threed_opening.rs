//! A placed model's lit opening view (`EditSession::set_3d_opening_view`,
//! request G247): the poster's camera, the automatic-lighting rule, and one
//! `/VA` for every shell. Synthetic documents and models only.

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, MarkupOptions};
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::threed::{
    OPENING_VIEW_NAME, ThreeDLighting, ThreeDSpec, default_3d_view, list_3d, opening_view,
    saved_3d_views,
};
use pdfcer_core::writer::{SaveOptions, save_full};

const RECT: Rect = Rect {
    llx: 20.0,
    lly: 20.0,
    urx: 180.0,
    ury: 120.0,
};

fn doc() -> Document {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>",
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
    Document::from_bytes(buf).expect("synthetic document parses")
}

fn square_prc() -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/prc/square.prc");
    std::fs::read(path).expect("synthetic fixture")
}

fn placed(spec: &ThreeDSpec) -> (EditSession, ObjId) {
    let mut session = EditSession::new(doc());
    let annot = session
        .add_3d_annotation(0, spec, &MarkupOptions::default())
        .expect("embeds")
        .annot_id;
    (session, annot)
}

fn saved(session: &EditSession) -> Vec<u8> {
    save_full(
        session.document(),
        &session.dirty_set(),
        &SaveOptions::identity(),
    )
    .expect("full rewrite")
    .0
}

fn stream_has_views(session: &EditSession, annot: ObjId) -> bool {
    let back = Document::from_bytes(saved(session)).expect("reloads");
    let Object::Dict(a) = &back.get(annot).expect("annot").value else {
        panic!("annot is not a dictionary");
    };
    let Some(Object::Reference(s)) = a.get(b"3DD") else {
        panic!("no /3DD");
    };
    let Object::Stream(stream) = &back.get(*s).expect("stream").value else {
        panic!("/3DD is not a stream");
    };
    stream.dict.contains_key(b"VA")
}

#[test]
fn automatic_lighting_on_a_prc_model_writes_no_view() {
    let spec = ThreeDSpec::new(RECT, square_prc()).expect("PRC sniffs");
    let (mut session, annot) = placed(&spec);
    let opened = session
        .set_3d_opening_view(0, annot, &spec, None, false)
        .expect("no error");
    assert_eq!(opened, None);
    assert!(!stream_has_views(&session, annot));
}

#[test]
fn automatic_lighting_on_a_converted_model_writes_cad_lights_as_the_default() {
    let spec = ThreeDSpec::new(RECT, square_prc()).expect("PRC sniffs");
    let (mut session, annot) = placed(&spec);
    let opened = session
        .set_3d_opening_view(0, annot, &spec, None, true)
        .expect("writes")
        .expect("a view");
    assert_eq!(opened.lighting, ThreeDLighting::Cad);
    assert!(opened.automatic);
    assert_eq!(opened.outcome.default, Some(0));
    assert!(opened.disclosure().contains("CAD scheme"));
    let back = Document::from_bytes(saved(&session)).expect("reloads");
    let art = list_3d(&back);
    assert_eq!(art[0].annot_id, Some(annot));
    let views = saved_3d_views(&back, &art[0]);
    assert_eq!(views.views.len(), 1);
    assert_eq!(views.views[0].name, OPENING_VIEW_NAME);
    assert_eq!(views.views[0].lighting, Some(ThreeDLighting::Cad));
    assert_eq!(
        default_3d_view(&back, &art[0]).map(|v| v.name),
        Some("Default".into())
    );
}

/// The acceptance: a shell composing the view itself and one calling the
/// verb write byte-identical files.
#[test]
fn the_verb_and_a_hand_composed_view_write_the_same_bytes() {
    let spec = ThreeDSpec::new(RECT, square_prc()).expect("PRC sniffs");
    let (mut by_verb, annot) = placed(&spec);
    by_verb
        .set_3d_opening_view(0, annot, &spec, Some(ThreeDLighting::Day), false)
        .expect("writes");
    let (mut by_hand, annot2) = placed(&spec);
    let view = opening_view(&spec).with_lighting(ThreeDLighting::Day);
    by_hand
        .set_3d_views(0, annot2, &[view], Some(0))
        .expect("writes");
    assert_eq!(saved(&by_verb), saved(&by_hand));
}

#[cfg(feature = "3d")]
#[test]
fn the_opening_view_is_the_poster_camera_fitted_to_the_rectangle() {
    let spec = ThreeDSpec::new(RECT, square_prc()).expect("PRC sniffs");
    let (mut session, annot) = placed(&spec);
    let opened = session
        .set_3d_opening_view(0, annot, &spec, Some(ThreeDLighting::White), false)
        .expect("writes")
        .expect("a view");
    assert!(opened.framed, "{}", opened.disclosure());
    let model = pdfcer_3d::assemble(&spec.data).expect("meshes");
    let aspect = 160.0 / 100.0;
    let camera = pdfcer_3d::default_view_camera(&model, aspect).expect("camera");
    let expected =
        pdfcer_core::threed::ThreeDSavedView::from_camera(OPENING_VIEW_NAME, &camera, aspect)
            .expect("a view")
            .with_lighting(ThreeDLighting::White);
    assert_eq!(opened.view, expected);
}

#[test]
fn a_model_pdfcer_cannot_mesh_opens_with_the_camera_left_to_the_reader() {
    let spec = ThreeDSpec::new(RECT, b"PRC\x08\x00\x01".to_vec()).expect("PRC sniffs");
    let (mut session, annot) = placed(&spec);
    let opened = session
        .set_3d_opening_view(0, annot, &spec, Some(ThreeDLighting::Headlamp), false)
        .expect("writes")
        .expect("a view");
    assert!(!opened.framed);
    assert!(!opened.automatic);
    assert_eq!(opened.view.camera_to_world, None);
    assert!(opened.disclosure().contains("left to the reader"));
    assert!(stream_has_views(&session, annot));
}
