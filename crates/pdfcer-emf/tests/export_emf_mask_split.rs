//! # Paint over a masked drop shadow patched only over the shadow (`Pass 700.24`)
//!
//! A drop shadow is a large rect under a soft mask whose group is an
//! image; the layer paints only where the mask is above zero (ISO 32000-2
//! §11.6.5.2). With `tight_mask_patches`, translucent or overprinted paint
//! over it (§11.7.4) is written as vector away from the shadow and patched
//! only where the two overlap; by default it is patched whole. The oracle is GDI's `PlayEnhMetaFile` against the raster, which
//! covers the pixels inside the shadow and outside it.

// Tests fail by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_emf::{EmfExport, EmfOptions, export_emf};
use pdfcer_render::{PageBackdrop, RenderOptions, render_page_with};
use tiny_skia::Pixmap;

use crate::export_emf::{DPI, assert_well_formed, build};
use crate::export_emf_patches::bitmap_sizes;
#[cfg(windows)]
use crate::export_emf_patches::{ROUNDING, playback_against_raster};

/// The shadow's mask: a 2×1 grey image over 20..80 × 15..55 pt, cut by
/// the group `/BBox` to 20..40 × 15..30 pt (40 × 30 px).
const SHADOW_MASK: &str = "q 60 0 0 40 20 15 cm BI /W 2 /H 1 /CS /G /BPC 8 /F /AHx ID 40C0> EI Q";

/// A page `0 0 100 80`: `under`, a black rect over most of the page under
/// the shadow mask, then `over` with the resources `/O` (`over_gs`).
fn shadowed(page_group: &str, under: &str, over: &str, over_gs: &str) -> Vec<u8> {
    let page = format!("{under} q /S gs 0 g 5 5 90 70 re f Q {over}\n");
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            format!(
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 80] /Resources << /ExtGState << /S << /SMask << /S /Luminosity /G 5 0 R >> >> /O << {over_gs} >> >> >> >>"
            ),
        ),
        (
            3,
            format!("<< /Type /Page /Parent 2 0 R /Contents 4 0 R {page_group} >>"),
        ),
        (
            4,
            format!("<< /Length {} >>\nstream\n{page}endstream", page.len()),
        ),
        (
            5,
            format!(
                "<< /Type /XObject /Subtype /Form /BBox [20 15 40 30] /Group << /S /Transparency /CS /DeviceGray >> /Length {} >>\nstream\n{SHADOW_MASK}\nendstream",
                SHADOW_MASK.len() + 1
            ),
        ),
    ])
}

/// Half-transparent green over a blue square and the shadow, reaching
/// well past the shadow on every side.
fn translucent() -> Vec<u8> {
    shadowed(
        "",
        "0 0.6 1 rg 10 10 80 60 re f",
        "/O gs 0 1 0 rg 15 10 70 55 re f",
        "/ca 0.5",
    )
}

/// The same on a DeviceCMYK page group, the green an overprinted yellow.
fn overprinted() -> Vec<u8> {
    shadowed(
        "/Group << /S /Transparency /CS /DeviceCMYK >>",
        "1 0 0 0 k 10 10 80 60 re f",
        "/O gs 0 0 1 0 k 15 10 70 55 re f",
        "/OP true /op true /OPM 1",
    )
}

/// Half-transparent blue over a 2×1 image at 20..60 × 15..40 pt (80 × 50
/// px), reaching past it on every side. The image is written as a bitmap
/// of its own, so only the cut over it can make the paint there right.
fn over_an_image() -> Vec<u8> {
    let page = "0.9 0.9 0 rg 5 5 90 70 re f \
                q 40 0 0 25 20 15 cm BI /W 2 /H 1 /CS /RGB /BPC 8 /F /AHx ID FF0000 00FF00> EI Q \
                /O gs 0 0 1 rg 10 10 70 55 re f\n";
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 80] /Resources << /ExtGState << /O << /ca 0.5 >> >> >> >>".into(),
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".into()),
        (
            4,
            format!("<< /Length {} >>\nstream\n{page}endstream", page.len()),
        ),
    ])
}

/// The white-backed raster and the vector-transparency export, with
/// `tight_mask_patches` as given.
fn export(bytes: &[u8], tight: bool) -> (Pixmap, EmfExport) {
    let doc = Document::from_bytes(bytes.to_vec()).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    let reference = render_page_with(
        &doc,
        &p,
        DPI / 72.0,
        &RenderOptions::default().with_backdrop(PageBackdrop::White),
    )
    .expect("render");
    let options = EmfOptions::default()
        .with_raster_dpi(DPI)
        .with_vector_transparency(true)
        .with_tight_mask_patches(tight);
    let emf = export_emf(&doc, &p, &RenderOptions::default(), &options).expect("emf export");
    assert_well_formed(&emf.emf);
    (reference.pixmap, emf)
}

fn cases() -> [(&'static str, Vec<u8>); 2] {
    [
        ("translucent", translucent()),
        ("overprinted", overprinted()),
    ]
}

/// Only the shadow is pixels: 40 × 30 px, grown a pixel each side, where
/// the whole layer box would be 180 × 140 and the paint over it 140 × 110.
#[test]
fn only_the_shadow_is_a_patch() {
    for (name, bytes) in cases() {
        let (_, on) = export(&bytes, true);
        let v = on.outcome.vectorised;
        assert_eq!(v.split, 1, "{name}: {v:?}");
        // The layer itself, and the paint over it where it is cut.
        assert_eq!(v.patched_not_flat, 2, "{name}: {v:?}");
        assert_eq!(v.not_flat.soft_mask_image, 2, "{name}: {v:?}");
        assert_eq!(v.not_flat.beneath, 1, "{name}: {v:?}");
        assert!(v.regions > 0, "{name}: {v:?}");
        let area: u32 = bitmap_sizes(&on.emf).iter().map(|(w, h)| w * h).sum();
        assert!(area > 0 && area <= 42 * 32, "{name}: {area} px");
    }
}

/// Off by default: the paint over the shadow is patched whole, as before
/// the option existed.
#[test]
fn by_default_the_paint_over_a_shadow_is_patched_whole() {
    for (name, bytes) in cases() {
        let (_, off) = export(&bytes, false);
        let v = off.outcome.vectorised;
        assert_eq!(v.split, 0, "{name}: {v:?}");
        assert_eq!(v.not_flat.beneath, 1, "{name}: {v:?}");
        let area: u32 = bitmap_sizes(&off.emf).iter().map(|(w, h)| w * h).sum();
        assert!(area >= 140 * 110, "{name}: {area} px");
    }
}

/// Over an image the cut is the image's box: the paint is pixels there
/// and regions around it.
#[test]
fn paint_over_an_image_is_cut_to_the_image() {
    let (_, on) = export(&over_an_image(), true);
    let v = on.outcome.vectorised;
    assert_eq!(v.split, 1, "{v:?}");
    assert_eq!(v.not_flat.image, 1, "{v:?}");
    assert_eq!(v.not_flat.beneath, 1, "{v:?}");
    let area: u32 = bitmap_sizes(&on.emf).iter().map(|(w, h)| w * h).sum();
    assert!(area > 0 && area <= 82 * 52 + 2, "{area} px");
    let (_, off) = export(&over_an_image(), false);
    assert_eq!(off.outcome.vectorised.split, 0);
}

#[cfg(windows)]
#[test]
fn paint_over_a_shadow_plays_back_like_the_raster() {
    let image = ("image", over_an_image());
    for (name, bytes) in cases().into_iter().chain([image]) {
        for tight in [false, true] {
            let (reference, emf) = export(&bytes, tight);
            let tag = format!("shadow-{name}-{tight}");
            let (bad, worst) = playback_against_raster(&tag, &reference, &emf.emf, ROUNDING);
            assert_eq!(bad, 0, "{name} tight={tight}: worst {worst}");
        }
    }
}
