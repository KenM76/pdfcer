//! An RGB colour entering a `/DeviceCMYK` blending space is separated through
//! the inverse of the table the buffer displays through (decision 202), not
//! max-GCR `rgb_to_cmyk`. Under `/BM /Difference` over pure K the result
//! shows which ink the colour became; the reference values are a reference
//! viewer's, from the request that reported it.
//!
//! One test per route that bridges an RGB source into the buffer: a separable
//! fill, a non-separable blend, and an image's pixels.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::color::{CmykIntent, srgb_to_cmyk_fast};
use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{InkProbeSource, RenderOptions, RenderedPage, render_page_with};

fn build(objects: &[(u32, &str)]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    let max_num = objects.iter().map(|(n, _)| *n).max().unwrap_or(0);
    buf.extend_from_slice(format!("xref\n0 {}\n", max_num + 1).as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for num in 1..=max_num {
        match offsets.iter().find(|(n, _)| *n == num) {
            Some((_, off)) => buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes()),
            None => buf.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            max_num + 1
        )
        .as_bytes(),
    );
    buf
}

/// Left column: the request's three Difference rows. Right column, top to
/// bottom: a plain grey fill, a grey under a non-separable blend, a grey
/// image pixel.
const CONTENT: &str = "\
q 0.8 0.4 0.2 rg 20 320 60 60 re f /D gs 0 0 0 1 k 20 320 60 60 re f Q
q 0.2 0.6 0.9 rg 20 220 60 60 re f /D gs 0 0 0 1 k 20 220 60 60 re f Q
q 0.5 0.5 0.5 rg 20 120 60 60 re f /D gs 0 0 0 1 k 20 120 60 60 re f Q
q 0.5 0.5 0.5 rg 200 320 60 60 re f Q
q /L gs 0.5 0.5 0.5 rg 200 220 60 60 re f Q
q 60 0 0 60 200 120 cm BI /W 1 /H 1 /CS /RGB /BPC 8 /F /AHx ID 808080> EI Q
";

fn page() -> Vec<u8> {
    let stream = CONTENT;
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 400 400] >>",
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R \
             /Group << /S /Transparency /CS /DeviceCMYK >> \
             /Resources << /ExtGState << /D << /BM /Difference >> \
             /L << /BM /Luminosity >> >> >> >>",
        ),
        (
            4,
            &format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
        ),
    ])
}

fn render(options: &RenderOptions) -> RenderedPage {
    let doc = Document::from_bytes(page()).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    let page = render_page_with(&doc, &p, 1.0, options).expect("render");
    assert!(page.diagnostics.cmyk_buffer_engaged, "no CMYK buffer");
    page
}

fn px(page: &RenderedPage, x: u32, y: u32) -> [i32; 3] {
    let p = page.pixmap.pixel(x, y).expect("in bounds").demultiply();
    [p.red().into(), p.green().into(), p.blue().into()]
}

/// The ink in the buffer at (x, y), before the exit conversion.
fn ink(x: u32, y: u32) -> [f32; 4] {
    let page = render(&RenderOptions::default().with_ink_probe(x, y));
    let probe = page.diagnostics.ink_probe.expect("probe requested");
    assert_eq!(probe.source, InkProbeSource::CmykBuffer);
    probe.cmyk.expect("a CMYK buffer reports ink")
}

/// The calibrated inverse's separation of 50 % grey, which carries CMY and
/// little K. Max-GCR gives [0, 0, 0, 0.5], far outside the tolerance.
fn assert_grey_ink(label: &str, got: [f32; 4]) {
    let want = srgb_to_cmyk_fast(CmykIntent::Calibrated, 0.5, 0.5, 0.5);
    for (g, w) in got.iter().zip(want.iter()) {
        assert!(
            (g - w).abs() < 0.02,
            "{label}: ink {got:?}, the calibrated inverse gives {want:?}"
        );
    }
}

#[test]
fn difference_over_black_lands_near_the_reference() {
    let page = render(&RenderOptions::default());
    // Row 2 is out of gamut: pdfcer clips it in sRGB, the reference through
    // its profile, so only its R and B are held to the reference.
    for (y, want, channels) in [
        (50, [7, 139, 193], 0..3),
        (150, [186, 105, 49], 0..1),
        (250, [137, 112, 106], 0..3),
    ] {
        let got = px(&page, 50, y);
        for c in channels {
            assert!(
                (got[c] - want[c]).abs() <= 8,
                "row at y={y}: {got:?}, reference {want:?}"
            );
        }
    }
    assert!((px(&page, 50, 150)[2] - 49).abs() <= 8);
    assert!(page.diagnostics.cmyk_bridged_pixels > 0);
}

#[test]
fn plain_grey_stays_neutral() {
    let page = render(&RenderOptions::default());
    let [r, g, b] = px(&page, 230, 50);
    assert!((r - 128).abs() <= 3 && (g - r).abs() <= 2 && (b - r).abs() <= 2);
}

#[test]
fn a_fill_separates_through_the_inverse() {
    assert_grey_ink("fill", ink(230, 50));
}

#[test]
fn a_non_separable_blend_separates_through_the_inverse() {
    assert_grey_ink("luminosity", ink(230, 150));
}

#[test]
fn an_image_pixel_separates_through_the_inverse() {
    assert_grey_ink("image", ink(230, 250));
}
