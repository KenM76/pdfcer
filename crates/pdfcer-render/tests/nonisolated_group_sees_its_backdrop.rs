//! ★ `Pass 97.1g` — a non-isolated group on a subtractive page must see the
//! backdrop, and this file is the only end-to-end evidence that it does.
//!
//! # Why this file exists rather than more unit tests
//!
//! `cmyk_buffer.rs` already unit-tests §11.4.4's backdrop removal: that a
//! `Normal`-only interior makes the one-walk and two-walk routes agree
//! exactly, that a blending interior makes them differ, that a
//! backdrop-seeded child comes back clean. Those prove the **arithmetic**.
//!
//! They cannot prove the arithmetic is **reachable**. The gap between "this
//! function is correct" and "a content stream can get to this function" is
//! where `Pass 97.1g` very nearly shipped as dead code, and the sequence is
//! worth recording because nothing in the test suite would have caught it:
//!
//! 1. The print-conformance suite is this project's transparency oracle. All
//!    51 patches were rendered with a binary built before this Pass and one
//!    built after. **Zero pixels differ, on any patch.** Thirteen second
//!    content walks do occur, but every one is an over-trigger of the
//!    deliberately-conservative `backdrop_dependent` test in `interpret.rs`,
//!    and where the interior does not truly read its backdrop, isolated and
//!    non-isolated agree by §11.4.4 NOTE 2. The suite's blend-mode patches
//!    put their `/BM` in the graphics state **around** the `Do`, which the
//!    group's own composite already honoured.
//! 2. A first hand-written fixture ALSO showed no change — twice, for two
//!    different reasons, each of which rendered a plausible picture:
//!    - its `/GSO` and `/GSI` indirect references were both off by one, so
//!      the *outer* graphics state picked up the interior's `Multiply` and
//!      the interior's reference dangled;
//!    - and once that was fixed, its outer state was neutral
//!      (`/BM /Normal`, `ca = CA = 1`), so `interpret.rs`'s
//!      `needs_buffer = is_transparency_group && (!outer_is_neutral ||
//!      isolated || knockout)` painted the group **inline** — which is not an
//!      approximation of non-isolated semantics, it *is* non-isolated
//!      semantics, and needs no buffer or second walk at all.
//!
//! ⇒ **The gap this Pass closes exists only where a buffer is FORCED.**
//! Buffering a non-isolated group is what turns it into an isolated one; the
//! second content walk is what turns it back. That is why every fixture here
//! carries an outer `/ca 0.5`, and why removing it would silently reduce this
//! file to four renders that agree for uninteresting reasons.
//!
//! # The four-way signature
//!
//! A correct implementation has a *shape* here, not just a set of passing
//! assertions — the same reasoning `nonseparable_blend_differential.rs`
//! applies to blend functions. Agreement everywhere would mean the walk never
//! runs; disagreement everywhere would mean it runs when §11.4.4 NOTE 2 says
//! it must not.
//!
//! | comparison | expected | what a violation would mean |
//! |---|---|---|
//! | non-isolated `Multiply` vs isolated `Multiply` | **differ** | the group still cannot see its backdrop — the defect itself |
//! | non-isolated `Normal` vs isolated `Normal` | **identical** | the removal does not cancel; the two-walk path corrupts backdrop-independent groups |
//! | non-isolated `Multiply` darker in the overlap | **yes** | `Multiply` against cyan must subtract, not add |
//!
//! Fixtures and their construction: `tools/gen-nonisolated-group-fixtures.py`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::RenderedPage;

/// Render one fixture's only page at 4× — the same scale the Pass was
/// measured at, so a number quoted in the record and a number this test
/// checks come from the same geometry.
fn render(name: &str) -> RenderedPage {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/transparency")
        .join(name);
    let doc =
        Document::from_bytes(std::fs::read(&path).expect("fixture file")).expect("fixture parses");
    let pages = page_tree::pages(&doc).expect("page tree");
    pdfcer_render::render_page(&doc, &pages[0], 4.0).expect("renders")
}

/// Count pixels whose RGB differs at all between two renders of the same
/// geometry.
///
/// Exact inequality rather than a tolerance, deliberately: both sides come
/// from the same rasteriser on the same paths at the same scale, so the only
/// thing that can move a pixel is the compositing arithmetic under test. A
/// tolerance here would be a place for a real regression to hide.
fn differing_pixels(a: &RenderedPage, b: &RenderedPage) -> usize {
    assert_eq!(a.pixmap.width(), b.pixmap.width(), "geometry must match");
    assert_eq!(a.pixmap.height(), b.pixmap.height(), "geometry must match");
    a.pixmap
        .pixels()
        .iter()
        .zip(b.pixmap.pixels().iter())
        .filter(|(p, q)| p.red() != q.red() || p.green() != q.green() || p.blue() != q.blue())
        .count()
}

/// The pixel at the centre of the page, which both rectangles cover.
fn centre(p: &RenderedPage) -> (u8, u8, u8) {
    let (w, h) = (p.pixmap.width(), p.pixmap.height());
    let px = p.pixmap.pixel(w / 2, h / 2).expect("centre in bounds");
    (px.red(), px.green(), px.blue())
}

/// The defect, stated as the measurement that used to hold.
///
/// Before `Pass 97.1g` these two files rendered **byte-identically** — a
/// non-isolated group was composited as if isolated, so its `Multiply`
/// blended against a transparent backdrop instead of the cyan beneath it.
/// That is what this assertion exists to keep from coming back, and it is
/// written as an inequality because the bug's signature was equality.
#[test]
fn a_non_isolated_multiply_differs_from_an_isolated_one() {
    let ni = render("nonisolated_multiply_cmyk.pdf");
    let iso = render("isolated_multiply_cmyk.pdf");
    let n = differing_pixels(&ni, &iso);
    assert!(
        n > 0,
        "a non-isolated group's interior must blend against the page \
         backdrop; identical output means it was composited as if isolated \
         (the pre-97.1g defect). centre ni={:?} iso={:?}",
        centre(&ni),
        centre(&iso)
    );
}

/// The direction, because "they differ" is satisfied by differing wrongly.
///
/// `Multiply` of magenta over cyan removes red. The isolated render blends
/// against nothing and keeps it. So the non-isolated centre pixel must be
/// **no brighter in red** than the isolated one, and strictly darker
/// somewhere — an implementation that differed by *adding* light would pass
/// the test above and fail this one.
#[test]
fn the_difference_is_in_the_direction_multiply_requires() {
    let (ni_r, _, _) = centre(&render("nonisolated_multiply_cmyk.pdf"));
    let (iso_r, _, _) = centre(&render("isolated_multiply_cmyk.pdf"));
    assert!(
        ni_r < iso_r,
        "Multiply over cyan must subtract red: non-isolated red {ni_r} \
         should be darker than isolated red {iso_r}"
    );
}

/// ★ §11.4.4 NOTE 2's exactness, end to end.
///
/// A group whose interior never reads its backdrop renders the same isolated
/// or not. `Canvas::group` relies on this to skip the second walk, and
/// `cmyk_buffer.rs` asserts it for the arithmetic; this asserts it for two
/// real PDFs that differ in exactly one dictionary key.
///
/// If it fails, the two-walk path is corrupting groups it should be leaving
/// alone — which is a strictly worse defect than the one this Pass fixed,
/// because it would affect every ordinary group rather than only blending
/// ones.
#[test]
fn a_normal_interior_renders_the_same_isolated_or_not() {
    let ni = render("nonisolated_normal_cmyk.pdf");
    let iso = render("isolated_normal_cmyk.pdf");
    let n = differing_pixels(&ni, &iso);
    assert_eq!(
        n,
        0,
        "a backdrop-independent interior must render identically whether \
         the group is isolated or not (§11.4.4 NOTE 2); {n} pixel(s) differ, \
         centre ni={:?} iso={:?}",
        centre(&ni),
        centre(&iso)
    );
}
