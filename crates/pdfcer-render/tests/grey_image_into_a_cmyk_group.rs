//! A `DeviceGray` source in a `/DeviceCMYK` blending space deposits K only,
//! `C = M = Y = 0, K = 1 - gray` (ISO 32000-2 §10.4.2.2), so a grey image and
//! a grey fill match a `k` square of the same darkness (X009).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use super::rgb_into_a_cmyk_group::build;
use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{RenderOptions, RenderedPage, render_page_with};

/// `k` square at 10..90, then `paint` through an X-shaped clip over it.
fn render(group: bool, gs: &str, k: &str, paint: &str) -> RenderedPage {
    let content = format!(
        "0 0 0 {k} k 10 10 80 80 re f\n\
         q /{gs} gs 10 10 m 20 10 l 90 80 l 90 90 l 80 90 l 10 20 l h \
         90 10 m 90 20 l 20 90 l 10 90 l 10 80 l 80 10 l h W n {paint} Q\n"
    );
    let group = if group {
        "/Group << /S /Transparency /CS /DeviceCMYK >>"
    } else {
        ""
    };
    let page = format!(
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /MediaBox [0 0 100 100] {group} \
         /Resources << /ExtGState << /N << >> /M << /BM /Multiply >> \
         /O << /OP true /op true >> >> >> >>"
    );
    let stream = format!(
        "<< /Length {} >>\nstream\n{content}endstream",
        content.len()
    );
    let bytes = build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, &page),
        (4, &stream),
    ]);
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    render_page_with(&doc, &p, 2.0, &RenderOptions::default()).expect("render")
}

fn px(page: &RenderedPage, x: u32, y: u32) -> [i32; 3] {
    let p = page.pixmap.pixel(x, y).expect("in bounds").demultiply();
    [p.red().into(), p.green().into(), p.blue().into()]
}

fn close(label: &str, got: [i32; 3], want: [i32; 3]) {
    assert!(
        got.iter().zip(want).all(|(a, b)| (a - b).abs() <= 3),
        "{label}: {got:?} vs {want:?}"
    );
}

/// The X's centre against a point inside the square but outside the X.
fn assert_uniform(label: &str, page: &RenderedPage) {
    close(label, px(page, 100, 100), px(page, 100, 40));
}

/// The grey `paint` against the same X painted `0 0 0 {k} k`. Under
/// `/BM /Multiply` both darken the square, so the square is no reference
/// there; the `k` twin is.
fn assert_matches_k(gs: &str, k: &str, paint: &str) {
    let twin = format!("0 0 0 {k} k 10 10 80 80 re f");
    let got = px(&render(true, gs, k, paint), 100, 100);
    let want = px(&render(true, gs, k, &twin), 100, 100);
    close(&format!("{paint} /{gs}"), got, want);
}

const IMAGE_25: &str = "80 0 0 80 10 10 cm BI /W 1 /H 1 /CS /G /BPC 8 /F /AHx ID 40> EI";
const IMAGE_0: &str = "80 0 0 80 10 10 cm BI /W 1 /H 1 /CS /G /BPC 8 /F /AHx ID 00> EI";
const FILL_25: &str = ".25 g 10 10 80 80 re f";
const FILL_0: &str = "0 g 10 10 80 80 re f";

#[test]
fn a_grey_image_deposits_k_only() {
    for gs in ["N", "O"] {
        assert_uniform(
            &format!("image .25 /{gs}"),
            &render(true, gs, ".75", IMAGE_25),
        );
        assert_uniform(&format!("image 0 /{gs}"), &render(true, gs, "1", IMAGE_0));
    }
    for gs in ["N", "M", "O"] {
        assert_matches_k(gs, ".75", IMAGE_25);
        assert_matches_k(gs, "1", IMAGE_0);
    }
}

#[test]
fn a_grey_fill_deposits_k_only() {
    for gs in ["N", "O"] {
        assert_uniform(
            &format!("fill .25 /{gs}"),
            &render(true, gs, ".75", FILL_25),
        );
        assert_uniform(&format!("fill 0 /{gs}"), &render(true, gs, "1", FILL_0));
    }
    for gs in ["N", "M", "O"] {
        assert_matches_k(gs, ".75", FILL_25);
        assert_matches_k(gs, "1", FILL_0);
    }
}

#[test]
fn an_indexed_image_over_grey_follows_its_base() {
    let image = "80 0 0 80 10 10 cm BI /W 1 /H 1 /CS [/I /G 0 <40>] /BPC 8 ID \x00 EI";
    assert_uniform("indexed", &render(true, "N", ".75", image));
}

#[test]
fn an_additive_page_is_unchanged() {
    let page = render(false, "N", ".75", IMAGE_25);
    assert_eq!(px(&page, 100, 100), [64, 64, 64]);
}
