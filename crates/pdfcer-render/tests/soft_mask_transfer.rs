//! A soft mask's `/TR` is applied (ISO 32000-2 Table 144): last, once, to
//! the mask value. A function that is not one-in one-out is counted in
//! `soft_mask_transfer_ignored` and the mask used untransformed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{PageBackdrop, RenderOptions, RenderedPage, render_page_with};

use crate::rgb_into_a_cmyk_group::build;

/// A red page through a luminosity mask whose group is a white square on
/// the left half; `tr` is the mask dictionary's `/TR` entry.
fn masked(tr: &str) -> Vec<u8> {
    let page = "/S gs 1 0 0 rg 0 0 100 80 re f\n";
    let group = "1 g 0 0 50 80 re f\n";
    let pages = format!(
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 80] /Resources << /ExtGState << /S << /SMask << /S /Luminosity {tr} /G 5 0 R >> >> >> >> >>"
    );
    let contents = format!("<< /Length {} >>\nstream\n{page}endstream", page.len());
    let form = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 100 80] /Group << /S /Transparency /CS /DeviceRGB >> /Length {} >>\nstream\n{group}endstream",
        group.len()
    );
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, &pages),
        (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>"),
        (4, &contents),
        (5, &form),
    ])
}

fn render(bytes: Vec<u8>) -> RenderedPage {
    let doc = Document::from_bytes(bytes).unwrap();
    let p = page_tree::pages(&doc).unwrap().remove(0);
    let options = RenderOptions::default().with_backdrop(PageBackdrop::Transparent);
    render_page_with(&doc, &p, 1.0, &options).unwrap()
}

/// `(left, right)` alpha of the page's middle row.
fn halves(r: &RenderedPage) -> (u8, u8) {
    let px = r.pixmap.pixel(25, 40).unwrap();
    let qx = r.pixmap.pixel(75, 40).unwrap();
    (px.alpha(), qx.alpha())
}

#[test]
fn an_inverting_transfer_swaps_what_the_mask_shows() {
    let plain = render(masked(""));
    assert_eq!(halves(&plain), (255, 0));
    let inverted = render(masked(
        "/TR << /FunctionType 2 /Domain [0 1] /C0 [1] /C1 [0] /N 1 >>",
    ));
    assert_eq!(halves(&inverted), (0, 255));
    assert_eq!(inverted.diagnostics.soft_mask_transfer_ignored, 0);
}

#[test]
fn identity_is_no_transfer() {
    let r = render(masked("/TR /Identity"));
    assert_eq!(halves(&r), (255, 0));
    assert_eq!(r.diagnostics.soft_mask_transfer_ignored, 0);
}

#[test]
fn a_two_output_transfer_is_counted_and_ignored() {
    let r = render(masked(
        "/TR << /FunctionType 2 /Domain [0 1] /C0 [1 1] /C1 [0 0] /N 1 >>",
    ));
    assert_eq!(halves(&r), (255, 0));
    assert_eq!(r.diagnostics.soft_mask_transfer_ignored, 1);
}
