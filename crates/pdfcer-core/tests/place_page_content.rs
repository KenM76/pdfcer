//! `EditSession::place_page_content`: one page of another document drawn in
//! a page's content as a form XObject, `q cm /Fx Do Q`, with no annotation.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::object::{Dict, Object};
use pdfcer_core::page_tree::Rect;

/// A one-page document whose `/F1` is `base_font`, with `media` as its
/// `/MediaBox` and `body` as its content.
fn one_page(media: &str, body: &str, base_font: &str) -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [{media}] \
             /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>"
        ),
        format!("<< /Length {} >>\nstream\n{body}\nendstream", body.len()),
        format!("<< /Type /Font /Subtype /Type1 /BaseFont /{base_font} >>"),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    buf
}

const ARTWORK: &str = "1 0 0 RG 4 w 10 10 m 130 60 l S BT /F1 9 Tf 5 5 Td (src) Tj ET";

fn source(media: &str) -> Document {
    Document::from_bytes(one_page(media, ARTWORK, "Courier")).expect("source loads")
}

fn target() -> EditSession {
    let doc = Document::from_bytes(one_page("0 0 612 792", "BT /F1 12 Tf ET", "Helvetica"))
        .expect("target loads");
    EditSession::new(doc)
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect {
        llx: x,
        lly: y,
        urx: x + w,
        ury: y + h,
    }
}

fn page_dict(session: &EditSession) -> Dict {
    let id = session.pages().unwrap()[0].id;
    match session.value(id) {
        Some(Object::Dict(d)) => d.clone(),
        other => panic!("{other:?}"),
    }
}

fn content_streams(session: &EditSession) -> Vec<Vec<u8>> {
    let pages = session.pages().unwrap();
    let view = session.view();
    pages[0]
        .contents
        .iter()
        .map(|id| match view.graph().value(*id) {
            Some(Object::Stream(s)) => view.slice(s.data_span).unwrap_or_default().to_vec(),
            other => panic!("{other:?}"),
        })
        .collect()
}

/// `/F1`'s `/BaseFont` in `resources`, following one reference.
fn f1_base_font(session: &EditSession, resources: &Object) -> Vec<u8> {
    let deref = |o: &Object| match o {
        Object::Reference(id) => session.value(*id).cloned().unwrap(),
        o => o.clone(),
    };
    let res = deref(resources);
    let fonts = deref(res.as_dict().unwrap().get(b"Font").unwrap());
    let f1 = deref(fonts.as_dict().unwrap().get(b"F1").unwrap());
    f1.as_dict()
        .unwrap()
        .get(b"BaseFont")
        .and_then(Object::as_name)
        .unwrap()
        .0
        .clone()
}

#[test]
fn the_page_draws_the_form_and_gains_no_annotation() {
    let src = source("0 0 144 72");
    let mut session = target();
    let before = content_streams(&session);
    assert!(!before.is_empty() && !before[0].is_empty());

    let placed = session
        .place_page_content(&src.view(), 0, 0, rect(100.0, 100.0, 200.0, 100.0))
        .unwrap();

    assert!(page_dict(&session).get(b"Annots").is_none());
    let after = content_streams(&session);
    assert!(
        after.contains(&before[0]),
        "the existing content stays verbatim"
    );
    assert_eq!(
        session.pages().unwrap()[0].contents.last(),
        Some(&placed.content_id)
    );
    let drawn = String::from_utf8(after.last().unwrap().clone()).unwrap();
    assert!(drawn.contains(" Do"), "{drawn}");
    assert!(drawn.trim_start().starts_with('q') && drawn.trim_end().ends_with('Q'));
    assert!((placed.scale_x - 200.0 / 144.0).abs() < 1e-9);
    assert!((placed.scale_y - 100.0 / 72.0).abs() < 1e-9);
    assert!(!placed.distorted, "144x72 into 200x100 keeps its aspect");
    match session.value(placed.form_id) {
        Some(Object::Stream(s)) => assert_eq!(
            session.view().slice(s.data_span).unwrap(),
            ARTWORK.as_bytes()
        ),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_source_f1_and_the_targets_f1_keep_their_own_fonts() {
    let src = source("0 0 144 72");
    let mut session = target();
    let placed = session
        .place_page_content(&src.view(), 0, 0, rect(0.0, 0.0, 144.0, 72.0))
        .unwrap();
    assert_eq!(placed.resources_renamed, 0);

    let page = page_dict(&session);
    assert_eq!(
        f1_base_font(&session, page.get(b"Resources").unwrap()),
        b"Helvetica"
    );
    let form = match session.value(placed.form_id) {
        Some(Object::Stream(s)) => s.dict.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        f1_base_font(&session, form.get(b"Resources").unwrap()),
        b"Courier"
    );
}

#[test]
fn the_crop_box_origin_maps_onto_the_rect() {
    let src = source("50 20 194 92");
    let mut session = target();
    let placed = session
        .place_page_content(&src.view(), 0, 0, rect(300.0, 400.0, 144.0, 72.0))
        .unwrap();
    assert!(!placed.distorted);
    let drawn = String::from_utf8(content_streams(&session).last().unwrap().clone()).unwrap();
    // e = 300 - 50, f = 400 - 20 at unit scale.
    assert!(drawn.contains("1 0 0 1 250 380 cm"), "{drawn}");
}

#[test]
fn undo_removes_the_whole_placement_in_one_step() {
    let src = source("0 0 144 72");
    let mut session = target();
    let before = page_dict(&session);
    let placed = session
        .place_page_content(&src.view(), 0, 0, rect(10.0, 10.0, 144.0, 72.0))
        .unwrap();
    session.undo().expect("one undo");
    assert_eq!(page_dict(&session), before);
    assert!(session.value(placed.form_id).is_none());
    assert!(session.value(placed.content_id).is_none());
    assert!(!session.can_undo());
}

#[test]
fn a_degenerate_rect_is_refused_before_anything_is_written() {
    let src = source("0 0 144 72");
    let mut session = target();
    let err = session
        .place_page_content(&src.view(), 0, 0, rect(10.0, 10.0, 0.0, 50.0))
        .unwrap_err();
    assert!(
        matches!(err, EditError::ImageRectDegenerate { .. }),
        "{err:?}"
    );
    assert!(!session.can_undo());
}

#[test]
fn a_missing_source_page_is_named_as_the_sources() {
    let src = source("0 0 144 72");
    let mut session = target();
    let err = session
        .place_page_content(&src.view(), 3, 0, rect(10.0, 10.0, 50.0, 50.0))
        .unwrap_err();
    assert!(
        matches!(err, EditError::SourcePageOutOfRange { index: 3, count: 1 }),
        "{err:?}"
    );
}
