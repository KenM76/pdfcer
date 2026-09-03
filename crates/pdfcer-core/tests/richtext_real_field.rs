//! `crate::richtext` against a REAL field in a real file, end to end.
//!
//! # Why this exists separately from the module's own unit tests
//!
//! `richtext`'s unit tests feed it strings written in the same session,
//! by the same person, in the shape that module expects. That proves the
//! parser self-consistent and proves nothing about whether the bytes a
//! PDF actually stores reach it intact.
//!
//! Three independent things sit between a `/RV` in a file and a
//! [`pdfcer_core::richtext::Run`], and each has its own way of being
//! wrong:
//!
//! 1. `forms::parse_acroform` must find `/RV` and `/DS` on the field and
//!    decode them as §7.9.2 text strings.
//! 2. `Field` must carry BOTH — `/DS` was missing from the model until
//!    this test's own Pass, so `/RV` was reachable and unresolvable.
//! 3. `richtext::parse` must accept what a real producer writes, which is
//!    not necessarily what a unit test's author would type.
//!
//! # The fixture, and the property that makes it non-vacuous
//!
//! `radio-choice-form.pdf` object 50 (`Notes`) carries `/Ff 33554432`
//! (bit 26), a `/DS` default style, an `/RV` body, and — load-bearing —
//! a `/V` whose wording DIFFERS from the `/RV` wording:
//!
//! ```text
//! /V  (RICH ORIGINAL)
//! /DS (font: 12pt Helvetica; color: #FF0000)
//! /RV (<body …><p><b>RICH</b> <i>ORIGINAL</i></p></body>)
//! ```
//!
//! With `/V` and `/RV` saying the same thing, a parser that silently read
//! the wrong entry would be indistinguishable from one that read the
//! right one. That difference is why the fixture is written this way, and
//! this file asserts against it directly.

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::forms;
use pdfcer_core::richtext::{self, Run};
use std::path::Path;

/// The `Notes` field of the shared forms fixture.
fn notes_field() -> forms::Field {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/forms/radio-choice-form.pdf");
    // Through a session, matching every other forms test: `graph()` is
    // the session's view, so a fixture read here and a fixture read by
    // an editing test see the same object model.
    let s = EditSession::new(Document::load(&path).expect("load fixture"));
    let form = forms::parse_acroform(&s.graph()).expect("fixture has an AcroForm");
    form.field_by_name("Notes")
        .expect("fixture has a Notes field")
        .clone()
}

/// Parse the field's rich text the way a real consumer would have to.
fn notes_runs() -> Vec<Run> {
    let f = notes_field();
    let rv = String::from_utf8(f.rich_value.clone().expect("Notes carries /RV"))
        .expect("the fixture's /RV is ASCII");
    let ds = f
        .default_style
        .clone()
        .map(|d| String::from_utf8(d).expect("the fixture's /DS is ASCII"));
    richtext::parse(&rv, ds.as_deref()).expect("the fixture's rich text parses")
}

/// Both entries survive the trip from file bytes to the model.
///
/// Asserted before anything is parsed, because a `None` here would make
/// every downstream assertion vacuous in the specific way that is hardest
/// to notice: the test would still fail, but on the wrong line, blaming
/// the parser for a model gap.
#[test]
fn the_field_carries_both_rv_and_ds() {
    let f = notes_field();
    assert!(f.rich_value.is_some(), "/RV must reach the model");
    assert!(
        f.default_style.is_some(),
        "/DS must reach the model — without it a run's style cannot be resolved (RT-M6)"
    );
    assert!(
        f.is_rich_text(),
        "the fixture sets /Ff bit 26; if this fails the fixture changed, not the parser"
    );
}

/// The real `/RV` splits into the three runs its markup describes.
///
/// `<p><b>RICH</b> <i>ORIGINAL</i></p>` is bold, a bare space, italic —
/// and the middle run is the one that catches a flattening reader. A
/// parser that concatenated text and children would produce `"RICH
/// ORIGINAL"` as one run with no styles, or two runs welded together.
#[test]
fn a_real_rv_splits_into_its_styled_runs() {
    let runs = notes_runs();
    let texts: Vec<&str> = runs.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(texts, vec!["RICH", " ", "ORIGINAL"], "runs: {runs:#?}");

    assert_eq!(runs[0].style.weight, Some(700), "<b> is weight 700");
    assert_eq!(runs[0].style.italic, None, "<b> must not imply italic");

    assert_eq!(runs[1].style.weight, None, "bold must not leak past </b>");
    assert_eq!(runs[1].style.italic, None);

    assert_eq!(runs[2].style.italic, Some(true), "<i> is italic");
    assert_eq!(runs[2].style.weight, None, "<i> must not imply bold");

    // One paragraph, so every run shares its index.
    assert!(runs.iter().all(|r| r.paragraph == 0));
}

/// The real `/DS` reaches every run, including the unstyled space.
///
/// This is RT-M6 working on real bytes: neither `<b>` nor `<i>` nor the
/// bare space sets a size, family or colour, so all three come from `/DS`
/// and from nowhere else. The bare space is the strictest of the three —
/// it sits inside no styling element at all, so a cascade that only
/// applied `/DS` when some other style was present would miss exactly it.
#[test]
fn the_real_ds_reaches_every_run_including_the_bare_space() {
    let runs = notes_runs();
    for (i, r) in runs.iter().enumerate() {
        assert_eq!(r.style.size_pt, Some(12.0), "run {i} size from /DS");
        assert_eq!(
            r.style.family,
            vec!["Helvetica".to_owned()],
            "run {i} family from /DS's `font` shorthand"
        );
        assert_eq!(
            r.style.color,
            Some([1.0, 0.0, 0.0]),
            "run {i} colour from /DS, converted to DeviceRGB (RT-M12)"
        );
    }
}

/// The rich text says something the plain `/V` does not.
///
/// The fixture's whole point, asserted so a future edit that "tidies" the
/// two into agreement has to break a test and read why. Flattening the
/// runs recovers the same characters as `/V`, but only the runs carry
/// which half is bold and which is italic — the information a plain-text
/// fill would destroy, and the reason `fill_text_field` refuses.
#[test]
fn the_runs_carry_what_the_plain_value_cannot() {
    let f = notes_field();
    let plain = match f.value {
        forms::FieldValue::Text(ref t) => String::from_utf8_lossy(t).into_owned(),
        ref other => panic!("expected a text value, got {other:?}"),
    };
    assert_eq!(plain, "RICH ORIGINAL");

    let runs = notes_runs();
    let flattened: String = runs.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(flattened, plain, "the same characters…");

    // …but the styling exists only in the runs.
    assert!(
        runs.iter().any(|r| r.style.weight == Some(700))
            && runs.iter().any(|r| r.style.italic == Some(true)),
        "the runs must distinguish the bold half from the italic half"
    );
}
