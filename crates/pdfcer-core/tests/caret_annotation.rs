//! `add_caret_annotation` and `add_replace_text` (`Pass 261.1`, §12.5.6.11):
//! the caret alone, and the caret + strikeout pair grouped as one Replace Text
//! edit, read back from the saved bytes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::annot_author::{CaretSpec, CaretSymbol, Quad};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, MarkupNote, MarkupOptions};
use pdfcer_core::object::{Name, ObjId, Object};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::writer::SaveOptions;

fn one_page_pdf() -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Resources << >> >>",
    ];
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 4\n0000000000 65535 f \n");
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    buf
}

const RECT: Rect = Rect {
    llx: 72.0,
    lly: 300.0,
    urx: 92.0,
    ury: 324.0,
};

fn session() -> EditSession {
    EditSession::new(Document::from_bytes(one_page_pdf()).unwrap())
}

fn saved(s: &EditSession) -> Document {
    Document::from_bytes(s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0).unwrap()
}

fn dict(doc: &Document, id: ObjId) -> pdfcer_core::object::Dict {
    match &doc.get(id).unwrap().value {
        Object::Dict(d) => d.clone(),
        other => panic!("{id:?} is not a dictionary: {other:?}"),
    }
}

fn name(d: &pdfcer_core::object::Dict, key: &[u8]) -> Option<Name> {
    d.get(key).and_then(Object::as_name).cloned()
}

fn annots(doc: &Document) -> Vec<ObjId> {
    let page = dict(doc, ObjId::new(3, 0));
    let arr = match page.get(b"Annots") {
        Some(Object::Array(a)) => a.clone(),
        Some(Object::Reference(r)) => match &doc.get(*r).unwrap().value {
            Object::Array(a) => a.clone(),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    };
    arr.iter()
        .map(|o| match o {
            Object::Reference(r) => *r,
            other => panic!("{other:?}"),
        })
        .collect()
}

fn struck() -> Vec<Quad> {
    vec![Quad {
        ul: (110.0, 322.0),
        ur: (180.0, 322.0),
        ll: (110.0, 308.0),
        lr: (180.0, 308.0),
    }]
}

#[test]
fn a_caret_carries_its_inserted_text_and_omits_sy_by_default() {
    let mut s = session();
    let options = MarkupOptions {
        note: Some(MarkupNote::new("not ").by("Ken")),
        opacity: Some(0.5),
        ..Default::default()
    };
    let id = s
        .add_caret_annotation(0, &CaretSpec::new(RECT), &options)
        .unwrap();
    let doc = saved(&s);
    let d = dict(&doc, id);
    assert_eq!(name(&d, b"Subtype"), Some(Name::from(b"Caret")));
    assert!(matches!(d.get(b"Contents"), Some(Object::String(t)) if t == b"not "));
    assert!(matches!(d.get(b"T"), Some(Object::String(t)) if t == b"Ken"));
    assert!(matches!(d.get(b"CA"), Some(Object::Real(a)) if (*a - 0.5).abs() < 1e-9));
    assert!(!d.contains_key(b"Sy"), "absent /Sy means None");
    assert!(!d.contains_key(b"RD"));
    assert!(!d.contains_key(b"IT"));
    assert!(matches!(d.get(b"AP"), Some(Object::Dict(_))));
    assert_eq!(annots(&doc), vec![id]);
}

#[test]
fn a_paragraph_caret_writes_sy_p() {
    let mut s = session();
    let mut spec = CaretSpec::new(RECT);
    spec.symbol = CaretSymbol::Paragraph;
    let id = s
        .add_caret_annotation(0, &spec, &MarkupOptions::default())
        .unwrap();
    let d = dict(&saved(&s), id);
    assert_eq!(name(&d, b"Sy"), Some(Name::from(b"P")));
    assert!(!d.contains_key(b"Contents"));
}

#[test]
fn replace_text_groups_a_strikeout_under_the_caret() {
    let mut s = session();
    let options = MarkupOptions {
        note: Some(MarkupNote::new("replacement").by("Ken")),
        ..Default::default()
    };
    let added = s
        .add_replace_text(0, &CaretSpec::new(RECT), &struck(), &options)
        .unwrap();
    let doc = saved(&s);

    let caret = dict(&doc, added.caret_id);
    assert_eq!(name(&caret, b"Subtype"), Some(Name::from(b"Caret")));
    assert_eq!(name(&caret, b"IT"), Some(Name::from(b"Replace")));
    assert!(matches!(caret.get(b"Contents"), Some(Object::String(t)) if t == b"replacement"));
    assert!(!caret.contains_key(b"IRT"));

    let strike = dict(&doc, added.strike_out_id);
    assert_eq!(name(&strike, b"Subtype"), Some(Name::from(b"StrikeOut")));
    assert_eq!(name(&strike, b"IT"), Some(Name::from(b"StrikeOutTextEdit")));
    assert_eq!(name(&strike, b"RT"), Some(Name::from(b"Group")));
    assert!(matches!(strike.get(b"IRT"), Some(Object::Reference(r)) if *r == added.caret_id));
    assert!(matches!(strike.get(b"QuadPoints"), Some(Object::Array(q)) if q.len() == 8));
    assert!(matches!(strike.get(b"AP"), Some(Object::Dict(_))));
    assert!(
        !strike.contains_key(b"Contents"),
        "the group's note is the caret's"
    );
    assert!(!strike.contains_key(b"T"));
    assert_eq!(strike.get(b"C"), caret.get(b"C"), "one colour for the pair");

    assert_eq!(annots(&doc), vec![added.caret_id, added.strike_out_id]);
}

#[test]
fn one_undo_removes_both_halves_of_a_replace_text() {
    let mut s = session();
    let before = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    s.add_replace_text(
        0,
        &CaretSpec::new(RECT),
        &struck(),
        &MarkupOptions::default(),
    )
    .unwrap();
    assert!(s.undo().is_some());
    assert_eq!(
        s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0,
        before
    );
}

#[test]
fn empty_struck_quads_and_a_bad_page_are_refused_by_name() {
    let mut s = session();
    let err = s
        .add_replace_text(0, &CaretSpec::new(RECT), &[], &MarkupOptions::default())
        .unwrap_err();
    assert!(matches!(err, EditError::EmptyGeometry), "{err:?}");
    let err = s
        .add_caret_annotation(2, &CaretSpec::new(RECT), &MarkupOptions::default())
        .unwrap_err();
    assert!(
        matches!(err, EditError::PageOutOfRange { index: 2, count: 1 }),
        "{err:?}"
    );
    let err = s
        .add_caret_annotation(
            0,
            &CaretSpec::new(RECT),
            &MarkupOptions {
                opacity: Some(1.5),
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(
        matches!(err, EditError::MarkupOpacityOutOfRange { .. }),
        "{err:?}"
    );
}
