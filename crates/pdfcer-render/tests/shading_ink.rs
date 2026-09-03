//! # A shading and a fill of the same ink must be the same colour
//!
//! # What was wrong
//!
//! A shading's colour was resolved to three-channel sRGB when its colour **ramp
//! was built**, so by the time anything composited there were no colorants
//! left. On a page that composites in ink that meant a `CMYK → sRGB → CMYK`
//! round trip, and the return leg is a *different function* from the outbound
//! one — a calibrated table out, a naive formula back. The ink that arrived was
//! not the ink that left.
//!
//! ## ★★ Why it stayed invisible for so long, which is the transferable part
//!
//! **Everything on the page took the same round trip**, so everything was
//! consistently slightly wrong *together* and nothing looked out of place.
//!
//! It became visible only when the other half was fixed. `Pass 130.1` gave a
//! `DeviceCMYK` image its authored ink, so images stopped round-tripping — and
//! from then on the same colour drawn as a shading and as an image came out
//! **different**. The operator found it on a conformance sheet whose shading
//! boxes print a live shading beside a reference *image* of what it should look
//! like, captioned *"the shadings should look like the reference image"*. Two
//! of four pairs visibly disagreed. **That box carries no trap cross**, so no
//! automated check in this project could see it; it was found by a human
//! looking at the page.
//!
//! ⇒ Fixing one half of a two-halves-agree-wrongly situation converts a silent
//! shared error into a visible disagreement. That is an argument *for* fixing
//! halves — the disagreement is information — but the second half becomes
//! urgent in a way it was not before.
//!
//! # The oracle, and why it needs no reference render
//!
//! Each fixture draws the **same `DeviceCMYK` colour twice**: once as a flat
//! filled rectangle, once as an axial shading whose function is **constant**.
//! A constant shading is the same colour everywhere, so any pixel of it is
//! comparable to any pixel of the fill and the assertion needs no geometry, no
//! parametric position, and nothing remembered.
//!
//! Verified to fail against a build with the fix disabled: the fill rendered
//! `(151, 64, 133)` and the shading `(160, 90, 113)` — **18 levels apart**.
//!
//! # What this does NOT cover
//!
//! **Mesh shadings** (types 4–7) — and the reader who fixes them should look
//! next door rather than here.
//!
//! ★ This section said *"a mesh still bridges through sRGB and still disagrees
//! with an image of the same colour… Named here so a reader who fixes the mesh
//! case knows this file is where its test belongs."* The first half stopped
//! being true in `Pass 137.1`, the very next Pass. The second half was a
//! prediction and it turned out **wrong**: the mesh tests live in
//! `mesh_ink.rs`, with their own fixtures, because a mesh needed an entirely
//! different carrier (`Shade::Ink`, per-vertex) rather than the ramp this
//! file's fixtures exercise. Two defects with one symptom had two fixes and
//! want two test files.
//!
//! Kept rather than deleted because the *shape* is the lesson: a doc comment
//! that says where a future change belongs is a guess about work nobody has
//! done yet, and it ages worse than a description of what is.

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

const SCALE: f32 = 3.0;

fn render(name: &str) -> RenderedPage {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/shading")
        .join(name);
    let doc = Document::load(Path::new(&path)).expect("fixture loads");
    let pages = page_tree::pages(&doc).expect("page tree");
    pdfcer_render::render_page(&doc, &pages[0], SCALE).expect("renders")
}

/// The mean RGB of a patch, as `(r, g, b)` in f64 so a comparison is not
/// quantised before it is made.
fn patch(page: &RenderedPage, x0: u32, x1: u32) -> (f64, f64, f64) {
    let w = page.pixmap.width();
    let h = page.pixmap.height();
    let (y0, y1) = (h / 2 - 6, h / 2 + 6);
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

/// The flat fill occupies roughly x 10–90 of a 200 pt page; the shading 110–190.
fn fill_and_shading(page: &RenderedPage) -> ((f64, f64, f64), (f64, f64, f64)) {
    let w = f64::from(page.pixmap.width());
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let at = |a: f64, b: f64| ((w * a) as u32, (w * b) as u32);
    let (f0, f1) = at(0.20, 0.30);
    let (s0, s1) = at(0.70, 0.80);
    (patch(page, f0, f1), patch(page, s0, s1))
}

fn mean_abs(a: (f64, f64, f64), b: (f64, f64, f64)) -> f64 {
    ((a.0 - b.0).abs() + (a.1 - b.1).abs() + (a.2 - b.2).abs()) / 3.0
}

/// ★★ A SPOT shading and a spot fill of one tint agree on an ink page
/// (`Pass 239.0`). The fill has deposited its colorant into a plane of its
/// own since `Pass 228.0`; the shading flattened it through the tint
/// transform until this Pass, and the two collapsed to sRGB by different
/// arithmetic. Same oracle as the `DeviceCMYK` pair above, one colour space
/// over: no reference render, just agreement.
#[test]
fn a_spot_shading_and_a_spot_fill_of_one_tint_agree_on_a_subtractive_page() {
    let page = render("spot-shading-vs-fill-cmyk.pdf");
    let (fill, shading) = fill_and_shading(&page);
    let d = mean_abs(fill, shading);
    assert!(
        d <= 1.5,
        "fill {fill:?} vs `sh` shading {shading:?}, mean |diff| {d:.2}: the spot \
         must reach the same plane by both routes"
    );
}

/// ★★★ THE DISCRIMINATING PAIR. On white paper the two tests above cannot
/// tell a deposited spot from a flattened one: the plane's curve is sampled
/// through the very conversion the flattened route takes, so both land on
/// the same sRGB by construction — a sabotage that refused every shading its
/// planes left them green. Over a `0 0 0 0.5 k` mark with `/OP true` the
/// routes separate: a deposited spot leaves the K standing (Table 149, spot
/// source × process colorant ⇒ `c_b`); a flattened one is a spot-only source
/// the native route refuses, paints normally, and knocks the K out. The fill
/// beside it deposits, so agreement here means the shading took the plane.
#[test]
fn an_overprinting_spot_shading_over_black_agrees_with_the_fill_and_keeps_the_black() {
    let page = render("spot-shading-op-over-k-vs-fill.pdf");
    let (fill, shading) = fill_and_shading(&page);
    let d = mean_abs(fill, shading);
    assert!(
        d <= 1.5,
        "fill {fill:?} vs `sh` shading {shading:?} over 50% K under /OP true, mean \
         |diff| {d:.2}: a shading that flattened its spot knocked the K out"
    );
    // And the K really is there: the spot alone (the two white-paper tests)
    // is a light green; over preserved 50 % K it must be markedly darker.
    assert!(
        fill.1 < 150.0,
        "the 50% K beneath must survive the overprinting spot: {fill:?}"
    );
}

/// The same through a shading PATTERN under `/OP true`.
#[test]
fn an_overprinting_spot_pattern_over_black_agrees_with_the_fill_and_keeps_the_black() {
    let page = render("spot-pattern-op-over-k-vs-fill.pdf");
    let (fill, pattern) = fill_and_shading(&page);
    let d = mean_abs(fill, pattern);
    assert!(
        d <= 1.5,
        "fill {fill:?} vs shading pattern {pattern:?} over 50% K under /OP true, \
         mean |diff| {d:.2}: a pattern that bridged through sRGB knocked the K out"
    );
    assert!(
        fill.1 < 150.0,
        "the 50% K beneath must survive the overprinting spot: {fill:?}"
    );
}

/// The same, through a `/PatternType 2` shading PATTERN — the route the
/// print-conformance suite's "shading" cells use, and the one that bridged
/// through sRGB for pdfcer's whole life while `sh` gained native routes in
/// `Pass 122.6` and `137.0`. One patch kept a white X in exactly this cell
/// after every other cell beside it went clean.
#[test]
fn a_spot_shading_pattern_and_a_spot_fill_of_one_tint_agree_on_a_subtractive_page() {
    let page = render("spot-pattern-vs-fill-cmyk.pdf");
    let (fill, pattern) = fill_and_shading(&page);
    let d = mean_abs(fill, pattern);
    assert!(
        d <= 1.5,
        "fill {fill:?} vs shading pattern {pattern:?}, mean |diff| {d:.2}: a \
         pattern fill is the same painter as `sh` and must take its native \
         ink route"
    );
}

/// ★★★ THE ONE THAT MATTERS. On a page that composites in ink, a shading and a
/// fill of the same authored `DeviceCMYK` colour must be the same colour.
#[test]
fn a_shading_and_a_fill_of_one_ink_agree_on_a_subtractive_page() {
    let page = render("shading-vs-fill-cmyk.pdf");
    let (fill, shading) = fill_and_shading(&page);
    let d = mean_abs(fill, shading);
    assert!(
        d <= 1.0,
        "★ the SAME authored ink rendered two different colours: fill {fill:?} \
         vs shading {shading:?}, mean |diff| {d:.2}. The shading's ramp resolved \
         to sRGB before anything composited, so it took a CMYK -> sRGB -> CMYK \
         round trip the fill did not. Measured at 18.33 before the fix"
    );
}

/// The additive control.
///
/// On a page with no group colour space there is no colorant buffer and no
/// round trip for either object, so they agreed even before the fix. Asserted
/// so that a future change which breaks the *additive* path cannot hide behind
/// the subtractive test passing — the two paths are different code.
#[test]
fn a_shading_and_a_fill_of_one_ink_agree_on_an_additive_page_too() {
    let page = render("shading-vs-fill-rgb.pdf");
    let (fill, shading) = fill_and_shading(&page);
    let d = mean_abs(fill, shading);
    assert!(
        d <= 1.0,
        "fill {fill:?} vs shading {shading:?}, mean |diff| {d:.2}"
    );
}

/// ★ The two pages are allowed to differ from EACH OTHER, and pinning that they
/// do not would be wrong.
///
/// A subtractive page converts its result out of ink at the end; an additive
/// one never entered ink. The two are *required* to differ where that
/// conversion is not the identity, and this project has measured that gap at up
/// to ~100 levels on saturated overlaps. What must hold is that within each
/// page the two objects agree — which the tests above assert — not that the
/// pages agree with one another.
///
/// This test exists to stop somebody "tightening" the two tests above into a
/// cross-page equality that would be false for a correct renderer.
#[test]
fn the_two_pages_are_not_required_to_match_each_other() {
    let cmyk = render("shading-vs-fill-cmyk.pdf");
    let rgb = render("shading-vs-fill-rgb.pdf");
    let (cf, cs) = fill_and_shading(&cmyk);
    let (rf, rs) = fill_and_shading(&rgb);
    // Each page is internally consistent...
    assert!(mean_abs(cf, cs) <= 1.0);
    assert!(mean_abs(rf, rs) <= 1.0);
    // ...and no claim is made about cmyk-vs-rgb. Recorded, not asserted.
    let across = mean_abs(cf, rf);
    assert!(
        across.is_finite(),
        "cross-page difference is {across:.2} — recorded deliberately, never pinned"
    );
}

// ---------------------------------------------------------------------------
// Pass 201.0 -- an overprinting MIXED /DeviceN shading must not write channels
// it never claims (ISO 32000-1 SS11.7.4.3, Table 149).
// ---------------------------------------------------------------------------

/// Mean luminance over the shaded rectangle's interior.
///
/// A mean rather than a probe pixel: the shading is a ramp, so any single point
/// is a statement about one `t` value and the claim is about the whole band.
fn mean_luma(page: &RenderedPage) -> f64 {
    let w = page.pixmap.width();
    let h = page.pixmap.height();
    let px = page.pixmap.pixels();
    let (mut total, mut n) = (0.0f64, 0.0f64);
    for y in (h / 4)..(h * 3 / 4) {
        for x in (w / 4)..(w * 3 / 4) {
            let p = px[(y * w + x) as usize];
            total += 0.30 * f64::from(p.red())
                + 0.59 * f64::from(p.green())
                + 0.11 * f64::from(p.blue());
            n += 1.0;
        }
    }
    total / n
}

/// THE ASSERTION. Overprint ON must KEEP the backdrop's black.
///
/// # The trade this pins
///
/// `Pass 195.0` fixed a spot colorant's ink being DROPPED by widening a mixed
/// `/DeviceN` to write all four components. That wrote channels the source
/// never claimed, and its own comment said so while concluding it was safe:
/// *"it writes the source's M and K, which are 0 for this shading, so it knocks
/// out backdrop magenta and black that the spot never claimed. No patch in the
/// conformance corpus detects that."*
///
/// One does. A `1 0 1 .5 k` mark under an overprinting
/// `/DeviceN [<spot>, /Cyan]` shading lost its `K = 0.5` to the shading's
/// `K = 0` and vanished -- sixteen times on one page.
///
/// K is a plane pdfcer HAS, so this was not the missing per-spot-colorant plane
/// that several other conformance patches need. It was ink being ERASED by a
/// fix for ink being DROPPED.
///
/// # Why this compares against the control rather than an absolute colour
///
/// The absolute value depends on the CMYK-to-sRGB conversion, which is
/// separately known-inaccurate. Pinning a number here would make this test fail
/// for the wrong reason the day that is fixed.
///
/// Sabotage-verified: reverting the narrowing makes both pages render at
/// luminance 171.5 -- IDENTICAL, because the K is knocked out either way.
#[test]
fn an_overprinting_mixed_devicen_shading_keeps_the_backdrops_black() {
    let on = mean_luma(&render("shading-overprint-mixed-spot-keeps-k.pdf"));
    let off = mean_luma(&render("shading-overprint-off-control.pdf"));
    assert!(
        on < off - 20.0,
        "overprint ON must keep the backdrop's K = 0.5 and render DARKER than \
         the overprint-OFF control, which correctly replaces it. Got on={on:.1}, \
         off={off:.1}. A small gap means the mixed-/DeviceN case is writing the \
         source's K = 0 over backdrop black the shading never claimed"
    );
}

/// The CONTROL, asserted positively: the shading really does paint, and ramps.
///
/// Without this, the test above is satisfied by a build where the overprinting
/// shading paints NOTHING -- which is not hypothetical, it is what the
/// spot-only refusal does one branch away in the same function. A blank page
/// over a `1 0 1 0.5 k` rectangle would be dark too.
#[test]
fn the_overprinting_shading_actually_paints_and_ramps() {
    let page = render("shading-overprint-mixed-spot-keeps-k.pdf");
    let w = page.pixmap.width();
    let h = page.pixmap.height();
    let px = page.pixmap.pixels();
    let at = |x: u32| {
        let p = px[((h / 2) * w + x) as usize];
        (p.red(), p.green(), p.blue())
    };
    let (left, right) = (at(w / 5), at(w * 4 / 5));
    assert_ne!(
        left, right,
        "the shading must RAMP across the band; identical ends mean its function \
         was not evaluated and this fixture asserts nothing"
    );
    // ★ `Pass 239.0` moved this from the BLUE channel to the RED one. The
    // shading names Cyan (`Source`) and its spot; with the spot on a plane
    // of its own the backdrop's Y and K are preserved at BOTH ends, so the
    // channel that ramps is the one the shading actually writes: cyan
    // rising is red falling. Blue was a witness of the flattened route,
    // where the spot's ink landed in Y and moved the blue by accident.
    assert!(
        left.0 > right.0 + 40,
        "expected red to FALL across the spot-to-cyan ramp as the named cyan rises; got left={left:?} right={right:?}"
    );
}

// ---------------------------------------------------------------------------
// Pass 202.0 -- a SPOT-ONLY /DeviceN shading under overprint must still paint.
// ---------------------------------------------------------------------------

/// ★★★ A shading that names no process colorant must not render as blank paper.
///
/// # The defect
///
/// A `/DeviceN` naming only SPOT colorants puts all four of the group's process
/// components in Table 149's "not named in the source space" column, which
/// under `/OP true` is `c_b` — the backdrop. Composited literally, the shading
/// preserves the entire backdrop and paints NOTHING: correct for a press, where
/// the ink is on its own plate, and a vanished mark for a renderer with four
/// process planes and no spot plane. The intended behaviour is to refuse the
/// native-ink route and let the bridge paint the flattened tint instead — a
/// disclosed approximation, and enormously better than nothing.
///
/// # ★★ Why this test exists at all, which is the transferable part
///
/// The refusal was **documented and not implemented**. `interpret.rs` carried a
/// long block headed "THE SPOT-ONLY REFUSAL" that named the conformance patch,
/// described the shape, and even quoted the measured damage — "451 × 29 device
/// pixels of bare white paper, with `shadings_painted = 1` and
/// `overprint_shadings_unsupported = 0`". Every word of it was accurate. The
/// guard it described was never added to that route; it went to the *path*
/// route only, and the comment sat above an unguarded call for every Pass
/// since.
///
/// ⇒ **A comment describing a safeguard is indistinguishable from a safeguard**
/// to review, to clippy and to the type system — and a detailed, measured one
/// is *more* convincing than most real code, not less. Nothing could have
/// caught this except rendering the file and asking why the bar was white.
/// Hence a render-based regression test rather than any form of inspection.
///
/// # The oracle
///
/// Blank paper is an unambiguous failure state and needs no reference render:
/// the assertion is that the band is not white, and that it *ramps*. Both
/// halves are required — see the sibling test below for why "not white" alone
/// would be satisfied by a bug.
#[test]
fn a_spot_only_devicen_shading_under_overprint_still_paints() {
    let page = render("shading-overprint-spot-only.pdf");
    let luma = mean_luma(&page);
    assert!(
        luma < 250.0,
        "a spot-only /DeviceN shading under /OP true rendered as bare white \
         paper (mean luma {luma:.1}). Table 149 puts every process component \
         in the backdrop column, so the native-ink route paints nothing and \
         must refuse in favour of the flattening bridge. This is the defect a \
         detailed comment described for several Passes while the guard it \
         announced was absent from the code beneath it"
    );
}

/// The control that stops "not white" from being satisfied by the wrong thing.
///
/// A build that painted one flat colour across the whole band — or that filled
/// it with an arbitrary solid — would pass the assertion above while having
/// lost the shading entirely. Requiring the band to RAMP pins that the tint
/// transform was actually evaluated across the parametric domain rather than
/// once.
#[test]
fn the_spot_only_shading_ramps_rather_than_painting_one_flat_colour() {
    let page = render("shading-overprint-spot-only.pdf");
    let w = page.pixmap.width();
    let (left, right) = (
        patch(&page, w / 8, w / 4),
        patch(&page, w * 3 / 4, w * 7 / 8),
    );
    let d = mean_abs(left, right);
    assert!(
        d > 10.0,
        "the band must RAMP across the two spot colorants; got left={left:?} \
         right={right:?}, mean |diff| {d:.2}. Ends that match mean the shading \
         function was not evaluated and the test above asserts nothing"
    );
}
