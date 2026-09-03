//! # Embedded no-cmap CIDFontType2 renders — the TrueType-misroute regression
//!
//! Pins the fix for the embedded-TrueType render class: a `/Type0`
//! font, `/Encoding /Identity-H`, `/CIDFontType2` descendant, embedded
//! subset TrueType (`FontFile2`) with a `/CIDToGIDMap` and **no `cmap`
//! table**. Real CAD/Office producers (SolidWorks / AutoCAD / Office)
//! ship exactly this; ISO 32000-1 §9.7.4.2 makes the font's own `cmap`
//! irrelevant (selection is CID -> GID via `/CIDToGIDMap`).
//!
//! The bug this guards against: the embedded programs use the ordinary
//! TrueType sfnt version `0x00010000`, whose first byte is `0x00` (NUL,
//! a PDF whitespace byte, Table 1). A format detector that trimmed
//! leading whitespace before sniffing the magic stripped that NUL,
//! shifted the data to `0x01 0x00 ...`, and misrouted the whole font to
//! the bare-CFF parser — which fails "offset out of bounds", so ALL of
//! the drawing's text was skipped while graphics rendered fine. See
//! `crates/pdfcer-render/src/font/program.rs` `FontProgram::parse`.
//!
//! Fixture provenance/construction:
//! `tools/gen-cidfont-nocmap-fixtures.py` (synthetic, CC0 — the
//! embedded TrueType is built from box outlines defined in that script,
//! contains no third-party font data). The page draws CID 1, which maps
//! to GID 1 (a filled box) near the top-left; if the box paints, the
//! glyf-by-GID render path worked end to end.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::RenderedPage;

fn fixture() -> Document {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/text/cidfonttype2-nocmap-embedded.pdf");
    Document::from_bytes(std::fs::read(&path).expect("fixture file")).expect("fixture parses")
}

fn render(doc: &Document) -> RenderedPage {
    let pages = page_tree::pages(doc).expect("page tree");
    pdfcer_render::render_page(doc, &pages[0], 1.0).expect("renders")
}

/// Count fully-opaque, non-white pixels — the painted box glyph.
fn painted_pixels(page: &RenderedPage) -> u32 {
    let mut n = 0;
    for y in 0..page.pixmap.height() {
        for x in 0..page.pixmap.width() {
            let px = page.pixmap.pixel(x, y).expect("pixel");
            if px.alpha() == 255 && (px.red() < 250 || px.green() < 250 || px.blue() < 250) {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn nocmap_cidfonttype2_paints_its_glyphs() {
    let doc = fixture();
    let page = render(&doc);
    let d = &page.diagnostics;

    // The whole point: NOTHING is skipped. Before the fix this font
    // landed in `fonts_unsupported` as `UnusableProgram` (the CFF
    // misroute) and the box never drew.
    assert_eq!(
        d.fonts_unsupported, 0,
        "embedded no-cmap CIDFontType2 must not be reported unsupported"
    );
    assert!(
        d.fonts_unsupported_by_reason.is_empty(),
        "no by-reason bucket should be populated: {:?}",
        d.fonts_unsupported_by_reason
    );
    assert_eq!(
        d.glyphs_substituted, 0,
        "the document's own program was used"
    );

    // And the glyph actually painted. The box is ~500x700 font units at
    // 200pt: thousands of pixels, so a generous floor still proves it.
    let painted = painted_pixels(&page);
    assert!(
        painted > 1_000,
        "expected the box glyph to paint many pixels, got {painted}"
    );
}
