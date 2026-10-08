//! A `/Line`'s `/IC` (ISO 32000-1 §12.5.6.7 Table 175) fills its closed
//! line endings (pdfcer-gui request G153).

use pdfcer_core::annot_author::{Color, LineEnding, MarkupSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, MarkupStyle, MarkupStyleSupport, StyleEdit};
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_core::writer::SaveOptions;
use std::path::Path;

fn session() -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/annot/no-ap-circle.pdf");
    EditSession::new(Document::load(&path).expect("load fixture"))
}

fn arrow(interior: Option<Color>) -> MarkupSpec {
    MarkupSpec::Line {
        start: (10.0, 10.0),
        end: (80.0, 40.0),
        color: Color::Rgb(1.0, 0.0, 0.0),
        width: 1.0,
        endings: (LineEnding::None, LineEnding::ClosedArrow),
        interior,
    }
}

/// The saved annotation dictionary and its `/AP /N` content.
fn saved(s: &EditSession, id: ObjId) -> (Dict, String) {
    let bytes = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save")
        .0;
    let doc = Document::from_bytes(bytes).expect("re-parse");
    let Object::Dict(annot) = doc.get(id).expect("annotation").value.clone() else {
        panic!("not a dict");
    };
    let Some(Object::Dict(ap)) = annot.get(b"AP").map(|o| doc.resolve(o)) else {
        panic!("no /AP");
    };
    let Object::Stream(st) = doc.resolve(ap.get(b"N").expect("/N")) else {
        panic!("no /N stream");
    };
    let text =
        String::from_utf8_lossy(st.data_span.slice(doc.bytes()).expect("bytes")).into_owned();
    (annot, text)
}

/// Content-stream painting operators, in order.
fn paints(content: &str) -> Vec<&str> {
    content
        .split_whitespace()
        .filter(|t| matches!(*t, "S" | "B" | "f" | "b"))
        .collect()
}

#[test]
fn a_line_with_an_interior_fills_its_closed_arrow() {
    let mut s = session();
    let id = s
        .add_markup(0, &arrow(Some(Color::Rgb(0.0, 0.0, 1.0))))
        .unwrap();
    let (annot, ap) = saved(&s, id);
    let Some(Object::Array(ic)) = annot.get(b"IC") else {
        panic!("no /IC: {annot:?}");
    };
    assert_eq!(ic.len(), 3);
    assert!(ap.contains("0 0 1 rg"), "{ap}");
    assert_eq!(paints(&ap), ["S", "B"], "{ap}");
}

#[test]
fn a_line_without_an_interior_leaves_its_closed_arrow_hollow() {
    let mut s = session();
    let id = s.add_markup(0, &arrow(None)).unwrap();
    let (annot, ap) = saved(&s, id);
    assert!(!annot.contains_key(b"IC"));
    assert_eq!(paints(&ap), ["S", "S"], "{ap}");
}

#[test]
fn restyling_a_line_sets_and_clears_its_interior() {
    assert!(MarkupStyleSupport::for_subtype(b"Line").takes_interior);
    let mut s = session();
    let id = s.add_markup(0, &arrow(None)).unwrap();
    let green = |edit| MarkupStyle {
        interior: Some(edit),
        ..MarkupStyle::default()
    };
    s.set_markup_style(id, &green(StyleEdit::Set(Color::Rgb(0.0, 1.0, 0.0))))
        .unwrap();
    let (annot, ap) = saved(&s, id);
    assert!(annot.contains_key(b"IC"));
    assert!(ap.contains("0 1 0 rg"), "{ap}");
    assert_eq!(paints(&ap), ["S", "B"], "{ap}");

    // A stroke-only restyle keeps the fill: /IC is read back, not dropped.
    s.set_markup_style(
        id,
        &MarkupStyle {
            stroke: Some(StyleEdit::Set(Color::Gray(0.0))),
            ..MarkupStyle::default()
        },
    )
    .unwrap();
    assert!(saved(&s, id).0.contains_key(b"IC"));

    s.set_markup_style(id, &green(StyleEdit::Clear)).unwrap();
    let (annot, ap) = saved(&s, id);
    assert!(!annot.contains_key(b"IC"));
    assert_eq!(paints(&ap), ["S", "S"], "{ap}");
}
