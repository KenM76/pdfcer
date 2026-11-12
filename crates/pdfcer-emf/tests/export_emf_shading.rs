//! # Smooth shadings as `EMR_GRADIENTFILL` triangles (`Pass 700.14`)
//!
//! With `EmfOptions::with_vector_shadings(true)` an opaque axial, radial or
//! mesh shading — painted by `sh` or as a shading pattern's fill — is
//! written as Gouraud triangles instead of a bitmap. The oracle is GDI's own
//! `PlayEnhMetaFile` (Windows only), played onto white and compared per
//! pixel against the white-backed raster within the rounding tolerance;
//! every case also asserts the metafile holds no bitmap record.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_emf::{EmfExport, EmfOptions, export_emf, walk_records};
use pdfcer_render::{PageBackdrop, RenderOptions, render_page_with};
use tiny_skia::Pixmap;

use crate::export_emf::{DPI, assert_well_formed, build, count, page};
use crate::export_emf_patches::fnv1a;

/// `EMR_GRADIENTFILL`.
const GRADIENT_FILL: u32 = 0x76;
/// `EMR_ALPHABLEND` and `EMR_STRETCHDIBITS`: the two bitmap records.
const BITMAPS: [u32; 2] = [0x72, 0x51];

/// An axial red-to-blue ramp from x = 20 to x = 80, extended both ways.
const AXIAL: &str = "<< /ShadingType 2 /ColorSpace /DeviceRGB /Coords [20 0 80 0] \
     /Extend [true true] /Function << /FunctionType 2 /Domain [0 1] \
     /C0 [1 0 0] /C1 [0 0 1] /N 1 >> >>";
/// The same ramp in `DeviceCMYK`, cyan to yellow.
const AXIAL_CMYK: &str = "<< /ShadingType 2 /ColorSpace /DeviceCMYK /Coords [20 0 80 0] \
     /Extend [true true] /Function << /FunctionType 2 /Domain [0 1] \
     /C0 [1 0 0 0] /C1 [0 0 1 0] /N 1 >> >>";
/// Concentric circles, r = 8 to r = 35, both extended.
const RADIAL_CONCENTRIC: &str = "<< /ShadingType 3 /ColorSpace /DeviceRGB \
     /Coords [50 40 8 50 40 35] /Extend [true true] /Function << /FunctionType 2 \
     /Domain [0 1] /C0 [1 1 0] /C1 [0 0.5 0] /N 1 >> >>";
/// An offset start circle inside the end circle, both extended.
const RADIAL_OFFSET: &str = "<< /ShadingType 3 /ColorSpace /DeviceRGB \
     /Coords [35 30 5 50 40 35] /Extend [true true] /Function << /FunctionType 2 \
     /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> >>";
/// Two circles side by side, neither inside the other: a cone.
const RADIAL_CONE: &str = "<< /ShadingType 3 /ColorSpace /DeviceRGB \
     /Coords [25 40 6 70 40 18] /Extend [true true] /Function << /FunctionType 2 \
     /Domain [0 1] /C0 [0 1 0] /C1 [1 0 1] /N 1 >> >>";
/// A radial ramp in a `Separation` space through `DeviceCMYK`.
const RADIAL_SEPARATION: &str = "<< /ShadingType 3 \
     /ColorSpace [/Separation /Spot /DeviceCMYK << /FunctionType 2 /Domain [0 1] \
     /C0 [0 0 0 0] /C1 [0 1 1 0] /N 1 >>] /Coords [50 40 4 50 40 30] \
     /Extend [false true] /Function << /FunctionType 2 /Domain [0 1] \
     /C0 [0.1] /C1 [1] /N 1 >> >>";

/// A type 1 shading over user `[20 80] × [20 60]` whose colour is
/// `(x, y, 0.25)` at domain point `(x, y)`: a 2-in type 4 function.
fn function_based_page() -> Vec<u8> {
    let function = "{ 0.25 }";
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 80] \
             /Resources << /Shading << /Sh0 << /ShadingType 1 /ColorSpace /DeviceRGB \
             /Domain [0 1 0 1] /Matrix [60 0 0 40 20 20] /Function 5 0 R >> >> >> >>"
                .into(),
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".into()),
        (4, "<< /Length 7 >>\nstream\n/Sh0 sh\nendstream".into()),
        (
            5,
            format!(
                "<< /FunctionType 4 /Domain [0 1 0 1] /Range [0 1 0 1 0 1] /Length {} >>\n\
                 stream\n{function}\nendstream",
                function.len()
            ),
        ),
    ])
}

/// A page whose `/Sh0` is `shading`, drawn by `content`.
fn shading_page(shading: &str, content: &str) -> Vec<u8> {
    page(&format!("/Shading << /Sh0 {shading} >>"), content)
}

/// A page filling `content`'s path with a shading pattern of `shading`.
fn pattern_page(shading: &str, extra: &str, content: &str) -> Vec<u8> {
    page(
        &format!("/Pattern << /P0 << /PatternType 2 /Shading {shading} {extra} >> >>"),
        content,
    )
}

/// A page whose `/Sh0` is a mesh shading stream of `dict` keys and
/// ASCII-hex `data`.
fn mesh_page(dict: &str, data: &str) -> Vec<u8> {
    let stream = format!("{data}>");
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 80] \
             /Resources << /Shading << /Sh0 5 0 R >> >> >>"
                .into(),
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".into()),
        (4, "<< /Length 7 >>\nstream\n/Sh0 sh\nendstream".into()),
        (
            5,
            format!(
                "<< {dict} /Filter /ASCIIHexDecode /Length {} >>\nstream\n{stream}\nendstream",
                stream.len()
            ),
        ),
    ])
}

/// Type 4: two triangles over the page, RGB corners.
fn mesh_triangles() -> Vec<u8> {
    mesh_page(
        "/ShadingType 4 /ColorSpace /DeviceRGB /BitsPerCoordinate 8 \
         /BitsPerComponent 8 /BitsPerFlag 8 /Decode [0 100 0 80 0 1 0 1 0 1]",
        "000000FF0000 00FF0000FF00 0000FF0000FF \
         00FF0000FF00 00FFFFFFFF00 0000FF0000FF",
    )
}

/// Type 6: one Coons patch with a bowed top edge.
fn mesh_coons() -> Vec<u8> {
    // Flag, then twelve points anticlockwise from the lower left (left
    // edge up, top edge right, right edge down, bottom edge left), then
    // the four corner colours.
    let points = "1010 1060 1090 10F0 50FF B0FF F0F0 F0A0 F050 F010 A010 5010";
    let colours = "FF0000 00FF00 0000FF FFFF00";
    mesh_page(
        "/ShadingType 6 /ColorSpace /DeviceRGB /BitsPerCoordinate 8 \
         /BitsPerComponent 8 /BitsPerFlag 8 /Decode [0 100 0 80 0 1 0 1 0 1]",
        &format!("00 {points} {colours}"),
    )
}

fn export(bytes: &[u8], shadings: bool) -> (Pixmap, EmfExport) {
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
        .with_vector_shadings(shadings);
    let emf = export_emf(&doc, &p, &RenderOptions::default(), &options).expect("emf export");
    assert_well_formed(&emf.emf);
    (reference.pixmap, emf)
}

/// The raster's alpha per pixel over a transparent backdrop.
#[cfg(windows)]
fn coverage(bytes: &[u8]) -> Vec<u8> {
    let doc = Document::from_bytes(bytes.to_vec()).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    let options = RenderOptions::default().with_backdrop(PageBackdrop::Transparent);
    let r = render_page_with(&doc, &p, DPI / 72.0, &options).expect("render");
    r.pixmap.pixels().iter().map(|c| c.alpha()).collect()
}

/// `(GRADIENTFILL records, bitmap records)`.
fn records(emf: &[u8]) -> (usize, usize) {
    let r = walk_records(emf).unwrap();
    (
        count(&r, GRADIENT_FILL),
        BITMAPS.iter().map(|&k| count(&r, k)).sum(),
    )
}

/// Export `bytes` both ways: the switch must turn `shadings` bitmaps into
/// triangles, leave no bitmap, and (on Windows) play back like the raster.
fn assert_vectorised(name: &str, bytes: &[u8], shadings: usize) {
    let (_, off) = export(bytes, false);
    assert!(
        records(&off.emf).1 > 0,
        "{name}: no bitmap without the switch"
    );
    let (reference, on) = export(bytes, true);
    let s = on.outcome.shadings;
    assert_eq!((s.vector, s.declined()), (shadings, 0), "{name}: {s:?}");
    let (fills, bitmaps) = records(&on.emf);
    assert!(fills >= shadings, "{name}: {fills} gradient fills");
    assert_eq!(bitmaps, 0, "{name}: bitmaps remain");
    assert_eq!(on.outcome.tally.shadings_rasterised, 0, "{name}");
    assert_eq!(on.outcome.gradients_rasterised, 0, "{name}");
    assert_eq!(on.outcome.print_fallback(), None, "{name}");
    #[cfg(windows)]
    {
        let coverage = coverage(bytes);
        let (bad, worst) = playback_within_a_pixel(name, &reference, &coverage, &on.emf);
        assert_eq!(bad, 0, "{name}: {bad} pixels off, worst {worst}");
    }
    #[cfg(not(windows))]
    let _ = reference;
}

/// GDI plays `emf` onto white; a pixel passes when every channel lies
/// between the raster's value there and its value at one of the 8
/// neighbours, give or take `ROUNDING` — the raster's colour somewhere
/// within one device pixel. A plain per-pixel match is too strict: GDI's
/// triangle filler steps each span's colour from the edge, so on a steep
/// ramp (8 levels a pixel in the offset radial) it lands about half a
/// pixel's step off the exact interpolant at the pixel centre.
///
/// That rule holds where the raster's 3×3 is fully covered (`coverage` is
/// its alpha over a transparent backdrop). A pixel whose 5×5 the raster
/// leaves untouched must play back white. The band between — the rim — is
/// not compared: there a half-pixel geometric difference flips coverage,
/// the raster's colour one pixel outward is an anti-aliased blend, a path
/// tip or a cone's apex is thinner than a pixel, and at a cone's envelope
/// `s` changes without bound across the rim, so no colour tolerance holds.
/// `(failing pixels, worst distance)`.
#[cfg(windows)]
fn playback_within_a_pixel(
    name: &str,
    reference: &Pixmap,
    coverage: &[u8],
    emf: &[u8],
) -> (usize, u8) {
    use crate::export_emf_patches::ROUNDING;
    let dir = crate::export_emf::scratch_dir(&format!("shading-{name}"));
    let (w, h) = (reference.width() as usize, reference.height() as usize);
    let raw = crate::export_emf::play_with_gdi(&dir, name, emf, w as u32, h as u32);
    let _ = std::fs::remove_dir_all(&dir);
    let px = reference.pixels();
    let rgb = |i: usize| [px[i].red(), px[i].green(), px[i].blue()];
    let outside = |v: u8, a: u8, b: u8| v.saturating_sub(a.max(b)).max(a.min(b).saturating_sub(v));
    let (mut bad, mut worst) = (0usize, 0u8);
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let g = [raw[i * 4 + 2], raw[i * 4 + 1], raw[i * 4]];
            let all = |reach: usize, f: &dyn Fn(u8) -> bool| {
                (y.saturating_sub(reach)..(y + reach + 1).min(h)).all(|ny| {
                    (x.saturating_sub(reach)..(x + reach + 1).min(w))
                        .all(|nx| f(coverage[ny * w + nx]))
                })
            };
            let best = if all(1, &|a| a == u8::MAX) {
                let mut best = u8::MAX;
                for ny in y.saturating_sub(1)..(y + 2).min(h) {
                    for nx in x.saturating_sub(1)..(x + 2).min(w) {
                        let (r, n) = (rgb(i), rgb(ny * w + nx));
                        let d = (0..3).map(|c| outside(g[c], r[c], n[c])).max().unwrap();
                        best = best.min(d);
                    }
                }
                best
            } else if all(2, &|a| a == 0) {
                g.into_iter().map(|v| u8::MAX - v).max().unwrap()
            } else {
                0
            };
            if best > ROUNDING {
                if bad < 8 {
                    eprintln!("{name} ({x},{y}): raster {:?}, GDI {g:?}", rgb(i));
                }
                bad += 1;
                worst = worst.max(best);
            }
        }
    }
    (bad, worst)
}

#[test]
fn an_extended_axial_shading_is_triangles() {
    assert_vectorised("axial", &shading_page(AXIAL, "/Sh0 sh"), 1);
}

#[test]
fn a_concentric_radial_shading_is_triangles() {
    assert_vectorised(
        "radial-concentric",
        &shading_page(RADIAL_CONCENTRIC, "/Sh0 sh"),
        1,
    );
}

#[test]
fn an_offset_radial_shading_is_triangles() {
    assert_vectorised("radial-offset", &shading_page(RADIAL_OFFSET, "/Sh0 sh"), 1);
}

#[test]
fn a_radial_cone_is_triangles() {
    assert_vectorised("radial-cone", &shading_page(RADIAL_CONE, "/Sh0 sh"), 1);
}

#[test]
fn a_triangle_mesh_is_triangles() {
    assert_vectorised("mesh-type4", &mesh_triangles(), 1);
}

#[test]
fn a_coons_patch_mesh_is_triangles() {
    assert_vectorised("mesh-type6", &mesh_coons(), 1);
}

#[test]
fn a_shading_under_a_clip_stays_inside_it() {
    let clip = "q 15 10 40 50 re W n 30 40 m 90 75 l 90 5 l h W n /Sh0 sh Q";
    assert_vectorised("axial-clip", &shading_page(AXIAL, clip), 1);
    assert_vectorised("radial-clip", &shading_page(RADIAL_OFFSET, clip), 1);
}

#[test]
fn a_shading_pattern_fill_is_clipped_to_its_path_and_bbox() {
    let fill = "/Pattern cs /P0 scn 10 10 m 90 20 l 60 75 l h f";
    assert_vectorised("axial-pattern", &pattern_page(AXIAL, "", fill), 1);
    let bbox = "/BBox [30 15 85 65]";
    let radial = RADIAL_CONCENTRIC.replace("/ShadingType 3", &format!("/ShadingType 3 {bbox}"));
    assert_vectorised("radial-pattern", &pattern_page(&radial, "", fill), 1);
}

#[test]
fn a_non_rgb_shading_takes_the_raster_colours() {
    assert_vectorised("axial-cmyk", &shading_page(AXIAL_CMYK, "/Sh0 sh"), 1);
    assert_vectorised(
        "radial-separation",
        &shading_page(RADIAL_SEPARATION, "/Sh0 sh"),
        1,
    );
}

/// The separation ramp's sampled knots sit closer than a device pixel; they
/// are merged into bands at least a pixel wide (14 280 triangles unmerged,
/// 4 488 merged).
#[test]
fn a_crowded_radial_ramp_is_merged_into_pixel_wide_bands() {
    let (_, on) = export(&shading_page(RADIAL_SEPARATION, "/Sh0 sh"), true);
    let s = on.outcome.shadings;
    assert_eq!(s.vector, 1, "{s:?}");
    assert!(s.triangles <= 6_000, "{s:?}");
}

/// A type 1 shading is painted inside its domain only (SH20, SH21) and
/// reaches the metafile as a bitmap that plays back like the raster, with
/// or without vector shadings (no triangle route takes type 1).
#[test]
fn a_function_based_shading_is_painted_and_kept_as_a_bitmap() {
    let bytes = function_based_page();
    let (reference, off) = export(&bytes, false);
    let at = |x: u32, y: u32| {
        let c = reference.pixel(x, y).unwrap();
        [c.red(), c.green(), c.blue()]
    };
    // User (50, 40) is domain (0.5, 0.5); user (5, 5) is outside it.
    let mid = at(100, 80);
    assert!(
        mid.iter()
            .zip([128u8, 128, 64])
            .all(|(g, w)| g.abs_diff(w) <= 3),
        "{mid:?}"
    );
    assert_eq!(at(10, 150), [255, 255, 255]);
    assert_eq!(off.outcome.tally.shadings_rasterised, 1);
    assert!(records(&off.emf).1 > 0, "no bitmap");
    let (_, on) = export(&bytes, true);
    assert_eq!(
        (on.outcome.shadings.vector, on.outcome.shadings.declined()),
        (0, 1)
    );
    assert!(records(&on.emf).1 > 0, "no bitmap with vector shadings");
    #[cfg(windows)]
    {
        use crate::export_emf_patches::{ROUNDING, playback_against_raster};
        let (bad, worst) = playback_against_raster("type1", &reference, &off.emf, ROUNDING);
        assert_eq!(bad, 0, "{bad} pixels off, worst {worst}");
    }
}

#[test]
fn a_translucent_shading_stays_a_bitmap() {
    let bytes = page(
        &format!("/Shading << /Sh0 {RADIAL_OFFSET} >> /ExtGState << /G0 << /ca 0.5 >> >>"),
        "/G0 gs /Sh0 sh",
    );
    let (_, on) = export(&bytes, true);
    assert_eq!(on.outcome.shadings.vector, 0);
    assert!(records(&on.emf).1 > 0);
}

#[test]
fn with_the_switch_off_the_metafile_is_unchanged() {
    let bytes = shading_page(RADIAL_OFFSET, "/Sh0 sh");
    let doc = Document::from_bytes(bytes).unwrap();
    let p = page_tree::pages(&doc).unwrap().remove(0);
    let base = EmfOptions::default().with_raster_dpi(DPI);
    let default = export_emf(&doc, &p, &RenderOptions::default(), &base).unwrap();
    let off = export_emf(
        &doc,
        &p,
        &RenderOptions::default(),
        &base.clone().with_vector_shadings(false),
    )
    .unwrap();
    assert_eq!(default.emf, off.emf);
    // The bytes the writer produced before the switch existed (measured
    // on the parent commit).
    assert_eq!(
        (off.emf.len(), fnv1a(&off.emf)),
        (128_328, 7_405_992_265_183_584_029)
    );
    assert_eq!(off.outcome.shadings, pdfcer_emf::EmfShadings::default());
}
