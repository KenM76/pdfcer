//! `EditSession::flatten_annotations`: burning annotations into the page
//! (§12.5.5 placement), what goes with them, and what is refused.
//!
//! The fixture page carries one annotation of each kind the verb treats
//! differently, so the "every annotation" call exercises the partition of
//! burned against skipped in one pass. Byte claims are checked on a saved and
//! re-parsed file (R159).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::annot::page_annotations;
use pdfcer_core::document::Document;
use pdfcer_core::edit::{AnnotFlattenRefusalReason as R, EditError, EditSession};
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_core::writer::SaveOptions;

pub(crate) fn assemble(bodies: &[String]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
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
    buf
}

fn stream(extra: &str, content: &str) -> String {
    format!(
        "<< {extra} /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

/// Objects, by number:
///  5 Square, /Popup 6, /StructParent 1, unprinted (/F 0)  -> burned
///  6 its Popup                                              -> removed with 5
///  7 Text reply to 5, no /AP                                -> skipped, un-linked
///  8 Square /CA 0.5                                         -> burned, grouped
///  9 Square /OC 16                                          -> burned, layered
/// 10 Link                                                   -> skipped
/// 11 Square Hidden                                          -> skipped
/// 12 Square /Locked                                         -> skipped
/// 13 Square with /A                                         -> skipped
pub(crate) fn fixture_bytes(rotate: u32, f5: u32) -> Vec<u8> {
    let ap = "/Type /XObject /Subtype /Form /BBox [0 0 10 10]";
    assemble(&[
        "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [16 0 R] /D << /ON [16 0 R] >> >> >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Rotate {rotate} /Contents 4 0 R /Annots [5 0 R 6 0 R 7 0 R 8 0 R 9 0 R 10 0 R 11 0 R 12 0 R 13 0 R] >>"),
        stream("", "0 0 1 rg 0 0 200 10 re f"),
        format!("<< /Type /Annot /Subtype /Square /Rect [20 20 60 60] /F {f5} /StructParent 1 /AP << /N 14 0 R >> /Popup 6 0 R >>"),
        "<< /Type /Annot /Subtype /Popup /Rect [100 150 180 190] /Parent 5 0 R >>".into(),
        "<< /Type /Annot /Subtype /Text /Rect [150 20 170 40] /F 4 /IRT 5 0 R /RT /R >>".into(),
        "<< /Type /Annot /Subtype /Square /Rect [100 100 140 140] /F 4 /CA 0.5 /AP << /N 14 0 R >> >>".into(),
        "<< /Type /Annot /Subtype /Square /Rect [100 20 140 60] /F 4 /OC 16 0 R /AP << /N 15 0 R >> >>".into(),
        "<< /Type /Annot /Subtype /Link /Rect [20 100 60 140] /F 4 /AP << /N 14 0 R >> >>".into(),
        "<< /Type /Annot /Subtype /Square /Rect [20 150 40 170] /F 6 /AP << /N 14 0 R >> >>".into(),
        "<< /Type /Annot /Subtype /Square /Rect [50 150 70 170] /F 132 /AP << /N 14 0 R >> >>".into(),
        "<< /Type /Annot /Subtype /Square /Rect [80 150 90 170] /F 4 /A << /S /JavaScript /JS (1) >> /AP << /N 14 0 R >> >>".into(),
        stream(ap, "1 0 0 rg 0 0 10 10 re f"),
        stream(ap, "0 1 0 rg 0 0 10 10 re f"),
        "<< /Type /OCG /Name (Marks) >>".into(),
    ])
}

fn session(rotate: u32) -> EditSession {
    EditSession::new(Document::from_bytes(fixture_bytes(rotate, 0)).expect("parses"))
}

fn id(n: u32) -> ObjId {
    ObjId::new(n, 0)
}

fn reload(s: &EditSession) -> Document {
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("incremental save");
    Document::from_bytes(bytes).expect("re-parse")
}

fn dict(doc: &Document, n: u32) -> Dict {
    match &doc.get(id(n)).expect("present").value {
        Object::Dict(d) => d.clone(),
        other => panic!("{n} is {other:?}"),
    }
}

fn annots_left(doc: Document) -> Vec<u32> {
    let s = EditSession::new(doc);
    let slots = s.page_slots().unwrap();
    page_annotations(&s.graph(), slots[0].id)
        .iter()
        .filter_map(|a| a.id.map(|i| i.num))
        .collect()
}

#[test]
fn every_burnable_annotation_is_burned_and_the_rest_are_reported() {
    let mut s = session(0);
    let out = s.flatten_annotations(0, None).expect("flatten");
    assert!(out.changed);
    assert_eq!(out.flattened, 3, "5, 8 and 9");
    assert_eq!(out.grouped, 1);
    assert_eq!(out.layered, 1);
    assert_eq!(out.popups_removed, 1);
    assert_eq!(out.replies_unlinked, 1);
    let mut skipped: Vec<(u32, R)> = out
        .skipped
        .iter()
        .map(|r| (r.id.unwrap().num, r.reason))
        .collect();
    skipped.sort_by_key(|p| p.0);
    assert_eq!(
        skipped,
        vec![
            (7, R::NoAppearance),
            (10, R::Link),
            (11, R::Hidden),
            (12, R::Locked),
            (13, R::HasAction),
        ]
    );
    let text = out.disclosures.join("\n");
    for needle in [
        "burned 3 annotation(s) into page 1",
        "stay on their layer",
        "pop-up",
        "no longer point",
        "structure-tree",
        "now print",
        "previous revision",
        "left 5 annotation(s)",
    ] {
        assert!(text.contains(needle), "{needle:?} missing from {text}");
    }

    let doc = reload(&s);
    let reply = dict(&doc, 7);
    assert!(doc.get(id(14)).is_some() && doc.get(id(15)).is_some());
    assert_eq!(annots_left(doc), vec![7, 10, 11, 12, 13]);
    assert!(reply.get(b"IRT".as_slice()).is_none() && reply.get(b"RT".as_slice()).is_none());
}

#[test]
fn the_burn_is_appended_as_one_content_stream_and_the_original_is_untouched() {
    let mut s = session(0);
    s.flatten_annotations(0, None).unwrap();
    let doc = reload(&s);
    let page = dict(&doc, 3);
    let Some(Object::Array(contents)) = page.get(b"Contents".as_slice()) else {
        panic!("contents became an array: {page:?}");
    };
    // The page leaves `rg` in effect, so the original sits inside the
    // overlay wrapper: [save, original, restore, burn].
    assert_eq!(contents.len(), 4);
    assert_eq!(contents[1], Object::Reference(id(4)));
    let (bytes, _) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    // Placement of 5: /BBox 10x10 onto /Rect 40x40 at (20,20).
    assert!(text.contains("4 0 0 4 20 20 cm"), "{text}");
    assert_eq!(
        text.matches("/pdfceAn").count(),
        6,
        "three invocations and three resource names"
    );
}

#[test]
fn a_ca_annotation_is_burned_inside_a_transparency_group_at_that_alpha() {
    let mut s = session(0);
    s.flatten_annotations(0, Some(&[id(8)])).unwrap();
    let g = s.graph();
    let slots = s.page_slots().unwrap();
    let page = g.resolved(slots[0].id).as_dict().cloned().unwrap();
    let res = page
        .get(b"Resources".as_slice())
        .map(|o| g.resolve(o).clone());
    let xobjs = res
        .as_ref()
        .and_then(Object::as_dict)
        .and_then(|r| r.get(b"XObject".as_slice()))
        .map(|o| g.resolve(o).clone())
        .and_then(|o| o.as_dict().cloned())
        .expect("page has an XObject resource");
    let (_, Object::Reference(wrapper)) = xobjs.iter().next().unwrap() else {
        panic!()
    };
    let w = g.resolved(*wrapper).as_dict().cloned().unwrap();
    let wres = w
        .get(b"Resources".as_slice())
        .and_then(Object::as_dict)
        .unwrap();
    let gs = wres
        .get(b"ExtGState".as_slice())
        .and_then(Object::as_dict)
        .and_then(|d| d.get(b"pdfceGs1".as_slice()))
        .and_then(Object::as_dict)
        .unwrap();
    assert_eq!(
        gs.get(b"CA".as_slice()).and_then(Object::as_number),
        Some(0.5)
    );
    assert_eq!(
        gs.get(b"ca".as_slice()).and_then(Object::as_number),
        Some(0.5)
    );
    let Some(Object::Reference(group)) = wres
        .get(b"XObject".as_slice())
        .and_then(Object::as_dict)
        .and_then(|d| d.get(b"pdfceGr1".as_slice()))
    else {
        panic!()
    };
    let gd = g.resolved(*group).as_dict().cloned().unwrap();
    assert!(gd.get(b"Group".as_slice()).is_some());
}

#[test]
fn an_oc_annotation_stays_on_its_layer() {
    let mut s = session(0);
    let out = s.flatten_annotations(0, Some(&[id(9)])).unwrap();
    assert_eq!(out.layered, 1);
    let g = s.graph();
    let slots = s.page_slots().unwrap();
    let page = g.resolved(slots[0].id).as_dict().cloned().unwrap();
    let xobjs = page
        .get(b"Resources".as_slice())
        .map(|o| g.resolve(o).clone())
        .and_then(|o| o.as_dict().cloned())
        .and_then(|r| r.get(b"XObject".as_slice()).cloned())
        .and_then(|o| o.as_dict().cloned())
        .unwrap();
    let (_, Object::Reference(wrapper)) = xobjs.iter().next().unwrap() else {
        panic!()
    };
    let w = g.resolved(*wrapper).as_dict().cloned().unwrap();
    assert_eq!(w.get(b"OC".as_slice()), Some(&Object::Reference(id(16))));
}

#[test]
fn a_named_refused_annotation_is_an_error_and_writes_nothing() {
    for (n, reason) in [
        (10, R::Link),
        (11, R::Hidden),
        (12, R::Locked),
        (13, R::HasAction),
        (7, R::NoAppearance),
        (6, R::Popup),
    ] {
        let mut s = session(0);
        let err = s.flatten_annotations(0, Some(&[id(5), id(n)])).unwrap_err();
        assert!(
            matches!(err, EditError::AnnotationNotFlattenable { id: i, reason: r } if i == id(n) && r == reason),
            "{n}: {err:?}"
        );
        assert!(!s.can_undo(), "{n}: nothing committed");
    }
}

#[test]
fn an_id_not_on_the_page_is_not_found() {
    let mut s = session(0);
    let err = s.flatten_annotations(0, Some(&[id(14)])).unwrap_err();
    assert!(
        matches!(err, EditError::AnnotationNotFound { .. }),
        "{err:?}"
    );
    let err = s.flatten_annotations(3, None).unwrap_err();
    assert!(matches!(err, EditError::PageOutOfRange { .. }), "{err:?}");
}

#[test]
fn no_rotate_on_a_rotated_page_is_burned() {
    // NoRotate is bit 5 (value 16); the pixel placement is pinned in
    // pdfcer-render's `flatten_annotations_look_the_same`.
    for rotate in [0, 90] {
        let s = EditSession::new(Document::from_bytes(fixture_bytes(rotate, 16)).unwrap());
        assert!(
            s.annotation_flatten_refusals(0)
                .unwrap()
                .iter()
                .all(|r| r.id != Some(id(5))),
            "rotate {rotate}"
        );
    }
}

#[test]
fn undo_restores_every_annotation() {
    let mut s = session(0);
    let before = reload(&s);
    s.flatten_annotations(0, None).unwrap();
    s.undo().expect("undo");
    let after = reload(&s);
    assert_eq!(dict(&after, 7), dict(&before, 7));
    assert_eq!(annots_left(after), annots_left(before));
}

#[test]
fn nothing_to_burn_commits_nothing() {
    let mut s = session(0);
    s.flatten_annotations(0, None).unwrap();
    let out = s.flatten_annotations(0, None).unwrap();
    assert!(!out.changed);
    assert_eq!(out.flattened, 0);
    assert_eq!(out.skipped.len(), 5);
}
