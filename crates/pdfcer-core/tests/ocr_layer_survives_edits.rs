//! An edit on a page carrying a pdfcer OCR layer leaves the layer a layer.
//!
//! Every content edit folds the page's `/Contents` into its first stream. The
//! layer must come back out as its own stream, or `find_ocr_layers` stops
//! seeing it and `remove_ocr_layer` / a re-run under `Replace` lose it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::path::Path;

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, LayerEdit, OcrPageLayer};
use pdfcer_core::ocr::layer::OcrLayerOptions;
use pdfcer_core::ocr::{OcrPage, RecognizedWord};
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_core::text_edit::{EditOptions, EditRequest};
use pdfcer_core::text_extract::{self, ExtractOptions};
use pdfcer_core::writer::SaveOptions;

/// A one-page session whose page carries one OCR layer reading `INVOICE`.
fn layered() -> EditSession {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/addtext/plain.pdf");
    let mut s = EditSession::new(Document::load(&path).unwrap());
    let recognised = OcrPage {
        words: vec![RecognizedWord {
            text: "INVOICE".to_owned(),
            rect: Rect::from_corners(72.0, 600.0, 200.0, 612.0),
            confidence: Some(0.9),
        }],
        confidence_available: true,
        ..OcrPage::default()
    };
    let page = OcrPageLayer {
        page_index: 0,
        recognised: &recognised,
    };
    s.add_ocr_layer(&[page], &OcrLayerOptions::new()).unwrap();
    assert_eq!(s.find_ocr_layers().unwrap().len(), 1);
    s
}

fn page_text(s: &EditSession) -> String {
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    let page = text_extract::extract_page(&doc, &pages[0], 0, &ExtractOptions::default()).unwrap();
    page.runs.iter().map(|r| r.text.as_str()).collect()
}

/// The layer is still found once, and removing it takes the OCR text and
/// leaves `kept` on the page.
fn assert_layer_survives(mut s: EditSession, ocr_text: &str, kept: &str) {
    let layers = s.find_ocr_layers().unwrap();
    assert_eq!(
        layers.len(),
        1,
        "the edit must not cost the page its OCR layer"
    );
    assert!(page_text(&s).contains(ocr_text));
    s.remove_ocr_layer(&layers[0]).unwrap();
    let text = page_text(&s);
    assert!(
        text.contains(kept),
        "the page's own text survives: {text:?}"
    );
    assert!(
        !text.contains(ocr_text),
        "the layer's text is removed: {text:?}"
    );
    assert!(s.find_ocr_layers().unwrap().is_empty());
}

#[test]
fn editing_page_text_keeps_the_layer() {
    let mut s = layered();
    let req = EditRequest::find_replace(0, "Original", "Changed");
    s.edit_text(&req, &EditOptions::default()).unwrap();
    assert_layer_survives(s, "INVOICE", "Changed");
}

#[test]
fn editing_the_layers_own_word_keeps_the_layer() {
    let mut s = layered();
    let req = EditRequest::find_replace(0, "INVOICE", "INVOlCE");
    s.edit_text(&req, &EditOptions::default()).unwrap();
    assert_layer_survives(s, "INVOlCE", "Original");
}

#[test]
fn moving_an_object_keeps_the_layer() {
    let mut s = layered();
    s.move_object(0, 0, 1.0, 0.0).unwrap();
    assert_layer_survives(s, "INVOICE", "Original");
}

#[test]
fn putting_the_layer_on_an_optional_content_group_keeps_the_layer() {
    let mut s = layered();
    let group = s.add_layer("OCR text", &LayerEdit::new()).unwrap();
    s.set_objects_layer(0, &[1], Some(group)).unwrap();
    assert_layer_survives(s, "INVOICE", "Original");
}

#[test]
fn an_untouched_layer_stream_is_not_rewritten() {
    let mut s = layered();
    let id = s.find_ocr_layers().unwrap()[0].content;
    let before = s.value(id).cloned();
    let req = EditRequest::find_replace(0, "Original", "Changed");
    s.edit_text(&req, &EditOptions::default()).unwrap();
    assert_eq!(
        s.value(id).cloned(),
        before,
        "an edit elsewhere on the page must not rewrite the layer's stream"
    );
}
