//! `RenderOptions::with_preserve_calibrated_cmyk` keeps an `ICCBased` N = 4
//! colour's numbers as the ink when the page composites in ink, instead of
//! converting them from their own profile to the output intent
//! (ISO 32000-2 §8.6.5.5). Off, the conversion runs (X011).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fmt::Write as _;

use super::rgb_into_a_cmyk_group::build;
use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{RenderOptions, render_page_with};

const DEST: &[u8] = include_bytes!("../../../fixtures/synthetic/icc-rgb/dest-cmyk.icc");
const SRC: &[u8] = include_bytes!("../../../fixtures/synthetic/icc-rgb/v4-cmyk-mab-lab.icc");
const AUTHORED: [f32; 4] = [0.0, 1.0, 0.0, 0.0];

fn hex_stream(dict: &str, bytes: &[u8]) -> String {
    let mut hex = String::with_capacity(bytes.len() * 2 + 1);
    for b in bytes {
        write!(hex, "{b:02X}").unwrap();
    }
    hex.push('>');
    format!(
        "<< {dict} /Filter /ASCIIHexDecode /Length {} >>\nstream\n{hex}\nendstream",
        hex.len()
    )
}

/// The ink under a full-page `ICCBased` CMYK fill of [`AUTHORED`].
fn probed_ink(preserve: bool) -> [f32; 4] {
    let content = "/C cs 0 1 0 0 sc 0 0 100 100 re f\n";
    let page = "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /MediaBox [0 0 100 100] \
         /Group << /S /Transparency /CS /DeviceCMYK >> \
         /Resources << /ColorSpace << /C [/ICCBased 5 0 R] >> >> >>";
    let stream = format!(
        "<< /Length {} >>\nstream\n{content}endstream",
        content.len()
    );
    let bytes = build(&[
        (
            1,
            "<< /Type /Catalog /Pages 2 0 R /OutputIntents [<< /Type /OutputIntent \
             /S /GTS_PDFX /OutputConditionIdentifier (synthetic) /DestOutputProfile 6 0 R >>] >>",
        ),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, page),
        (4, &stream),
        (5, &hex_stream("/N 4", SRC)),
        (6, &hex_stream("/N 4", DEST)),
    ]);
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    let options = RenderOptions::default()
        .with_preserve_calibrated_cmyk(preserve)
        .with_ink_probe(100, 100);
    let r = render_page_with(&doc, &p, 2.0, &options).expect("render");
    r.diagnostics
        .ink_probe
        .and_then(|p| p.cmyk)
        .expect("the page composites in ink")
}

fn max_diff(a: [f32; 4], b: [f32; 4]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max)
}

#[test]
fn by_default_the_colour_converts_to_the_output_intent() {
    // The control: equal profiles would make the test below vacuous.
    let ink = probed_ink(false);
    assert!(
        max_diff(ink, AUTHORED) > 0.01,
        "{ink:?}: the profile pair must move the numbers"
    );
}

#[test]
fn preserved_the_authored_numbers_are_the_ink() {
    let ink = probed_ink(true);
    assert!(max_diff(ink, AUTHORED) < 1e-3, "{ink:?}");
}
