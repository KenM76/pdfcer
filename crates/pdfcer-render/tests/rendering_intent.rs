//! **The rendering intent is read, not discarded** — `/RI`, `ri`
//! (ISO 32000-1 §8.6.5.8, Table 70) (`Pass 199.0`).
//!
//! # Why this is a conformance test and not a nicety
//!
//! pdfcer parsed `ri` and threw the value away — a recognised no-op — until this
//! Pass. Three `shall`s make that a defect rather than a quality gap:
//! §8.6.5.8 says the four names *"shall be recognized"*; §8.6.5.8 says an
//! unrecognised one *"shall use the `RelativeColorimetric` intent by default"*;
//! §11.7.5.3 says the intent used *"shall be the current rendering intent in
//! effect in the graphics state at the time of the painting operation"*.
//!
//! ★ **The sentence that reads as permission has been struck.** The printed
//! NOTE — *"a particular device does not have to support all PDF rendering
//! intents"* — was removed by ISO-approved erratum `pdf-issues` #63, whose
//! resolution reads *"NOTEs are informative only … the existing normative
//! requirements to support all 4 rendering intents remains"*. A reader working
//! from the printed page alone concludes the opposite of the truth, which is
//! exactly why this file cites the erratum rather than the page.
//!
//! # What is asserted, and what is deliberately NOT
//!
//! Asserted: that the value **arrives** and is modelled with the right rules.
//! Not asserted: any pixel. pdfcer carries the intent and does not yet act on
//! it — the conversion that would consume it is the next Pass — and a test
//! asserting a colour here would be asserting something the standard does not
//! constrain anyway.
//!
//! ★★ **That last point is load-bearing.** ISO 32000 gives the two
//! *colorimetric* intents a testable rule (*"in-gamut colours shall be
//! reproduced exactly"*) and gives `Saturation` and `Perceptual` none —
//! reproduction *"may or may not be colourimetrically accurate"*. ISO 32000-2
//! defers to ICC.1:2010, whose clause 0.4 then calls those two *"vendor
//! specific"*. So a future test that pins a colour under `Saturation` would be
//! pinning one engine's opinion, and a measurement matching a fixture is **not**
//! evidence that an intent is the correct one.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::render_page;

/// Build a one-page document whose content stream is `content`, with two
/// `/ExtGState`s: `/GSri` carrying `/RI /Saturation`, and `/GSplain` carrying
/// no `/RI` at all.
///
/// The second one is the whole point of the fixture — see
/// `an_ext_gstate_without_ri_does_not_reset_the_intent`.
fn doc(content: &str) -> Document {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /ExtGState << \
         /GSri 5 0 R /GSplain 6 0 R >> >> >>"
            .to_string(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        "<< /Type /ExtGState /RI /Saturation >>".to_string(),
        "<< /Type /ExtGState /LW 2 >>".to_string(),
    ];
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    Document::from_bytes(buf).expect("synthetic doc parses")
}

/// How many times the page set a rendering intent.
fn intents_set(content: &str) -> usize {
    let d = doc(content);
    let pages = page_tree::pages(&d).expect("page tree");
    render_page(&d, &pages[0], 1.0)
        .expect("rasterises")
        .diagnostics
        .rendering_intents_set
}

/// ★★★ THE HEADLINE: `ri` is no longer discarded.
///
/// Before this Pass the operator was a recognised no-op and this counter did
/// not exist. A page that names an intent must be observed to have named one.
#[test]
fn the_ri_operator_is_read_rather_than_discarded() {
    assert_eq!(
        intents_set("/Saturation ri 0 0 50 50 re f"),
        1,
        "the `ri` operator must set the graphics-state intent (§8.6.5.8, §11.7.5.3)"
    );
}

/// `/RI` in an `/ExtGState` is the other carrier (ISO 32000-1 Table 58).
#[test]
fn ri_in_an_ext_gstate_is_read() {
    assert_eq!(
        intents_set("/GSri gs 0 0 50 50 re f"),
        1,
        "`/RI` in an /ExtGState must set the intent"
    );
}

/// ★★ AN `/ExtGState` WITHOUT `/RI` MUST NOT RESET THE INTENT.
///
/// §8.4.5: *"The results of `gs` shall be cumulative … parameter values …
/// persist until explicitly overridden."*
///
/// This is the rule a careful reader gets wrong, and the standard itself is
/// why: ISO 32000-2's Table 57 uniquely printed *"The default value is:
/// Default"* for this one entry, and ISO-approved erratum `pdf-issues` #360
/// **deleted** it because no other entry claims one. It was re-raised as #746
/// in 2026 and closed as a duplicate — a live implementer trap, not history.
///
/// An implementation that reset here would silently discard an intent set once
/// at the top of a page, which is exactly how producers write it.
#[test]
fn an_ext_gstate_without_ri_does_not_reset_the_intent() {
    // `ri` sets it; then a `gs` carrying LW but no /RI must leave it alone. If
    // the second `gs` reset the intent it would count as a second set.
    assert_eq!(
        intents_set("/Saturation ri /GSplain gs 0 0 50 50 re f"),
        1,
        "a `gs` with no /RI must not touch the rendering intent (§8.4.5, cumulative)"
    );
}

/// ★ AN UNRECOGNISED NAME IS NOT AN ERROR, AND NOT A NO-OP.
///
/// §8.6.5.8, `shall`: it *"shall use the `RelativeColorimetric` intent by
/// default"*. So it still SETS the intent — to `RelativeColorimetric` — which
/// is a different outcome from leaving the previous value in place.
///
/// The distinction matters: a page that says `/Saturation ri` and later
/// `/Nonsense ri` ends on `RelativeColorimetric`, not on `Saturation`.
#[test]
fn an_unrecognised_intent_name_still_sets_the_intent() {
    assert_eq!(
        intents_set("/NoSuchIntent ri 0 0 50 50 re f"),
        1,
        "an unrecognised name must SET RelativeColorimetric, not be ignored (§8.6.5.8)"
    );
}

/// The counter counts sets, so a page that never mentions an intent reads 0.
///
/// The control: without it, every assertion above is satisfied by a counter
/// that increments on every operator.
#[test]
fn a_page_that_names_no_intent_counts_none() {
    assert_eq!(
        intents_set("0 0 50 50 re f"),
        0,
        "a page with no `ri` and no /RI must not report an intent being set"
    );
}

/// The four names resolve, and an unknown one falls to `RelativeColorimetric`.
///
/// Unit-level, against the core type, because the four-name table is the part
/// that is verbatim from ISO 32000-1 Table 70 and the part a typo would break
/// silently.
#[test]
fn the_four_names_resolve_and_an_unknown_one_falls_back() {
    use pdfcer_core::color::RenderingIntent as R;
    assert_eq!(
        R::from_name(b"AbsoluteColorimetric"),
        R::AbsoluteColorimetric
    );
    assert_eq!(
        R::from_name(b"RelativeColorimetric"),
        R::RelativeColorimetric
    );
    assert_eq!(R::from_name(b"Saturation"), R::Saturation);
    assert_eq!(R::from_name(b"Perceptual"), R::Perceptual);
    assert_eq!(R::from_name(b"Nonsense"), R::RelativeColorimetric);
    // Table 52's *Initial value*, made binding by §8.4.1.
    assert_eq!(R::default(), R::RelativeColorimetric);
    // ★ Only the two colorimetric intents have a testable output rule; the
    // other two are "vendor specific" per ICC.1:2010 clause 0.4. Asserted so a
    // future author reaches for this instead of inventing a colour expectation.
    assert!(R::RelativeColorimetric.output_is_constrained());
    assert!(!R::Saturation.output_is_constrained());
}

/// ★★ D3 — an image's own `/Intent` overrides the graphics state, FOR THAT
/// IMAGE ONLY (ISO 32000-1 Table 89) (`Pass 199.1`).
///
/// # The trap this pins
///
/// Table 89's default is *"the current rendering intent in the graphics
/// state"* — **not a constant**. Three of this area's four defaults are
/// `RelativeColorimetric` and this one is not, so a single
/// `unwrap_or_default()` would be wrong on every page that sets an intent at
/// the top and then draws an image.
///
/// ★ It was DOCUMENTED before it was implemented. `Pass 199.0` wrote D3 into
/// the module docs as a rule and wired only `ri` and `/RI`; an outbound reply
/// to a sibling project noticed that the two `b"Intent"` hits in the tree were
/// the annotation and optional-content keys — different keys with the same
/// name. A documented rule with no implementation is a claim, and this test is
/// what turns it back into a fact.
#[test]
fn an_image_intent_overrides_the_graphics_state() {
    use pdfcer_core::color::{RenderingIntent as R, image_intent};

    let gs = R::Saturation;
    // Absent: the graphics state survives. NOT the type default.
    assert_eq!(
        image_intent(gs, None, false),
        R::Saturation,
        "an image with no /Intent must INHERIT, not fall to RelativeColorimetric"
    );
    // Present: it wins.
    assert_eq!(image_intent(gs, Some(b"Perceptual"), false), R::Perceptual);
    // Unrecognised: §8.6.5.8's fallback still applies inside D3.
    assert_eq!(
        image_intent(gs, Some(b"Nonsense"), false),
        R::RelativeColorimetric
    );
    // ★ An image MASK has no colour, so the entry is ignored (ISO 32000-2's
    // Table 87 says so outright, and §8.9.6.2 implies it).
    assert_eq!(
        image_intent(gs, Some(b"Perceptual"), true),
        R::Saturation,
        "/Intent must be ignored when ImageMask is true"
    );
}
