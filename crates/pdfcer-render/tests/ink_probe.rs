//! # The ink probe — what is in the colorant buffer at the moment it stops existing
//!
//! `RenderOptions::with_ink_probe` reports the four colorant tints at one
//! device pixel, read **immediately before** the page's colorant buffer is
//! converted to sRGB (§11.4.7's *"convert the result to the device's native
//! colour space"*). One line later there is nothing left to ask: the buffer is
//! consumed into a pixmap and dropped.
//!
//! # Why an instrument was built rather than a `println!`
//!
//! The sibling `iccce` project asked, on 2026-08-29, for exactly one number:
//! the CMYK sitting in the buffer just before that conversion, for a saturated
//! green. The question is an **attribution** question. Under decision 064 a
//! colorant buffer does two separable things —
//!
//! | stage | whose |
//! |---|---|
//! | composites the page **in ink** | pdfcer's |
//! | converts the composited result **to sRGB on the way out** | `iccce`'s |
//!
//! — and pdfcer's only switch for the buffer, `--max-cmyk-buffer-bytes`, turns
//! **both** off together. No measurement taken through that switch can say
//! which half moved a pixel. A probe between the two stages can.
//!
//! A one-off `println!` would have answered it once. The reason it is a
//! shipped, tested capability instead is `Pass 165.0`'s own headline finding:
//! `cmyk_bridged_pixels` sat at **0** across 40 000 pixels it was supposed to
//! be counting, and the cost of that was six hypotheses and a wrong test to
//! find something an honest instrument would have shown in one render. The
//! lesson recorded then was *"a counter stuck at zero reads identically to a
//! correct one"*; the corollary acted on here is that the instrument you only
//! build when you already need the answer is the one nobody has calibrated.
//!
//! # The claim these fixtures pin, and why it looks too obvious to test
//!
//! **For a single opaque paint over an empty page, a correct colorant
//! composite is the identity on its operand.** The backdrop is transparent,
//! the alpha is 1, the blend mode is Normal — there is nothing to blend with.
//! So the four numbers in the buffer must be the four numbers the content
//! stream wrote.
//!
//! It had never been tested. Every existing colorant assertion in this crate
//! is made on **sRGB pixels after the conversion**, which is a measurement of
//! the composite and the conversion *together* — precisely the conflation
//! `iccce` declined to accept.
//!
//! # Non-vacuity
//!
//! Both halves are asserted on **both** fixtures, which are identical but for
//! the page group (`fixtures/synthetic/ink-probe/PROVENANCE.md`):
//!
//! * an implementation that echoed the content stream's operands instead of
//!   reading the buffer passes the subtractive page and fails the additive
//!   one, where the correct report is *"there are no colorant values"*;
//! * an implementation that reported colorants for every page passes the
//!   additive page's `srgb` assertion and fails its `source` assertion.
//!
//! `R188`: two routes to a value are one measurement only when they are
//! independent. `cmyk` and `srgb` here are not — the second is the first put
//! through the conversion — which is exactly why the probe reports both and
//! labels which is which, rather than reconstructing either from the other.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{InkProbeSource, RenderOptions, RenderedPage};

/// Device pixels per point. 2.0 puts the 100 pt patch at 200 px square, so a
/// probe aimed at its centre is a hundred pixels from the nearest edge and no
/// antialiasing can reach it.
const SCALE: f32 = 2.0;

/// The operand the fixtures write, as `tools/gen-ink-probe-fixtures.py` writes
/// it. Duplicated here rather than parsed out of the PDF deliberately: a test
/// that reads its expectation from the file under test cannot fail when both
/// are wrong together.
const INK: [f32; 4] = [0.75, 0.0, 1.0, 0.0];

/// The centre of the patch in device pixels: the page is 200 pt, the patch is
/// 100 pt at (50, 50), so its centre is (100, 100) in points and (200, 200)
/// in device pixels at `SCALE`. Y is not flipped by the arithmetic because
/// the patch is centred on the page in both axes.
const PROBE_XY: (u32, u32) = (200, 200);

fn render(name: &str, options: &RenderOptions) -> RenderedPage {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/ink-probe")
        .join(name);
    let doc = Document::load(Path::new(&path)).expect("fixture loads");
    let pages = page_tree::pages(&doc).expect("page tree");
    pdfcer_render::render_page_with_view(&doc.view(), &pages[0], SCALE, options).expect("renders")
}

/// ★ THE ANSWER `iccce` ASKED FOR, as an assertion rather than a printout.
///
/// The operand goes into the colorant buffer and comes out of it unchanged.
/// The composite is therefore an identity here, and any residual difference
/// between pdfcer's green and a reference engine's is downstream of this point
/// — in the conversion, which is `iccce`'s half of decision 064.
#[test]
fn a_single_opaque_paint_reaches_the_exit_conversion_with_its_operand_intact() {
    let options = RenderOptions::default().with_ink_probe(PROBE_XY.0, PROBE_XY.1);
    let page = render("flat-cmyk-subtractive.pdf", &options);

    assert!(
        page.diagnostics.cmyk_buffer_engaged,
        "the fixture declares a /DeviceCMYK page group; if this is false the \
         test below is measuring the wrong thing entirely"
    );
    let probe = page.diagnostics.ink_probe.expect("a probe was requested");
    assert_eq!(probe.source, InkProbeSource::CmykBuffer);
    assert_eq!((probe.x, probe.y), PROBE_XY);

    let cmyk = probe.cmyk.expect("a colorant buffer reports colorants");
    for (i, (got, want)) in cmyk.iter().zip(INK.iter()).enumerate() {
        assert!(
            (got - want).abs() < 1e-6,
            "component {i}: the buffer holds {got} where the content stream \
             wrote {want} — the composite is NOT an identity on a single \
             opaque paint, which is a defect in the compositor and not in the \
             conversion"
        );
    }
    assert!(
        (probe.alpha.expect("alpha") - 1.0).abs() < 1e-6,
        "an opaque fill covers this pixel completely"
    );
    // The sRGB half is reported too, and is the number the PNG carries. It is
    // asserted only to be PRESENT and to be a plausible green: pinning its
    // exact value here would make this test a second copy of `cmyk_intent.rs`,
    // which owns the conversion's numbers.
    let srgb = probe.srgb.expect("the raster always has a colour here");
    assert!(
        srgb[1] > srgb[0] && srgb[1] > srgb[2],
        "0.75 0 1 0 is a green; got {srgb:?}"
    );
}

/// The control. Same paint, same geometry, no page group — so no colorant
/// buffer ever exists, and the probe says so rather than manufacturing four
/// numbers by running the sRGB result backwards.
///
/// That reconstruction is available (`overprint::rgb_to_cmyk`) and would have
/// filled the fields convincingly. It is refused because it is a **different
/// quantity**: a max-GCR reconstruction of the output, not a reading of a
/// composite that never happened. Decision 098 is the standing example of what
/// that substitution costs — yellow `1.0` arriving as `0.59` and a black of
/// `0.29` invented out of nothing.
#[test]
fn a_page_composited_on_screen_reports_no_colorants_rather_than_reconstructing_them() {
    let options = RenderOptions::default().with_ink_probe(PROBE_XY.0, PROBE_XY.1);
    let page = render("flat-cmyk-additive.pdf", &options);

    assert!(
        !page.diagnostics.cmyk_buffer_engaged,
        "the fixture declares no page group, so there is nothing to composite in ink"
    );
    let probe = page.diagnostics.ink_probe.expect("a probe was requested");
    assert_eq!(probe.source, InkProbeSource::ScreenSrgb);
    assert_eq!(probe.cmyk, None, "there was no colorant buffer to read");
    assert_eq!(probe.alpha, None, "likewise");
    assert!(
        probe.srgb.is_some(),
        "the raster exists either way, so its colour is always reportable"
    );
}

/// A probe outside the raster is a question with no answer, not a reason to
/// withhold the operator's page.
///
/// The raster's size is not known until the page geometry has been resolved —
/// `--region`, `--scale` and the `/MediaBox` decide it between them — so this
/// cannot be validated when the coordinate is parsed, and turning it into a
/// hard error would mean a diagnostic could destroy the output it was asked
/// about.
#[test]
fn a_probe_outside_the_raster_is_reported_and_the_page_still_renders() {
    let options = RenderOptions::default().with_ink_probe(100_000, 7);
    let page = render("flat-cmyk-subtractive.pdf", &options);

    let probe = page.diagnostics.ink_probe.expect("a probe was requested");
    assert_eq!(probe.source, InkProbeSource::OutOfRange);
    assert_eq!(probe.cmyk, None);
    assert_eq!(probe.srgb, None);
    assert!(
        page.pixmap.width() > 0 && page.pixmap.height() > 0,
        "the page renders regardless"
    );
}

/// Nobody asked, so nothing is reported. Pins that a caller cannot read a
/// probe that was never requested — which is what keeps `Option` meaningful
/// rather than a field that is always `Some` with a sentinel inside it.
#[test]
fn no_probe_is_reported_when_none_was_asked_for() {
    let page = render("flat-cmyk-subtractive.pdf", &RenderOptions::default());
    assert_eq!(page.diagnostics.ink_probe, None);
}
