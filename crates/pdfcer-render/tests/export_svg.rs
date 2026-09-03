//! # The SVG export renders the same picture as the raster export (`Pass 248.1`)
//!
//! The SVG writer consumes the renderer's own recording, so its correctness
//! claim is *"an independent SVG renderer draws this file the way pdfcer
//! draws the page"*. These tests hold it to that with **resvg** as the
//! independent renderer: every vector fixture is exported, parsed by
//! `usvg`, rasterised by `resvg` at the recording scale, and compared pixel
//! by pixel against `render_page_with(.., PageBackdrop::Transparent)` of
//! the same page. The tolerance is anti-aliasing — two rasterisers do not
//! agree to the bit on a diagonal edge — expressed as a cap on the number
//! of pixels differing by more than a small amount, never as an average
//! that a large wrong region could hide inside.
//!
//! What resvg cannot check here (its image decoders are off — see
//! `Cargo.toml`) is checked two other ways: **structurally** (a shading
//! page carries an `<image>` and reports `shadings_rasterised`, a
//! soft-masked page carries a `<mask>`), and **end to end through
//! Inkscape** when it is installed on the machine running the tests —
//! that test says so on stdout and passes vacuously otherwise, because a
//! CI runner without Inkscape must not go red for lacking it, and a
//! developer's machine with it must not skip silently.
//!
//! Fixtures are built inline (`docs/LEGAL.md` §5).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::svg::{SvgOptions, export_svg};
use pdfcer_render::{PageBackdrop, RenderOptions, render_page_with};
use tiny_skia::Pixmap;

fn build(objects: &[(u32, String)]) -> Vec<u8> {
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
            "trailer\n<< /Size {} /Root 1 0 R /ID [<0102> <0304>] >>\nstartxref\n{xref_at}\n%%EOF\n",
            max_num + 1
        )
        .as_bytes(),
    );
    buf
}

/// A 100 × 80 page with `resources` and `content`, optionally rotated.
fn page(resources: &str, content: &str, rotate: u16) -> Vec<u8> {
    let stream = format!("{content}\n");
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            format!(
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 80] /Resources << {resources} >> >>"
            ),
        ),
        (
            3,
            format!("<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Rotate {rotate} >>"),
        ),
        (
            4,
            format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
        ),
    ])
}

/// A stream object with a computed `/Length` — hand-counted lengths are
/// how the first version of this file shipped a `StreamExtentMismatch`.
fn stream(dict: &str, content: &str) -> String {
    format!(
        "<< {dict} /Length {} >>\nstream\n{content}\nendstream",
        content.len() + 1
    )
}

const DPI: f32 = 144.0;

/// pdfcer's own transparent raster at the recording scale, and the SVG.
fn export(bytes: Vec<u8>) -> (Pixmap, pdfcer_render::svg::SvgExport) {
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let p = page_tree::pages(&doc).expect("page tree").remove(0);
    let render = RenderOptions::default().with_backdrop(PageBackdrop::Transparent);
    let reference = render_page_with(&doc, &p, DPI / 72.0, &render).expect("render");
    let svg = export_svg(
        &doc,
        &p,
        &RenderOptions::default(),
        &SvgOptions::default().with_raster_dpi(DPI),
    )
    .expect("svg export");
    (reference.pixmap, svg)
}

/// Rasterise an SVG with resvg onto a transparent pixmap of `w × h`.
fn rasterise(svg: &str, w: u32, h: u32) -> Pixmap {
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).expect("usvg parses");
    let mut pixmap = Pixmap::new(w, h).unwrap();
    // The root is sized in points; the viewBox is the device grid. Scale
    // the tree's own size onto the requested raster.
    let size = tree.size();
    let ts = tiny_skia::Transform::from_scale(w as f32 / size.width(), h as f32 / size.height());
    resvg::render(&tree, ts, &mut pixmap.as_mut());
    pixmap
}

/// Pixels whose premultiplied RGBA differs by more than `tol` in any
/// channel — the count, and the worst single difference.
fn differing(a: &Pixmap, b: &Pixmap, tol: u8) -> (usize, u8) {
    assert_eq!((a.width(), a.height()), (b.width(), b.height()));
    let mut count = 0;
    let mut worst = 0u8;
    for (pa, pb) in a.pixels().iter().zip(b.pixels()) {
        let d = [
            pa.red().abs_diff(pb.red()),
            pa.green().abs_diff(pb.green()),
            pa.blue().abs_diff(pb.blue()),
            pa.alpha().abs_diff(pb.alpha()),
        ];
        let m = d.into_iter().max().unwrap();
        worst = worst.max(m);
        if m > tol {
            count += 1;
        }
    }
    (count, worst)
}

fn assert_matches(name: &str, reference: &Pixmap, svg: &str, max_bad_fraction: f64) {
    let raster = rasterise(svg, reference.width(), reference.height());
    let total = reference.pixels().len();
    let (bad, worst) = differing(reference, &raster, 24);
    let fraction = bad as f64 / total as f64;
    assert!(
        fraction <= max_bad_fraction,
        "{name}: {bad} of {total} pixels ({:.2}%) differ by more than 24 levels (worst {worst}); \
         the SVG does not render as the page does",
        fraction * 100.0
    );
}

// ---------------------------------------------------------------------------
// Vector fixtures — pixel parity through resvg
// ---------------------------------------------------------------------------

#[test]
fn solid_fills_with_alpha_and_even_odd_render_identically() {
    let bytes = page(
        "/ExtGState << /GS0 << /ca 0.5 >> >>",
        "1 0 0 rg 10 10 40 30 re f \
         /GS0 gs 0 0 1 rg 30 20 40 30 re f \
         0 1 0 rg 60 50 30 20 re 70 55 10 10 re f*",
        0,
    );
    let (reference, svg) = export(bytes);
    assert!(svg.outcome.tally.is_exact());
    assert_eq!(svg.outcome.ops, 3);
    assert!(svg.svg.contains(r#"fill-opacity="0.502""#));
    assert!(svg.svg.contains(r#"fill-rule="evenodd""#));
    assert_matches("fills", &reference, &svg.svg, 0.01);
}

#[test]
fn strokes_keep_width_cap_join_and_a_dash_is_pre_applied() {
    let bytes = page(
        "",
        "4 w 1 J 1 j 0 0 0 RG 10 10 m 90 70 l S \
         [6 3] 0 d 2 w 0 J 0 0 1 RG 10 70 m 90 10 l S",
        0,
    );
    let (reference, svg) = export(bytes);
    assert_eq!(svg.outcome.dashed_strokes_pre_applied, 1);
    assert!(svg.svg.contains(r#"stroke-linecap="round""#));
    assert!(svg.svg.contains(r#"stroke-linejoin="round""#));
    assert!(
        !svg.svg.contains("stroke-dasharray"),
        "the dash is geometry, not an attribute"
    );
    assert_matches("strokes", &reference, &svg.svg, 0.02);
}

#[test]
fn nested_clips_carry_the_whole_chain() {
    // Two nested clips; the fill must show only their intersection.
    let bytes = page(
        "",
        "q 10 10 60 60 re W n q 40 40 50 30 re W n 1 0 0 rg 0 0 100 80 re f Q Q",
        0,
    );
    let (reference, svg) = export(bytes);
    assert!(
        svg.svg
            .contains(r#"<clipPath id="c1" clip-path="url(#c0)">"#)
    );
    assert_matches("clips", &reference, &svg.svg, 0.01);
    // And the intersection is where the red is: (50,50) inside both, (20,20)
    // only in the outer.
    let raster = rasterise(&svg.svg, reference.width(), reference.height());
    let at = |x: u32, y: u32| raster.pixels()[(y * raster.width() + x) as usize];
    let s = DPI / 72.0;
    let inside = at((50.0 * s) as u32, (80.0 - 50.0) as u32 * 2);
    let outer_only = at((20.0 * s) as u32, ((80.0 - 20.0) * s) as u32);
    assert!(inside.alpha() > 200, "inside both clips: {inside:?}");
    assert_eq!(
        outer_only.alpha(),
        0,
        "inside the outer clip only: {outer_only:?}"
    );
}

#[test]
fn a_transparency_group_becomes_a_group_with_opacity_and_blend() {
    let bytes = build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 80] \
             /Resources << /ExtGState << /GS0 << /ca 0.5 /BM /Multiply >> >> /XObject << /Fx 5 0 R >> >> >>"
                .into(),
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".into()),
        (4, stream("", "0 1 0 rg 0 0 100 80 re f /GS0 gs /Fx Do")),
        (
            5,
            stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 100 80] /Group << /S /Transparency /I true >>",
                "1 0 0 rg 10 10 50 50 re f 30 30 50 40 re f",
            ),
        ),
    ]);
    let (reference, svg) = export(bytes);
    assert!(
        svg.svg
            .contains(r#"<g opacity="0.5" style="mix-blend-mode:multiply">"#),
        "{}",
        svg.svg
    );
    assert_eq!(svg.outcome.blend_modes_used, 1);
    assert_matches("group", &reference, &svg.svg, 0.01);
}

#[test]
fn a_rotated_page_exports_rotated_with_swapped_extents() {
    let bytes = page("", "1 0 0 rg 0 0 50 80 re f", 90);
    let (reference, svg) = export(bytes);
    // 100 × 80 pt rotated 90° is 80 × 100 pt.
    assert_eq!(reference.width(), 160);
    assert_eq!(reference.height(), 200);
    assert!(
        svg.svg
            .contains(r#"width="80pt" height="100pt" viewBox="0 0 160 200""#),
        "{}",
        &svg.svg[..200]
    );
    assert_matches("rotate", &reference, &svg.svg, 0.01);
}

#[test]
fn an_empty_page_is_an_empty_svg_that_still_states_its_size() {
    let (reference, svg) = export(page("", "", 0));
    assert_eq!(svg.outcome.ops, 0);
    assert!(
        svg.svg
            .contains(r#"width="100pt" height="80pt" viewBox="0 0 200 160""#)
    );
    assert!(reference.pixels().iter().all(|p| p.alpha() == 0));
    assert_matches("empty", &reference, &svg.svg, 0.0);
}

#[test]
fn a_background_colour_is_an_opaque_rect_under_everything() {
    let doc = Document::from_bytes(page("", "1 0 0 rg 10 10 40 30 re f", 0)).unwrap();
    let p = page_tree::pages(&doc).unwrap().remove(0);
    let svg = export_svg(
        &doc,
        &p,
        &RenderOptions::default(),
        &SvgOptions::default()
            .with_raster_dpi(DPI)
            .with_background(Some(pdfcer_render::export::Rgb { r: 0, g: 0, b: 255 })),
    )
    .unwrap();
    let first_element = svg.svg.lines().nth(1).unwrap_or("");
    assert!(
        first_element.starts_with("<rect "),
        "the background comes first: {first_element}"
    );
    assert!(first_element.contains(r##"fill="#0000ff""##));
}

// ---------------------------------------------------------------------------
// Rasterised fallbacks — structural, since resvg's decoders are off here
// ---------------------------------------------------------------------------

fn axial_shading_page() -> Vec<u8> {
    page(
        "/Shading << /Sh0 << /ShadingType 2 /ColorSpace /DeviceRGB /Coords [10 0 90 0] \
         /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> /Extend [true true] >> >>",
        "q 10 10 80 60 re W n /Sh0 sh Q",
        0,
    )
}

/// A two-circle radial (both radii non-zero): SVG 1.1 has no inner radius,
/// so this one stays raster.
fn two_circle_radial_page() -> Vec<u8> {
    page(
        "/Shading << /Sh0 << /ShadingType 3 /ColorSpace /DeviceRGB /Coords [50 40 10 50 40 35] \
         /Function << /FunctionType 2 /Domain [0 1] /C0 [1 1 0] /C1 [0 0 1] /N 1 >> /Extend [false false] >> >>",
        "q 10 10 80 60 re W n /Sh0 sh Q",
        0,
    )
}

#[test]
fn a_shading_with_no_native_form_is_embedded_as_a_raster_and_said_so() {
    let (_, svg) = export(two_circle_radial_page());
    assert_eq!(
        svg.outcome.tally.shadings_rasterised, 1,
        "{:?}",
        svg.outcome.tally
    );
    assert_eq!(svg.outcome.tally.shadings_as_gradients, 0);
    assert_eq!(svg.outcome.images_embedded, 1);
    assert!(!svg.outcome.tally.is_exact());
    assert!(svg.svg.contains(r#"<image x="0" y="0" width="#));
    assert!(svg.svg.contains("data:image/png;base64,"));
    // The harvested image is clipped by the clip that was in force.
    assert!(svg.svg.contains(r#"<g clip-path="url(#c0)"#));
}

// ---------------------------------------------------------------------------
// Native gradients (`Pass 248.3`) — pixel parity through resvg, which
// renders gradients without any feature flag
// ---------------------------------------------------------------------------

#[test]
fn an_axial_shading_is_a_linear_gradient_and_renders_identically() {
    let (reference, svg) = export(axial_shading_page());
    let t = &svg.outcome.tally;
    assert_eq!(t.shadings_as_gradients, 1, "{t:?}");
    assert_eq!(t.shadings_rasterised, 0);
    assert!(t.is_exact(), "a native gradient is exact: {t:?}");
    assert_eq!(svg.outcome.images_embedded, 0);
    assert!(svg.svg.contains(r#"<linearGradient id="g"#), "{}", svg.svg);
    assert!(svg.svg.contains(r#"gradientUnits="userSpaceOnUse""#));
    assert!(svg.svg.contains(r#"fill="url(#g"#));
    // A linear ramp thins to its two ends.
    assert_eq!(svg.svg.matches("<stop ").count(), 2, "{}", svg.svg);
    assert_matches("axial", &reference, &svg.svg, 0.01);
}

#[test]
fn a_focal_radial_shading_is_a_radial_gradient_and_renders_identically() {
    let bytes = page(
        "/Shading << /Sh0 << /ShadingType 3 /ColorSpace /DeviceRGB /Coords [45 35 0 50 40 35] \
         /Function << /FunctionType 2 /Domain [0 1] /C0 [1 1 1] /C1 [0 0.5 0] /N 1 >> /Extend [false true] >> >>",
        "q 10 10 80 60 re W n /Sh0 sh Q",
        0,
    );
    let (reference, svg) = export(bytes);
    assert_eq!(
        svg.outcome.tally.shadings_as_gradients, 1,
        "{:?}",
        svg.outcome.tally
    );
    assert!(svg.svg.contains(r#"<radialGradient id="g"#), "{}", svg.svg);
    assert!(svg.svg.contains(r#" fx="45" fy="35""#), "{}", svg.svg);
    assert_matches("radial", &reference, &svg.svg, 0.015);
}

#[test]
fn an_unextended_axial_shading_paints_nothing_beyond_its_ends() {
    // Axis from x=40 to x=60 inside a clip spanning x=10..90: with
    // /Extend [false false] the strips 10..40 and 60..90 stay unpainted.
    let bytes = page(
        "/Shading << /Sh0 << /ShadingType 2 /ColorSpace /DeviceRGB /Coords [40 0 60 0] \
         /Function << /FunctionType 2 /Domain [0 1] /C0 [1 0 0] /C1 [0 0 1] /N 1 >> /Extend [false false] >> >>",
        "q 10 10 80 60 re W n /Sh0 sh Q",
        0,
    );
    let (reference, svg) = export(bytes);
    assert_eq!(svg.outcome.tally.shadings_as_gradients, 1);
    assert!(
        svg.svg.contains(r#"<clipPath id="e"#),
        "the extent clip: {}",
        svg.svg
    );
    assert_matches("extend-false", &reference, &svg.svg, 0.01);
    let raster = rasterise(&svg.svg, reference.width(), reference.height());
    let s = DPI / 72.0;
    let at = |x: f32, y: f32| {
        raster.pixels()[((y * s) as u32 * raster.width() + (x * s) as u32) as usize]
    };
    assert_eq!(at(20.0, 40.0).alpha(), 0, "left of the band");
    assert_eq!(at(80.0, 40.0).alpha(), 0, "right of the band");
    assert!(at(50.0, 40.0).alpha() > 200, "inside the band");
}

#[test]
fn a_gradient_shading_pattern_fill_keeps_the_fill_path() {
    // The same axial shading used as a PATTERN fill of a triangle.
    let bytes = page(
        "/Pattern << /P0 << /PatternType 2 /Shading << /ShadingType 2 /ColorSpace /DeviceRGB \
         /Coords [10 0 90 0] /Function << /FunctionType 2 /Domain [0 1] /C0 [0 1 0] /C1 [1 0 1] /N 1 >> \
         /Extend [true true] >> >> >>",
        "/Pattern cs /P0 scn 10 10 m 90 10 l 50 70 l h f",
        0,
    );
    let (reference, svg) = export(bytes);
    assert_eq!(
        svg.outcome.tally.shadings_as_gradients, 1,
        "{:?}",
        svg.outcome.tally
    );
    assert!(svg.svg.contains(r#"<linearGradient id="g"#));
    assert_matches("pattern-gradient", &reference, &svg.svg, 0.01);
}

fn soft_masked_page() -> Vec<u8> {
    // A luminosity soft mask: white on the left half, black on the right,
    // over a full-page red fill. The red must survive only on the left.
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 80] \
             /Resources << /ExtGState << /GS0 << /SMask << /S /Luminosity /G 5 0 R >> >> >> >> >>"
                .into(),
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".into()),
        (4, stream("", "/GS0 gs 1 0 0 rg 0 0 100 80 re f")),
        (
            5,
            stream(
                "/Type /XObject /Subtype /Form /BBox [0 0 100 80] /Group << /S /Transparency /CS /DeviceGray >>",
                "1 g 0 0 50 80 re f",
            ),
        ),
    ])
}

#[test]
fn a_soft_mask_on_an_elementary_object_is_kept_as_a_mask() {
    let (reference, svg) = export(soft_masked_page());
    assert_eq!(
        svg.outcome.tally.soft_masks_kept, 1,
        "{:?}",
        svg.outcome.tally
    );
    assert!(svg.svg.contains(r#"<mask id="m"#), "{}", svg.svg);
    assert!(svg.svg.contains(r#"style="color-interpolation:sRGB""#));
    assert!(svg.svg.contains(r#" mask="url(#m"#));
    // The reference has red on the left and nothing on the right — the
    // thing the mask must reproduce.
    let w = reference.width();
    let at = |x: u32, y: u32| reference.pixels()[(y * w + x) as usize];
    assert!(at(w / 4, 40).alpha() > 200);
    assert_eq!(at(3 * w / 4, 40).alpha(), 0);
}

#[test]
fn the_cache_recorder_now_refuses_an_elementary_soft_mask_by_name() {
    // Until `Pass 248.1` a cached replay painted an object under a
    // `gs /SMask` UNMASKED, silently: the mask was folded into the clip
    // MASK, which a recording never carries, and nothing poisoned. The
    // export mode keeps it; the cache mode must now refuse it, or `R211`'s
    // "a cache that is subtly wrong is worse than none" is violated.
    let doc = Document::from_bytes(soft_masked_page()).unwrap();
    let p = page_tree::pages(&doc).unwrap().remove(0);
    let err = pdfcer_render::record_page(&doc.view(), &p, 1.0, 0, &RenderOptions::default())
        .expect_err("the cache recorder refuses");
    assert!(
        matches!(
            err,
            pdfcer_render::RenderError::PageNotRecordable {
                reason: pdfcer_render::PoisonReason::SoftMask
            }
        ),
        "{err:?}"
    );
}

// ---------------------------------------------------------------------------
// Inkscape, when present — the paste target itself, end to end
// ---------------------------------------------------------------------------

fn inkscape() -> Option<std::path::PathBuf> {
    let candidates = [
        std::path::PathBuf::from(r"C:\Program Files\Inkscape\bin\inkscape.exe"),
        std::path::PathBuf::from("/usr/bin/inkscape"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

#[test]
fn inkscape_renders_the_export_like_pdfcer_when_it_is_installed() {
    let Some(ink) = inkscape() else {
        println!("export_svg: Inkscape not installed here; the end-to-end oracle did not run");
        return;
    };
    let dir = std::env::temp_dir().join(format!("pdfcer-svg-oracle-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (name, bytes) in [
        ("shading", axial_shading_page()),
        ("softmask", soft_masked_page()),
        (
            "fills",
            page(
                "/ExtGState << /GS0 << /ca 0.5 >> >>",
                "1 0 0 rg 10 10 40 30 re f /GS0 gs 0 0 1 rg 30 20 40 30 re f",
                0,
            ),
        ),
    ] {
        let (reference, svg) = export(bytes);
        let svg_path = dir.join(format!("{name}.svg"));
        let png_path = dir.join(format!("{name}.png"));
        std::fs::write(&svg_path, &svg.svg).unwrap();
        let status = std::process::Command::new(&ink)
            .arg("--export-type=png")
            .arg(format!("--export-filename={}", png_path.display()))
            .arg("--export-background-opacity=0")
            .arg(format!("--export-width={}", reference.width()))
            .arg(format!("--export-height={}", reference.height()))
            .arg(&svg_path)
            .output()
            .expect("inkscape runs");
        assert!(
            status.status.success(),
            "inkscape: {}",
            String::from_utf8_lossy(&status.stderr)
        );
        let ink_png = Pixmap::load_png(&png_path).expect("inkscape wrote a png");
        let total = reference.pixels().len();
        let (bad, worst) = differing(&reference, &ink_png, 24);
        let fraction = bad as f64 / total as f64;
        assert!(
            fraction <= 0.02,
            "{name}: Inkscape differs from pdfcer on {bad} of {total} pixels ({:.2}%, worst {worst})",
            fraction * 100.0
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
