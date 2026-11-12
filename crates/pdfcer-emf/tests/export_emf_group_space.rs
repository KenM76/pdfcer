//! # A group's blending space against a subtractive page's (`Pass 700.23`)
//!
//! On a page whose group blends in DeviceCMYK, an isolated group that also
//! declares DeviceCMYK changes nothing about where it blends, so the
//! vector writer resolves it like any group. One declaring DeviceRGB does
//! change it, and the export must say so rather than model it in ink.

// Tests fail by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_emf::{EmfExport, EmfOptions, export_emf};
use pdfcer_render::RenderOptions;
#[cfg(windows)]
use pdfcer_render::{PageBackdrop, render_page_with};

use crate::export_emf::{DPI, assert_well_formed, build};
#[cfg(windows)]
use crate::export_emf_patches::{ROUNDING, playback_against_raster};

/// A DeviceCMYK page: a cyan backdrop, then form `/G`, an isolated group
/// declaring `cs`, painted at 70 % alpha; inside, two overlapping fills in
/// colours `a` and `b` (operands and operator), the second multiplied.
fn on_cmyk_page(cs: &str, a: &str, b: &str) -> Vec<u8> {
    let form = format!("{a} 30 20 30 30 re f /M gs {b} 45 30 30 30 re f\n");
    let content = "0.8 0.1 0 0 k 10 10 50 40 re f /H gs /G Do\n";
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 80] /Resources << /XObject << /G 5 0 R >> /ExtGState << /H << /ca 0.7 >> >> >> >>"
                .into(),
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Group << /S /Transparency /CS /DeviceCMYK >> >>"
                .into(),
        ),
        (
            4,
            format!("<< /Length {} >>\nstream\n{content}endstream", content.len()),
        ),
        (
            5,
            format!(
                "<< /Type /XObject /Subtype /Form /BBox [0 0 100 80] /Group << /S /Transparency /I true /CS {cs} >> /Resources << /ExtGState << /M << /BM /Multiply >> >> >> /Length {} >>\nstream\n{form}endstream",
                form.len()
            ),
        ),
    ])
}

fn cmyk_group() -> Vec<u8> {
    on_cmyk_page("/DeviceCMYK", "0 0.2 1 0 k", "0 0.9 0.3 0.1 k")
}

fn rgb_group() -> Vec<u8> {
    on_cmyk_page("/DeviceRGB", "1 0.8 0 rg", "1 0 0.5 rg")
}

fn export(bytes: &[u8]) -> EmfExport {
    let doc = Document::from_bytes(bytes.to_vec()).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    let options = EmfOptions::default()
        .with_raster_dpi(DPI)
        .with_vector_transparency(true);
    let emf = export_emf(&doc, &p, &RenderOptions::default(), &options).expect("emf export");
    assert_well_formed(&emf.emf);
    emf
}

#[test]
fn a_cmyk_group_on_a_cmyk_page_is_written_as_regions() {
    let o = export(&cmyk_group()).outcome;
    let v = o.vectorised;
    assert_eq!(v.patched_unrecorded, 0, "{v:?}");
    assert_eq!(v.patched(), 0, "{v:?}");
    assert!(v.groups > 0 && v.regions > 0, "{v:?}");
    assert_eq!(o.patches.cases(), 0, "{:?}", o.patches);
}

#[test]
fn an_rgb_group_on_a_cmyk_page_is_patched_as_unrecorded() {
    let v = export(&rgb_group()).outcome.vectorised;
    assert!(v.patched_unrecorded > 0, "{v:?}");
    assert_eq!(v.groups, 0, "{v:?}");
}

#[cfg(windows)]
#[test]
fn both_play_back_like_the_raster() {
    for (name, bytes) in [("cmyk-group", cmyk_group()), ("rgb-group", rgb_group())] {
        let doc = Document::from_bytes(bytes.clone()).unwrap();
        let p = page_tree::pages(&doc).unwrap().remove(0);
        let reference = render_page_with(
            &doc,
            &p,
            DPI / 72.0,
            &RenderOptions::default().with_backdrop(PageBackdrop::White),
        )
        .unwrap();
        let emf = export(&bytes);
        let (bad, worst) = playback_against_raster(
            &format!("group-space-{name}"),
            &reference.pixmap,
            &emf.emf,
            ROUNDING,
        );
        assert_eq!(bad, 0, "{name}: worst {worst}");
    }
}
