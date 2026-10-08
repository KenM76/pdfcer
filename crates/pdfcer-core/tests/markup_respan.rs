//! `respan_text_markup` replaces a text markup's `/QuadPoints` and re-bakes
//! its appearance (pdfcer-gui request G152), keeping colour, comment and
//! object identity, and survives save and reopen.

use pdfcer_core::annot::AnnotFlags;
use pdfcer_core::annot_author::{Color, MarkupSpec, Quad, TextMarkupKind};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditError, EditSession, MarkupNote};
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::writer::SaveOptions;

const BLANK: &[u8] = b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >> endobj\n\
trailer << /Size 4 /Root 1 0 R >>\n";

fn quad(llx: f64, lly: f64, urx: f64, ury: f64) -> Quad {
    Quad::from_rect(Rect { llx, lly, urx, ury })
}

/// A session holding one yellow highlight over one line, with a comment.
fn highlighted(kind: TextMarkupKind) -> (EditSession, ObjId) {
    let mut s = EditSession::new(Document::from_bytes(BLANK.to_vec()).expect("rebuildable"));
    let id = s
        .add_markup(
            0,
            &MarkupSpec::TextMarkup {
                kind,
                quads: vec![quad(72.0, 700.0, 200.0, 712.0)],
                color: Color::Rgb(1.0, 1.0, 0.0),
            },
        )
        .expect("place");
    s.set_markup_note(id, &MarkupNote::new("check this"))
        .expect("comment");
    (s, id)
}

fn reopened(s: &EditSession) -> EditSession {
    let bytes = s.to_full_bytes(&SaveOptions::identity()).expect("save").0;
    EditSession::new(Document::from_bytes(bytes).expect("re-parse"))
}

fn numbers(s: &EditSession, id: ObjId, key: &[u8]) -> Vec<f64> {
    let Some(Object::Dict(d)) = s.value(id) else {
        panic!("not a dict")
    };
    let Some(Object::Array(a)) = d.get(key).map(|o| s.graph().resolve(o).clone()) else {
        panic!("no array")
    };
    a.iter()
        .map(|o| match o {
            Object::Integer(i) => *i as f64,
            Object::Real(r) => *r,
            other => panic!("{other:?}"),
        })
        .collect()
}

fn key(s: &EditSession, id: ObjId, k: &[u8]) -> Option<Object> {
    let Some(Object::Dict(d)) = s.value(id) else {
        panic!("not a dict")
    };
    d.get(k).cloned()
}

/// The raw `/AP` `/N` bytes of `id` in the saved form of `s`.
fn ap_bytes(s: &EditSession, id: ObjId) -> Vec<u8> {
    let bytes = s.to_full_bytes(&SaveOptions::identity()).expect("save").0;
    let doc = Document::from_bytes(bytes).expect("re-parse");
    let Object::Dict(d) = &doc.get(id).expect("annot").value else {
        panic!("not a dict")
    };
    let Some(Object::Dict(ap)) = d.get(b"AP") else {
        panic!("no /AP")
    };
    let n = ap.get(b"N").and_then(Object::as_reference).expect("/N ref");
    match &doc.get(n).expect("stream").value {
        Object::Stream(st) => st.data_span.slice(doc.bytes()).expect("raw").to_vec(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_respan_moves_quads_rect_and_appearance_and_keeps_the_rest() {
    let (mut s, id) = highlighted(TextMarkupKind::Highlight);
    let colour = key(&s, id, b"C");
    let before = ap_bytes(&s, id);
    let two = [
        quad(72.0, 700.0, 300.0, 712.0),
        quad(72.0, 686.0, 150.0, 698.0),
    ];
    let change = s.respan_text_markup(id, &two, None).expect("respan");
    assert_eq!((change.quads_before, change.quads_after), (1, 2));
    assert_eq!(change.annot_id, id);
    assert!(change.dropped.is_empty(), "{:?}", change.dropped);
    assert!(!change.mod_date_written);
    assert!(change.rect_after.urx >= 300.0 && change.rect_after.lly <= 686.0);

    let back = reopened(&s);
    assert_eq!(numbers(&back, id, b"QuadPoints").len(), 16);
    assert_eq!(key(&back, id, b"C"), colour);
    assert_eq!(
        key(&back, id, b"Contents"),
        Some(Object::String(b"check this".to_vec()))
    );
    assert_ne!(ap_bytes(&s, id), before, "the appearance was re-baked");
}

#[test]
fn every_text_markup_kind_respans() {
    for kind in [
        TextMarkupKind::Underline,
        TextMarkupKind::StrikeOut,
        TextMarkupKind::Squiggly,
    ] {
        let (mut s, id) = highlighted(kind);
        s.respan_text_markup(id, &[quad(72.0, 700.0, 120.0, 712.0)], None)
            .unwrap_or_else(|e| panic!("{kind:?}: {e}"));
        let q = numbers(&s, id, b"QuadPoints");
        assert_eq!(q[2], 120.0, "{kind:?}: {q:?}");
    }
}

#[test]
fn a_respan_is_one_undo_and_undo_restores_the_old_span() {
    let (mut s, id) = highlighted(TextMarkupKind::Highlight);
    let old = numbers(&s, id, b"QuadPoints");
    let depth = s.undo_kinds().len();
    s.respan_text_markup(
        id,
        &[quad(72.0, 700.0, 400.0, 712.0)],
        Some("D:20261007120000Z"),
    )
    .expect("respan");
    assert_eq!(s.undo_kinds().len(), depth + 1);
    assert_eq!(
        key(&s, id, b"M"),
        Some(Object::String(b"D:20261007120000Z".to_vec()))
    );
    assert_eq!(s.undo(), Some(CommandKind::RespanTextMarkup));
    assert_eq!(numbers(&s, id, b"QuadPoints"), old);
}

#[test]
fn empty_and_non_finite_quads_are_refused_and_nothing_changes() {
    let (mut s, id) = highlighted(TextMarkupKind::Highlight);
    let depth = s.undo_kinds().len();
    let err = s.respan_text_markup(id, &[], None).unwrap_err();
    assert!(matches!(err, EditError::EmptyGeometry), "{err:?}");
    let err = s
        .respan_text_markup(id, &[quad(72.0, 700.0, f64::NAN, 712.0)], None)
        .unwrap_err();
    assert!(
        matches!(err, EditError::AnnotationVertexNotPlaceable { .. }),
        "{err:?}"
    );
    assert_eq!(s.undo_kinds().len(), depth);
}

#[test]
fn locked_refuses_and_locked_contents_does_not() {
    let (mut s, id) = highlighted(TextMarkupKind::Highlight);
    s.set_annotation_flags(id, AnnotFlags(AnnotFlags::LOCKED))
        .expect("lock");
    let err = s
        .respan_text_markup(id, &[quad(0.0, 0.0, 10.0, 10.0)], None)
        .unwrap_err();
    assert!(matches!(err, EditError::AnnotationLocked { .. }), "{err:?}");

    let (mut s, id) = highlighted(TextMarkupKind::Highlight);
    s.set_annotation_flags(id, AnnotFlags(AnnotFlags::LOCKED_CONTENTS))
        .expect("lock contents");
    s.respan_text_markup(id, &[quad(0.0, 0.0, 10.0, 10.0)], None)
        .expect("LockedContents guards the comment, not the span");
}

#[test]
fn a_shape_is_refused_by_name() {
    let mut s = EditSession::new(Document::from_bytes(BLANK.to_vec()).expect("rebuildable"));
    let id = s
        .add_markup(
            0,
            &MarkupSpec::Square {
                rect: Rect {
                    llx: 10.0,
                    lly: 10.0,
                    urx: 50.0,
                    ury: 50.0,
                },
                border: Some(Color::Gray(0.0)),
                interior: None,
                border_width: 1.0,
                border_effect: None,
            },
        )
        .expect("place");
    let err = s
        .respan_text_markup(id, &[quad(0.0, 0.0, 10.0, 10.0)], None)
        .unwrap_err();
    assert!(
        matches!(&err, EditError::TextMarkupVerbOnOther { subtype, .. } if subtype == "Square"),
        "{err:?}"
    );
}
