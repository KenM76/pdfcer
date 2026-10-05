//! An OCR layer written on an optional-content group: the text sits in the
//! group's `/OC` section inside the OCR marker, the layer is still found and
//! removed whole, and removal reports whether the group is left empty.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::path::Path;

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, LayerEdit, OcrPageLayer};
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::ocr::layer::{OcrLayerError, OcrLayerOptions};
use pdfcer_core::ocr::{OcrPage, RecognizedWord};
use pdfcer_core::page_tree::Rect;

fn session() -> EditSession {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/addtext/plain.pdf");
    EditSession::new(Document::load(&path).unwrap())
}

fn invoice() -> OcrPage {
    OcrPage {
        words: vec![RecognizedWord {
            text: "INVOICE".to_owned(),
            rect: Rect::from_corners(72.0, 600.0, 200.0, 612.0),
            confidence: Some(0.9),
        }],
        confidence_available: true,
        ..OcrPage::default()
    }
}

fn add(s: &mut EditSession, opts: &OcrLayerOptions) -> Result<(), OcrLayerError> {
    let recognised = invoice();
    let page = OcrPageLayer {
        page_index: 0,
        recognised: &recognised,
    };
    s.add_ocr_layer(&[page], opts).map(|_| ())
}

fn stream_text(s: &EditSession, id: ObjId) -> String {
    let Some(Object::Stream(st)) = s.value(id) else {
        panic!("layer content is a stream");
    };
    let view = s.view();
    let raw = view.slice(st.data_span).unwrap();
    let data = pdfcer_core::filters::decode_stream(&st.dict, raw).unwrap();
    String::from_utf8_lossy(&data).into_owned()
}

#[test]
fn the_layer_is_written_on_the_group_and_reports_it() {
    let mut s = session();
    let group = s.add_layer("OCR text", &LayerEdit::new()).unwrap();
    add(&mut s, &OcrLayerOptions::new().on_layer(group)).unwrap();

    let layers = s.find_ocr_layers().unwrap();
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].optional_content, Some(group));
    let text = stream_text(&s, layers[0].content);
    let marker = text.find("/pdfc_OCR").expect("marker");
    let oc = text.find("/OC /OC").expect("group section");
    assert!(marker < oc, "the marker stays outermost: {text}");
    assert!(text.trim_end().ends_with("EMC\nEMC"), "{text}");

    let outcome = s.remove_ocr_layer(&layers[0]).unwrap();
    assert_eq!(outcome.optional_content, Some(group));
    assert!(outcome.group_emptied, "nothing else draws on the group");
    assert!(s.find_ocr_layers().unwrap().is_empty());
}

#[test]
fn a_rerun_on_the_group_replaces_the_layer() {
    let mut s = session();
    let group = s.add_layer("OCR text", &LayerEdit::new()).unwrap();
    let opts = OcrLayerOptions::new().on_layer(group);
    add(&mut s, &opts).unwrap();
    add(&mut s, &opts).unwrap();
    let layers = s.find_ocr_layers().unwrap();
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].optional_content, Some(group));
}

#[test]
fn a_group_still_drawn_on_is_not_reported_empty() {
    let mut s = session();
    let group = s.add_layer("OCR text", &LayerEdit::new()).unwrap();
    s.set_objects_layer(0, &[0], Some(group)).unwrap();
    add(&mut s, &OcrLayerOptions::new().on_layer(group)).unwrap();
    let layers = s.find_ocr_layers().unwrap();
    let outcome = s.remove_ocr_layer(&layers[0]).unwrap();
    assert_eq!(outcome.optional_content, Some(group));
    assert!(!outcome.group_emptied, "the page's own text is still on it");
}

#[test]
fn a_layer_without_a_group_reports_none() {
    let mut s = session();
    add(&mut s, &OcrLayerOptions::new()).unwrap();
    let layers = s.find_ocr_layers().unwrap();
    assert_eq!(layers[0].optional_content, None);
    let outcome = s.remove_ocr_layer(&layers[0]).unwrap();
    assert_eq!(outcome.optional_content, None);
    assert!(!outcome.group_emptied);
}

#[test]
fn an_unregistered_group_is_refused() {
    let mut s = session();
    let page = s.find_ocr_layers().unwrap();
    assert!(page.is_empty());
    let not_a_group = ObjId::new(1, 0);
    let err = add(&mut s, &OcrLayerOptions::new().on_layer(not_a_group)).unwrap_err();
    assert!(
        matches!(err, OcrLayerError::NotALayerGroup { id } if id == not_a_group),
        "{err:?}"
    );
}
