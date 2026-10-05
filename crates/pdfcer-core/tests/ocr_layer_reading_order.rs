//! An OCR layer written from words alone extracts in reading order: a
//! two-column page, recognised row-major across both columns, reads the left
//! column to its end before the right.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::Path;

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, OcrPageLayer};
use pdfcer_core::ocr::layer::OcrLayerOptions;
use pdfcer_core::ocr::{OcrPage, RecognizedWord};
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_core::text_extract::{self, ExtractOptions, OcrLayerFilter};
use pdfcer_core::writer::SaveOptions;

fn two_columns_row_major() -> OcrPage {
    let mut words = Vec::new();
    for row in 0..12 {
        let y = 700.0 - f64::from(row) * 14.0;
        for (side, llx, urx) in [("L", 72.0, 260.0), ("R", 330.0, 520.0)] {
            words.push(RecognizedWord {
                text: format!("{side}{row}"),
                rect: Rect::from_corners(llx, y, urx, y + 10.0),
                confidence: Some(0.9),
            });
        }
    }
    OcrPage {
        words,
        confidence_available: true,
        ..OcrPage::default()
    }
}

#[test]
fn an_inferred_layer_extracts_column_by_column() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/addtext/plain.pdf");
    let mut s = EditSession::new(Document::load(&path).unwrap());
    let recognised = two_columns_row_major();
    let page = OcrPageLayer {
        page_index: 0,
        recognised: &recognised,
    };
    s.add_ocr_layer(&[page], &OcrLayerOptions::new()).unwrap();
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let doc = Document::from_bytes(bytes).unwrap();

    let pages = page_tree::pages(&doc).unwrap();
    let options = ExtractOptions::default().with_ocr_layer(OcrLayerFilter::OnlyOcrLayer);
    let text = text_extract::extract_page(&doc, &pages[0], 0, &options).unwrap();
    let words: Vec<String> = text
        .runs
        .iter()
        .flat_map(|r| r.text.split_whitespace().map(str::to_owned).collect::<Vec<_>>())
        .collect();
    assert_eq!(words.len(), 24, "{words:?}");
    let last_left = words.iter().rposition(|w| w.starts_with('L')).unwrap();
    let first_right = words.iter().position(|w| w.starts_with('R')).unwrap();
    assert!(last_left < first_right, "column by column: {words:?}");
}
