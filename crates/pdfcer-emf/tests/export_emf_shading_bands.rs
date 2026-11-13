//! # Shadings under transparency as flat vector bands (`Pass 700.23`)
//!
//! With `EmfOptions::with_vector_transparency(true)` an axial or radial
//! shading that composites with other paint is written as flat-colour band
//! regions, as a print RIP flattens a smooth shade, instead of patching the
//! stack; an overprinted one on an ink page keeps the backdrop in the
//! components it does not name, and a spot one deposits each band's tints
//! on the colorant's plane. A mesh stays a patch. The oracle is GDI's
//! `PlayEnhMetaFile` (Windows only) against the white-backed raster.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_emf::{EmfExport, EmfOptions, export_emf, walk_records};
use pdfcer_render::{PageBackdrop, RenderOptions, render_page_with};
use tiny_skia::Pixmap;

use crate::export_emf::{DPI, assert_well_formed, build, count, page};
#[cfg(windows)]
use crate::export_emf_patches::playback_against_raster;

/// An axial red-to-blue ramp from x = 20 to x = 80, extended both ways.
const AXIAL: &str = "<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [20 0 80 0] \
     /Extend [true true] /Function << /FunctionType 2 /Domain [0 1] \
     /C0 [1 0 0] /C1 [0 0 1] /N 1 >> >>";
/// An offset start circle inside the end circle, both extended. The ramp
/// is gentle (51 levels over the 24 pixels where the rings crowd): a band
/// is at least a pixel wide, so a ramp steeper per pixel than the playback
/// tolerance would measure the tolerance, not the banding.
const RADIAL: &str = "<< /ShadingType 3 /ColorSpace /DeviceRGB \
     /Coords [35 30 5 50 40 35] /Extend [true true] /Function << /FunctionType 2 \
     /Domain [0 1] /C0 [1 0 0] /C1 [0.8 0 0.2] /N 1 >> >>";

/// Circles a point apart on one side and nine on the other: the bands
/// crowd where the circles nearly touch and must stay fine where they
/// do not.
const CROWDED: &str = "<< /ShadingType 3 /ColorSpace /DeviceRGB      /Coords [40 40 4 44 40 9] /Extend [true true] /Function << /FunctionType 2      /Domain [0 1] /C0 [1 0 0] /C1 [0.85 0 0.15] /N 1 >> >>";

/// A half-opaque green square over the shading.
const OVER: &str = "/Sh0 sh /A gs 0 1 0 rg 30 20 40 40 re f";

/// A page whose `/Sh0` is `shading` and `/A` is `ca 0.5`.
fn over(shading: &str) -> Vec<u8> {
    page(
        &format!("/Shading << /Sh0 {shading} >> /ExtGState << /A << /ca 0.5 >> >>"),
        OVER,
    )
}

/// An axial cyan-to-yellow ramp in `DeviceCMYK`.
const AXIAL_CMYK: &str = "<< /ShadingType 2 /ColorSpace /DeviceCMYK /Coords [20 0 80 0] \
     /Extend [true true] /Function << /FunctionType 2 /Domain [0 1] \
     /C0 [1 0 0 0] /C1 [0 0 1 0] /N 1 >> >>";

/// A two-circle radial in `DeviceCMYK`: no native gradient form, so the
/// recorder harvests it as a raster with its source.
const RADIAL_CMYK: &str = "<< /ShadingType 3 /ColorSpace /DeviceCMYK \
     /Coords [35 30 5 50 40 35] /Extend [true true] /Function << /FunctionType 2 \
     /Domain [0 1] /C0 [1 0 0 0] /C1 [0.8 0 0.2 0] /N 1 >> >>";

/// A `DeviceCMYK` page group (an ink page) painting `shading` by `sh` or,
/// with `pattern`, as a shading pattern's fill, under a half-opaque
/// magenta square.
fn over_on_ink(shading: &str, pattern: bool) -> Vec<u8> {
    let paint = if pattern {
        "/Pattern cs /P0 scn 0 0 100 80 re f"
    } else {
        "/Sh0 sh"
    };
    let content = format!("{paint} /A gs 0 1 0 0 k 30 20 40 40 re f\n");
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            format!(
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 80] \
                 /Resources << /Shading << /Sh0 {shading} >> \
                 /Pattern << /P0 << /PatternType 2 /Shading {shading} >> >> \
                 /ExtGState << /A << /ca 0.5 >> >> >> >>"
            ),
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R \
             /Group << /S /Transparency /CS /DeviceCMYK >> >>"
                .into(),
        ),
        (
            4,
            format!(
                "<< /Length {} >>\nstream\n{content}endstream",
                content.len()
            ),
        ),
    ])
}

/// An axial ramp of the process colorant `/Cyan` through a `Separation`.
const AXIAL_SEP_CYAN: &str = "<< /ShadingType 2 /ColorSpace [/Separation /Cyan /DeviceCMYK      << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [1 0 0 0] /N 1 >>]      /Coords [20 0 80 0] /Extend [true true] /Function << /FunctionType 2      /Domain [0 1] /C0 [0.2] /C1 [1] /N 1 >> >>";

/// [`AXIAL_SEP_CYAN`] in a spot colorant, `/Green`.
const AXIAL_SPOT: &str = "<< /ShadingType 2 /ColorSpace [/Separation /Green /DeviceCMYK      << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [1 0 1 0] /N 1 >>]      /Coords [20 0 80 0] /Extend [true true] /Function << /FunctionType 2      /Domain [0 1] /C0 [0.2] /C1 [1] /N 1 >> >>";

/// A yellow backdrop over the whole page.
const YELLOW: &str = "0 0 1 0 k 0 0 100 80 re f";

/// A 2 × 2 cyan-and-yellow image over the whole page.
const IMAGE: &str = "q 100 0 0 80 0 0 cm BI /W 2 /H 2 /CS /CMYK /BPC 8 /F /AHx ID \
     FF000000 0000FF00 0000FF00 FF000000> EI Q";

/// An ink page: [`YELLOW`] beneath, then `shading` by `sh` (under
/// `/OP /op /OPM 1` when `overprint`), then a half-opaque magenta square.
fn on_ink_backdrop(shading: &str, overprint: bool) -> Vec<u8> {
    on_ink(YELLOW, shading, overprint)
}

/// [`on_ink_backdrop`] with `beneath` painted first.
fn on_ink(beneath: &str, shading: &str, overprint: bool) -> Vec<u8> {
    let o = if overprint { "/O gs " } else { "" };
    let content = format!("{beneath} q {o}/Sh0 sh Q /A gs 0 1 0 0 k 30 20 40 40 re f\n");
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            format!(
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 80] \
                 /Resources << /Shading << /Sh0 {shading} >> /ExtGState << \
                 /A << /ca 0.5 >> /O << /OP true /op true /OPM 1 >> >> >> >>"
            ),
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R \
             /Group << /S /Transparency /CS /DeviceCMYK >> >>"
                .into(),
        ),
        (
            4,
            format!(
                "<< /Length {} >>\nstream\n{content}endstream",
                content.len()
            ),
        ),
    ])
}

/// An overprinted shading bands as overprinted flat paints, by both
/// routes: a gradient brush naming only `/Cyan` (the yellow beneath is
/// kept, Table 149) and a harvested `DeviceCMYK` radial (every process
/// component named, so it paints normally).
#[test]
fn an_overprinted_shading_on_an_ink_page_is_banded() {
    for (name, shading) in [
        ("sep-cyan-op", AXIAL_SEP_CYAN),
        ("cmyk-radial-op", RADIAL_CMYK),
    ] {
        let (_, on) = export(&on_ink_backdrop(shading, true));
        let v = on.outcome.vectorised;
        assert_eq!(v.patched(), 0, "{name}: {v:?}");
        assert!(v.ops > 0 && v.regions > 10, "{name}: {v:?}");
        assert_eq!(bitmaps(&on.emf), 0, "{name}");
        assert_eq!(on.outcome.print_fallback(), None, "{name}");
    }
}

/// The overprinted `/Cyan` ramp keeps the yellow beneath in every band:
/// playback matches the colorant buffer's raster, which composites the
/// shading by Table 149.
#[cfg(windows)]
#[test]
fn an_overprinted_shading_plays_back_like_the_raster() {
    let (reference, on) = export(&on_ink_backdrop(AXIAL_SEP_CYAN, true));
    assert_eq!(on.outcome.vectorised.patched(), 0);
    let (bad, worst) = playback_against_raster("bands-op", &reference, &on.emf, 2);
    assert!(bad == 0 && worst <= 6, "{bad} bad, worst {worst}");
}

/// An overprinted shading over an image composites with a backdrop that
/// is not flat, so it stays a patch, recorded as what lies beneath.
#[test]
fn an_overprinted_shading_over_an_image_stays_a_patch() {
    let (_, on) = export(&on_ink(IMAGE, AXIAL_SEP_CYAN, true));
    let v = on.outcome.vectorised;
    assert!(v.patched() >= 1 && v.not_flat.beneath >= 1, "{v:?}");
    assert_eq!(v.not_flat.shading, 0, "{v:?}");
    assert!(bitmaps(&on.emf) > 0);
}

/// [`AXIAL_SPOT`] as a two-circle radial: no native gradient form, so the
/// recorder harvests it with its source.
const RADIAL_SPOT: &str = "<< /ShadingType 3 /ColorSpace [/Separation /Green /DeviceCMYK \
     << /FunctionType 2 /Domain [0 1] /C0 [0 0 0 0] /C1 [1 0 1 0] /N 1 >>] \
     /Coords [35 30 5 50 40 35] /Extend [true true] /Function << /FunctionType 2 \
     /Domain [0 1] /C0 [0.2] /C1 [1] /N 1 >> >>";

/// The spot shadings, by both routes, overprinted and not.
const SPOT_CASES: [(&str, &str, bool); 4] = [
    ("axial-spot", AXIAL_SPOT, false),
    ("axial-spot-op", AXIAL_SPOT, true),
    ("radial-spot", RADIAL_SPOT, false),
    ("radial-spot-op", RADIAL_SPOT, true),
];

/// A spot shading on an ink page bands as flat paints on the colorant's
/// own plane, by both routes, as a RIP keeps it on its spot plate.
#[test]
fn a_spot_shading_on_an_ink_page_is_banded_on_its_plane() {
    for (name, shading, op) in SPOT_CASES {
        let (_, on) = export(&on_ink_backdrop(shading, op));
        let v = on.outcome.vectorised;
        assert_eq!(v.patched(), 0, "{name}: {v:?}");
        assert!(v.ops > 0 && v.regions > 10, "{name}: {v:?}");
        assert_eq!(bitmaps(&on.emf), 0, "{name}");
        assert_eq!(on.outcome.print_fallback(), None, "{name}");
    }
}

/// The spot bands play back like the colorant buffer's raster, which
/// deposits each tint on the colorant's plane and folds the plane's ink
/// into the page: overprinted, the yellow beneath survives the spot-only
/// source (Table 149), and knocked out it does not. The steep two-circle
/// radial bands no closer to the raster in a process colorant, so it is
/// held to that: the spot route adds no error of its own.
#[cfg(windows)]
#[test]
fn a_spot_shading_plays_back_like_the_raster() {
    for (name, shading, op) in SPOT_CASES {
        let (bad, worst) = playback_error(name, shading, op);
        let (most, most_worst) = if shading == RADIAL_SPOT {
            let cyan = shading
                .replace("/Green", "/Cyan")
                .replace("[1 0 1 0]", "[1 0 0 0]");
            let (b, w) = playback_error(&format!("{name}-cyan"), &cyan, op);
            (b + b / 4, w + 1)
        } else {
            (0, 6)
        };
        assert!(
            bad <= most && worst <= most_worst,
            "{name}: {bad} bad, worst {worst}; allowed {most}, {most_worst}"
        );
    }
}

/// Pixels of `shading`'s EMF over [`on_ink_backdrop`] off the raster by
/// more than rounding, and the worst of them; the shading must not patch.
#[cfg(windows)]
fn playback_error(name: &str, shading: &str, op: bool) -> (usize, u8) {
    let (reference, on) = export(&on_ink_backdrop(shading, op));
    assert_eq!(on.outcome.vectorised.patched(), 0, "{name}");
    playback_against_raster(&format!("bands-{name}"), &reference, &on.emf, 2)
}

/// On an ink page the bands paint the shading's authored process ink, by
/// both routes: a gradient brush (axial, by `sh` or a pattern) and a
/// harvested shading (the two-circle radial).
#[test]
fn on_an_ink_page_a_cmyk_shading_is_banded_in_ink() {
    for (name, shading, pattern) in [
        ("axial-sh", AXIAL_CMYK, false),
        ("axial-pattern", AXIAL_CMYK, true),
        ("radial-sh", RADIAL_CMYK, false),
        ("radial-pattern", RADIAL_CMYK, true),
    ] {
        let (_, on) = export(&over_on_ink(shading, pattern));
        let v = on.outcome.vectorised;
        assert_eq!(v.patched(), 0, "{name}: {v:?}");
        assert!(v.ops > 0 && v.regions > 10, "{name}: {v:?}");
        // A banded shading is vector: the print route does not fall back.
        assert_eq!(on.outcome.print_fallback(), None, "{name}");
    }
}

/// [`over`] with a type 4 mesh: two RGB triangles over the page.
fn over_mesh() -> Vec<u8> {
    let data = "000000FF0000 00FF0000FF00 0000FF0000FF \
                00FF0000FF00 00FFFFFFFF00 0000FF0000FF>";
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 80] \
             /Resources << /Shading << /Sh0 5 0 R >> \
             /ExtGState << /A << /ca 0.5 >> >> >> >>"
                .into(),
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".into()),
        (
            4,
            format!(
                "<< /Length {} >>\nstream\n{OVER}\nendstream",
                OVER.len() + 1
            ),
        ),
        (
            5,
            format!(
                "<< /ShadingType 4 /ColorSpace /DeviceRGB /BitsPerCoordinate 8 \
                 /BitsPerComponent 8 /BitsPerFlag 8 /Decode [0 100 0 80 0 1 0 1 0 1] \
                 /Filter /ASCIIHexDecode /Length {} >>\nstream\n{data}\nendstream",
                data.len()
            ),
        ),
    ])
}

fn export(bytes: &[u8]) -> (Pixmap, EmfExport) {
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
        .with_vector_shadings(true);
    let emf = export_emf(&doc, &p, &RenderOptions::default(), &options).expect("emf export");
    assert_well_formed(&emf.emf);
    (reference.pixmap, emf)
}

/// Bitmap records: `EMR_ALPHABLEND` and `EMR_STRETCHDIBITS`.
fn bitmaps(emf: &[u8]) -> usize {
    let r = walk_records(emf).unwrap();
    count(&r, 0x72) + count(&r, 0x51)
}

#[test]
fn a_shading_under_a_translucent_fill_is_banded_with_no_bitmap() {
    for (name, shading) in [("axial", AXIAL), ("radial", RADIAL), ("crowded", CROWDED)] {
        let (_, on) = export(&over(shading));
        let v = on.outcome.vectorised;
        assert_eq!(v.patched(), 0, "{name}: {v:?}");
        assert!(v.ops > 0 && v.regions > 10, "{name}: {v:?}");
        // A banded shading is vector: the print route does not fall back.
        assert_eq!(on.outcome.print_fallback(), None, "{name}");
        assert_eq!(
            bitmaps(&on.emf),
            0,
            "{name}: {:?} {:?}",
            on.outcome.shadings,
            on.outcome.patches
        );
    }
}

/// Each band is within half a level of the smooth shade and GDI fills a
/// region a pixel off tiny-skia, so playback matches within a pixel and two
/// levels: one of band quantisation, one of rounding. Where crowded circles
/// pack several rings into a pixel GDI samples one and tiny-skia blends
/// them, so a handful of pixels there may differ by a few levels; coarse
/// banding on the wide side shows as hundreds.
#[cfg(windows)]
#[test]
fn a_banded_shading_plays_back_like_the_raster() {
    for (name, shading, most) in [
        ("axial", AXIAL, 0),
        ("radial", RADIAL, 0),
        ("crowded", CROWDED, 8),
    ] {
        let (reference, on) = export(&over(shading));
        assert_eq!(on.outcome.vectorised.patched(), 0, "{name}");
        let (bad, worst) =
            playback_against_raster(&format!("bands-{name}"), &reference, &on.emf, 2);
        assert!(
            bad <= most && worst <= 6,
            "{name}: {bad} bad, worst {worst}"
        );
    }
}

#[test]
fn a_mesh_under_a_translucent_fill_stays_a_patch() {
    let (_, on) = export(&over_mesh());
    let v = on.outcome.vectorised;
    assert_eq!(v.patched_not_flat, 1, "{v:?}");
    assert_eq!(v.not_flat.shading, 1, "{v:?}");
    assert!(bitmaps(&on.emf) > 0);
}
