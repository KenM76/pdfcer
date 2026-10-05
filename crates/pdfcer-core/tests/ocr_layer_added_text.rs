//! Text an operator adds into a page's OCR layer is part of that layer:
//! found, removed and replaced with it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::path::Path;

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, LayerEdit, OcrPageLayer};
use pdfcer_core::ocr::layer::OcrLayerOptions;
use pdfcer_core::ocr::{OcrPage, RecognizedWord};
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_core::text_edit::{AddTextError, AddTextRequest};
use pdfcer_core::text_extract::{self, ExtractOptions};
use pdfcer_core::writer::SaveOptions;

fn plain() -> Document {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/addtext/plain.pdf");
    Document::load(&path).unwrap()
}

fn ocr(s: &mut EditSession) {
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
}

fn page_text(s: &EditSession) -> String {
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    let page = text_extract::extract_page(&doc, &pages[0], 0, &ExtractOptions::default()).unwrap();
    page.runs.iter().map(|r| r.text.as_str()).collect()
}

fn missed_word() -> AddTextRequest {
    AddTextRequest::new(0, (72.0, 500.0), "TOTAL").into_ocr_layer()
}

#[test]
fn added_text_is_found_as_a_manual_layer() {
    let mut s = EditSession::new(plain());
    s.add_text(&missed_word()).unwrap();
    let layers = s.find_ocr_layers().unwrap();
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].engine.as_deref(), Some("manual"));
    assert!(page_text(&s).contains("TOTAL"));
    assert_eq!(missed_word().render_mode, 3, "OCR text is invisible");
}

#[test]
fn removing_the_layer_takes_the_added_text() {
    let mut s = EditSession::new(plain());
    s.add_text(&missed_word()).unwrap();
    let layers = s.find_ocr_layers().unwrap();
    s.remove_ocr_layer(&layers[0]).unwrap();
    let text = page_text(&s);
    assert!(!text.contains("TOTAL"), "{text:?}");
    assert!(text.contains("Original"), "{text:?}");
}

#[test]
fn an_ocr_rerun_replaces_the_added_text() {
    let mut s = EditSession::new(plain());
    ocr(&mut s);
    s.add_text(&missed_word()).unwrap();
    assert_eq!(s.find_ocr_layers().unwrap().len(), 2);
    ocr(&mut s);
    let layers = s.find_ocr_layers().unwrap();
    assert_eq!(layers.len(), 1);
    assert_eq!(layers[0].engine, None);
    assert!(!page_text(&s).contains("TOTAL"));
}

#[test]
fn added_text_on_a_group_keeps_the_marker_outermost() {
    let mut s = EditSession::new(plain());
    let group = s.add_layer("OCR text", &LayerEdit::new()).unwrap();
    s.add_text(&missed_word().on_layer(group)).unwrap();
    let layers = s.find_ocr_layers().unwrap();
    assert_eq!(layers.len(), 1, "the marker must open the stream");
    assert_eq!(layers[0].optional_content, Some(group));
}

#[test]
fn the_one_shot_route_refuses() {
    let err = pdfcer_core::text_edit::add_text(&plain(), &missed_word()).unwrap_err();
    assert!(matches!(err, AddTextError::OcrLayerNeedsSession), "{err:?}");
}
