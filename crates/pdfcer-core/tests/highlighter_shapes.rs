//! `MarkupOptions::highlighter` (`Pass 695.0`): a shape painted Multiply,
//! on the annotation (`/BM`) and inside its appearance (`/GS0 gs`), and
//! kept that way by an edit that regenerates the appearance.

#![allow(clippy::unwrap_used, clippy::expect_used)] // Tests fail loudly.

use std::path::{Path, PathBuf};

use pdfcer_core::annot_author::{self, Color, MarkupSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, MarkupOptions, ResizeOptions};
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::writer::SaveOptions;

fn session() -> EditSession {
    let p: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/annot/demo-annotated.pdf");
    EditSession::new(Document::load(&p).expect("load fixture"))
}

fn reload(s: &EditSession) -> Document {
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("incremental save");
    Document::from_bytes(bytes).expect("re-parse")
}

fn dict_of(doc: &Document, id: ObjId) -> Dict {
    match &doc.get(id).expect("object present").value {
        Object::Dict(d) => d.clone(),
        other => panic!("{id} is not a dictionary: {other:?}"),
    }
}

/// The `/AP /N` stream's dictionary and raw bytes.
fn ap(doc: &Document, d: &Dict) -> (Dict, Vec<u8>) {
    let Some(Object::Dict(ap)) = d.get(b"AP") else {
        panic!("no /AP");
    };
    let n = ap.get(b"N").and_then(Object::as_reference).expect("/N ref");
    match &doc.get(n).expect("stream present").value {
        Object::Stream(st) => (
            st.dict.clone(),
            st.data_span.slice(doc.bytes()).expect("raw").to_vec(),
        ),
        other => panic!("/N is not a stream: {other:?}"),
    }
}

fn circle() -> MarkupSpec {
    MarkupSpec::Circle {
        rect: Rect::from_corners(100.0, 100.0, 200.0, 140.0),
        border: None,
        interior: Some(Color::Rgb(1.0, 1.0, 0.0)),
        border_width: 1.0,
    }
}

fn highlighter() -> MarkupOptions {
    MarkupOptions {
        highlighter: true,
        ..Default::default()
    }
}

fn assert_multiply(doc: &Document, id: ObjId) {
    let d = dict_of(doc, id);
    assert!(annot_author::is_highlighter(&d), "no /BM /Multiply: {d:?}");
    let (stream_dict, bytes) = ap(doc, &d);
    assert!(
        bytes.starts_with(b"/GS0 gs\n"),
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    let res = format!("{:?}", stream_dict.get(b"Resources"));
    assert!(
        res.contains("Multiply"),
        "GS0 not in the AP resources: {res}"
    );
}

#[test]
fn a_highlighter_circle_is_multiply_on_the_annotation_and_in_its_appearance() {
    let mut s = session();
    let id = s.add_markup_with(0, &circle(), &highlighter()).unwrap();
    assert_multiply(&reload(&s), id);
}

#[test]
fn a_resize_keeps_the_highlighter_blend() {
    let mut s = session();
    let id = s.add_markup_with(0, &circle(), &highlighter()).unwrap();
    s.resize_annotation(id, (100.0, 100.0), 2.0, 1.5, &ResizeOptions::default())
        .unwrap();
    assert_multiply(&reload(&s), id);
}

#[test]
fn a_plain_circle_is_not_a_highlighter() {
    let mut s = session();
    let id = s.add_markup(0, &circle()).unwrap();
    let doc = reload(&s);
    let d = dict_of(&doc, id);
    assert!(!annot_author::is_highlighter(&d));
    assert!(!ap(&doc, &d).1.starts_with(b"/GS0 gs"));
}

#[test]
fn a_line_ignores_the_option() {
    let spec = MarkupSpec::Line {
        start: (100.0, 100.0),
        end: (200.0, 100.0),
        color: Color::Rgb(1.0, 0.0, 0.0),
        width: 1.0,
        endings: (
            annot_author::LineEnding::None,
            annot_author::LineEnding::None,
        ),
        interior: None,
    };
    assert!(!annot_author::takes_highlighter(&spec));
}
