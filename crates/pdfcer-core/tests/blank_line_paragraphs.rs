//! A word processor separates paragraphs with a whitespace-only line at the
//! normal line pitch, and sets the body in from its headings. Both the
//! reflow editor's options and the defaults must see one block per
//! paragraph: never the whole page as one block, never one block per line.

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree;
use pdfcer_core::text_edit::{
    BlockRecognitionOptions, EditableTextModel, reflow_recognition_options,
};
use pdfcer_core::text_extract::{self, ExtractOptions};

use crate::block_layout::doc_from_pages;

/// Heading at the margin, then paragraphs at a 36 pt body indent, each
/// separated by a `( )` line at the margin; every baseline 14 pt apart.
const LINES: &[(u32, &str)] = &[
    (50, "1. PURPOSE AND APPLICATION"),
    (50, " "),
    (86, "The office provides contractor laboratories"),
    (86, "with test procedures which serve as"),
    (86, "guidelines for obtaining test data."),
    (50, " "),
    (86, "Contractors submit a detailed procedure"),
    (86, "before the test program starts."),
    (50, " "),
    (50, "NOTE:"),
    (50, " "),
    (86, "The procedures are not intended to limit"),
    (86, "the contractor."),
];

fn page() -> Document {
    let mut content = String::new();
    for (k, &(x, text)) in LINES.iter().enumerate() {
        let y = 730 - 14 * k;
        content.push_str(&format!("BT /F1 12 Tf {x} {y} Td ({text}) Tj ET\n"));
    }
    doc_from_pages(&[&content], 0)
}

/// Each block's text, trimmed, blank blocks dropped.
fn paragraphs(options: &BlockRecognitionOptions) -> Vec<String> {
    let doc = page();
    let pages = page_tree::pages(&doc).expect("page tree");
    let text = text_extract::extract_page(&doc, &pages[0], 0, &ExtractOptions::default())
        .expect("extracts");
    let model = EditableTextModel::recognize(&text, options);
    assert_eq!(model.diagnostics().paragraph_breaks_by_blank_line, 4);
    model
        .blocks()
        .iter()
        .map(|b| {
            b.line_indices
                .iter()
                .map(|&i| model.line_text(&model.lines()[i]).trim().to_owned())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|t| !t.is_empty())
        .collect()
}

fn expected() -> Vec<String> {
    vec![
        "1. PURPOSE AND APPLICATION".to_owned(),
        "The office provides contractor laboratories with test procedures which serve as \
         guidelines for obtaining test data."
            .to_owned(),
        "Contractors submit a detailed procedure before the test program starts.".to_owned(),
        "NOTE:".to_owned(),
        "The procedures are not intended to limit the contractor.".to_owned(),
    ]
}

#[test]
fn the_reflow_editor_sees_one_block_per_paragraph() {
    assert_eq!(paragraphs(&reflow_recognition_options()), expected());
}

#[test]
fn the_default_options_do_not_split_an_indented_body_per_line() {
    assert_eq!(paragraphs(&BlockRecognitionOptions::default()), expected());
}

/// A heading drawn in two pieces on one baseline, the first's word-spaced
/// trailing space overlapping the second (so extraction makes two lines),
/// then a paragraph whose short last line sits under the first piece only.
#[test]
fn a_heading_in_two_overlapping_pieces_neither_splits_nor_founds_a_column() {
    let content = "BT /F1 12 Tf 22 Tw 50 730 Td (2. GENERAL ) Tj ET\n\
BT /F1 12 Tf 22 Tw 148 730 Td (REQUIREMENTS ) Tj ET\n\
BT /F1 12 Tf 50 716 Td ( ) Tj ET\n\
BT /F1 12 Tf 86 702 Td (Every seat belt anchorage in passenger cars) Tj ET\n\
BT /F1 12 Tf 86 688 Td (and multipurpose vehicles, trucks) Tj ET\n\
BT /F1 12 Tf 86 674 Td (and buses.) Tj ET\n";
    let doc = doc_from_pages(&[content], 0);
    let pages = page_tree::pages(&doc).expect("page tree");
    let text = text_extract::extract_page(&doc, &pages[0], 0, &ExtractOptions::default())
        .expect("extracts");
    let model = EditableTextModel::recognize(&text, &reflow_recognition_options());
    let blocks: Vec<String> = model
        .blocks()
        .iter()
        .map(|b| {
            b.line_indices
                .iter()
                .map(|&i| model.line_text(&model.lines()[i]).trim().to_owned())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|t| !t.is_empty())
        .collect();
    assert_eq!(model.columns(), 1, "{blocks:?}");
    assert_eq!(blocks.len(), 2, "{blocks:?}");
    assert!(blocks[0].contains("GENERAL") && blocks[0].contains("REQUIREMENTS"));
    assert!(blocks[1].ends_with("and buses."), "{blocks:?}");
}

#[test]
fn a_click_in_a_paragraph_hits_only_that_paragraph() {
    let session = EditSession::new(page());
    // The second line of the first body paragraph.
    let hit = session
        .block_at_point(0, 150.0, 690.0)
        .expect("reads")
        .expect("a block is under the point");
    assert_eq!(hit.line_ends.len(), 3, "{:?}", hit.text);
    assert!(hit.text.starts_with("The office"), "{:?}", hit.text);
}
