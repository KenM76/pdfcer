//! The `render_cmyk_page` fuzz target is only worth running while its fixed
//! page actually composites in the colorant buffer. This pins that, and
//! that every resource the page offers renders from the seed content.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "../../../fuzz/render_cmyk_page.rs"]
mod render_cmyk_page;

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{InkProbeSource, RenderOptions, render_page_with};

/// Names every resource the page offers, as the committed fuzz seed does.
const SEED: &[u8] = b"/Gi Do /Gn Do /Gk Do q 48 0 0 48 0 0 cm /Im Do Q \
q /OP gs 24 0 0 24 12 12 cm /Ii Do Q /DN cs 0.5 0.7 scn 0 0 10 48 re f \
q /SM gs /Sh sh Q q /L gs 0 0 0 1 k 30 0 18 18 re f Q";

#[test]
fn the_fuzz_page_renders_through_the_colorant_buffer() {
    let doc = Document::from_bytes(render_cmyk_page::page_pdf(SEED)).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    let options = RenderOptions::default().with_ink_probe(24, 24);
    let r = render_page_with(&doc, &pages[0], 1.0, &options).unwrap();
    let probe = r.diagnostics.ink_probe.expect("probe inside the page");
    assert_eq!(probe.source, InkProbeSource::CmykBuffer);
    let painted = r
        .pixmap
        .data()
        .chunks(4)
        .filter(|p| p[..3] != [255, 255, 255])
        .count();
    assert!(
        painted > 48 * 48 / 2,
        "the seed paints most of the page: {painted}"
    );
}
