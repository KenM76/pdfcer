//! # Flat transparency as vector regions (`Pass 700.13`)
//!
//! With `EmfOptions::with_vector_transparency(true)` a constant alpha, a
//! blend, a transparency group or an overprint over flat colours is written
//! as opaque regions, each already composited, and no bitmap. The oracle is
//! GDI's own `PlayEnhMetaFile` (Windows only), played onto white and
//! compared per pixel against the white-backed raster.

// Tests fail by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_emf::{EmfExport, EmfOptions, export_emf, walk_records};
use pdfcer_render::{PageBackdrop, RenderOptions, render_page_with};
use tiny_skia::Pixmap;

use crate::export_emf::{DPI, assert_well_formed, build, count, page};
use crate::export_emf_patches::{OVERPRINT, OVERPRINT_RES, fnv1a, non_isolated_group};
#[cfg(windows)]
use crate::export_emf_patches::{ROUNDING, playback_against_raster};

/// Two overlapping squares in RGB, the second painted through `/X` with `/X` as given.
fn rgb(x: &str) -> Vec<u8> {
    let content = "1 0.8 0 rg 10 10 50 40 re f /X gs 0 0.4 1 rg 30 20 40 40 re f";
    page(&format!("/ExtGState << /X << {x} >> >>"), content)
}

/// The same pair in CMYK, on a page whose group blends in DeviceCMYK.
fn cmyk(x: &str) -> Vec<u8> {
    let content = "0 0.2 1 0 k 10 10 50 40 re f /X gs 0.9 0.3 0 0.1 k 30 20 40 40 re f\n";
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            format!(
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 80] /Resources << /ExtGState << /X << {x} >> >> >> >>"
            ),
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
    ])
}

/// A blue backdrop, then form `/G` (group dictionary `group`) painted
/// through `/H` (`h`); inside, two overlapping fills, the second through
/// `/M` (`m`).
fn grouped(group: &str, h: &str, m: &str) -> Vec<u8> {
    let form = "1 1 0 rg 30 20 30 30 re f /M gs 1 0 0.5 rg 45 30 30 30 re f\n";
    let content = "0 0.6 1 rg 10 10 50 40 re f /H gs /G Do\n";
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            format!(
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 80] /Resources << /XObject << /G 5 0 R >> /ExtGState << /H << {h} >> >> >> >>"
            ),
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".into()),
        (
            4,
            format!(
                "<< /Length {} >>\nstream\n{content}endstream",
                content.len()
            ),
        ),
        (
            5,
            format!(
                "<< /Type /XObject /Subtype /Form /BBox [0 0 100 80] /Group << /S /Transparency {group} >> /Resources << /ExtGState << /M << {m} >> >> >> /Length {} >>\nstream\n{form}endstream",
                form.len()
            ),
        ),
    ])
}

/// Every case: a name and its fixture.
fn cases() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("alpha", rgb("/ca 0.5")),
        ("multiply", rgb("/BM /Multiply")),
        ("difference-alpha", rgb("/BM /Difference /ca 0.6")),
        ("cmyk-alpha", cmyk("/ca 0.5")),
        ("cmyk-multiply", cmyk("/BM /Multiply")),
        ("cmyk-difference", cmyk("/BM /Difference")),
        ("overprint", page(OVERPRINT_RES, OVERPRINT)),
        ("cmyk-overprint", cmyk("/OP true /op true /OPM 1")),
        (
            "group-luminosity",
            grouped("/I true", "/BM /Luminosity", "/ca 1"),
        ),
        (
            "group-hue-alpha",
            grouped("/I true", "/BM /Hue /ca 0.8", "/ca 1"),
        ),
        (
            "isolated",
            grouped("/I true", "/ca 0.7", "/BM /Multiply /ca 0.6"),
        ),
        ("knockout", grouped("/K true", "/ca 1", "/ca 0.5")),
        (
            "knockout-isolated",
            grouped("/I true /K true", "/ca 0.8", "/BM /Screen /ca 0.5"),
        ),
        ("non-isolated", non_isolated_group()),
        (
            "non-isolated-knockout",
            grouped("/I false /K true", "/ca 0.7", "/BM /Multiply"),
        ),
    ]
}

/// The white-backed raster and the export with the switch as given.
fn export(bytes: &[u8], vector: bool) -> (Pixmap, EmfExport) {
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
        .with_vector_transparency(vector);
    let emf = export_emf(&doc, &p, &RenderOptions::default(), &options).expect("emf export");
    assert_well_formed(&emf.emf);
    (reference.pixmap, emf)
}

#[test]
fn every_flat_case_is_written_as_regions_and_no_bitmap() {
    for (name, bytes) in cases() {
        let (_, off) = export(&bytes, false);
        assert!(
            off.outcome.print_fallback().is_some(),
            "{name}: the case bites"
        );
        let (_, on) = export(&bytes, true);
        let o = &on.outcome;
        let v = o.vectorised;
        assert!(v.ops > 0 && v.regions > 0, "{name}: {v:?}");
        assert_eq!(v.patched(), 0, "{name}: {v:?}");
        assert_eq!(o.patches.cases(), 0, "{name}: {:?}", o.patches);
        assert_eq!(o.print_fallback(), None, "{name}: {o:?}");
        let records = walk_records(&on.emf).unwrap();
        assert_eq!(count(&records, 0x72), 0, "{name}: no EMR_ALPHABLEND");
    }
}

#[test]
fn each_cause_is_counted() {
    let v = |b: Vec<u8>| export(&b, true).1.outcome.vectorised;
    let alpha = v(rgb("/ca 0.5"));
    assert_eq!((alpha.ops, alpha.alpha, alpha.blends), (1, 1, 0));
    let blend = v(rgb("/BM /Multiply"));
    assert_eq!((blend.ops, blend.alpha, blend.blends), (1, 0, 1));
    let over = v(page(OVERPRINT_RES, OVERPRINT));
    assert_eq!((over.ops, over.overprint), (1, 1), "{over:?}");
    let group = v(grouped("/K true", "/ca 1", "/ca 0.5"));
    assert_eq!((group.ops, group.groups), (1, 1), "{group:?}");
}

#[cfg(windows)]
#[test]
fn every_flat_case_plays_back_like_the_raster() {
    for (name, bytes) in cases() {
        let (reference, emf) = export(&bytes, true);
        let (bad, worst) =
            playback_against_raster(&format!("vector-{name}"), &reference, &emf.emf, ROUNDING);
        assert_eq!(bad, 0, "{name}: worst {worst}");
    }
}

#[test]
fn a_paint_over_an_image_is_patched_and_counted() {
    let (_, on) = export(&with_image(), true);
    let o = &on.outcome;
    assert_eq!(o.vectorised.patched_not_flat, 1, "{:?}", o.vectorised);
    assert_eq!(o.vectorised.ops, 0, "{:?}", o.vectorised);
    assert_eq!(o.patches.transparency, 1, "{:?}", o.patches);
    assert_eq!(o.print_fallback(), None, "{o:?}");
}

/// A 2 x 2 image, then a half-transparent square over it.
fn with_image() -> Vec<u8> {
    let content = "q 40 0 0 40 10 10 cm /Im Do Q /X gs 0 0.4 1 rg 30 20 40 40 re f\n";
    let hex = "FF000000FF000000FFFFFF00>";
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 80] /Resources << /XObject << /Im 5 0 R >> /ExtGState << /X << /ca 0.5 >> >> >> >>"
                .into(),
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".into()),
        (
            4,
            format!("<< /Length {} >>\nstream\n{content}endstream", content.len()),
        ),
        (
            5,
            format!(
                "<< /Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /ASCIIHexDecode /Length {} >>\nstream\n{hex}\nendstream",
                hex.len() + 1
            ),
        ),
    ])
}

#[test]
fn with_the_switch_off_the_metafile_is_unchanged() {
    // Length and FNV-1a of the bytes the writer produced before the switch
    // existed, measured on the parent commit.
    let pins = [
        ("alpha", rgb("/ca 0.5"), 26116, 0x96ef_f99e_74e6_501b_u64),
        (
            "knockout",
            grouped("/K true", "/ca 1", "/ca 0.5"),
            29316,
            0x14f5_b98c_60f5_53bd,
        ),
        (
            "cmyk-multiply",
            cmyk("/BM /Multiply"),
            26116,
            0x59f0_c210_f91e_5161,
        ),
    ];
    for (name, bytes, len, hash) in pins {
        let doc = Document::from_bytes(bytes).unwrap();
        let p = page_tree::pages(&doc).unwrap().remove(0);
        let base = EmfOptions::default().with_raster_dpi(DPI);
        let default = export_emf(&doc, &p, &RenderOptions::default(), &base).unwrap();
        let off = export_emf(
            &doc,
            &p,
            &RenderOptions::default(),
            &base.clone().with_vector_transparency(false),
        )
        .unwrap();
        assert_eq!(default.emf, off.emf, "{name}");
        assert_eq!(off.outcome.vectorised, pdfcer_emf::EmfVectorised::default());
        assert_eq!((off.emf.len(), fnv1a(&off.emf)), (len, hash), "{name}");
    }
}
