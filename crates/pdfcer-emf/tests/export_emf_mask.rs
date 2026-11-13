//! # Soft masks with flat content as vector regions (`Pass 700.15`)
//!
//! With `EmfOptions::with_vector_transparency(true)` a soft mask whose
//! group is flat colour (ISO 32000-2 §11.6.5) resolves into regions of one
//! mask value each, so what it masks is written as opaque regions and no
//! bitmap. A mask with a gradient in its group stays a patch. The oracle
//! is GDI's `PlayEnhMetaFile`, compared per pixel against the raster.

// Tests fail by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_emf::{EmfExport, EmfOptions, export_emf, walk_records};
use pdfcer_render::{PageBackdrop, RenderOptions, render_page_with};
use tiny_skia::Pixmap;

use crate::export_emf::{DPI, assert_well_formed, build, count};
use crate::export_emf_patches::fnv1a;
#[cfg(windows)]
use crate::export_emf_patches::{ROUNDING, playback_against_raster};

/// Flat mask content: white and mid grey squares, overlapping.
const FLAT: &str = "1 g 20 15 40 30 re f 0.6 g 40 25 40 40 re f";

/// Mask content with one fill at half opacity, through `/A`.
const HALF: &str = "1 g 20 15 40 30 re f /A gs 0.2 g 40 25 40 40 re f";

/// Mask content painted by an axial shading.
const GRADIENT: &str = "/Sh sh";

/// A blue square, then mask `/S` (`smask` entries, group object 5 with
/// `bbox` and `content`), then a red rectangle over most of the page.
/// With `form` the red is painted from a transparency-group form instead.
fn masked(smask: &str, bbox: &str, content: &str, form: bool) -> Vec<u8> {
    masked_in("/DeviceRGB", smask, bbox, content, form)
}

/// [`masked`] with the mask group blending in `cs`.
fn masked_in(cs: &str, smask: &str, bbox: &str, content: &str, form: bool) -> Vec<u8> {
    let red = "1 0 0 rg 5 5 90 70 re f";
    let page = if form {
        "0 0.6 1 rg 10 10 50 40 re f /S gs /F Do\n".to_owned()
    } else {
        format!("0 0.6 1 rg 10 10 50 40 re f /S gs {red}\n")
    };
    let group_res = "/ExtGState << /A << /ca 0.5 >> >> /Shading << /Sh << /ShadingType 2 /ColorSpace /DeviceGray /Coords [0 0 100 0] /Function << /FunctionType 2 /Domain [0 1] /C0 [0] /C1 [1] /N 1 >> /Extend [true true] >> >>";
    let form_body = format!("{red}\n");
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            format!(
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 80] /Resources << /XObject << /F 6 0 R >> /ExtGState << /S << /SMask << {smask} /G 5 0 R >> >> >> >> >>"
            ),
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".into()),
        (
            4,
            format!("<< /Length {} >>\nstream\n{page}endstream", page.len()),
        ),
        (
            5,
            format!(
                "<< /Type /XObject /Subtype /Form /BBox [{bbox}] /Group << /S /Transparency /CS {cs} >> /Resources << {group_res} >> /Length {} >>\nstream\n{content}\nendstream",
                content.len() + 1
            ),
        ),
        (
            6,
            format!(
                "<< /Type /XObject /Subtype /Form /BBox [0 0 100 80] /Group << /S /Transparency >> /Length {} >>\nstream\n{form_body}endstream",
                form_body.len()
            ),
        ),
    ])
}

const PAGE: &str = "0 0 100 80";

/// [`FLAT`] in CMYK: opaque, blending normally.
const FLAT_CMYK: &str = "0 0 0 0 k 20 15 40 30 re f 0 0 0 0.4 k 40 25 40 40 re f";

/// [`HALF`] in CMYK: one fill at half opacity.
const HALF_CMYK: &str = "0 0 0 0 k 20 15 40 30 re f /A gs 0 0 0 0.8 k 40 25 40 40 re f";

/// `/TR` inverting the mask value: `1 − v`.
const INVERT: &str = "/TR << /FunctionType 2 /Domain [0 1] /C0 [1] /C1 [0] /N 1 >>";

/// Every flat case: a name and its fixture.
fn flat_cases() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("luminosity", masked("/S /Luminosity", PAGE, FLAT, false)),
        ("alpha", masked("/S /Alpha", PAGE, HALF, false)),
        (
            "backdrop",
            masked(
                "/S /Luminosity /BC [0.5 0.5 0.5]",
                "15 10 70 50",
                FLAT,
                false,
            ),
        ),
        (
            "transfer",
            masked(&format!("/S /Luminosity {INVERT}"), PAGE, FLAT, false),
        ),
        (
            "alpha-transfer",
            masked(&format!("/S /Alpha {INVERT}"), "15 10 70 50", HALF, false),
        ),
        ("group", masked("/S /Luminosity", PAGE, FLAT, true)),
        (
            "cmyk-luminosity",
            masked_in("/DeviceCMYK", "/S /Luminosity", PAGE, FLAT_CMYK, false),
        ),
        (
            "cmyk-alpha",
            masked_in("/DeviceCMYK", "/S /Alpha", PAGE, HALF_CMYK, false),
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
fn a_flat_mask_is_written_as_regions_and_no_bitmap() {
    for (name, bytes) in flat_cases() {
        let (_, on) = export(&bytes, true);
        let o = &on.outcome;
        let v = o.vectorised;
        assert!(v.ops > 0 && v.regions > 0, "{name}: {v:?}");
        assert_eq!(v.patched(), 0, "{name}: {v:?}");
        assert_eq!(o.patches.cases(), 0, "{name}: {:?}", o.patches);
        let records = walk_records(&on.emf).unwrap();
        assert_eq!(count(&records, 0x72), 0, "{name}: no EMR_ALPHABLEND");
        assert_eq!(count(&records, 0x51), 0, "{name}: no EMR_STRETCHDIBITS");
    }
}

#[cfg(windows)]
#[test]
fn a_flat_mask_plays_back_like_the_raster() {
    for (name, bytes) in flat_cases() {
        let (reference, emf) = export(&bytes, true);
        let (bad, worst) =
            playback_against_raster(&format!("mask-{name}"), &reference, &emf.emf, ROUNDING);
        assert_eq!(bad, 0, "{name}: worst {worst}");
    }
}

/// A mask painted by an axial shading is banded (`Pass 700.23`): each band
/// is a flat mask piece, so the masked op is regions with no bitmap.
#[test]
fn a_gradient_mask_is_banded_into_regions() {
    let (_, on) = export(&masked("/S /Luminosity", PAGE, GRADIENT, false), true);
    let v = on.outcome.vectorised;
    assert_eq!(v.patched(), 0, "{v:?}");
    assert!(v.ops > 0 && v.regions > 0, "{v:?}");
    let records = walk_records(&on.emf).unwrap();
    assert_eq!(count(&records, 0x72) + count(&records, 0x51), 0);
}

/// Mask content painted by a 2×1 grey image, as a drop shadow's blur is.
const IMAGE: &str = "q 60 0 0 40 20 15 cm BI /W 2 /H 1 /CS /G /BPC 8 /F /AHx ID 40C0> EI Q";

/// An image in a mask group is raster by design (a print flattener
/// rasterises a drop shadow too), so it is counted apart from a varying
/// mask, which is a vector gap.
#[test]
fn an_image_mask_is_counted_apart_from_a_gradient_mask() {
    let (_, on) = export(&masked("/S /Luminosity", PAGE, IMAGE, false), true);
    let v = on.outcome.vectorised;
    assert_eq!(v.patched_not_flat, 1, "{v:?}");
    assert_eq!(v.not_flat.soft_mask_image, 1, "{v:?}");
    assert_eq!(v.not_flat.soft_mask_varying, 0, "{v:?}");
    assert!(on.outcome.patches.cases() > 0, "{:?}", on.outcome.patches);
}

#[cfg(windows)]
#[test]
fn an_image_mask_patch_plays_back_like_the_raster() {
    let bytes = masked("/S /Luminosity", PAGE, IMAGE, false);
    let (reference, emf) = export(&bytes, true);
    let (bad, worst) = playback_against_raster("mask-image", &reference, &emf.emf, ROUNDING);
    assert_eq!(bad, 0, "worst {worst}");
}

/// With the switch off a masked op is written from the recording's own
/// mask raster, which must honour the mask group's `/BBox` and `/TR`.
/// That route already differs from the raster along the masked edges, so
/// a `/BBox` or `/TR` case may differ in no more pixels than its plain
/// twin; ignoring either adds a whole band.
#[cfg(windows)]
#[test]
fn with_the_switch_off_a_mask_bitmap_honours_bbox_and_transfer() {
    let cases = flat_cases();
    let bad = |name: &str| {
        let bytes = &cases.iter().find(|(n, _)| *n == name).unwrap().1;
        let (reference, emf) = export(bytes, false);
        playback_against_raster(&format!("mask-off-{name}"), &reference, &emf.emf, ROUNDING).0
    };
    for (case, twin) in [
        ("backdrop", "luminosity"),
        ("transfer", "luminosity"),
        ("alpha-transfer", "alpha"),
    ] {
        let (b, t) = (bad(case), bad(twin));
        assert!(b <= t, "{case}: {b} pixels differ, {twin}: {t}");
    }
}

/// A luminosity mask whose CMYK group composites a translucent fill:
/// the colour depends on the blending space, so it stays a patch.
#[test]
fn a_translucent_cmyk_luminosity_mask_stays_a_patch() {
    let bytes = masked_in("/DeviceCMYK", "/S /Luminosity", PAGE, HALF_CMYK, false);
    let (_, on) = export(&bytes, true);
    let v = on.outcome.vectorised;
    assert_eq!(v.patched_not_flat, 1, "{v:?}");
    assert_eq!(v.not_flat.soft_mask_flat, 1, "{v:?}");
    assert!(on.outcome.patches.cases() > 0, "{:?}", on.outcome.patches);
}

#[cfg(windows)]
#[test]
fn a_gradient_mask_plays_back_like_the_raster() {
    let bytes = masked("/S /Luminosity", PAGE, GRADIENT, false);
    let (reference, emf) = export(&bytes, true);
    let (bad, worst) = playback_against_raster("mask-gradient", &reference, &emf.emf, ROUNDING);
    assert_eq!(bad, 0, "worst {worst}");
}

#[test]
fn with_the_switch_off_the_metafile_is_unchanged() {
    // Length and FNV-1a of the bytes the writer produced before masks were
    // resolved, measured with the parent commit's CLI. The `/TR` and
    // `/BBox` cases are left out: the raster itself changed for them.
    let pins = [
        (
            "luminosity",
            flat_cases().swap_remove(0).1,
            48516,
            18_446_165_635_234_477_511_u64,
        ),
        (
            "alpha",
            flat_cases().swap_remove(1).1,
            48516,
            13_937_342_575_707_889_479,
        ),
        (
            "group",
            flat_cases().swap_remove(5).1,
            48516,
            18_446_165_635_234_477_511,
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
