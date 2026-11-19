//! A luminosity mask's `/BC` is read in the mask group's own `/CS`
//! (ISO 32000-1 §11.6.5.2, Table 144): outside the `/BBox` the mask is the
//! luminosity of `/BC` painted as an `sc` in that space paints. A missing,
//! unloadable or non-blending (§11.3.4) `/CS` falls back to reading `/BC` by
//! component count, with a diagnostic; a length mismatch is black.
//!
//! Each page fills black through the mask with a transparent backdrop, so a
//! pixel's alpha IS the mask value. The group paints `inside` over its left
//! half; the right half lies outside the `/BBox`, where only `/BC` reaches.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{PageBackdrop, RenderOptions, RenderedPage, render_page_with};

use crate::gray_icc_through_the_output_intent::{gray_profile, hex_stream};
use crate::rgb_into_a_cmyk_group::build;

const SWAP_RG: &[u8] = include_bytes!("../../../fixtures/synthetic/icc-rgb/swap-rg.icc");
const DEST: &[u8] = include_bytes!("../../../fixtures/synthetic/icc-rgb/dest-cmyk.icc");
const V4_CMYK: &[u8] = include_bytes!("../../../fixtures/synthetic/icc-rgb/v4-cmyk-mab-lab.icc");

/// A luminosity-masked page. `cs` is the group's `/CS` (or `None`), `bc`
/// the `/BC` array body, `inside` the operands painted in `cs` over the
/// group's left half (empty: nothing painted). `profile` is object 6 and
/// `intent` names `dest-cmyk.icc` as the output intent.
fn page(cs: Option<&str>, bc: &str, inside: &str, profile: &[u8], intent: bool) -> Vec<u8> {
    let content = "/S gs 0 0 0 rg 0 0 100 80 re f\n";
    let catalog = if intent {
        "<< /Type /Catalog /Pages 2 0 R /OutputIntents [<< /Type /OutputIntent \
         /S /GTS_PDFX /OutputConditionIdentifier (synthetic) /DestOutputProfile 7 0 R >>] >>"
    } else {
        "<< /Type /Catalog /Pages 2 0 R >>"
    };
    let page = format!(
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /MediaBox [0 0 100 80] \
         /Resources << /ExtGState << /S << /SMask << /S /Luminosity /BC [{bc}] /G 5 0 R >> >> >> >> >>"
    );
    let contents = format!(
        "<< /Length {} >>\nstream\n{content}endstream",
        content.len()
    );
    let (group_cs, paint) = match cs {
        Some(cs) if !inside.is_empty() => (
            format!("/CS {cs}"),
            format!("/C cs {inside} sc 0 0 50 80 re f\n"),
        ),
        Some(cs) => (format!("/CS {cs}"), String::new()),
        None => (String::new(), String::new()),
    };
    let res = cs.map_or(String::new(), |cs| {
        format!("/Resources << /ColorSpace << /C {cs} >> >>")
    });
    let form = format!(
        "<< /Type /XObject /Subtype /Form /BBox [0 0 50 80] {res} \
         /Group << /S /Transparency {group_cs} >> /Length {} >>\nstream\n{paint}endstream",
        paint.len()
    );
    let n = if profile == SWAP_RG {
        3
    } else if profile == V4_CMYK {
        4
    } else {
        1
    };
    let src = hex_stream(&format!("/N {n}"), profile);
    let dest = hex_stream("/N 4", DEST);
    build(&[
        (1, catalog),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, &page),
        (4, &contents),
        (5, &form),
        (6, &src),
        (7, &dest),
    ])
}

fn render(bytes: Vec<u8>) -> RenderedPage {
    let doc = Document::from_bytes(bytes).unwrap();
    let p = page_tree::pages(&doc).unwrap().remove(0);
    let options = RenderOptions::default().with_backdrop(PageBackdrop::Transparent);
    render_page_with(&doc, &p, 1.0, &options).unwrap()
}

/// `(inside the /BBox, outside it)` mask values.
fn halves(r: &RenderedPage) -> (u8, u8) {
    let a = |x| r.pixmap.pixel(x, 40).unwrap().alpha();
    (a(25), a(75))
}

fn noted(r: &RenderedPage, needle: &str) -> bool {
    r.diagnostics.sample_ops.iter().any(|s| s.contains(needle))
}

/// `/BC` and an `sc` of the same operands give the same mask value, and
/// that value is not what the component-count reading gives.
fn agrees_with_the_paint(r: &RenderedPage, by_count: u8) -> u8 {
    let (inside, outside) = halves(r);
    assert!(
        inside.abs_diff(outside) <= 1,
        "paint {inside} vs /BC {outside}"
    );
    assert!(
        outside.abs_diff(by_count) > 4,
        "/BC {outside} still reads by count ({by_count})"
    );
    outside
}

#[test]
fn an_icc_rgb_backdrop_goes_through_its_profile() {
    // swap-rg.icc swaps red and green: [1 0 0] is sRGB green, luminosity
    // about 0.59, where DeviceRGB red is 0.30 (77).
    let r = render(page(
        Some("[/ICCBased 6 0 R]"),
        "1 0 0",
        "1 0 0",
        SWAP_RG,
        false,
    ));
    let v = agrees_with_the_paint(&r, 77);
    assert!(v.abs_diff(150) <= 2, "{v}");
}

#[test]
fn an_icc_cmyk_backdrop_goes_through_its_profile_to_the_output_intent() {
    let cs = "[/ICCBased 6 0 R]";
    let device = render(page(Some("/DeviceCMYK"), "0 0 0 1", "", DEST, true));
    let (_, by_count) = halves(&device);
    let r = render(page(Some(cs), "0 0 0 1", "0 0 0 1", V4_CMYK, true));
    let v = agrees_with_the_paint(&r, by_count);
    assert!(
        v > by_count,
        "v4 profile's black {v} vs device black {by_count}"
    );
}

#[test]
fn a_cal_gray_backdrop_uses_its_gamma() {
    // Gamma 1: A = 0.5 is Y = 0.5, sRGB-encoded about 0.735 (188).
    let cs = "[/CalGray << /WhitePoint [0.9505 1 1.089] /Gamma 1 >>]";
    let r = render(page(Some(cs), "0.5", "0.5", DEST, false));
    let v = agrees_with_the_paint(&r, 128);
    assert!(v.abs_diff(188) <= 1, "{v}");
}

#[test]
fn an_icc_gray_backdrop_agrees_with_an_icc_gray_paint() {
    // An `N 1` paint on a page with no output intent is Table 66's
    // reinterpretation as DeviceGray, so the paint and /BC both give 0.5.
    let r = render(page(
        Some("[/ICCBased 6 0 R]"),
        "0.5",
        "0.5",
        &gray_profile(),
        false,
    ));
    let (inside, outside) = halves(&r);
    assert_eq!(outside, 128);
    assert!(
        inside.abs_diff(outside) <= 1,
        "paint {inside} vs /BC {outside}"
    );
}

#[test]
fn a_device_space_backdrop_is_the_component_count_reading() {
    for (cs, bc, want) in [
        ("/DeviceGray", "0.5", 128),
        ("/DeviceRGB", "0.2 0.4 0.6", 92),
        ("/DeviceCMYK", "0 0 0 0", 255),
    ] {
        let missing = render(page(None, bc, "", DEST, false));
        let device = render(page(Some(cs), bc, "", DEST, false));
        assert_eq!(device.pixmap.data(), missing.pixmap.data(), "{cs}");
        assert_eq!(halves(&device).1, want, "{cs}");
        assert!(!noted(&device, "/BC"), "{cs}");
        assert!(noted(&missing, "without group /CS"), "{cs}");
    }
}

#[test]
fn a_separation_group_reads_by_count_with_a_diagnostic() {
    let cs =
        "[/Separation /Spot /DeviceGray << /FunctionType 2 /Domain [0 1] /C0 [1] /C1 [0] /N 1 >>]";
    let r = render(page(Some(cs), "0.5", "", DEST, false));
    assert_eq!(halves(&r).1, 128);
    assert!(noted(&r, "not a blending space"));
}

#[test]
fn an_unloadable_group_cs_reads_by_count_with_a_diagnostic() {
    let r = render(page(Some("/NoSuchSpace"), "0.5", "", DEST, false));
    assert_eq!(halves(&r).1, 128);
    assert!(noted(&r, "unloadable"));
}

#[test]
fn a_backdrop_of_the_wrong_length_is_black() {
    let r = render(page(Some("/DeviceRGB"), "0.5", "", DEST, false));
    assert_eq!(halves(&r).1, 0);
    assert!(noted(&r, "length does not match"));
}
