//! # A `DeviceGray` fill overprinting a spot backdrop — a DIVERGENCE from
//! ISO 32000-1 toward Acrobat (`Pass 143.0`), offered as a setting
//!
//! ## ★★ THIS HEADER SAID "the §8.6.7 ambiguity" AND "both defensible
//! readings" UNTIL `Pass 174.6`, AND THAT WAS WRONG FOR ISO 32000-1
//!
//! Struck rather than rewritten, because the mistake is the transferable
//! part: ~~"Both are defensible, so pdfcer ships both"~~. **Three provisions
//! settle it against the convert-first reading** (spec register `OP-A5`):
//!
//! 1. §8.6.7's **next sentence**: *"shall not apply … to any colours that are
//!    the result of a computation, such as those in a shading pattern **or
//!    conversions from some other colour space**."* A `DeviceGray` →
//!    `DeviceCMYK` map is exactly that, and §10.3.3 gives the arithmetic as a
//!    `shall`.
//! 2. **Tables 148/149 row 2** tabulate *"any process colour space (including
//!    other cases of `DeviceCMYK`)"* × process colorant × `OP true, OPM 1` =
//!    **"Paint source"**, identical to the `OPM 0` column. The standard did
//!    not omit non-CMYK process spaces; it enumerated them.
//! 3. §8.6.7's escape hatch — *"or is implicitly converted to `DeviceCMYK`;
//!    see 8.6.5.7"* — points at **"Implicit Conversion of CIE-Based Colour
//!    Spaces"**, CIE-based and nothing else. **This third ground is the only
//!    one the old header had**, and alone it does read like a silence, which
//!    is how the error was made.
//!
//! ★ **ISO 32000-2 DELETES the first two**, so the question really is open
//! there. The behaviour is **edition-gated**, and a test file about it has to
//! say which edition it is talking about.
//!
//! So: the **literal** (and, under 1.7, the **conforming**) reading is that
//! `OPM 1` does not reach a `DeviceGray` source — the fill writes all four
//! components and knocks a spot backdrop out. **Acrobat** converts grey to
//! K-only `DeviceCMYK` first and then applies `OPM 1`, preserving the
//! backdrop's C, M and Y. **pdfcer defaults to Acrobat's and therefore
//! DIVERGES**, deliberately: this is a print-conformance axis whose
//! measurement instrument is authored to press behaviour. That is a choice
//! the operator can reverse with `device_cmyk_only`, and it owes them a
//! disclosure that an ambiguity would not.
//!
//! ★★★ **AND THESE FIXTURES CANNOT DISCRIMINATE THE SETTING IF THE BACKDROP
//! IS A SPOT.** Tables 148/149 put *"any process colour space"* × **spot**
//! colorant × `OP true` at `c_b` — *do not paint* — in **both** overprint-mode
//! columns, so a conforming engine preserves that backdrop whichever way the
//! setting is read (spec register `OP-N3`). pdfcer's settings differ here only
//! because **pdfcer flattens a spot into C, M and Y**, leaving no spot colorant
//! for that row to protect. The difference these tests observe is real; its
//! cause is pdfcer's representation, and **it will change when the n-colorant
//! buffer lands** — at which point these assertions are expected to move and
//! should be re-derived rather than patched.
//!
//! ## ★★★ Why these tests exist rather than a conformance-suite run
//!
//! Because the licensed corpus **cannot score the case**, though it is
//! touched by it. Measured 2026-08-28 on `4094e49` by rendering all 51 of its
//! patches twice with the shipped binary — `--overprint-zero-tint-scope
//! device_cmyk_only`, which reproduces the pre-Pass behaviour exactly, against
//! the default — and counting differing pixels: **3 of 51 change (8,491 /
//! 1,827 / 804 px), and 0 of those 3 change verdict.**
//!
//! So without these fixtures the setting would be correct, wired, documented
//! and **unexercised by any test in the repository** (`R151`) — the corpus
//! would move under it and report nothing.
//!
//! ### ★★ This paragraph first said the corpus was BLIND, and that was wrong
//!
//! It read: *"zero paint a `DeviceGray` source through Table 149; the patch
//! whose name promises that authors its greys as `DeviceCMYK [0 0 0 k]`."*
//!
//! **That scan ran between the two halves of the fix** — after the `classify`
//! change, before the `overprint_would_change` repair that is *what lets grey
//! sources reach Table 149 at all*. So it measured the **defect**, not the
//! fix, and was quoted in the file documenting the fix.
//!
//! ⇒ **The numbers most at risk of going stale are the ones gathered while
//! the thing they measure is being changed.** Prefer the differential form
//! (render twice, diff) for any figure that will outlive the work: it
//! re-measures itself on whatever tree it is run against.
//!
//! And the second clause was simply false. Reading that patch's content
//! streams shows it paints the same 50 % grey **both ways** — `0.5 g` *and*
//! `0 0 0 0.5 k` — deliberately, so an engine that treats them differently is
//! caught. That is the identical comparison
//! `grey_matches_the_cmyk_k_only_reference_exactly` makes below; the suite
//! made it first, and pdfcer could not see it.
//!
//! ## ★★ And the route the filed diagnosis named contributed 0 %
//!
//! `Pass 143.0` was filed against `overprint::classify` mapping `DeviceGray`
//! to `SourceKind::OtherProcess`, whose Table 149 row is `[Source; 4]`.
//! Changing that alone moved **zero pixels** — on these fixtures and on all
//! 51 corpus patches — because `Interpreter::overprint_would_change` returned
//! `false` for `DeviceGray`, so the paint never reached `paint_overprint`,
//! never reached `classify`, and was painted normally.
//!
//! That predicate carried a comment calling its `_ => false` arm *"a known
//! under-count rather than a claim of zero"* — true of the **disclosure**, and
//! the sentence did not say that the same arm also gated the **behaviour**.
//! Only an A/B of rendered pixels separated the two routes (`R219`): the
//! classification change compiled, was reached, looked correct, and did
//! nothing.
//!
//! Fixture provenance: `fixtures/synthetic/overprint/PROVENANCE.md`;
//! generator `tools/gen-grey-overprint-fixtures.py`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_core::settings::OverprintZeroTintScope;
use pdfcer_render::{RenderOptions, RenderedPage};

/// 1.0 keeps the 200 × 200 pt page a 200 × 200 raster, so the sampled point
/// below is a whole device pixel and no rounding judgement enters any
/// assertion.
const SCALE: f32 = 1.0;

fn render(name: &str, scope: OverprintZeroTintScope) -> RenderedPage {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/overprint")
        .join(name);
    let doc =
        Document::from_bytes(std::fs::read(&path).expect("fixture file")).expect("fixture parses");
    let pages = page_tree::pages(&doc).expect("page tree");
    let opts = RenderOptions::default().with_overprint_zero_tint_scope(scope);
    pdfcer_render::render_page_with(&doc, &pages[0], SCALE, &opts).expect("renders")
}

fn px(page: &RenderedPage, x: u32, y: u32) -> (u8, u8, u8) {
    let p = page
        .pixmap
        .pixels()
        .get((y * page.pixmap.width() + x) as usize)
        .expect("pixel in range");
    (p.red(), p.green(), p.blue())
}

/// A point well inside the 80 × 80 pt mark, which sits on the 120 × 120 pt
/// spot backdrop. Both fixtures place the mark identically, so one coordinate
/// serves every test.
fn mark(page: &RenderedPage) -> (u8, u8, u8) {
    // The page is 200 pt tall and the mark spans y = 60..140 in PDF space;
    // device y is flipped, so device y = 100 is the mark's middle either way.
    px(page, 100, 100)
}

/// The spot ink is deliberately **chromatic** (C=0.8 M=0.2 Y=0.9 K=0,
/// green-dominant). A neutral backdrop would make "preserved" and "knocked
/// out" differ only in lightness, which a rounding change could imitate.
fn is_greenish(c: (u8, u8, u8)) -> bool {
    c.1 > c.0 && c.1 > c.2
}

/// The "knocked out" signature: the grey covered the spot, so what is left is
/// *achromatic ink* — equal C, M and Y.
///
/// # ★ Why this is a tolerance and not an equality
///
/// It was `(r - g).abs() <= 1`, and that held until `Pass 153.0` moved the
/// shipped `CmykIntent` from `NeutralBlack` to `Calibrated` on the operator's
/// ruling. `NeutralBlack` forces an achromatic CMYK to an exactly neutral
/// sRGB; `Calibrated` — which is what Acrobat and pdfium produce — renders it
/// **slightly cool**, and that is its whole documented consequence.
///
/// So a correct knocked-out grey is now `(147, 148, 152)`, a spread of 5, and
/// the three tests using this helper failed on the *colour default changing*
/// rather than on anything about overprint.
///
/// The claim worth keeping is that the spot's **hue is gone**, not that the
/// result is bit-neutral. A green backdrop surviving would show a spread in
/// the tens with green dominant; 16 admits the calibrated coolness and
/// nothing that could be mistaken for ink.
fn is_neutral(c: (u8, u8, u8)) -> bool {
    let (r, g, b) = (i32::from(c.0), i32::from(c.1), i32::from(c.2));
    let spread = r.max(g).max(b) - r.min(g).min(b);
    spread <= 16 && !is_greenish(c)
}

// ---------------------------------------------------------------------------
// 1. THE ORACLE-FREE CLAIM — the strongest assertion in this file
// ---------------------------------------------------------------------------

/// ★ Needs no reference render, no remembered colour and no threshold.
///
/// `grey_op_over_spot.pdf` and `cmyk_k_op_over_spot.pdf` differ in exactly one
/// way: one says `0.5 g`, the other says `0 0 0 0.5 k`. They are the same ink
/// stated two ways. *"Treat grey as the K-only CMYK it converts to"* means
/// precisely that they must land on **identical pixels** — so the setting's
/// whole meaning is checkable by comparing pdfcer against itself.
///
/// A memorised expected colour can be memorised wrong (`R215`); this cannot.
#[test]
fn grey_matches_the_cmyk_k_only_reference_exactly() {
    let grey = render("grey_op_over_spot.pdf", OverprintZeroTintScope::GreyAsKOnly);
    let cmyk = render(
        "cmyk_k_op_over_spot.pdf",
        OverprintZeroTintScope::GreyAsKOnly,
    );
    assert_eq!(
        mark(&grey),
        mark(&cmyk),
        "a 0.5 grey and a 0 0 0 0.5 CMYK are the same ink; under GreyAsKOnly \
         they must composite identically"
    );
    assert!(
        is_greenish(mark(&grey)),
        "and the shared result must be the PRESERVED spot, not a shared \
         failure to paint: got {:?}",
        mark(&grey)
    );
}

// ---------------------------------------------------------------------------
// 2. THE SETTING MOVES THE PIXEL, IN THE DIRECTION EACH READING PREDICTS
// ---------------------------------------------------------------------------

/// ★★ THIS TEST WAS `the_literal_reading_knocks_the_spot_out`, AND THAT
/// EXPECTATION WAS PDFCER'S REPRESENTATION, NOT THE STANDARD'S (`Pass 238.0`).
///
/// `OverprintZeroTintScope`'s own docs said so before the change: *"a
/// conforming engine preserves that spot backdrop whichever way this setting
/// is read. The reason pdfcer's two settings differ here at all is that pdfcer
/// flattens a spot into C, M and Y … it will change when the n-colorant
/// buffer lands."* It has landed. Table 149's *"any process colour space ×
/// spot colorant"* row is `c_b` under `OP true` in BOTH mode columns, with no
/// scope in sight — so under the literal reading the grey now paints all four
/// PROCESS components (the reading's actual content) and the spot survives in
/// its own plane. What the two readings disagree about is the process
/// channels, and `grey_over_a_process_backdrop_separates_the_two_readings`
/// is where that is pinned.
#[test]
fn the_literal_reading_paints_the_process_channels_and_the_spot_survives() {
    let page = render(
        "grey_op_over_spot.pdf",
        OverprintZeroTintScope::DeviceCmykOnly,
    );
    assert!(
        is_greenish(mark(&page)),
        "§8.6.7 to the letter governs the four process components; the spot \
         plane is Table 149's `c_b` under every scope. Expected the green spot \
         to survive under the grey, got {:?}",
        mark(&page)
    );
}

#[test]
fn the_default_reading_preserves_the_spot() {
    let page = render("grey_op_over_spot.pdf", OverprintZeroTintScope::GreyAsKOnly);
    assert!(
        is_greenish(mark(&page)),
        "the default reading: grey converts to K-only CMYK, whose zero C, M \
         and Y leave the backdrop standing. Expected the green spot to \
         survive, got {:?}. (This message said \"Acrobat's reading\" until \
         Pass 206.0 refuted that on process geometry -- fourteen lines below \
         the doc comment carrying the refutation, in the file that Pass \
         edited. Renaming the test swept the NAME; the assertion message is a \
         second copy of the same claim and survived it.)",
        mark(&page)
    );
}

/// The shipped default must BE `GreyAsKOnly`, not merely reachable. Asserted
/// against `Default::default()` rather than against the named variant, so
/// flipping the `#[default]` attribute fails here rather than silently
/// changing what every consumer renders.
///
/// # ★★ THIS TEST WAS CALLED `the_shipped_default_is_the_acrobat_reading`,
/// AND IT COULD NOT CHECK THAT
///
/// The body is unchanged. Only the name and this comment are, because the
/// name asserted something the assertions do not reach — and which has since
/// been measured to be **false**.
///
/// Over a SPOT backdrop, which is the only geometry this file had, the two
/// readings of `OverprintZeroTintScope` produce different pictures — so the
/// test looked discriminating — but neither picture says which reading
/// *Acrobat* uses. The project's own note `OP-N3` had already said so: "the
/// discriminating case is grey over PROCESS components". That case was named
/// as missing and then not built, and in the gap a test name became a claim
/// nobody could check.
///
/// It is now built (`grey_op_over_cmyk.pdf`, the test below), and the claim
/// is refuted: on a conformance patch of exactly that shape Acrobat renders
/// the backdrop REPLACED (255,255,255) where this default renders it
/// PRESERVED (142,198,63). The literal reading matches Acrobat; the default
/// does not.
///
/// ⇒ The default was deliberately left unchanged at the time, as a
/// sequencing decision rather than an endorsement: flipping it alone was
/// trap-neutral on the conformance corpus, because it corrected one cell and
/// broke another that passed only through a compensating error (pdfcer
/// flattened the spot into C/M/Y, and the wrong row assignment then happened
/// to preserve exactly those planes). The honest fix was the literal row
/// assignment **together with** the per-spot-colorant plane.
///
/// # ★★★ FLIPPED in `Pass 244.0` (2026-09-03)
///
/// The plane landed (`Pass 238.0`/`239.0`) and the literal reading was
/// re-measured on the whole sweep: 0 FAIL / 43 pass, against 2 FAIL / 41
/// pass for `GreyAsKOnly` — the two failures being the grey-over-process
/// cells this file's discriminating test below describes. So the default is
/// now `DeviceCmykOnly`, and this test pins THAT, still against
/// `Default::default()` so that flipping the attribute back fails here.
///
/// The spot-backdrop fixture is the right one to pin it on: under a spot
/// plane BOTH readings preserve a spot backdrop (`OP-N3`), so the default
/// must still render this page greenish — a regression that knocked the
/// spot out would fail the second assertion whichever reading was default.
#[test]
fn the_shipped_default_is_device_cmyk_only() {
    let explicit = render(
        "grey_op_over_spot.pdf",
        OverprintZeroTintScope::DeviceCmykOnly,
    );
    let defaulted = render("grey_op_over_spot.pdf", OverprintZeroTintScope::default());
    assert_eq!(mark(&explicit), mark(&defaulted));
    assert!(
        is_greenish(mark(&defaulted)),
        "with a spot plane the literal reading still preserves a spot backdrop: {:?}",
        mark(&defaulted)
    );
    // And the discriminating geometry: under the default a white mark over a
    // PROCESS backdrop replaces it, as the reference does.
    let process = mark(&render(
        "grey_op_over_cmyk.pdf",
        OverprintZeroTintScope::default(),
    ));
    assert!(
        is_neutral(process),
        "the default must be the literal reading on process geometry; got {process:?}"
    );
}

/// ★★★ THE DISCRIMINATING CASE: grey over a PROCESS backdrop.
///
/// This is the geometry `OP-N3` named as the one that can tell the two
/// readings apart *against a reference*, and which did not exist until now.
/// Every other fixture in this file paints over a spot, where both readings
/// are defensible.
///
/// # What each reading does, and why the values are not arbitrary
///
/// The backdrop is `0.5 0 1 0 k`; the mark is `1 g` — white — under
/// `/OP true /OPM 1`.
///
/// | reading | Table 149 row | result |
/// |---|---|---|
/// | `GreyAsKOnly` | row 1, whose OPM-1 cell is value-dependent (`c_b` where `c_s = 0`) | backdrop SURVIVES, `142,198,63` |
/// | `DeviceCmykOnly` | row 2, "any process colour space" — `c_s` in all three columns | backdrop REPLACED, `255,255,255` |
///
/// **`1 g` is chosen deliberately over `0.5 g`.** White converts to
/// `0 0 0 0` under every CMYK profile, so no "it converted differently"
/// explanation can survive this comparison — the only thing that can produce
/// two different pictures here is the row assignment.
///
/// # The ground truth, which this test records but cannot assert
///
/// Acrobat renders this shape `255,255,255`. That was measured on a licensed
/// conformance patch which cannot be checked into this repository, so the
/// number lives in this comment rather than in an assertion, and the test
/// asserts only what pdfcer does. Both synthetic values above match that
/// patch's cells exactly, which is what makes this fixture a faithful stand-in
/// rather than an approximation of one.
#[test]
fn grey_over_a_process_backdrop_separates_the_two_readings() {
    let k_only = mark(&render(
        "grey_op_over_cmyk.pdf",
        OverprintZeroTintScope::GreyAsKOnly,
    ));
    let literal = mark(&render(
        "grey_op_over_cmyk.pdf",
        OverprintZeroTintScope::DeviceCmykOnly,
    ));
    assert!(
        is_greenish(k_only),
        "GreyAsKOnly routes a DeviceGray source through Table 149 row 1, whose \
         OPM-1 cell hands back the backdrop where the source component is zero, \
         so the process backdrop must SURVIVE. Got {k_only:?}"
    );
    assert!(
        is_neutral(literal),
        "DeviceCmykOnly puts a DeviceGray source in row 2, which is c_s in all \
         three columns, so a white mark must REPLACE the backdrop. Got \
         {literal:?}"
    );
    assert_ne!(
        k_only, literal,
        "★ if these ever agree, this fixture has stopped discriminating and \
         every claim about which reading matches the reference is unchecked \
         again — which is the exact state this file was in before it existed"
    );
}

// ---------------------------------------------------------------------------
// 3. THE TWO CONTROLS — what must NOT move, which is how over-breadth shows
// ---------------------------------------------------------------------------

/// Overprint is **off**. §8.6.7 does not apply at all, so no value of this
/// setting may touch the result. A fix that moves this pixel is reaching
/// paints it has no business reaching.
#[test]
fn overprint_off_is_untouched_by_every_scope() {
    let a = render(
        "grey_noop_over_spot.pdf",
        OverprintZeroTintScope::DeviceCmykOnly,
    );
    let b = render(
        "grey_noop_over_spot.pdf",
        OverprintZeroTintScope::GreyAsKOnly,
    );
    let c = render(
        "grey_noop_over_spot.pdf",
        OverprintZeroTintScope::AllProcessSpaces,
    );
    assert_eq!(mark(&a), mark(&b));
    assert_eq!(mark(&b), mark(&c));
    assert!(
        is_neutral(mark(&a)),
        "with overprint off the grey simply covers the spot: {:?}",
        mark(&a)
    );
}

/// The property: a grey **image** is unaffected by every scope. Table 149
/// gives the direct-CMYK row the qualifier *"and not in a sampled image"*, so
/// a CMYK image already falls to the process row where `OPM 0` and `OPM 1`
/// are identical, and a grey image is that case's analogue.
///
/// # ★★ WHAT THIS TEST DOES **NOT** PIN, established by sabotage
///
/// Its first version claimed to pin the `!in_image_sample` guard in
/// `overprint::classify` — *"if that guard is ever removed, this test fails
/// rather than the comment quietly going stale."* **That claim was false, and
/// three separate sabotages proved it:** removing the guard, widening
/// `GreyAsKOnly` to every space, and changing both image call sites' literal
/// `DeviceCmykOnly` to `AllProcessSpaces` each left this test GREEN.
///
/// The reason is that a grey image never enters the overprint machinery at
/// all under any scope — there are **three** redundant things stopping it, so
/// disabling any one changes nothing. The test therefore verifies a true and
/// useful END-TO-END property (a grey image does not move) while pinning
/// **none** of the individual mechanisms it names.
///
/// ★ That distinction is the point of writing it down rather than deleting
/// the test. A surviving sabotage does not always mean the test is weak; here
/// it meant the **comment's claim about coverage** was wrong. The test stays,
/// with an honest description of its own reach.
#[test]
fn a_grey_image_is_never_upgraded_whatever_the_scope() {
    let a = render(
        "grey_image_op_over_spot.pdf",
        OverprintZeroTintScope::DeviceCmykOnly,
    );
    let b = render(
        "grey_image_op_over_spot.pdf",
        OverprintZeroTintScope::GreyAsKOnly,
    );
    let c = render(
        "grey_image_op_over_spot.pdf",
        OverprintZeroTintScope::AllProcessSpaces,
    );
    assert_eq!(mark(&a), mark(&b), "GreyAsKOnly must not reach an image");
    assert_eq!(
        mark(&b),
        mark(&c),
        "and neither must AllProcessSpaces — the guard is on the image, not \
         on the space"
    );
    // ★ `Pass 238.0`: this asserted `is_neutral` — "the grey image covers
    // the spot under every scope" — and that was the missing spot plane on
    // the image path, counted for a Pass as
    // `overprint_process_images_unsupported` and then fixed. A grey IMAGE
    // under `/OP true` writes its process channels and leaves every spot
    // plane to the backdrop (Table 149, process source × spot colorant ⇒
    // `c_b`), exactly as the grey FILL beside it does. The property this
    // test exists for — scope-independence — is unchanged and still pinned
    // by the two equalities above.
    assert!(
        is_greenish(mark(&a)),
        "the grey image leaves the spot standing under every scope: {:?}",
        mark(&a)
    );
}

// ---------------------------------------------------------------------------
// 4. THE SCOPES ARE DISTINGUISHABLE — without this, three names for one thing
// ---------------------------------------------------------------------------

/// ★★ The test that makes `AllProcessSpaces` a real value rather than a
/// synonym.
///
/// Found by sabotage: widening `GreyAsKOnly` to match **every** space left
/// the entire suite green, because no fixture put a non-grey process source
/// over a spot backdrop. A setting whose values cannot be told apart by any
/// test is three names for one behaviour, and nothing would have caught a
/// later change collapsing them.
///
/// Pure red converts to `C=0, M=1, Y=1, K=0`, so exactly one component is
/// zero and the backdrop's **cyan** is what is at stake. Under
/// `AllProcessSpaces` it survives; under the other two it does not.
/// ★ `Pass 238.0` moved this test from `rgb_op_over_spot.pdf` to
/// `rgb_op_over_cmyk.pdf`. Over a SPOT backdrop the three scopes now render
/// identically — correctly: the spot lives in its own plane and is preserved
/// under every scope, and a pure spot states no process ink, so there is no
/// cyan in C for `AllProcessSpaces` to preserve differently. `OP-N3` said the
/// discriminating case is over PROCESS components; `rgb_op_over_spot`'s new
/// job is `an_rgb_source_leaves_the_spot_standing_under_every_scope`.
#[test]
fn all_process_spaces_reaches_rgb_and_the_narrower_scopes_do_not() {
    let literal = render(
        "rgb_op_over_cmyk.pdf",
        OverprintZeroTintScope::DeviceCmykOnly,
    );
    let grey_only = render("rgb_op_over_cmyk.pdf", OverprintZeroTintScope::GreyAsKOnly);
    let all = render(
        "rgb_op_over_cmyk.pdf",
        OverprintZeroTintScope::AllProcessSpaces,
    );

    assert_eq!(
        mark(&literal),
        mark(&grey_only),
        "GreyAsKOnly must NOT reach a DeviceRGB source — if this fails, the scope has been widened and the three values have collapsed into two"
    );
    assert_ne!(
        mark(&grey_only),
        mark(&all),
        "AllProcessSpaces must reach it, or the widest scope is unreachable and the enum has a variant that does nothing"
    );
    // And the direction: the preserved cyan pulls the red toward the spot.
    let (r0, _, _) = mark(&grey_only);
    let (r1, _, _) = mark(&all);
    assert!(
        r1 < r0,
        "preserving the backdrop's cyan must DARKEN the red, not lighten it:          {:?} -> {:?}",
        mark(&grey_only),
        mark(&all)
    );
}

/// The spot half of the same fixture family (`Pass 238.0`): a `DeviceRGB`
/// source under `/OP true` over a spot backdrop leaves the spot's plane
/// standing under all three scopes, and the three agree with each other. The
/// scope governs the process channels; it never reaches a spot plane.
#[test]
fn an_rgb_source_leaves_the_spot_standing_under_every_scope() {
    let a = render(
        "rgb_op_over_spot.pdf",
        OverprintZeroTintScope::DeviceCmykOnly,
    );
    let b = render("rgb_op_over_spot.pdf", OverprintZeroTintScope::GreyAsKOnly);
    let c = render(
        "rgb_op_over_spot.pdf",
        OverprintZeroTintScope::AllProcessSpaces,
    );
    assert_eq!(mark(&a), mark(&b));
    assert_eq!(mark(&b), mark(&c));
    // Red ink laid over green ink multiplies to near-black (§10.8.3 step
    // (c)): measured (34, 17, 12). A KNOCKED-OUT spot would leave the red
    // alone at its saturated (237, 28, 36). So the red channel is the
    // witness: dark means the green survived underneath.
    let (r, _, _) = mark(&a);
    assert!(
        r < 120,
        "the spot plane must survive an overprinting RGB source, darkening the red through the green it sits on: {:?}",
        mark(&a)
    );
}
