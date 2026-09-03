//! # `gate_synthesis` names a face `set_font` will actually accept
//! (`Pass 144.0`)
//!
//! **The defect this file exists to prevent recurring.** On
//! `fixtures/synthetic/textedit/format_family.pdf`, before this Pass:
//!
//! ```text
//! --bold-synthetic          refused: a REAL bold face is available as
//!                                    'Times-Bold' (resource /F3)
//! --set-font Times-Bold     refused: 'o' has no code in Times-Bold's
//!                                    encoding (/Differences took code 111)
//! --set-font F2             SUCCEEDS  <- named by neither refusal
//! ```
//!
//! Both verbs refused and the one face that could show the run was never
//! mentioned, so **bold was unreachable on that page** except by an operator
//! who already knew to try a resource pdfcer did not offer. `pdfcer-gui` found it
//! with a driven test of pdfcer's own fixture; the engineer reproduced all
//! three commands before accepting the report.
//!
//! ## `R90` is NOT weakened here, and this note is why the diff reads as if it
//! were
//!
//! `R90` says synthesis is a fallback for when no real face **resolves**, never
//! an alternative to one. Untouched. What changed is the predicate for
//! *resolves*: from *"exists with a matching family name and a style word in
//! it"* to *"would actually be accepted by `set_font` for this run"*. That is
//! `R90` applied more accurately. `refusal_still_fires_when_a_usable_face_is_present`
//! is the pin: any change that lets synthesis run past a genuinely usable real
//! face is a regression, not this Pass.
//!
//! ## The three branches, one test each
//!
//! 1. family match accepted ⇒ refuse and name it (unchanged behaviour);
//! 2. family match refuses, another resource accepts ⇒ refuse and name
//!    **that** one, disclosing that it is a different family;
//! 3. nothing on the page accepts ⇒ **synthesis proceeds**.
//!
//! Branch 3 is authored to fail if a future change turns it into a refusal —
//! that is the direction in which "tighten the gate" would break the feature
//! outright.
//!
//! Fixture provenance: `fixtures/synthetic/textedit/PROVENANCE.md`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::settings::StylePolicy;
use pdfcer_core::text_edit::{
    FontSelector, FormatError, FormatOptions, FormatRequest, StyleSynthesis, set_format,
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/textedit")
        .join(name)
}

fn doc(name: &str) -> Document {
    Document::from_bytes(std::fs::read(fixture(name)).expect("the fixture is readable"))
        .expect("the fixture parses")
}

/// Ask for synthetic bold on `find`, and return the refusal if there was one.
///
/// # ★ Under `StylePolicy::Refuse`, deliberately
///
/// This whole file is about the CONTENT of the refusal — that it names a face
/// `set_font` will actually accept — and decision 106 did not change that
/// content. What it changed is when the refusal fires: it is now a **posture**
/// rather than pdfcer's only behaviour, because the operator ruled that bold
/// should resolve automatically *and*, separately, that the refusal should
/// stay available.
///
/// So the assertions here are unchanged and only the posture is named. Leaving
/// this at `default()` would silently retarget every test in the file at
/// `Auto`, where there is no refusal to inspect — the suite would go green by
/// testing nothing, which is worse than the failure that led here.
fn ask_bold(doc: &Document, find: &str) -> Result<(), FormatError> {
    set_format(
        doc,
        &FormatRequest::new(0, find).synthetic(StyleSynthesis::Bold),
        &FormatOptions::default().with_style_policy(StylePolicy::Refuse),
    )
    .map(|_| ())
}

/// The `RealFaceAvailable` payload, or a panic naming what came back instead.
///
/// `FormatError` is `#[non_exhaustive]`, so this is written the way a consumer
/// must write it.
fn real_face(err: FormatError) -> (String, String, String, bool) {
    if let FormatError::RealFaceAvailable {
        real_font,
        resource,
        selector,
        same_family,
        ..
    } = err
    {
        return (real_font, resource, selector, same_family);
    }
    panic!("expected RealFaceAvailable, got: {err}");
}

// ---------------------------------------------------------------------------
// Branch 2 — THE DEFECT. The named face must be one `set_font` will take.
// ---------------------------------------------------------------------------

#[test]
fn the_refusal_names_a_face_that_can_actually_show_the_run() {
    let d = doc("format_family.pdf");
    let err = ask_bold(&d, "hello world").expect_err("a usable real bold exists on this page");
    let (real_font, resource, selector, same_family) = real_face(err);

    assert_eq!(
        resource, "F2",
        "the offered face is /F2 (Calibri-Bold), which covers the run — NOT /F3 \
         (Times-Bold), whose /Differences cannot show 'o'"
    );
    assert_eq!(real_font, "Calibri-Bold");
    assert_eq!(selector, "Calibri-Bold");
    assert!(
        !same_family,
        "and it is disclosed as a different family, because no Times face here can show the run"
    );
}

#[test]
fn the_remedy_the_refusal_names_actually_succeeds() {
    // The whole point of the Pass, asserted end to end: take the offer and it
    // must work. Before this change, taking the offer produced a second
    // refusal and the operator had no route to bold at all.
    let d = doc("format_family.pdf");
    let err = ask_bold(&d, "hello world").unwrap_err();
    let (_, _, selector, _) = real_face(err);

    let report = set_format(
        &d,
        &FormatRequest::new(0, "hello world").font(FontSelector::new(&selector)),
        &FormatOptions::default(),
    )
    .map(|o| o.report)
    .unwrap_or_else(|e| panic!("the remedy pdfcer recommended must work, but: {e}"));

    assert_eq!(
        report.font_change,
        Some(("Times-Roman".to_owned(), "Calibri-Bold".to_owned()))
    );
}

#[test]
fn the_face_the_old_name_matching_would_have_named_is_still_refused_by_set_font() {
    // The other half of the defect, pinned so the fixture cannot drift out
    // from under this file: /F3 remains a face whose NAME says Bold and whose
    // encoding cannot show the run. If this ever starts passing, the fixture
    // stopped exercising the bug and the tests above became vacuous.
    let d = doc("format_family.pdf");
    let err = set_format(
        &d,
        &FormatRequest::new(0, "hello world").font(FontSelector::new("F3")),
        &FormatOptions::default(),
    )
    .expect_err("/F3 cannot encode 'o'");
    let text = err.to_string();
    assert!(
        text.contains("U+006F") || text.contains("'o'"),
        "the refusal is still the coverage one: {text}"
    );
}

// ---------------------------------------------------------------------------
// Branch 1 — unchanged behaviour: a usable family match still refuses
// ---------------------------------------------------------------------------

#[test]
fn refusal_still_fires_when_a_usable_face_is_present() {
    // `R90` is not weakened. A page with a real, usable Times-Bold still
    // refuses synthetic bold and points at it.
    let d = doc("format_twins.pdf");
    let err = ask_bold(&d, "hello world").expect_err("/FB2 is a usable real Times-Bold");
    let (real_font, resource, selector, same_family) = real_face(err);

    assert_eq!(real_font, "Times-Bold");
    assert_eq!(
        resource, "FB2",
        "the TWIN THAT WORKS, not /FB1 which shares its /BaseFont and refuses"
    );
    assert!(same_family, "and it is the run's own family");
    assert_eq!(
        selector, "FB2",
        "offered by RESOURCE KEY, because 'Times-Bold' is ambiguous on this page \
         and the name match would reach /FB1"
    );
}

#[test]
fn the_selector_offered_on_an_ambiguous_page_is_the_one_that_works() {
    // The failure this guards against is subtle and silent: a caller retrying
    // with `real_font` ("Times-Bold") lands on whichever twin the name match
    // reaches first, which on this fixture is the one that refuses. Retrying
    // with `selector` cannot.
    let d = doc("format_twins.pdf");
    let (_, _, selector, _) = real_face(ask_bold(&d, "hello world").unwrap_err());

    let ok = set_format(
        &d,
        &FormatRequest::new(0, "hello world").font(FontSelector::new(&selector)),
        &FormatOptions::default(),
    );
    assert!(ok.is_ok(), "the offered selector works: {:?}", ok.err());
}

// ---------------------------------------------------------------------------
// Branch 3 — synthesis PROCEEDS when nothing on the page is usable
// ---------------------------------------------------------------------------

#[test]
fn synthesis_proceeds_when_no_resource_would_be_accepted() {
    // Authored to fail if a future change turns branch 3 into a refusal.
    // `format_color.pdf` carries one non-bold face and nothing else, so
    // synthesis is genuinely the only route to bold and must not be blocked.
    let d = doc("format_color.pdf");
    let report = set_format(
        &d,
        &FormatRequest::new(0, "hello world").synthetic(StyleSynthesis::Bold),
        &FormatOptions::default(),
    )
    .map(|o| o.report)
    .unwrap_or_else(|e| panic!("synthesis must proceed when nothing real is usable, but: {e}"));

    assert!(
        report.synthetic_bold_width.is_some(),
        "a stroke width is reported, so the synthesis really happened"
    );
    assert!(
        report
            .disclosures
            .iter()
            .any(|d| d.contains("SYNTHETIC STYLE")),
        "and it is disclosed, never silent (rule 4)"
    );
}

#[test]
fn a_request_no_single_face_covers_falls_through_to_synthesis() {
    // The all-or-nothing shape of the gate, pinned against branch 3. On
    // `format_family.pdf` two faces claim Bold, one of them usable — but
    // NEITHER claims Italic, so a Bold+Italic request has no candidate at all
    // and synthesis is the only route. A gate that answered per axis would
    // wrongly refuse here on the strength of the bold half.
    let d = doc("format_family.pdf");
    let report = set_format(
        &d,
        &FormatRequest::new(0, "hello world").synthetic(StyleSynthesis::BoldItalic),
        &FormatOptions::default(),
    )
    .map(|o| o.report)
    .unwrap_or_else(|e| panic!("no face covers bold+italic here, so synthesis must run: {e}"));

    assert!(report.synthetic_bold_width.is_some());
    assert!(report.synthetic_italic.is_some());
}

#[test]
fn text_that_is_not_on_the_page_fails_to_locate_rather_than_to_gate() {
    // Asserted so a location failure can never be mistaken for a gate result
    // when reading a test run — the two are different outcomes with different
    // remedies, and only one of them is about fonts.
    let d = doc("format_family.pdf");
    let err = set_format(
        &d,
        &FormatRequest::new(0, "not on this page").synthetic(StyleSynthesis::Bold),
        &FormatOptions::default(),
    )
    .expect_err("that text is not on the page");
    assert!(
        matches!(err, FormatError::NoMatch(_)),
        "expected a location failure, got: {err}"
    );
}
