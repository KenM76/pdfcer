//! Overprint on an additive page converts a source that states its own
//! process tints through the same calibrated pair as a plain paint
//! (decision 202), so an overprint that changes nothing else lands where the
//! paint would with overprint off (X010).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use super::rgb_into_a_cmyk_group::build;
use pdfcer_core::color::{CmykIntent, cmyk_to_srgb_with};
use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{RenderOptions, RenderedPage, render_page_with};

fn render(content: &str) -> RenderedPage {
    let page = "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /MediaBox [0 0 300 100] \
         /Resources << /ExtGState << \
         /O1 << /OP true /op true /OPM 1 >> /O0 << /OP true /op true /OPM 0 >> >> >> >>";
    let stream = format!(
        "<< /Length {} >>\nstream\n{content}endstream",
        content.len()
    );
    let bytes = build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, page),
        (4, &stream),
    ]);
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    render_page_with(&doc, &p, 1.0, &RenderOptions::default()).expect("render")
}

fn px(page: &RenderedPage, x: u32, y: u32) -> [i32; 3] {
    let p = page.pixmap.pixel(x, y).expect("in bounds").demultiply();
    [p.red().into(), p.green().into(), p.blue().into()]
}

fn close(label: &str, got: [i32; 3], want: [i32; 3]) {
    within(label, got, want, 3);
}

fn within(label: &str, got: [i32; 3], want: [i32; 3], tol: i32) {
    assert!(
        got.iter().zip(want).all(|(g, w)| (g - w).abs() <= tol),
        "{label}: {got:?}, want {want:?}"
    );
}

#[test]
fn a_k_square_on_white_matches_with_overprint_off() {
    let page = render(
        "0 0 0 .75 k 10 10 80 80 re f \
         q /O1 gs 0 0 0 .75 k 110 10 80 80 re f Q \
         q /O0 gs 0 0 0 .75 k 210 10 80 80 re f Q\n",
    );
    let off = px(&page, 50, 50);
    let [r, g, b] = cmyk_to_srgb_with(CmykIntent::default(), 0.0, 0.0, 0.0, 0.75);
    #[allow(clippy::cast_possible_truncation)]
    let calibrated = [r, g, b].map(|v| (v * 255.0).round() as i32);
    close("overprint off", off, calibrated);
    close("/OPM 1", px(&page, 150, 50), off);
    close("/OPM 0", px(&page, 250, 50), off);
}

#[test]
fn magenta_overprinting_cyan_shows_the_calibrated_blue() {
    let page = render("1 0 0 0 k 10 10 80 80 re f q /O1 gs 0 1 0 0 k 50 10 80 80 re f Q\n");
    let [r, g, b] = cmyk_to_srgb_with(CmykIntent::default(), 1.0, 1.0, 0.0, 0.0);
    #[allow(clippy::cast_possible_truncation)]
    let blue = [r, g, b].map(|v| (v * 255.0).round() as i32);
    // The cyan backdrop is reconstructed from sRGB through the calibrated
    // inverse, which recovers cyan ink to within a few levels, not exactly.
    within("overlap", px(&page, 70, 50), blue, 8);
    let [r, g, b] = cmyk_to_srgb_with(CmykIntent::default(), 0.0, 1.0, 0.0, 0.0);
    #[allow(clippy::cast_possible_truncation)]
    let magenta = [r, g, b].map(|v| (v * 255.0).round() as i32);
    close("magenta alone", px(&page, 110, 50), magenta);
}
