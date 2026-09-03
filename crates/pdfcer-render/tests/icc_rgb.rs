//! # An `ICCBased /N 3` colour is managed through its own profile, on every route
//!
//! # What was wrong
//!
//! Until `Pass 240.0` a three-component `ICCBased` space was parsed for its
//! `/N` and its profile thrown away — on the screen path by every object
//! type, and on the ink path by images. A print-conformance patch that draws
//! one colour four ways through one deliberately-not-sRGB profile rendered a
//! saturated red trap where its vector twin was green, and the two 16-bit and
//! V4 RGB image patches beside it were each ~30 levels from the reference.
//!
//! # The oracle
//!
//! `fixtures/synthetic/icc-rgb/` (see its `PROVENANCE.md` and
//! `tools/gen-icc-rgb-fixtures.py`) draws `(0.4, 0.2, 0.8)` four ways through
//! a profile whose red and green colorants are **swapped**:
//!
//! 1. a flat fill (`/Cs0 cs … scn`),
//! 2. a direct 8-bit image in the same space,
//! 3. an `/Indexed` image whose one palette entry is the same colour,
//! 4. a JPEG 2000 image with **no `/ColorSpace`**, whose codestream carries
//!    the profile in its `colr` box (§7.4.9's "the colour space
//!    specifications in the JPEG2000 data shall be used").
//!
//! Two assertions, and each catches a different failure:
//!
//! * **Agreement.** All four land on one colour. This is the assertion that
//!   has found every defect in this area — a route fixed on its own looks
//!   correct until something beside it disagrees.
//! * **The closed form.** On the additive page that colour is
//!   `(48, 102, 205)`: the sRGB encoding of the swapped gamma-2.2 linear
//!   values, derived in the generator and confirmed by lcms2 on the same
//!   bytes. The unmanaged answer is `(102, 51, 204)`. Agreement alone would
//!   pass a renderer that manages nothing (rule R225); this does not.
//!
//! # The three pages
//!
//! | page | what the fourth column proves |
//! |---|---|
//! | additive | the display bridge (`Space::IccRgb::display`, `Interpreter::display_managed_rgb`) |
//! | subtractive, no output intent | the display bridge feeding `rgb_to_cmyk` uniformly — no ink bridge exists, so all four bridge from the SAME managed sRGB |
//! | subtractive, with output intent | the ink bridge (`Space::IccRgb::ink`, `Interpreter::authored_cmyk`) — and the ink probe shows the output intent CHANGED the ink, which `rgb_to_cmyk` cannot do |

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
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::page_tree;
use pdfcer_render::{InkProbeSource, RenderOptions, RenderedPage};

const SCALE: f32 = 2.0;

/// The generator's closed form, confirmed by lcms2 — see the module docs.
const EXPECTED: (f64, f64, f64) = (48.0, 102.0, 205.0);
/// Table 66's reinterpretation of the same operands as `DeviceRGB`.
const UNMANAGED: (f64, f64, f64) = (102.0, 51.0, 204.0);

/// The page is 300 pt wide; the four boxes sit at x 10–70, 80–140, 150–210,
/// 220–280. Each is sampled over its middle third so no edge pixel enters.
const BOXES: [(f64, f64); 4] = [
    (30.0 / 300.0, 50.0 / 300.0),
    (100.0 / 300.0, 120.0 / 300.0),
    (170.0 / 300.0, 190.0 / 300.0),
    (240.0 / 300.0, 260.0 / 300.0),
];
const LABELS: [&str; 4] = ["fill", "direct image", "/Indexed image", "JPX codestream"];

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/icc-rgb")
        .join(name)
}

fn render(name: &str, options: &RenderOptions) -> RenderedPage {
    let doc = Document::load(Path::new(&fixture(name))).expect("fixture loads");
    let pages = page_tree::pages(&doc).expect("page tree");
    pdfcer_render::render_page_with_view(&doc.view(), &pages[0], SCALE, options).expect("renders")
}

/// Mean RGB over a box given in fractions of the page width, across the
/// middle half of the page height.
fn patch(page: &RenderedPage, x0: f64, x1: f64) -> (f64, f64, f64) {
    let w = f64::from(page.pixmap.width());
    let h = f64::from(page.pixmap.height());
    let (px0, px1) = ((w * x0) as u32, (w * x1) as u32);
    let (py0, py1) = ((h * 0.35) as u32, (h * 0.65) as u32);
    let stride = page.pixmap.width();
    let px = page.pixmap.pixels();
    let (mut r, mut g, mut b, mut n) = (0.0, 0.0, 0.0, 0.0);
    for y in py0..py1 {
        for x in px0..px1 {
            let p = px[(y * stride + x) as usize];
            r += f64::from(p.red());
            g += f64::from(p.green());
            b += f64::from(p.blue());
            n += 1.0;
        }
    }
    (r / n, g / n, b / n)
}

fn four_ways(page: &RenderedPage) -> [(f64, f64, f64); 4] {
    let mut out = [(0.0, 0.0, 0.0); 4];
    for (i, (x0, x1)) in BOXES.iter().enumerate() {
        out[i] = patch(page, *x0, *x1);
    }
    out
}

fn dist(a: (f64, f64, f64), b: (f64, f64, f64)) -> f64 {
    (a.0 - b.0)
        .abs()
        .max((a.1 - b.1).abs())
        .max((a.2 - b.2).abs())
}

/// Every route must agree with the fill to within `tol` levels.
///
/// The fill and an image of the same colour quantise on different paths (a
/// `BrushSpec` colour versus a premultiplied texel), and on the additive
/// fixture they land one level apart in red. One level is quantisation; a
/// route that took a different transform is tens of levels away.
fn assert_agree(label: &str, ways: &[(f64, f64, f64); 4], tol: f64) {
    for (i, way) in ways.iter().enumerate().skip(1) {
        assert!(
            dist(ways[0], *way) <= tol,
            "{label}: the {} landed on {way:?} where the fill landed on {:?} — \
             two routes through one profile disagree",
            LABELS[i],
            ways[0]
        );
    }
}

/// The display route: profile → sRGB, for a fill, two sampled images and a
/// codestream-tagged one, on a page with no output intent and no group.
#[test]
fn on_an_additive_page_all_four_routes_land_on_the_profiles_answer() {
    let page = render("icc-rgb-four-ways-additive.pdf", &RenderOptions::default());
    assert!(
        !page.diagnostics.cmyk_buffer_engaged,
        "the additive fixture has no page group; if a buffer engaged the test is measuring the wrong route"
    );
    let ways = four_ways(&page);
    assert_agree("additive", &ways, 1.5);
    for (i, way) in ways.iter().enumerate() {
        assert!(
            dist(*way, EXPECTED) <= 2.0,
            "{}: landed on {way:?}, expected the profile's answer {EXPECTED:?} \
             (Table 66's reinterpretation would give {UNMANAGED:?})",
            LABELS[i]
        );
    }
    assert_eq!(
        page.diagnostics.icc_managed_paints, 4,
        "four objects went through the profile; the counter must say so \
         (the JPX one has no /ColorSpace and is counted from the decoder's own flag)"
    );
    assert_eq!(page.diagnostics.icc_unmanaged_paints, 0);
}

/// The unmanaged answer is not merely different — it is the value a renderer
/// that dropped the profile WOULD produce, so this pins that the assertion
/// above is discriminating and not tautological.
#[test]
fn the_expected_and_unmanaged_answers_are_far_apart() {
    assert!(
        dist(EXPECTED, UNMANAGED) > 40.0,
        "a fixture whose two candidate answers coincide cannot distinguish them (R225)"
    );
}

/// The display route feeding a colorant buffer. No output intent, so no ink
/// bridge can exist and every object must reach the buffer through
/// `rgb_to_cmyk` from the SAME managed sRGB — including the fill, whose
/// `authored_cmyk` answers `None` for an RGB space.
#[test]
fn on_a_subtractive_page_without_an_intent_all_four_routes_still_agree() {
    let page = render(
        "icc-rgb-four-ways-subtractive-no-intent.pdf",
        &RenderOptions::default(),
    );
    assert!(
        page.diagnostics.cmyk_buffer_engaged,
        "the fixture declares a /DeviceCMYK group"
    );
    let ways = four_ways(&page);
    assert_agree("subtractive, no intent", &ways, 1.5);
    // Still the profile's colour, seen through the buffer's round trip: green
    // must dominate red by a wide margin, which the unmanaged red-dominant
    // reading cannot produce whatever the round trip does to it.
    for (i, way) in ways.iter().enumerate() {
        assert!(
            way.1 > way.0 + 30.0,
            "{}: {way:?} is not green-dominant — the profile was not applied on this page",
            LABELS[i]
        );
    }
    assert_eq!(page.diagnostics.icc_managed_paints, 4);
}

/// The ink route. With an output intent every object goes profile → CMYK
/// and deposits that ink. Two things are asserted at the probe:
///
/// * all four objects deposit the SAME ink (agreement in the buffer, before
///   the exit conversion can hide anything), and
/// * that ink is DIFFERENT from what the intent-less twin deposits. The two
///   fixtures differ only in the `/OutputIntents` entry, so if the ink bridge
///   never ran the buffers would be identical — `rgb_to_cmyk` does not read
///   an output intent.
#[test]
fn on_a_subtractive_page_with_an_intent_all_four_routes_deposit_the_bridges_ink() {
    let probe_y = (100.0 * SCALE * 0.5) as u32;
    let centres: Vec<u32> = BOXES
        .iter()
        .map(|(x0, x1)| ((x0 + x1) * 0.5 * 300.0 * f64::from(SCALE)) as u32)
        .collect();

    let mut with_intent = Vec::new();
    let mut without = Vec::new();
    for &x in &centres {
        let options = RenderOptions::default().with_ink_probe(x, probe_y);
        for (name, out) in [
            ("icc-rgb-four-ways-subtractive.pdf", &mut with_intent),
            ("icc-rgb-four-ways-subtractive-no-intent.pdf", &mut without),
        ] {
            let page = render(name, &options);
            let probe = page.diagnostics.ink_probe.expect("a probe was requested");
            assert_eq!(probe.source, InkProbeSource::CmykBuffer, "{name} at x={x}");
            out.push(probe.cmyk.expect("a colorant buffer reports colorants"));
        }
    }

    for (i, ink) in with_intent.iter().enumerate().skip(1) {
        for c in 0..4 {
            assert!(
                (ink[c] - with_intent[0][c]).abs() < 0.01,
                "{}: ink {ink:?} where the fill deposited {:?} — the image and \
                 the fill took different transforms to the output intent",
                LABELS[i],
                with_intent[0]
            );
        }
    }
    let moved = (0..4).any(|c| (with_intent[0][c] - without[0][c]).abs() > 0.05);
    assert!(
        moved,
        "the output intent did not change the fill's ink ({:?} both ways) — \
         the ink bridge never ran and the colour was reconstructed by rgb_to_cmyk",
        with_intent[0]
    );

    let page = render(
        "icc-rgb-four-ways-subtractive.pdf",
        &RenderOptions::default(),
    );
    assert_agree("subtractive, with intent", &four_ways(&page), 1.5);
    assert_eq!(page.diagnostics.icc_managed_paints, 4);
    assert_eq!(page.diagnostics.icc_unmanaged_paints, 0);
}

/// A caller that declines colour management (`IccContext::unmanaged`, which
/// is what the public image decoder offers) still gets Table 66's fallback:
/// the profile is not applied, and nothing fails.
#[test]
fn a_decoder_call_without_a_context_still_falls_back_to_the_device_space() {
    let doc = Document::load(Path::new(&fixture("icc-rgb-four-ways-additive.pdf")))
        .expect("fixture loads");
    let view = doc.view();
    let pages = page_tree::pages(&doc).expect("page tree");
    let page = &pages[0];
    let resources = &page.resources;
    let xobjects = resources
        .get(b"XObject")
        .map(|o| view.resolve(o))
        .and_then(pdfcer_core::object::Object::as_dict)
        .expect("XObject dict");
    let im0 = xobjects.get(b"Im0").map(|o| view.resolve(o)).expect("Im0");
    let pdfcer_core::object::Object::Stream(stream) = im0 else {
        panic!("Im0 is a stream")
    };
    let raw = view.slice(stream.data_span).expect("stream bytes");
    let decoded = pdfcer_render::image::decode(
        &view,
        &stream.dict,
        raw,
        resources,
        pdfcer_render::gstate::Rgb::BLACK,
        pdfcer_render::image::ImageOrigin::XObject,
        pdfcer_render::RenderPolicy::default(),
        pdfcer_render::image::IccContext::unmanaged(),
    )
    .expect("decodes");
    let p = decoded.pixmap.pixels()[0];
    assert!(
        !decoded.icc_managed,
        "an unmanaged context must not build a bridge"
    );
    assert_eq!(
        (
            f64::from(p.red()),
            f64::from(p.green()),
            f64::from(p.blue())
        ),
        UNMANAGED,
        "with no context the texel is Table 66's reinterpretation, exactly"
    );
}
