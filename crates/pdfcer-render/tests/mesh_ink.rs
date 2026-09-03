//! # A mesh and a fill of the same ink must be the same colour
//!
//! The mesh half of what `crates/pdfcer-render/tests/shading_ink.rs` asserts
//! for analytic shadings. Read that file first — the defect, the round trip
//! and the reason a constant colour is the right oracle are all explained
//! there and are not repeated here.
//!
//! # What is different about the mesh case, and why it needed its own Pass
//!
//! An analytic shading keeps its colour in a `ColorRamp`, which `Pass 122.6`
//! taught to carry colorants alongside its sRGB samples. A mesh has **no
//! ramp** unless it is parametric: its colour is per-vertex, decoded from
//! the mesh stream itself and converted to sRGB inside the parser, one value
//! per vertex, before any geometry exists. There was nowhere to put the
//! authored ink, so `Pass 137.0`'s widened analytic route — correct, and
//! measurably so for types 2 and 3 — **could not reach a mesh at all**.
//!
//! ★ The gate in front of it read `ramp.is_some_and(has_colorants)`, which
//! is `false` for a fully ink-bearing `DeviceCMYK` mesh. A predicate about
//! the wrong carrier is indistinguishable, at the call site, from a
//! predicate about the right one.
//!
//! # Two mesh types, deliberately, and they are not redundant
//!
//! | fixture mark | reaches ink via |
//! |---|---|
//! | type 4 triangle mesh | shades straight into `fill_triangle`'s barycentric interpolation |
//! | type 6 Coons patch | four CORNER shades, bilinearly interpolated by `Patch::shade_at` through `Shade::lerp`, *then* triangulated |
//!
//! A carrier that survived one path and was dropped by the other would be
//! invisible to a fixture testing only one — and the real file that exposed
//! the defect contains **patches**, not triangles, so the patch path is the
//! one that actually mattered. Testing only the simpler shape would have
//! been the more comfortable choice and the wrong one.
//!
//! # Verified to fail
//!
//! Against a build with the mesh branch of `Shading::paint_cmyk` disabled,
//! on these exact fixtures: the fill renders `(151, 64, 133)` and **both**
//! meshes render `(160, 90, 113)` — mean |diff| `18.33`, the same magnitude
//! the analytic defect had, because it is the same round trip.
//!
//! On the file that prompted the work, live-versus-its-own-reference mean
//! absolute distance on the two type 7 patches went `24.06 → 6.29` and
//! `16.87 → 2.15`, with the remaining difference in the first being edge
//! detail rather than colour (its mean colour matches the reference to
//! within 2.4 of 255).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::RenderedPage;

const SCALE: f32 = 4.0;

fn render(name: &str) -> RenderedPage {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/mesh-ink")
        .join(name);
    let doc = Document::load(Path::new(&path)).expect("fixture loads");
    let pages = page_tree::pages(&doc).expect("page tree");
    pdfcer_render::render_page(&doc, &pages[0], SCALE).expect("renders")
}

/// Mean RGB of a horizontal band across the middle of the page, between the
/// two given fractions of its width.
fn patch(page: &RenderedPage, fx0: f64, fx1: f64) -> (f64, f64, f64) {
    let w = page.pixmap.width();
    let h = page.pixmap.height();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (x0, x1) = ((f64::from(w) * fx0) as u32, (f64::from(w) * fx1) as u32);
    let (y0, y1) = (h * 2 / 5, h * 3 / 5);
    let px = page.pixmap.pixels();
    let (mut r, mut g, mut b, mut n) = (0.0, 0.0, 0.0, 0.0);
    for y in y0..y1 {
        for x in x0..x1 {
            let p = px[(y * w + x) as usize];
            r += f64::from(p.red());
            g += f64::from(p.green());
            b += f64::from(p.blue());
            n += 1.0;
        }
    }
    (r / n, g / n, b / n)
}

/// The three marks: flat fill, type 4 mesh, type 6 patch.
fn marks(page: &RenderedPage) -> [(f64, f64, f64); 3] {
    [
        patch(page, 0.08, 0.28),
        patch(page, 0.41, 0.61),
        patch(page, 0.75, 0.95),
    ]
}

fn mean_abs(a: (f64, f64, f64), b: (f64, f64, f64)) -> f64 {
    ((a.0 - b.0).abs() + (a.1 - b.1).abs() + (a.2 - b.2).abs()) / 3.0
}

/// ★★★ THE ONE THAT MATTERS.
#[test]
fn a_mesh_and_a_fill_of_one_ink_agree_on_a_subtractive_page() {
    let page = render("mesh-vs-fill-cmyk.pdf");
    let [fill, t4, t6] = marks(&page);
    let d4 = mean_abs(fill, t4);
    let d6 = mean_abs(fill, t6);
    assert!(
        d4 <= 1.0,
        "★ TYPE 4: the SAME authored ink rendered two different colours — \
         fill {fill:?} vs triangle mesh {t4:?}, mean |diff| {d4:.2}. A mesh's \
         vertex colours are converted to sRGB inside the parser, so on an ink \
         page they take a CMYK -> sRGB -> CMYK round trip the fill does not. \
         Measured at 18.33 before the fix"
    );
    assert!(
        d6 <= 1.0,
        "★ TYPE 6: fill {fill:?} vs Coons patch {t6:?}, mean |diff| {d6:.2}. \
         A patch reaches the rasteriser through Patch::shade_at and \
         Shade::lerp rather than directly, so it can lose the carrier even \
         where a triangle mesh keeps it — which is why both are asserted"
    );
}

/// The additive control.
///
/// Passed before the fix and after. It exists so that a future change which
/// breaks the *additive* mesh path cannot hide behind the subtractive test
/// staying green — they are different code, reached by different branches.
#[test]
fn a_mesh_and_a_fill_of_one_ink_agree_on_an_additive_page_too() {
    let page = render("mesh-vs-fill-rgb.pdf");
    let [fill, t4, t6] = marks(&page);
    assert!(
        mean_abs(fill, t4) <= 1.0,
        "fill {fill:?} vs triangle mesh {t4:?}"
    );
    assert!(
        mean_abs(fill, t6) <= 1.0,
        "fill {fill:?} vs Coons patch {t6:?}"
    );
}

/// ★ The two mesh types must agree with EACH OTHER, not merely each with the
/// fill.
///
/// Strictly implied by the test above under exact arithmetic, and asserted
/// anyway because it fails differently: a change that shifts *both* meshes
/// by the same amount — a shared conversion, a shared rounding — moves them
/// away from the fill together and would be reported twice by one test as
/// though two independent things broke. This one stays green in that case
/// and localises the fault to the shared step.
#[test]
fn the_two_mesh_types_agree_with_each_other() {
    for name in ["mesh-vs-fill-cmyk.pdf", "mesh-vs-fill-rgb.pdf"] {
        let page = render(name);
        let [_, t4, t6] = marks(&page);
        let d = mean_abs(t4, t6);
        assert!(
            d <= 1.0,
            "{name}: triangle mesh {t4:?} vs Coons patch {t6:?}, mean |diff| {d:.2} \
             — the two decode paths disagree about the same authored colour"
        );
    }
}

/// The mesh actually covers the area being sampled.
///
/// ★ Without this, every assertion above could pass on **bare white paper**:
/// if the mesh painted nothing, `t4` and `t6` would both be white, they would
/// agree with each other perfectly, and only the comparison against the fill
/// would catch it — through a difference that reads as a colour error rather
/// than as absence. This project has shipped that exact confusion before: a
/// conformance harness once scored five blank cells as passes, because a
/// white mark on white paper has no contrast for a detector to find.
#[test]
fn the_meshes_are_actually_painted_and_not_bare_paper() {
    let page = render("mesh-vs-fill-cmyk.pdf");
    let [_, t4, t6] = marks(&page);
    for (what, c) in [("type 4", t4), ("type 6", t6)] {
        assert!(
            c.0 < 240.0 || c.1 < 240.0 || c.2 < 240.0,
            "{what} sampled {c:?} — that is paper, not a mesh. Every other \
             assertion in this file would pass on an unpainted page"
        );
    }
}
