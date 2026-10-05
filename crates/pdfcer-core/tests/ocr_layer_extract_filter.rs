//! Extraction tells a pdfcer OCR layer's text from the page's own, and the
//! `ExtractOptions::ocr_layer` filter keeps either side.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::Path;

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, OcrPageLayer};
use pdfcer_core::ocr::layer::OcrLayerOptions;
use pdfcer_core::ocr::{OcrPage, RecognizedWord};
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_core::text_extract::{self, ExtractOptions, OcrLayerFilter, PageText};
use pdfcer_core::writer::SaveOptions;

/// `plain.pdf` (visible "Original") with an OCR layer reading "INVOICE".
fn mixed_page() -> Document {
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
    };
    let page = OcrPageLayer {
        page_index: 0,
        recognised: &recognised,
    };
    s.add_ocr_layer(&[page], &OcrLayerOptions::new()).unwrap();
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    Document::from_bytes(bytes).unwrap()
}

fn extract(doc: &Document, filter: OcrLayerFilter) -> PageText {
    let pages = page_tree::pages(doc).unwrap();
    let options = ExtractOptions::default().with_ocr_layer(filter);
    text_extract::extract_page(doc, &pages[0], 0, &options).unwrap()
}

fn text(page: &PageText) -> String {
    page.runs.iter().map(|r| r.text.as_str()).collect()
}

#[test]
fn the_ocr_layer_run_is_flagged_and_the_page_text_is_not() {
    let page = extract(&mixed_page(), OcrLayerFilter::All);
    let flag = |needle: &str| {
        page.runs
            .iter()
            .find(|r| r.text.contains(needle))
            .map(|r| r.in_ocr_layer)
    };
    assert_eq!(flag("INVOICE"), Some(true), "{:?}", text(&page));
    assert_eq!(flag("Original"), Some(false), "{:?}", text(&page));
}

#[test]
fn each_filter_yields_its_side() {
    let doc = mixed_page();
    let all = text(&extract(&doc, OcrLayerFilter::All));
    assert!(
        all.contains("INVOICE") && all.contains("Original"),
        "{all:?}"
    );
    let only = text(&extract(&doc, OcrLayerFilter::OnlyOcrLayer));
    assert!(
        only.contains("INVOICE") && !only.contains("Original"),
        "{only:?}"
    );
    let without = text(&extract(&doc, OcrLayerFilter::WithoutOcrLayer));
    assert!(
        !without.contains("INVOICE") && without.contains("Original"),
        "{without:?}"
    );
}

#[test]
fn the_default_keeps_all_text() {
    let doc = mixed_page();
    let pages = page_tree::pages(&doc).unwrap();
    let default = text_extract::extract_page(&doc, &pages[0], 0, &ExtractOptions::default());
    assert_eq!(
        text(&default.unwrap()),
        text(&extract(&doc, OcrLayerFilter::All))
    );
}
