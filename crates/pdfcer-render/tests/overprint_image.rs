//! # An overprinting image and an overprinting rectangle must agree
//!
//! End-to-end evidence for `Pass 130.2`: ISO 32000-1 §11.7.4.3's
//! `CompatibleOverprint` applied to a **sampled image**, which
//! `Canvas::fill_image_overprint` composites and which nothing composited
//! before.
//!
//! # What was wrong, in one sentence
//!
//! `overprint::composite` had exactly one call site — the path and glyph
//! painter — so an image XObject reached the destination through an ordinary
//! paint no matter what `/OP` said, and **Table 149's third row was never
//! consulted for one**. That row is the only one that is not inert for an
//! image: a process component the source space does not name takes `c_b`
//! under `OP true`, so an overprinting `/DeviceN` image must leave the
//! backdrop standing where it claims nothing.
//!
//! # The oracle, and why it is a PAIR rather than a constant
//!
//! §11.7.4.3 is written about a source **colour**, not a source **object**.
//! Table 149's rows are colour spaces; nothing in the clause distinguishes a
//! rectangle from a raster. So an overprinting `/DeviceN [/Black]` rectangle
//! and an overprinting `/DeviceN [/Black]` image *of the same tint, over the
//! same backdrop, at the same coordinates* must land on identical pixels —
//! and that is checkable without a reference render, without pdfium, and
//! without a number anybody had to remember.
//!
//! Before `Pass 130.2` they were 255 levels apart in two channels: the
//! rectangle came out cyan and the image came out white. **Nothing but the
//! comparison could have said which one was wrong** (`R215` — a remembered
//! expected colour is the wrong-oracle shape).
//!
//! The print-conformance suite *did* adjudicate it — three of its patches
//! went FAIL → pass on this change — but that suite is not redistributable,
//! is not in this repository, and is not on CI. This file is the half that
//! travels.
//!
//! # The four-way signature
//!
//! A correct implementation has a *shape* here, not just a set of passing
//! assertions. Every one of these would be satisfied by a renderer that had
//! simply stopped painting images, or by one that ignored `/OP` entirely, if
//! taken alone:
//!
//! | comparison | expected | what a violation would mean |
//! |---|---|---|
//! | image `/OP true` vs path `/OP true`, subtractive | **identical** | the two call sites disagree — the defect itself |
//! | image `/OP true` vs path `/OP true`, additive | **identical** | the sRGB arm and the colorant arm disagree |
//! | image `/OP true` vs image `/OP false` | **differ** | overprint is being ignored for images |
//! | image `/OP true` naming all four colorants vs `/OP false` | **identical** | Table 149's inertness for a fully-specifying source is not honoured |
//!
//! # What the fixtures are
//!
//! Six single-page PDFs in `fixtures/synthetic/overprint/`, each 100 × 100 pt,
//! each painting a 40 × 40 pt mark at (30, 30) over an 80 × 80 pt `1 0 0 1 k`
//! backdrop — 100 % cyan **and** 100 % black. Construction, and why the
//! colorant is `/Black` rather than the more obvious `/Cyan`:
//! `tools/gen-overprint-image-fixtures.py` and that directory's
//! `PROVENANCE.md`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::RenderedPage;

/// Render scale. 2.0 gives a 200 × 200 raster for a 100 × 100 pt page, so
/// every sampled region below is a whole number of device pixels and no
/// rounding judgement enters the assertions.
const SCALE: f32 = 2.0;

fn render(name: &str) -> RenderedPage {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/overprint")
        .join(name);
    let doc =
        Document::from_bytes(std::fs::read(&path).expect("fixture file")).expect("fixture parses");
    let pages = page_tree::pages(&doc).expect("page tree");
    pdfcer_render::render_page(&doc, &pages[0], SCALE).expect("renders")
}

/// The RGB of one device pixel.
fn px(page: &RenderedPage, x: u32, y: u32) -> (u8, u8, u8) {
    let p = page
        .pixmap
        .pixels()
        .get((y * page.pixmap.width() + x) as usize)
        .expect("pixel in range");
    (p.red(), p.green(), p.blue())
}

/// A pixel well inside the 40 × 40 pt mark.
///
/// The mark occupies user `(30, 30)`–`(70, 70)`, i.e. device `(60, 60)`–
/// `(140, 140)` at `SCALE`, with device *y* measured from the top of a
/// 200-pixel page — which for a square centred on the page is the same
/// interval either way. `(100, 100)` is its centre and is 40 pixels from
/// every edge, so nothing about anti-aliasing or resampling reaches it.
fn inside_mark(page: &RenderedPage) -> (u8, u8, u8) {
    px(page, 100, 100)
}

/// A pixel on the backdrop but outside the mark — device `(40, 40)` is
/// inside the `(20, 20)`–`(180, 180)` backdrop and 20 pixels clear of the
/// mark's edge.
///
/// Sampled in every test that samples the mark, deliberately: a renderer
/// that painted nothing at all would satisfy an equality between two marks
/// while leaving both white, and the backdrop check is what refuses that.
fn on_backdrop(page: &RenderedPage) -> (u8, u8, u8) {
    px(page, 40, 40)
}

/// White paper. Named because three assertions below turn on the difference
/// between "the backdrop survived" and "everything was knocked out", and
/// `(255, 255, 255)` written inline reads as a magic number.
const PAPER: (u8, u8, u8) = (255, 255, 255);

/// ★ THE LOAD-BEARING TEST. An overprinting image and an overprinting
/// rectangle of the same `/DeviceN [/Black]` colour must be the same pixels.
///
/// On a page whose group colour space is `/DeviceCMYK`, so the composite
/// runs in the four-colorant buffer and the backdrop's component split is
/// **remembered** rather than reconstructed.
#[test]
fn image_and_path_agree_under_overprint_on_a_subtractive_page() {
    let image = render("devicen_op_cmyk.pdf");
    let path = render("devicen_op_path_cmyk.pdf");

    assert_eq!(
        inside_mark(&image),
        inside_mark(&path),
        "an overprinting /DeviceN image and an overprinting /DeviceN rectangle \
         of the same tint over the same backdrop must render identically -- \
         §11.7.4.3 is written about a source COLOUR, not a source OBJECT"
    );
    // The backdrop is 100% cyan + 100% black. `/DeviceN [/Black]` names K
    // and nothing else, so K takes the source's zero and C survives:
    // the mark must be CYAN, and in particular must NOT be white (which is
    // what a knocked-out backdrop looks like) and must not still be the
    // backdrop's near-black (which is what painting nothing looks like).
    let (r, g, b) = inside_mark(&image);
    assert!(
        r < 32 && g > 128 && b > 192,
        "the preserved cyan should dominate; got ({r}, {g}, {b})"
    );
    assert_ne!(inside_mark(&image), PAPER, "the backdrop was knocked out");
    assert_ne!(
        inside_mark(&image),
        on_backdrop(&image),
        "the image did not paint at all -- the mark is indistinguishable \
         from the backdrop around it"
    );
}

/// The same equality on an **additive** page, where the composite takes
/// `overprint::composite_varying` instead of the colorant buffer.
///
/// ★ The colour here is NOT the cyan of the test above, and that is expected
/// rather than a defect. An additive canvas has no colorant planes, so the
/// backdrop's component split is reconstructed by `rgb_to_cmyk`, and
/// `100C + 100K` reconstructs as `C = 1, M = 0.914, Y = 0, K = 0.863` — the
/// same colour by a different route. Preserving that reconstructed `M` is
/// what turns the result blue.
///
/// The test asserts the **symmetry** and not the colour, because the
/// symmetry is the contract: the two arms of one clause must not disagree,
/// whatever the shared answer turns out to be. Pinning the colour instead
/// would freeze an approximation that the per-colorant buffer is expected to
/// improve.
#[test]
fn image_and_path_agree_under_overprint_on_an_additive_page() {
    let image = render("sep_devicen_op_rgb.pdf");
    let path = render("devicen_op_path_rgb.pdf");

    assert_eq!(
        inside_mark(&image),
        inside_mark(&path),
        "the sRGB arm of CompatibleOverprint must give an image and a path \
         the same answer, exactly as the colorant arm does"
    );
    assert_eq!(
        on_backdrop(&image),
        on_backdrop(&path),
        "the two fixtures differ only in the mark; their backdrops must match"
    );
    assert_ne!(inside_mark(&image), PAPER, "the backdrop was knocked out");
}

/// The control that makes the test above a measurement rather than a
/// coincidence: the SAME image with `/OP false` must knock the backdrop out.
///
/// `SP-N2`: overprint OFF does not merely "not preserve" — it **erases** the
/// process colorants the source does not name, `c_s (= 0.0)`. The standard
/// states that twice and it is not a typo, so a zero-tint `/DeviceN [/Black]`
/// image with overprint off paints white.
#[test]
fn the_same_image_with_overprint_off_knocks_the_backdrop_out() {
    let on = render("devicen_op_cmyk.pdf");
    let off = render("devicen_noop_cmyk.pdf");

    assert_ne!(
        inside_mark(&on),
        inside_mark(&off),
        "if these agree, /OP is being ignored for images and the test above \
         proves nothing"
    );
    assert_eq!(
        inside_mark(&off),
        PAPER,
        "overprint OFF erases the colorants the source does not name \
         (Table 148, and Table 149's `c_s (= 0.0)` notation)"
    );
    assert_eq!(
        on_backdrop(&on),
        on_backdrop(&off),
        "the two fixtures differ only in /OP; outside the mark they must be \
         identical, and a difference there means the flag reached something \
         it should not have"
    );
}

/// A `/DeviceN` naming **every** process colorant is inert under overprint.
///
/// Table 149's third row preserves only the components the source space does
/// *not* name, so `[/Cyan /Magenta /Yellow /Black]` names them all and every
/// rule is `Source`. The render must therefore be indistinguishable from the
/// same image with `/OP false`.
///
/// ★ This is the assertion that stops the fix from over-applying. A renderer
/// that preserved the backdrop for *any* `/DeviceN` image would pass the two
/// equality tests above and fail this one.
#[test]
fn a_devicen_naming_all_four_colorants_is_inert_under_overprint() {
    let all4 = render("devicen_all4_op_cmyk.pdf");
    let off = render("devicen_noop_cmyk.pdf");

    assert_eq!(
        inside_mark(&all4),
        inside_mark(&off),
        "a source space that specifies every component of the group leaves \
         nothing for the backdrop to survive in; overprint must be a no-op"
    );
    assert_eq!(inside_mark(&all4), PAPER);
}

/// The disclosure, which is a separate obligation from the pixels
/// (project rule 4).
///
/// ★ `overprint_images_unsupported` CHANGED MEANING in `Pass 130.2` and now
/// counts a strictly smaller set — it used to count every image painted
/// under `/OP` whether or not anything was owed. Zero here is therefore a
/// *stronger* statement than zero would have been before: the composite was
/// owed and ran, rather than never having been offered.
///
/// `overprint_effective` is checked too, because a composite that ran while
/// the counter said nothing was affected would leave an operator reading a
/// page that overprinted with a diagnostic line saying it did not.
#[test]
fn the_counters_say_what_happened() {
    let on = render("devicen_op_cmyk.pdf");
    assert_eq!(
        on.diagnostics.overprint_images_unsupported, 0,
        "this image was owed CompatibleOverprint and got it"
    );
    assert_eq!(on.diagnostics.overprint_composited, 1);
    assert_eq!(on.diagnostics.overprint_effective, 1);
    assert_eq!(on.diagnostics.overprint_refused, 0);
    assert!(on.diagnostics.overprint_pixels > 0);

    // The inert case must not be counted as an effective overprint at all --
    // counting it would report a divergence where Table 149 promises none,
    // and an operator diffing this number between files would chase it.
    let all4 = render("devicen_all4_op_cmyk.pdf");
    assert_eq!(all4.diagnostics.overprint_effective, 0);
    assert_eq!(all4.diagnostics.overprint_composited, 0);
    assert_eq!(
        all4.diagnostics.overprint_images_unsupported, 0,
        "nothing was owed here, so nothing is missing -- this counter must \
         not fire for a source space that specifies every component"
    );
}

/// ★★★ A SPOT COLOUR MUST PUT INK ON THE SHEET ON BOTH KINDS OF PAGE.
///
/// `Pass 130.3`. This is not an overprint test at all — **both fixtures set
/// `/OP false`** — and it lives in this file because the defect and its cause
/// are one function away from everything above.
///
/// # What was wrong
///
/// `overprint::authored_tints` answers *"which **process** tints did this
/// source state?"* — Table 149's question. A spot colorant has no process
/// channel to state a tint into, so a `/Separation /SpotInk /DeviceCMYK`
/// source answers `[0, 0, 0, 0]`, **correctly**. `Interpreter::authored_cmyk`
/// was handing that same answer to the colorant buffer as the paint's
/// **colour**, where zero ink is blank paper.
///
/// So on a page whose group declares `/CS /DeviceCMYK`, a spot square
/// rendered **completely invisible** — with overprint off, with no
/// diagnostic, and with every counter reading green. The identical square on
/// an additive page rendered correctly, which is why nothing caught it: the
/// defect needs a page group to reproduce, and a page group is exactly what a
/// print-bound PDF carries and a hand-written fixture usually does not.
///
/// # The oracle, and why it is again a PAIR
///
/// Neither fixture's colour is asserted, because the two pages legitimately
/// disagree about it — the subtractive one crosses `CMYK → sRGB` on the way
/// out and the additive one does not. What they must not disagree about is
/// **whether the mark exists**. So the assertion is: both put ink down, both
/// put down *the same kind* of ink (the tint transform is the same function),
/// and neither is paper.
///
/// ★ Measured consequence, recorded because it looks like a regression and is
/// not: this fix moved two print-conformance patches CLOSER to Acrobat
/// (mean absolute distance 24.8 → 19.9 and 41.4 → 28.5) while *raising* the
/// suite's failure count. Both patches paint CMYK over a spot backdrop, and
/// while the spot was invisible there was nothing for the CMYK to wrongly
/// knock out — a white trap cross on white paper has no contrast, so the
/// detector could not fire. **They were passing because they were rendering
/// nothing.** Five of six cells on one of them were blank.
#[test]
fn a_spot_colour_paints_ink_on_a_subtractive_page_too() {
    let cmyk = render("spot_only_noop_cmyk.pdf");
    let rgb = render("spot_only_noop_rgb.pdf");

    let (cr, cg, cb) = inside_mark(&cmyk);
    let (rr, rg, rb) = inside_mark(&rgb);

    assert_ne!(
        inside_mark(&cmyk),
        PAPER,
        "a /Separation spot square painted NOTHING on a subtractive page. \
         `authored_tints` reports a spot as zero PROCESS tints -- true for \
         Table 149, and blank paper when used as a paint colour"
    );
    assert_ne!(
        inside_mark(&rgb),
        PAPER,
        "the additive control must paint too"
    );

    // Same ink, same tint transform, so the two must at least agree on which
    // channel dominates. Asserting the exact triple would freeze the
    // subtractive page's CMYK -> sRGB conversion, which is iccce's to improve.
    assert!(
        cg > cr && cg > cb && rg > rr && rg > rb,
        "both pages should render this spot green-dominant; \
         subtractive ({cr}, {cg}, {cb}) additive ({rr}, {rg}, {rb})"
    );
    // These fixtures carry the same `1 0 0 1 k` backdrop square as the rest of
    // the family, so outside the mark is dark cyan-black rather than paper.
    // Asserted from both ends: the mark must differ from the backdrop (or a
    // renderer that painted nothing at all and left the backdrop showing would
    // satisfy every assertion above), and the backdrop must itself be ink (or
    // one that flooded the page white would).
    assert_ne!(
        inside_mark(&cmyk),
        on_backdrop(&cmyk),
        "the spot mark is indistinguishable from the backdrop it sits on -- \
         which is what painting nothing looks like"
    );
    assert_ne!(inside_mark(&rgb), on_backdrop(&rgb));
    assert_ne!(on_backdrop(&cmyk), PAPER);
    assert_ne!(on_backdrop(&rgb), PAPER);
}
