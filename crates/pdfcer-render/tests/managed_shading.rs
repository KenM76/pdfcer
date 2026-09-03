//! # A shading and a mesh convert their colour the way a fill does
//!
//! # What was wrong
//!
//! Three Passes gave fills and images a route through the page's colour
//! bridges — an `ICCBased` colour through its embedded profile, a `Lab`
//! colour through the output intent — and each left the shading and mesh
//! readers behind: their colour is resolved inside `shading.rs`/`mesh.rs`
//! through a bare `ColorSpace` that never saw the bridge cache. On one page,
//! through one profile, a gradient's stop and a flat fill of the same
//! operands were two colours. `Pass 243.0` routes `ColorRamp::build` and
//! `mesh::read_shade` through `icc::ColorBridges` — the fill path's ladder in
//! one place — so the last unmanaged colour route is closed.
//!
//! # The oracle
//!
//! `fixtures/synthetic/managed-shading/` (`tools/gen-managed-shading-fixtures.py`):
//! one colour painted three ways — fill, a constant axial shading, a type 4
//! mesh — in two families (`ICCBased` through the swap-RG profile, `Lab`) on
//! three page kinds (additive; subtractive with a synthetic CMYK output
//! intent; subtractive without). Per page: all three agree; and the value is
//! the MANAGED one where a route exists (`(48,102,205)` on the additive ICC
//! page, chromatic ink under the Lab intent page), which the unmanaged
//! answer is far from (R225).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{InkProbeSource, RenderOptions, RenderedPage};

const SCALE: f32 = 2.0;
const PAGE_H: f64 = 100.0;
/// Swatch centres in points: fill, axial shading, type 4 mesh.
const CENTRES: [f64; 3] = [50.0, 150.0, 250.0];
const LABELS: [&str; 3] = ["fill", "axial shading", "type 4 mesh"];

const EXPECTED_ICC: (f64, f64, f64) = (48.0, 102.0, 205.0);
const UNMANAGED_ICC: (f64, f64, f64) = (102.0, 51.0, 204.0);

fn render(name: &str, options: &RenderOptions) -> RenderedPage {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/managed-shading")
        .join(name);
    let doc = Document::load(Path::new(&path)).expect("fixture loads");
    let pages = page_tree::pages(&doc).expect("page tree");
    pdfcer_render::render_page_with_view(&doc.view(), &pages[0], SCALE, options).expect("renders")
}

/// Mean RGB over the middle of one swatch.
fn swatch(page: &RenderedPage, cx: f64) -> (f64, f64, f64) {
    let s = f64::from(SCALE);
    let (x0, x1) = (((cx - 20.0) * s) as u32, ((cx + 20.0) * s) as u32);
    let (y0, y1) = ((PAGE_H * 0.35 * s) as u32, (PAGE_H * 0.65 * s) as u32);
    let stride = page.pixmap.width();
    let px = page.pixmap.pixels();
    let (mut r, mut g, mut b, mut n) = (0.0, 0.0, 0.0, 0.0);
    for y in y0..y1 {
        for x in x0..x1 {
            let p = px[(y * stride + x) as usize];
            r += f64::from(p.red());
            g += f64::from(p.green());
            b += f64::from(p.blue());
            n += 1.0;
        }
    }
    (r / n, g / n, b / n)
}

fn three(page: &RenderedPage) -> [(f64, f64, f64); 3] {
    [
        swatch(page, CENTRES[0]),
        swatch(page, CENTRES[1]),
        swatch(page, CENTRES[2]),
    ]
}

fn dist(a: (f64, f64, f64), b: (f64, f64, f64)) -> f64 {
    (a.0 - b.0)
        .abs()
        .max((a.1 - b.1).abs())
        .max((a.2 - b.2).abs())
}

fn assert_agree_px(name: &str, ways: &[(f64, f64, f64); 3]) {
    for (i, w) in ways.iter().enumerate().skip(1) {
        assert!(
            dist(ways[0], *w) <= 1.5,
            "{name}: the {} landed on {w:?} where the fill landed on {:?}",
            LABELS[i],
            ways[0]
        );
    }
}

/// The probed ink under each swatch of a subtractive page.
fn inks(name: &str) -> [[f32; 4]; 3] {
    let y = (PAGE_H * 0.5 * f64::from(SCALE)) as u32;
    let mut out = [[0.0; 4]; 3];
    for (i, cx) in CENTRES.iter().enumerate() {
        let x = (cx * f64::from(SCALE)) as u32;
        let page = render(name, &RenderOptions::default().with_ink_probe(x, y));
        let probe = page.diagnostics.ink_probe.expect("a probe was requested");
        assert_eq!(probe.source, InkProbeSource::CmykBuffer, "{name} at x={x}");
        out[i] = probe.cmyk.expect("a colorant buffer reports colorants");
    }
    out
}

fn assert_agree_ink(name: &str, inks: &[[f32; 4]; 3]) {
    for (i, ink) in inks.iter().enumerate().skip(1) {
        for c in 0..4 {
            assert!(
                (ink[c] - inks[0][c]).abs() < 0.01,
                "{name}: the {} deposited {ink:?} where the fill deposited {:?}",
                LABELS[i],
                inks[0]
            );
        }
    }
}

/// The display bridge inside the ramp and the vertex reader.
#[test]
fn an_icc_rgb_shading_and_mesh_take_the_display_bridge() {
    let page = render("icc-rgb-shading-additive.pdf", &RenderOptions::default());
    assert!(!page.diagnostics.cmyk_buffer_engaged);
    let ways = three(&page);
    assert_agree_px("icc additive", &ways);
    for (i, w) in ways.iter().enumerate() {
        assert!(
            dist(*w, EXPECTED_ICC) <= 2.0,
            "{}: {w:?}, expected the profile's {EXPECTED_ICC:?} (unmanaged is {UNMANAGED_ICC:?})",
            LABELS[i]
        );
    }
    assert_eq!(
        page.diagnostics.shading.ramps_managed, 2,
        "both the axial shading and the mesh had a bridge"
    );
}

/// The ink bridge inside both, on a page with an output intent.
#[test]
fn an_icc_rgb_shading_and_mesh_take_the_ink_bridge() {
    let with = inks("icc-rgb-shading-subtractive.pdf");
    assert_agree_ink("icc with intent", &with);
    let without = inks("icc-rgb-shading-subtractive-no-intent.pdf");
    assert_agree_ink("icc without intent", &without);
    assert!(
        (0..4).any(|c| (with[0][c] - without[0][c]).abs() > 0.05),
        "the output intent did not change the ink ({:?}) — the ink bridge never ran",
        with[0]
    );
    let page = render("icc-rgb-shading-subtractive.pdf", &RenderOptions::default());
    assert_agree_px("icc with intent", &three(&page));
}

/// The PCS bridge inside both: a Lab shading separates through the intent
/// into chromatic ink, exactly as the Lab fill beside it does.
#[test]
fn a_lab_shading_and_mesh_take_the_pcs_bridge() {
    let with = inks("lab-shading-subtractive.pdf");
    assert_agree_ink("lab with intent", &with);
    let [c, m, y, _] = with[0];
    assert!(
        c > 0.05 && m > 0.05 && y > 0.05,
        "a press separates a neutral into C, M and Y; {:?} is the K-only shape of rgb_to_cmyk",
        with[0]
    );
    let without = inks("lab-shading-subtractive-no-intent.pdf");
    assert_agree_ink("lab without intent", &without);
    assert!(
        without[0][0] < 0.01 && without[0][1] < 0.01 && without[0][2] < 0.01,
        "without an intent a neutral bridges to K only; got {:?}",
        without[0]
    );
    let page = render("lab-shading-subtractive.pdf", &RenderOptions::default());
    assert_eq!(page.diagnostics.shading.ramps_managed, 2);
}

/// The control that pins nothing moved where no route exists: a Lab
/// shading on an additive page is `xyz_to_srgb` on all three, unmanaged,
/// and the counter says so.
#[test]
fn a_lab_shading_on_an_additive_page_is_unchanged_and_uncounted() {
    let page = render("lab-shading-additive.pdf", &RenderOptions::default());
    let ways = three(&page);
    assert_agree_px("lab additive", &ways);
    // L* 60 is a neutral: the three channels are within a level of each other.
    let (r, g, b) = ways[0];
    assert!(
        (r - g).abs() < 1.5 && (g - b).abs() < 1.5,
        "not neutral: {:?}",
        ways[0]
    );
    assert_eq!(page.diagnostics.shading.ramps_managed, 0);
}
