//! # A `Lab` colour separates through the output intent, on every route
//!
//! # What was wrong
//!
//! On a page that composites in ink, a `Lab`/`CalRGB`/`CalGray` colour — a
//! space with colorimetry but no colorants and no embedded profile — reached
//! the colorant buffer through `rgb_to_cmyk`: a max-GCR formula that knows
//! nothing about the press. A `Lab (60, 0, 0)` grey became `K = 0.43` alone
//! where the document's own output intent separates it into all four inks,
//! and on the print-conformance patch built around exactly that a
//! `ColorBurn` over the K-only grey burned to black and its trap X — authored
//! to vanish under the press separation — stood out. `Pass 242.0` gives such
//! a colour the PCS route: D50-relative XYZ straight into the output intent's
//! B2A table (`IccBridgeCache::pcs_to_ink`, `image::Space::Special { pcs }`).
//!
//! Writing the fixture found a second, older defect the same hour: an
//! `/Indexed` palette over a `Lab` base was decoded by dividing each byte by
//! 255, so `L* = 60` arrived as `L* = 0.6` and the palette image painted
//! near-black on BOTH pages, beside a fill and a direct image that agreed
//! with each other. `image::palette_entry` now scales into the base's
//! component range as §8.6.6.3 says and as the fill path's `indexed_to_rgb`
//! already did.
//!
//! # The oracle
//!
//! `fixtures/synthetic/lab-ink/` (`tools/gen-lab-ink-fixtures.py`): one
//! `Lab (60, 0, 0)` drawn three ways — fill, direct image, `/Indexed` image —
//! on two pages that differ ONLY in whether an `/OutputIntents` entry
//! exists. Three assertions:
//!
//! * the three routes deposit one ink on each page (agreement);
//! * with an intent the ink is CHROMATIC — C, M and Y non-zero — because a
//!   press profile separates a neutral into all four inks, where
//!   `rgb_to_cmyk` puts a neutral into K alone (the two candidate answers
//!   are far apart, rule R225);
//! * the intent CHANGED the ink relative to the intent-less twin — which
//!   `rgb_to_cmyk` cannot do, since it never reads an output intent.

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
/// Box centres in points: fill, direct image, `/Indexed` image.
const CENTRES: [f64; 3] = [40.0, 110.0, 180.0];
const LABELS: [&str; 3] = ["fill", "direct image", "/Indexed image"];

fn render(name: &str, options: &RenderOptions) -> RenderedPage {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/lab-ink")
        .join(name);
    let doc = Document::load(Path::new(&path)).expect("fixture loads");
    let pages = page_tree::pages(&doc).expect("page tree");
    pdfcer_render::render_page_with_view(&doc.view(), &pages[0], SCALE, options).expect("renders")
}

/// The probed ink under each of the three boxes.
fn inks(name: &str) -> [[f32; 4]; 3] {
    let y = (PAGE_H * 0.5 * f64::from(SCALE)) as u32;
    let mut out = [[0.0; 4]; 3];
    for (i, cx) in CENTRES.iter().enumerate() {
        let x = (cx * f64::from(SCALE)) as u32;
        let page = render(name, &RenderOptions::default().with_ink_probe(x, y));
        assert!(
            page.diagnostics.cmyk_buffer_engaged,
            "{name}: /DeviceCMYK group engages the buffer"
        );
        let probe = page.diagnostics.ink_probe.expect("a probe was requested");
        assert_eq!(probe.source, InkProbeSource::CmykBuffer, "{name} at x={x}");
        out[i] = probe.cmyk.expect("a colorant buffer reports colorants");
    }
    out
}

fn assert_agree(name: &str, inks: &[[f32; 4]; 3]) {
    for (i, ink) in inks.iter().enumerate().skip(1) {
        for c in 0..4 {
            assert!(
                (ink[c] - inks[0][c]).abs() < 0.01,
                "{name}: the {} deposited {ink:?} where the fill deposited {:?} — two routes \
                 for one Lab colour disagree",
                LABELS[i],
                inks[0]
            );
        }
    }
}

/// The PCS route: every object separates through the output intent, and
/// the separation is a press one — chromatic ink under a neutral.
#[test]
fn with_an_output_intent_all_three_routes_separate_through_it() {
    let with = inks("lab-three-ways-subtractive.pdf");
    assert_agree("with intent", &with);
    let [c, m, y, k] = with[0];
    assert!(
        c > 0.05 && m > 0.05 && y > 0.05,
        "a press profile separates a neutral into C, M and Y as well as K; got {:?} — the \
         K-only shape of rgb_to_cmyk, so the PCS route did not run",
        with[0]
    );
    assert!(
        k > 0.0 && k < 1.0,
        "a mid grey is not solid or absent K: {k}"
    );

    let without = inks("lab-three-ways-subtractive-no-intent.pdf");
    let moved = (0..4).any(|i| (with[0][i] - without[0][i]).abs() > 0.05);
    assert!(
        moved,
        "the output intent did not change the ink ({:?} both ways) — the two fixtures differ \
         only in /OutputIntents, so the PCS route never ran",
        with[0]
    );
}

/// The control. No output intent, so no PCS route can exist; all three
/// objects bridge through `rgb_to_cmyk` from one sRGB and must still agree —
/// which is where the `/Indexed`-over-`Lab` palette defect showed, on the
/// page that had nothing to do with the new route.
#[test]
fn without_an_output_intent_all_three_routes_still_agree() {
    let without = inks("lab-three-ways-subtractive-no-intent.pdf");
    assert_agree("without intent", &without);
    // rgb_to_cmyk puts a neutral into K alone: this pins that the control
    // really is the bridge and not a silently substituted route.
    let [c, m, y, _] = without[0];
    assert!(
        c < 0.01 && m < 0.01 && y < 0.01,
        "without an intent a neutral bridges to K only; got {:?}",
        without[0]
    );
}

/// The counter: a Lab paint separated through the intent counts as managed;
/// on the intent-less page nothing could be, and nothing is claimed.
#[test]
fn the_managed_counter_sees_the_pcs_route() {
    let with = render("lab-three-ways-subtractive.pdf", &RenderOptions::default());
    assert!(
        with.diagnostics.icc_managed_paints >= 1,
        "the Lab fill went through the output intent and must be counted"
    );
    let without = render(
        "lab-three-ways-subtractive-no-intent.pdf",
        &RenderOptions::default(),
    );
    assert_eq!(without.diagnostics.icc_managed_paints, 0);
    assert_eq!(
        without.diagnostics.icc_unmanaged_paints, 0,
        "with no destination nothing could have been managed, and the page's blend-space \
         disclosure already says so; counting it here would double-report"
    );
}
