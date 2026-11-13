//! `page_overprint`: "set" and "takes effect" on fixtures whose overprint
//! behaviour the overprint provenance table already pins.

use pdfcer_core::document::Document;
use pdfcer_render::{OverprintPreview, PageOverprint, RenderOptions, page_overprint};

fn answer(bytes: &[u8]) -> PageOverprint {
    let doc = Document::from_bytes(bytes.to_vec()).expect("fixture parses");
    let pages = pdfcer_core::page_tree::pages(&doc).expect("page tree");
    page_overprint(&doc.view(), &pages[0], &RenderOptions::default()).expect("renders")
}

#[test]
fn a_k_overprint_over_a_spot_is_set_and_takes_effect() {
    let found = answer(include_bytes!(
        "../../../fixtures/synthetic/overprint/cmyk_k_op_over_spot.pdf"
    ));
    assert!(found.set && found.takes_effect, "{found:?}");
}

#[test]
fn naming_every_process_ink_is_set_without_effect() {
    let found = answer(include_bytes!(
        "../../../fixtures/synthetic/overprint/devicen_all4_op_cmyk.pdf"
    ));
    assert!(found.set, "{found:?}");
    assert!(!found.takes_effect, "{found:?}");
}

#[test]
fn a_page_without_overprint_says_no() {
    let found = answer(include_bytes!(
        "../../../fixtures/synthetic/overprint/spot_only_noop_cmyk.pdf"
    ));
    assert_eq!(found, PageOverprint::default());
}

#[test]
fn the_answer_ignores_a_callers_overprint_preview_off() {
    let bytes = include_bytes!("../../../fixtures/synthetic/overprint/cmyk_k_op_over_spot.pdf");
    let doc = Document::from_bytes(bytes.to_vec()).expect("fixture parses");
    let pages = pdfcer_core::page_tree::pages(&doc).expect("page tree");
    let off = RenderOptions::default().with_overprint_preview(OverprintPreview::Off);
    let found = page_overprint(&doc.view(), &pages[0], &off).expect("renders");
    assert!(found.takes_effect, "{found:?}");
}

#[test]
fn from_diagnostics_reads_the_same_counters() {
    let bytes = include_bytes!("../../../fixtures/synthetic/overprint/cmyk_k_op_over_spot.pdf");
    let doc = Document::from_bytes(bytes.to_vec()).expect("fixture parses");
    let pages = pdfcer_core::page_tree::pages(&doc).expect("page tree");
    let rendered = pdfcer_render::render_page_with_view(
        &doc.view(),
        &pages[0],
        0.5,
        &RenderOptions::default(),
    )
    .expect("renders");
    let found = PageOverprint::from_diagnostics(&rendered.diagnostics);
    assert!(found.set && found.takes_effect, "{found:?}");
}
