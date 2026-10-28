//! A one-component `ICCBased` colour in a `/DeviceCMYK` blending space is
//! converted through the document's CMYK output intent, image and fill alike
//! (ISO 32000-2 §8.6.5.5, §14.11.5; X011). With no output intent it keeps
//! X009's `K = 1 - gray`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation
)]

use std::fmt::Write as _;

use super::rgb_into_a_cmyk_group::build;
use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{RenderOptions, RenderedPage, render_page_with};

const DEST: &[u8] = include_bytes!("../../../fixtures/synthetic/icc-rgb/dest-cmyk.icc");
const D50: [f64; 3] = [0.9642, 1.0, 0.8249];
const GRAY: f32 = 0.2;

fn s15f16(v: f64) -> [u8; 4] {
    ((v * 65536.0).round() as i32).to_be_bytes()
}

/// A v2.1 `mntr GRAY XYZ` profile whose `kTRC` is a 2.2 gamma.
fn gray_profile() -> Vec<u8> {
    let mut desc = b"desc\0\0\0\0".to_vec();
    let text = b"pdfcer synthetic gamma 2.2 gray\0";
    desc.extend_from_slice(&u32::try_from(text.len()).unwrap().to_be_bytes());
    desc.extend_from_slice(text);
    desc.extend_from_slice(&[0; 8 + 3 + 67]);
    let mut wtpt = b"XYZ \0\0\0\0".to_vec();
    for c in D50 {
        wtpt.extend_from_slice(&s15f16(c));
    }
    let mut ktrc = b"curv\0\0\0\0".to_vec();
    ktrc.extend_from_slice(&1_u32.to_be_bytes());
    ktrc.extend_from_slice(&((2.2_f64 * 256.0).round() as u16).to_be_bytes());
    let tags: [(&[u8; 4], Vec<u8>); 3] = [(b"desc", desc), (b"wtpt", wtpt), (b"kTRC", ktrc)];
    let table_len = 4 + 12 * tags.len();
    let mut entries = Vec::new();
    let mut payload = Vec::new();
    for (sig, data) in &tags {
        let off = u32::try_from(128 + table_len + payload.len()).unwrap();
        entries.extend_from_slice(*sig);
        entries.extend_from_slice(&off.to_be_bytes());
        entries.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
        payload.extend_from_slice(data);
        payload.resize(payload.len().next_multiple_of(4), 0);
    }
    let mut header = vec![0_u8; 128];
    let size = u32::try_from(128 + table_len + payload.len()).unwrap();
    header[0..4].copy_from_slice(&size.to_be_bytes());
    header[4..8].copy_from_slice(b"none");
    header[8..12].copy_from_slice(&[2, 0x10, 0, 0]);
    header[12..16].copy_from_slice(b"mntr");
    header[16..20].copy_from_slice(b"GRAY");
    header[20..24].copy_from_slice(b"XYZ ");
    header[36..40].copy_from_slice(b"acsp");
    for (i, c) in D50.iter().enumerate() {
        header[68 + 4 * i..72 + 4 * i].copy_from_slice(&s15f16(*c));
    }
    header[80..84].copy_from_slice(b"pdfc");
    let mut out = header;
    out.extend_from_slice(&u32::try_from(tags.len()).unwrap().to_be_bytes());
    out.extend_from_slice(&entries);
    out.extend_from_slice(&payload);
    out
}

/// What the output intent makes of the gray, computed independently of the
/// renderer with the same CMM.
fn expected_ink() -> [f32; 4] {
    let src = icccer_profile::Profile::parse(&gray_profile()).unwrap();
    let dst = icccer_profile::Profile::parse(DEST).unwrap();
    let chain = icccer_cmm::transform::Chain::new(
        &src,
        &dst,
        icccer_cmm::matrix_trc::Intent::MediaRelative,
    )
    .unwrap();
    let out = chain.convert(&[f64::from(GRAY)]).unwrap();
    std::array::from_fn(|i| out[i] as f32)
}

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

/// A CMYK square at 10..90 in `ink`, then `paint` through an X-shaped clip.
fn render(intent: bool, gs: &str, ink: [f32; 4], paint: &str) -> RenderedPage {
    let [c, m, y, k] = ink;
    let content = format!(
        "{c} {m} {y} {k} k 10 10 80 80 re f\n\
         q /{gs} gs 10 10 m 20 10 l 90 80 l 90 90 l 80 90 l 10 20 l h \
         90 10 m 90 20 l 20 90 l 10 90 l 10 80 l 80 10 l h W n {paint} Q\n"
    );
    let catalog = if intent {
        "<< /Type /Catalog /Pages 2 0 R /OutputIntents [<< /Type /OutputIntent \
         /S /GTS_PDFX /OutputConditionIdentifier (synthetic) /DestOutputProfile 7 0 R >>] >>"
    } else {
        "<< /Type /Catalog /Pages 2 0 R >>"
    };
    let page = "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /MediaBox [0 0 100 100] \
         /Group << /S /Transparency /CS /DeviceCMYK >> \
         /Resources << /ColorSpace << /G [/ICCBased 6 0 R] >> /XObject << /Im 5 0 R >> \
         /ExtGState << /N << >> /O << /OP true /op true >> >> >> >>";
    let stream = format!(
        "<< /Length {} >>\nstream\n{content}endstream",
        content.len()
    );
    let sample = (GRAY * 255.0).round() as u8;
    let image = hex_stream(
        "/Type /XObject /Subtype /Image /Width 1 /Height 1 \
         /ColorSpace [/ICCBased 6 0 R] /BitsPerComponent 8",
        &[sample],
    );
    let gray = hex_stream("/N 1", &gray_profile());
    let dest = hex_stream("/N 4", DEST);
    let bytes = build(&[
        (1, catalog),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, page),
        (4, &stream),
        (5, &image),
        (6, &gray),
        (7, &dest),
    ]);
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    render_page_with(&doc, &p, 2.0, &RenderOptions::default()).expect("render")
}

const IMAGE: &str = "100 0 0 100 0 0 cm /Im Do";
const FILL: &str = "/G cs 0.2 sc 0 0 100 100 re f";

fn px(page: &RenderedPage, x: u32, y: u32) -> [i32; 3] {
    let p = page.pixmap.pixel(x, y).expect("in bounds").demultiply();
    [p.red().into(), p.green().into(), p.blue().into()]
}

/// The X's centre against a point inside the square but outside the X.
fn assert_uniform(label: &str, page: &RenderedPage) {
    let (x, square) = (px(page, 100, 100), px(page, 100, 40));
    assert!(
        x.iter().zip(square).all(|(a, b)| (a - b).abs() <= 3),
        "{label}: X {x:?} vs square {square:?}"
    );
}

#[test]
fn the_output_intent_separates_the_gray_into_more_than_black() {
    let [c, m, y, _] = expected_ink();
    assert!(
        c.max(m).max(y) > 0.05,
        "the profile pair must give chromatic ink, or the test cannot tell \
         the managed route from K = 1 - gray"
    );
}

#[test]
fn a_gray_icc_image_converts_through_the_output_intent() {
    for gs in ["N", "O"] {
        assert_uniform(
            &format!("image, /{gs}"),
            &render(true, gs, expected_ink(), IMAGE),
        );
    }
}

#[test]
fn a_gray_icc_fill_converts_through_the_output_intent() {
    for gs in ["N", "O"] {
        assert_uniform(
            &format!("fill, /{gs}"),
            &render(true, gs, expected_ink(), FILL),
        );
    }
}

#[test]
fn without_an_output_intent_a_gray_icc_colour_is_k_only() {
    let k_only = [0.0, 0.0, 0.0, 1.0 - GRAY];
    for gs in ["N", "O"] {
        for (label, paint) in [("image", IMAGE), ("fill", FILL)] {
            assert_uniform(
                &format!("{label}, /{gs}, no intent"),
                &render(false, gs, k_only, paint),
            );
        }
    }
}
