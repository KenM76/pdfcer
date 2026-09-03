//! # The `DeviceCMYK` conversion knob is real, not a stored preference
//!
//! ★ **`CmykIntent` is NOT an ICC rendering intent, and this file used to be
//! titled as though it were.** It selects *which fitted lookup table* pdfcer's
//! interim `DeviceCMYK`→sRGB conversion uses, and it is per-invocation. The
//! PDF rendering intent — `/RI` in an `/ExtGState`, or the `ri` operator
//! (§8.6.5.8, §11.7.5.3) — is a **different thing that pdfcer does not carry
//! at all**: `ri` is an explicit no-op and `/RI` is never read. The two were
//! conflated in this title until 2026-08-25, when a sibling project nearly
//! designed an API against the confusion. See `docs/NEXT_SESSION.md` §4.
//!
//! ISO 32000-1 §8.6.4.4 defines `DeviceCMYK` and specifies **no**
//! conversion to a display's RGB — device colour spaces are
//! device-dependent by definition, and the standard's silence is by design
//! rather than by omission. Acrobat's own answer is a user-configurable
//! working-space profile. So there is no correct conversion to implement,
//! only a choice to make, which is exactly the shape the operator's
//! 2026-08-08 directive addresses: *"where standards are ambiguous those
//! should become settings that the user can choose direction one, with the
//! initial installed default as the best guess of what is usually
//! followed."*
//!
//! ## What this file is defending against
//!
//! **R83 — no affordance without capability.** A settings file that
//! accepts `cmyk_intent = neutral_black` and a renderer that ignores it is
//! worse than having no setting at all: the operator changes it, sees no
//! difference, and reasonably concludes pdfcer is broken rather than that
//! the knob is decorative. The `cmyk_to_srgb_with` unit tests prove the
//! *function* honours the intent; only a render proves the **pixels** do,
//! and the gap between those two is the entire distance the value has to
//! travel — `RenderOptions` → `Interpreter` → `Rgb::from_cmyk` for the
//! `k`/`K` operators, and a second, independent path through
//! `image::decode` → `Space::to_rgb` for `DeviceCMYK` image samples.
//!
//! Both paths are exercised here, because they are separately wired and
//! could separately rot. That the two must agree is not a new requirement
//! — `pdfcer-core::color`'s module docs already state that a filled
//! rectangle and an image of the "same" CMYK must not come out different
//! colours — but the intent gives them a *second* way to disagree.
//!
//! ## The specific number being pinned, and why it is startling
//!
//! Solid black ink is `0 0 0 1 k`. Under the shipped default it renders
//! `#231F20` — a warm near-black — because that is what the reference
//! renders, and pdfcer's default follows the reference. Under
//! `NeutralBlack` it renders `#000000`, which is what a CAD or engineering
//! drawing expects, every line of which is stroked in pure K.
//!
//! The test asserts the default is *not* pure black on purpose. An
//! assertion that "black is black" would pass on a renderer that had
//! silently reverted to the naive formula, which is the regression most
//! likely to happen and the hardest to notice by eye.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_core::settings::CmykIntent;
use pdfcer_render::{RenderOptions, RenderedPage, render_page_with};

/// Build an offset-consistent classic PDF from `(number, body)` pairs.
fn build(objects: &[(u32, &str)]) -> Vec<u8> {
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

/// A page whose whole area is one `k`-filled rectangle in the given CMYK.
fn page_filled_with(cmyk: &str) -> Vec<u8> {
    let content = format!("{cmyk} k 0 0 40 40 re f\n");
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 \
             /MediaBox [0 0 40 40] /Resources << >> >>",
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>"),
        (
            4,
            &format!(
                "<< /Length {} >>\nstream\n{content}endstream",
                content.len()
            ),
        ),
    ])
}

fn render(bytes: Vec<u8>, intent: CmykIntent) -> RenderedPage {
    let doc = Document::from_bytes(bytes).expect("fixture parses");
    let page = page_tree::pages(&doc).expect("page tree walks").remove(0);
    let options = RenderOptions::default().with_cmyk_intent(intent);
    render_page_with(&doc, &page, 1.0, &options).expect("render succeeds")
}

/// The centre pixel's RGB, well inside the filled rectangle.
///
/// Demultiplied rather than read raw: `tiny_skia` stores premultiplied
/// colour, and although every fill here is fully opaque, reading the raw
/// channels would make this helper quietly wrong the first time someone
/// reuses it on a page with alpha.
fn centre(rendered: &RenderedPage) -> (u8, u8, u8) {
    let pixmap = &rendered.pixmap;
    let px = pixmap
        .pixel(pixmap.width() / 2, pixmap.height() / 2)
        .expect("centre pixel is in bounds")
        .demultiply();
    (px.red(), px.green(), px.blue())
}

#[test]
fn the_shipped_default_renders_solid_black_ink_as_a_warm_near_black() {
    // Deliberately asserting that black is NOT #000000. The regression
    // this catches — a silent revert to the naive additive formula — would
    // sail past an assertion that merely said "dark".
    //
    // ★★ THIS TEST'S NAME WAS A LIE FOR TWENTY DAYS, AND NOTHING NOTICED.
    //
    // It says *the shipped default*, and from 2026-08-08 to 2026-08-28 the
    // shipped default was `NeutralBlack`, which renders `#000000` — the exact
    // opposite of what this asserts. It stayed green throughout because it
    // names `CmykIntent::Calibrated` explicitly rather than
    // `CmykIntent::default()`, so it was testing a variant while claiming to
    // test the default. A second test, `the_shipped_default_renders_pure_k_
    // _as_true_black`, was added beside it asserting the opposite under
    // `default()`, and the two coexisted without contradiction.
    //
    // `Pass 153.0` reversed the ruling — the default is `Calibrated` again —
    // so the name is true once more and the sibling test has been deleted
    // rather than inverted. Recorded because the failure mode is worth more
    // than the fix: **a test that hard-codes the value it calls "the default"
    // cannot detect the default moving**, and reads as coverage of exactly
    // the thing it has stopped covering. `the_default_render_options_carry_
    // _the_default_intent` below is the one that actually pins `default()`.
    let (r, g, b) = centre(&render(page_filled_with("0 0 0 1"), CmykIntent::Calibrated));
    assert!(
        (r, g, b) != (0, 0, 0),
        "the calibrated default must not render pure K as #000000"
    );
    assert!(
        (0x20..=0x27).contains(&r) && (0x1C..=0x23).contains(&g) && (0x1D..=0x24).contains(&b),
        "expected the reference's warm near-black around #231F20, got #{r:02X}{g:02X}{b:02X}"
    );
}

#[test]
fn neutral_black_renders_solid_black_ink_as_true_black() {
    // The whole point of the setting, and the reason a CAD operator would
    // ever touch it.
    assert_eq!(
        centre(&render(
            page_filled_with("0 0 0 1"),
            CmykIntent::NeutralBlack
        )),
        (0, 0, 0),
        "pure K under NeutralBlack must be #000000"
    );
}

#[test]
fn neutral_black_leaves_every_colour_that_is_not_pure_k_alone() {
    // The guard that keeps this a targeted fix rather than a second colour
    // model: a drawing's black lines go true black while a photograph on
    // the same page keeps its calibrated rendering.
    for cmyk in ["0.5 0.2 0.1 0.3", "1 0 0 0", "0 1 0 0.5", "0.1 0.1 0.1 1"] {
        let page = page_filled_with(cmyk);
        assert_eq!(
            centre(&render(page.clone(), CmykIntent::NeutralBlack)),
            centre(&render(page, CmykIntent::Calibrated)),
            "`{cmyk} k` is not on the pure-K axis and must be untouched by the intent"
        );
    }
}

#[test]
fn the_grey_ramp_is_neutral_under_neutral_black() {
    // 25% K measured (199, 200, 202) under the calibrated table — very
    // slightly cool. Under NeutralBlack it must be exactly neutral, which
    // is the other half of what a line-art operator is asking for.
    for (k, expected) in [(0.0_f32, 255_u8), (0.25, 191), (0.5, 128), (1.0, 0)] {
        let (r, g, b) = centre(&render(
            page_filled_with(&format!("0 0 0 {k}")),
            CmykIntent::NeutralBlack,
        ));
        assert_eq!(r, g, "grey must be neutral at k={k}");
        assert_eq!(g, b, "grey must be neutral at k={k}");
        assert!(
            r.abs_diff(expected) <= 1,
            "k={k} expected about {expected}, got {r}"
        );
    }
}

#[test]
fn an_image_sample_honours_the_same_intent_as_the_k_operator() {
    // The second, independently-wired path. `pdfcer-core::color`'s module
    // docs require a filled rectangle and an image of the "same" CMYK to
    // agree on screen; the intent gives them a new way to disagree, so the
    // agreement is asserted rather than assumed.
    //
    // One 1x1 DeviceCMYK sample of solid black ink, scaled over the page.
    let content = "q 40 0 0 40 0 0 cm /Im0 Do Q\n";
    let bytes = build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 40 40] \
             /Resources << /XObject << /Im0 5 0 R >> >> >>",
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>"),
        (
            4,
            &format!(
                "<< /Length {} >>\nstream\n{content}endstream",
                content.len()
            ),
        ),
        (
            5,
            // ASCIIHexDecode rather than raw bytes, so the whole fixture
            // stays a Rust `&str`: `\xFF` is not a legal char escape, and
            // §7.4.2's hex filter is the sanctioned way to write binary
            // sample data into a text-safe file. `000000FF` is one
            // DeviceCMYK sample of solid black ink.
            "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 \
             /ColorSpace /DeviceCMYK /BitsPerComponent 8 \
             /Filter /ASCIIHexDecode /Length 9 >>\nstream\n000000FF>\nendstream",
        ),
    ]);

    for intent in [CmykIntent::Calibrated, CmykIntent::NeutralBlack] {
        let image = centre(&render(bytes.clone(), intent));
        let fill = centre(&render(page_filled_with("0 0 0 1"), intent));
        // ±1, and the tolerance is a measured rasterizer artefact rather
        // than colour slack: the image path writes `ColorU8` straight into
        // the pixmap, while a fill goes through tiny-skia's premultiplied
        // round trip, which loses one 8-bit step. Calibrated black comes
        // out (35, 31, 32) as an image and (35, 31, 31) as a fill. Both
        // came from the same `cmyk_to_srgb_with` call; the difference
        // arrives after the colour does. An exact assertion here would be
        // testing tiny-skia's rounding, not pdfcer's colour.
        for (channel, (a, b)) in [(image.0, fill.0), (image.1, fill.1), (image.2, fill.2)]
            .into_iter()
            .enumerate()
        {
            assert!(
                a.abs_diff(b) <= 1,
                "channel {channel} of an image sample and a `k` fill of the same CMYK \
                 disagree under {intent:?}: {image:?} vs {fill:?}"
            );
        }
    }
}

#[test]
fn the_default_render_options_carry_the_default_intent() {
    // R83 from the other end: the no-options entry point every existing
    // caller uses must not silently acquire a DIFFERENT colour rendering
    // from the one the operator's settings name. Asserted as an identity
    // against `CmykIntent::default()` rather than against a named variant,
    // so flipping the shipped default is a one-line change in the settings
    // module and not a hunt through the test suite.
    assert_eq!(
        RenderOptions::default().cmyk_intent,
        CmykIntent::default(),
        "the options default and the setting default must be the same answer"
    );
}

/// ★ WHICH variant ships, pinned as an operator ruling rather than left to
/// a `#[default]` attribute nothing asserts.
///
/// The test directly above is deliberately variant-AGNOSTIC — it survives any
/// default change, which is what makes it a good invariant and a useless
/// record. This one is the opposite: it exists to FAIL if the default moves,
/// because which variant ships is a decision Ken made twice and reversed once,
/// not an implementation detail.
///
/// - 2026-08-08: `NeutralBlack`, over the better evidence, for CAD line art.
/// - 2026-08-28: `Calibrated` — *"change our default to Match other PDF
///   viewers"* (`Pass 153.0`).
///
/// If this fails, do not "fix" it. Find out whether he moved it again.
#[test]
fn the_shipped_default_is_calibrated_by_operator_ruling() {
    assert_eq!(CmykIntent::default(), CmykIntent::Calibrated);
}
