//! Writing one saved 3D view's measurements, and rewriting views without
//! losing what pdfcer does not edit (ISO 32000-2 §13.6.4 Table 315 `/MA`,
//! §13.6.7.3). Synthetic documents only.

use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditError, EditSession};
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_core::threed::{
    ThreeDEmbedError, ThreeDLighting, ThreeDMeasureKind, ThreeDMeasurement, ThreeDRenderMode,
    ThreeDRenderModeKind, ThreeDSavedView, extract_3d, list_3d, saved_3d_views,
};
use pdfcer_core::writer::{SaveOptions, save_full};

const ANNOT: ObjId = ObjId::new(4, 0);
const STREAM: ObjId = ObjId::new(5, 0);
const VIEW_OBJECT: ObjId = ObjId::new(6, 0);
const ARRAY_OBJECT: ObjId = ObjId::new(7, 0);

const FRONT: &str = "<< /Type /3DView /XN (Front) /IN (front-internal) /U3DPath (/a) /MS /M \
    /C2W [1 0 0 0 1 0 0 0 1 0 0 5] /CO 5 /BG << /Type /3DBG /C [1 0 0] >> /SA [] /NA [] \
    /MA [ << /Type /3DMeasure /Subtype /LD3 /A1 [0 0 0] /A2 [1 0 0] /V 1 /U (mm) /S 8 0 R /Zz 7 >> \
    << /Subtype /XX9 /Q 1 >> 42 << /Subtype /3DC /A1 [0 0 0] /UT (note) >> ] >>";
const LAST: &str = "<< /Type /3DView /XN (Last) /NA [ << /Marker 3 >> ] >>";

fn va() -> String {
    format!("[ {FRONT} 6 0 R 42 {LAST} ]")
}

/// Page 0's `/3D` annotation (4) over a stream (5) with `/VA` = `va`; view
/// object 6; a `/VA` array object 7 holding `FRONT`; a Projection annotation 8.
fn doc_with(va: &str) -> Document {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R] >>".to_owned(),
        "<< /Type /Annot /Subtype /3D /Rect [0 0 100 50] /3DD 5 0 R >>".to_owned(),
        format!(
            "<< /Type /3D /Subtype /PRC /VA {va} /Length 6 >>\nstream\nPRC\x08\x00\x01\nendstream"
        ),
        "<< /Type /3DView /XN (Indirect) /IN (kept-internal) /BG << /Type /3DBG >> /Marker 9 >>"
            .to_owned(),
        format!("[ {FRONT} ]"),
        "<< /Type /Annot /Subtype /Projection /Rect [0 0 1 1] >>".to_owned(),
    ];
    let mut buf = b"%PDF-2.0\n".to_vec();
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
    doc_with(&va())
}

fn views(doc: &Document) -> Vec<ThreeDSavedView> {
    saved_3d_views(doc, &list_3d(doc)[0]).views
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

fn va_of(doc: &Document) -> Vec<Object> {
    match dict(doc, STREAM).get(b"VA") {
        Some(Object::Array(a)) => a.clone(),
        other => panic!("no direct /VA: {other:?}"),
    }
}

fn as_dict(o: &Object) -> &Dict {
    o.as_dict().expect("a direct dictionary")
}

fn ma_of(view: &Dict) -> Vec<Object> {
    match view.get(b"MA") {
        Some(Object::Array(a)) => a.clone(),
        other => panic!("no direct /MA: {other:?}"),
    }
}

fn new_measure() -> ThreeDMeasurement {
    ThreeDMeasurement::linear([0.; 3], [3., 4., 0.], [0., 0., 1.], "in").expect("a measurement")
}

fn says(disclosures: &[String], what: &str) -> bool {
    disclosures.iter().any(|d| d.contains(what))
}

#[test]
fn adding_one_keeps_everything_else_and_undo_restores_it() {
    let original = doc();
    let mut list = views(&original)[0].measurements.clone();
    assert_eq!(list.len(), 2, "the two readable entries");
    list.push(new_measure());
    let was_annot = dict(&original, ANNOT);
    let was_va = va_of(&original);
    let art = list_3d(&original)[0].clone();
    let was_data = extract_3d(&original.view(), &art).expect("model").data;

    let mut session = EditSession::new(original);
    let o = session
        .set_3d_view_measurements(0, ANNOT, 0, &list)
        .expect("writes");
    assert_eq!(o.written, Some(STREAM));
    assert_eq!((o.annot_id, o.stream_id, o.view_index), (ANNOT, STREAM, 0));
    assert_eq!((o.measurements_before, o.measurements_after), (2, 3));
    assert_eq!(o.unread_entries_kept, 2);
    assert!(says(&o.disclosures, "PDF 2.0"), "{:?}", o.disclosures);
    assert!(says(&o.disclosures, "cannot read"), "{:?}", o.disclosures);
    assert!(!says(&o.disclosures, "Projection"), "{:?}", o.disclosures);
    assert_eq!(session.undo_depth(), 1);

    let back = saved(&session);
    assert_eq!(
        dict(&back, ANNOT),
        was_annot,
        "the annotation is not touched"
    );
    let art = list_3d(&back)[0].clone();
    assert_eq!(
        extract_3d(&back.view(), &art).expect("model").data,
        was_data
    );
    let now = va_of(&back);
    assert_eq!(now.len(), 4);
    assert_eq!(
        &now[1..],
        &was_va[1..],
        "the other views are the same objects"
    );
    let (mut front, mut was_front) = (as_dict(&now[0]).clone(), as_dict(&was_va[0]).clone());
    let (ma, was_ma) = (ma_of(&front), ma_of(&was_front));
    front.remove(b"MA");
    was_front.remove(b"MA");
    assert_eq!(front, was_front, "every other key of the view");
    assert_eq!(
        &ma[..4],
        &was_ma[..],
        "existing entries, /S and /Zz included"
    );
    assert_eq!(ma.len(), 5);
    assert_eq!(views(&back)[0].measurements, list);

    assert_eq!(session.undo(), Some(CommandKind::SetThreeDViews));
    assert_eq!(va_of(&saved(&session)), was_va);
}

#[test]
fn removing_a_commented_one_says_so_and_unread_entries_stay_put() {
    let original = doc();
    let note = views(&original)[0].measurements[1].clone();
    assert_eq!(note.kind, ThreeDMeasureKind::Comment);
    let mut session = EditSession::new(original);
    let o = session
        .set_3d_view_measurements(0, ANNOT, 0, std::slice::from_ref(&note))
        .expect("writes");
    assert_eq!((o.measurements_before, o.measurements_after), (2, 1));
    assert!(says(&o.disclosures, "removed"), "{:?}", o.disclosures);
    let back = saved(&session);
    let ma = ma_of(as_dict(&va_of(&back)[0]));
    assert_eq!(
        ma.len(),
        3,
        "note, the unknown subtype, the integer: {ma:?}"
    );
    assert!(
        as_dict(&ma[0]).contains_key(b"UT"),
        "the note fills the first slot"
    );
    assert!(
        as_dict(&ma[1]).contains_key(b"Q"),
        "unknown subtype in place"
    );
    assert_eq!(ma[2], Object::Integer(42));
    assert_eq!(views(&back)[0].measurements, vec![note]);
}

#[test]
fn clearing_a_list_of_only_readable_entries_removes_the_key() {
    let readable = "[ << /Type /3DView /XN (A) /BG << >> /MA [ << /Subtype /3DC /UT (n) >> ] >> ]";
    let mut session = EditSession::new(doc_with(readable));
    let o = session
        .set_3d_view_measurements(0, ANNOT, 0, &[])
        .expect("writes");
    assert_eq!(o.measurements_after, 0);
    assert!(!says(&o.disclosures, "PDF 2.0"), "{:?}", o.disclosures);
    let back = saved(&session);
    let va = va_of(&back);
    assert!(!as_dict(&va[0]).contains_key(b"MA"));
    assert!(as_dict(&va[0]).contains_key(b"BG"));
}

#[test]
fn a_new_measurement_with_a_comment_link_says_the_link_is_not_written() {
    let mut m = new_measure();
    m.has_comment = true;
    let mut session = EditSession::new(doc());
    let o = session
        .set_3d_view_measurements(0, ANNOT, 1, &[m])
        .expect("writes");
    assert!(says(&o.disclosures, "Projection"), "{:?}", o.disclosures);
}

#[test]
fn an_indirect_view_is_the_only_object_written() {
    let original = doc();
    let was_va = va_of(&original);
    let mut session = EditSession::new(original);
    let o = session
        .set_3d_view_measurements(0, ANNOT, 1, &[new_measure()])
        .expect("writes");
    assert_eq!(o.written, Some(VIEW_OBJECT));
    let back = saved(&session);
    assert_eq!(va_of(&back), was_va, "the stream is not rewritten");
    let view = dict(&back, VIEW_OBJECT);
    assert!(view.contains_key(b"Marker") && view.contains_key(b"IN"));
    assert_eq!(views(&back)[1].measurements.len(), 1);
}

#[test]
fn an_indirect_array_is_the_only_object_written() {
    let mut session = EditSession::new(doc_with("7 0 R"));
    let o = session
        .set_3d_view_measurements(0, ANNOT, 0, &[new_measure()])
        .expect("writes");
    assert_eq!(o.written, Some(ARRAY_OBJECT));
    let back = saved(&session);
    assert_eq!(
        dict(&back, STREAM).get(b"VA"),
        Some(&Object::Reference(ARRAY_OBJECT))
    );
    assert_eq!(views(&back)[0].measurements, vec![new_measure()]);
}

#[test]
fn the_same_indirect_view_listed_twice_gets_its_own_copy() {
    let mut session = EditSession::new(doc_with("[ 6 0 R 6 0 R ]"));
    let o = session
        .set_3d_view_measurements(0, ANNOT, 1, &[new_measure()])
        .expect("writes");
    assert_eq!(o.written, Some(STREAM), "object 6 also serves view 0");
    let back = saved(&session);
    let found = views(&back);
    assert!(found[0].measurements.is_empty());
    assert_eq!(found[1].measurements.len(), 1);
}

#[test]
fn the_index_counts_views_not_array_entries() {
    let mut session = EditSession::new(doc());
    session
        .set_3d_view_measurements(0, ANNOT, 2, &[new_measure()])
        .expect("writes");
    let back = saved(&session);
    let va = va_of(&back);
    assert_eq!(va[2], Object::Integer(42), "not a view, so not counted");
    assert!(as_dict(&va[3]).contains_key(b"MA"));
    assert!(as_dict(&va[3]).contains_key(b"NA"));
}

#[test]
fn a_bad_index_or_measurement_is_refused_and_writes_nothing() {
    let mut session = EditSession::new(doc());
    let err = session
        .set_3d_view_measurements(0, ANNOT, 3, &[])
        .expect_err("only three views");
    assert!(matches!(
        err,
        EditError::ThreeD(ThreeDEmbedError::ViewIndexOutOfRange { index: 3, count: 3 })
    ));
    let bad = ThreeDMeasurement::new(ThreeDMeasureKind::Linear);
    let err = session
        .set_3d_view_measurements(0, ANNOT, 0, &[bad])
        .expect_err("no anchors");
    assert!(matches!(
        err,
        EditError::ThreeD(ThreeDEmbedError::ViewInvalid { .. })
    ));
    assert!(!session.can_undo());
}

#[test]
fn the_list_it_already_holds_writes_nothing() {
    let original = doc();
    let list = views(&original)[0].measurements.clone();
    let mut session = EditSession::new(original);
    let o = session
        .set_3d_view_measurements(0, ANNOT, 0, &list)
        .expect("nothing to do");
    assert_eq!(o.written, None);
    assert!(o.disclosures.is_empty(), "{:?}", o.disclosures);
    assert!(!session.can_undo());
}

#[test]
fn rewriting_read_views_keeps_what_pdfcer_does_not_edit() {
    let original = doc();
    let was_va = va_of(&original);
    let mut list = views(&original);
    let mut session = EditSession::new(original);
    let o = session
        .set_3d_views(0, ANNOT, &list, Some(0))
        .expect("nothing changed in them");
    assert!(!says(&o.disclosures, "carried"), "{:?}", o.disclosures);
    let back = saved(&session);
    let now = va_of(&back);
    assert_eq!(now.len(), 3, "the integer is not a view");
    assert_eq!(now[0], was_va[0], "unchanged, so as it was");
    assert_eq!(now[1], was_va[1], "still the reference");
    assert_eq!(now[2], was_va[3]);

    list[0].camera_to_world = Some([1., 0., 0., 0., 0., 1., 0., -1., 0., 0., 2., 0.]);
    let mut session = EditSession::new(doc());
    let o = session
        .set_3d_views(0, ANNOT, &list, Some(0))
        .expect("writes");
    assert!(!says(&o.disclosures, "carried"), "{:?}", o.disclosures);
    let back = saved(&session);
    let now = va_of(&back);
    let front = as_dict(&now[0]);
    assert!(
        !front.contains_key(b"U3DPath"),
        "a new camera replaces the path"
    );
    for key in [&b"BG"[..], b"SA", b"NA", b"IN"] {
        assert!(front.contains_key(key), "{}", String::from_utf8_lossy(key));
    }
    let was_ma = ma_of(as_dict(&was_va[0]));
    assert_eq!(ma_of(front), was_ma, "the measurements did not change");
    assert_eq!(views(&back)[0].camera_to_world, list[0].camera_to_world);
}

#[test]
fn a_changed_indirect_view_is_written_in_place_and_keeps_its_keys() {
    let original = doc();
    let mut list = views(&original);
    list[1].name = "Renamed".to_owned();
    let mut session = EditSession::new(original);
    session.set_3d_views(0, ANNOT, &list, None).expect("writes");
    let back = saved(&session);
    assert_eq!(va_of(&back)[1], Object::Reference(VIEW_OBJECT));
    let view = dict(&back, VIEW_OBJECT);
    assert!(view.contains_key(b"IN"), "a rename keeps the internal name");
    assert!(view.contains_key(b"Marker") && view.contains_key(b"BG"));
    assert_eq!(views(&back)[1].name, "Renamed");
}

#[test]
fn changing_measurements_through_a_rewrite_merges_as_the_verb_does() {
    let original = doc();
    let mut list = views(&original);
    list[0].measurements.remove(0);
    let mut session = EditSession::new(original);
    let o = session.set_3d_views(0, ANNOT, &list, None).expect("writes");
    assert!(says(&o.disclosures, "removed"), "{:?}", o.disclosures);
    assert!(says(&o.disclosures, "cannot read"), "{:?}", o.disclosures);
    let back = saved(&session);
    let ma = ma_of(as_dict(&va_of(&back)[0]));
    assert_eq!(ma.len(), 3, "{ma:?}");
    assert_eq!(ma[2], Object::Integer(42));
}

#[test]
fn a_view_from_another_document_is_written_whole_and_disclosed() {
    let foreign = views(&doc_with(
        "[ << /Type /3DView /XN (Elsewhere) /BG << /Type /3DBG >> /Other 1 >> ]",
    ));
    let mut session = EditSession::new(doc());
    let o = session
        .set_3d_views(0, ANNOT, &foreign, None)
        .expect("writes");
    assert!(says(&o.disclosures, "carried"), "{:?}", o.disclosures);
    let back = saved(&session);
    let va = va_of(&back);
    assert_eq!(views(&back)[0].name, "Elsewhere");
    assert!(!as_dict(&va[0]).contains_key(b"Other"));
}

#[test]
fn a_view_built_by_hand_has_nothing_to_carry() {
    let built = vec![ThreeDSavedView::new(
        "Built",
        [1., 0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 5.],
    )];
    let mut session = EditSession::new(doc());
    let o = session
        .set_3d_views(0, ANNOT, &built, None)
        .expect("writes");
    assert!(!says(&o.disclosures, "carried"), "{:?}", o.disclosures);
}

#[test]
fn views_with_the_same_fields_compare_equal_whatever_they_were_read_from() {
    let plain = views(&doc_with("[ << /Type /3DView /XN (Only) >> ]"));
    let more = views(&doc_with(
        "[ << /Type /3DView /XN (Only) /BG << >> /Other 1 >> ]",
    ));
    assert_eq!(plain, more);
}

#[test]
fn a_new_measurement_in_a_read_view_is_still_checked() {
    let original = doc();
    let mut list = views(&original);
    list[0]
        .measurements
        .push(ThreeDMeasurement::new(ThreeDMeasureKind::Linear));
    let mut session = EditSession::new(original);
    let err = session
        .set_3d_views(0, ANNOT, &list, None)
        .expect_err("the new one has no anchors");
    assert!(matches!(
        err,
        EditError::ThreeD(ThreeDEmbedError::ViewInvalid { index: 0, .. })
    ));
}

#[test]
fn every_field_a_view_edits_reaches_the_file_and_the_rest_stay() {
    let original = doc();
    let mut list = views(&original);
    list[0].name = "Renamed".to_owned();
    list[0].lighting = Some(ThreeDLighting::Day);
    list[0].render_mode = Some(ThreeDRenderMode::new(ThreeDRenderModeKind::Transparent));
    list[0].field_of_view = Some(40.0);
    let mut session = EditSession::new(original);
    session
        .set_3d_views(0, ANNOT, &list, Some(0))
        .expect("writes");
    let back = saved(&session);
    let read = &views(&back)[0];
    assert_eq!(read.name, "Renamed");
    assert_eq!(read.lighting, Some(ThreeDLighting::Day));
    assert_eq!(read.render_mode, list[0].render_mode);
    assert_eq!(read.field_of_view, Some(40.0));
    let now = va_of(&back);
    let front = as_dict(&now[0]);
    for key in [&b"BG"[..], b"IN", b"U3DPath"] {
        assert!(front.contains_key(key), "{}", String::from_utf8_lossy(key));
    }
}
