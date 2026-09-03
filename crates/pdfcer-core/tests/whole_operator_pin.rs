//! # A pinned request can say "the whole operator" (`Pass 145.0`)
//!
//! **The gap.** A caller that had already *located* a show operator — by
//! walking the text model and taking `GlyphProvenance::operator_span` — still
//! had to *describe* it, because `find` was required. `pdfcer-gui` reported
//! three attempts at describing it, each of which looked right and each of
//! which failed differently:
//!
//! | attempt | outcome |
//! |---|---|
//! | `find: ""` with a pin | refused — *"empty find text"* |
//! | `find` = the run's `text` | `NoMatch` |
//! | `find` = the glyph-covered bytes | `NoMatch` on some runs |
//!
//! The operator-facing symptom was *"eleven pieces of text went bold and the
//! twelfth refused"*, on a page where nothing is unusual.
//!
//! ## What is pinned here
//!
//! 1. **The affordance works** on both editing verbs — `format_text` and
//!    `edit_text` — because they share one locator and diverging would be a
//!    second implementation of the same idea.
//! 2. **An empty `find` with NO pin is still refused.** This is the criterion
//!    that keeps the feature from becoming a footgun: a caller who forgot to
//!    pin must get a refusal, not silent whole-operator behaviour on an
//!    operator pdfcer chose for them.
//! 3. **It is disclosed.** The extent was chosen by pdfcer, not typed by the
//!    operator, so `CLAUDE.md` rule 4 applies — and the disclosure names the
//!    multi-operator case, because one text run can carry glyphs from several
//!    show operators (13 % of runs over pdfcer's corpus; see
//!    `operator_span_invariant.rs`).
//! 4. **The font-coverage gate sees the resolved text, not the empty string.**
//!    This is the subtle one: `plan_font` used to take `req.find`, and a
//!    whole-operator request would have handed it `""` — which every face
//!    covers. A family change would then have been accepted for a face that
//!    cannot show the run.
//!
//! Fixture provenance: `fixtures/synthetic/textedit/PROVENANCE.md`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::span::ByteSpan;
use pdfcer_core::text_edit::{
    EditError, EditOptions, EditRequest, FontSelector, FormatError, FormatOptions, FormatRequest,
    set_format,
};
use pdfcer_core::text_extract::{ExtractOptions, extract_page};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/textedit")
        .join(name)
}

fn doc(name: &str) -> Document {
    Document::from_bytes(std::fs::read(fixture(name)).expect("readable")).expect("parses")
}

/// The operator span of the first glyph on page 1, **the way a shell gets
/// one** — from extraction provenance, not from a hand-written constant.
///
/// Written this way on purpose: a test that hard-codes a byte offset proves
/// the surgery works on a number the test author chose, not on the number the
/// public API hands a caller. The two conventions for "the show operator's
/// span" differ (`pin_names_operator` documents why), and this is the one a
/// consumer actually has.
fn first_operator_span(doc: &Document) -> ByteSpan {
    let pages = pdfcer_core::page_tree::pages(doc).expect("pages");
    let opts = ExtractOptions::default().with_provenance(true);
    let page = extract_page(doc, &pages[0], 0, &opts).expect("extract");
    page.runs
        .iter()
        .flat_map(|r| r.glyphs.iter())
        .find_map(|g| g.provenance.as_ref().map(|p| p.operator_span))
        .expect("the fixture has at least one glyph with provenance")
}

// ---------------------------------------------------------------------------
// 1. The affordance
// ---------------------------------------------------------------------------

#[test]
fn a_pinned_request_with_no_find_formats_the_whole_operator() {
    let d = doc("format_family.pdf");
    let span = first_operator_span(&d);

    let report = set_format(
        &d,
        &FormatRequest::whole_operator(0, span).size(24.0),
        &FormatOptions::default(),
    )
    .map(|o| o.report)
    .unwrap_or_else(|e| panic!("a pinned whole-operator request must work, but: {e}"));

    assert_eq!(report.size_change, Some((12.0, 24.0)));
}

#[test]
fn the_named_constructor_and_the_empty_find_spelling_agree() {
    // `whole_operator` is the discoverable spelling; the empty-`find` one is
    // the mechanism. If they ever diverge, a caller reading the docs and a
    // caller reading the source get different behaviour.
    let d = doc("format_family.pdf");
    let span = first_operator_span(&d);

    let a = set_format(
        &d,
        &FormatRequest::whole_operator(0, span).size(24.0),
        &FormatOptions::default(),
    )
    .expect("named constructor");
    let b = set_format(
        &d,
        &FormatRequest::new(0, "").pinned(span).size(24.0),
        &FormatOptions::default(),
    )
    .expect("empty-find spelling");

    assert_eq!(a.bytes, b.bytes, "the two spellings produce the same bytes");
}

#[test]
fn edit_text_gets_the_same_affordance() {
    // One locator, so both verbs get it or the two drift.
    let d = doc("format_family.pdf");
    let span = first_operator_span(&d);

    let mut req = EditRequest::find_replace(0, "", "goodbye");
    req.pinned_span = Some(span);
    let outcome = pdfcer_core::text_edit::edit_text(&d, &req, &EditOptions::default())
        .unwrap_or_else(|e| panic!("a pinned whole-operator replace must work, but: {e}"));

    // The result is checked by RE-EXTRACTING, not by trusting a report field:
    // the claim being made is that the whole operator's text was replaced,
    // and the only witness to that is the saved bytes.
    let after = Document::from_bytes(outcome.bytes.clone()).expect("the output parses");
    let pages = pdfcer_core::page_tree::pages(&after).expect("pages");
    let text = extract_page(&after, &pages[0], 0, &ExtractOptions::default())
        .expect("extract")
        .plain_text();
    assert!(
        text.contains("goodbye") && !text.contains("hello"),
        "the whole operator was replaced, not part of it: {text:?}"
    );
}

/// ★★ The named spelling on the EDIT verb (`Pass 152.0`), and the reason it
/// was added when the behaviour above already worked.
///
/// The test directly above proves the affordance. This one proves the
/// *spelling*, and the difference between them is the entire content of a
/// defect `pdfcer-gui` filed on 2026-08-28.
///
/// Their report cites `Pass 145.0` and `FormatRequest::whole_operator` **by
/// name** — so they had read the section documenting this — and still
/// concluded the edit verb could only be addressed by `find`. The edit half
/// was one trailing sentence at the end of the format section, with no
/// example and no symbol to grep for. They then described three ways they had
/// tried to reconstruct a `find` for an operator they had already located.
///
/// ★ No gate in this project can catch that. The code was correct, this
/// file's tests were green, and the sentence was true. **The only symptom of
/// an undiscoverable capability is somebody asking for what they already
/// have** — which is why the remedy is a symbol, not a longer sentence.
#[test]
fn the_named_edit_constructor_and_the_empty_find_spelling_agree() {
    let d = doc("format_family.pdf");
    let span = first_operator_span(&d);

    let a = pdfcer_core::text_edit::edit_text(
        &d,
        &EditRequest::whole_operator(0, span, "goodbye"),
        &EditOptions::default(),
    )
    .expect("named constructor");

    let mut mech = EditRequest::find_replace(0, "", "goodbye");
    mech.pinned_span = Some(span);
    let b = pdfcer_core::text_edit::edit_text(&d, &mech, &EditOptions::default())
        .expect("empty-find mechanism");

    // BYTES, not report fields. Two spellings of one request must produce one
    // document; comparing anything less would let them drift in exactly the
    // place a caller cannot see.
    assert_eq!(
        a.bytes, b.bytes,
        "the discoverable spelling and the mechanism must be the same request"
    );
}

/// The `pinned` builder, which `EditRequest` lacked while `FormatRequest` had
/// it — so the two siblings read differently for the same idea and a caller
/// reaching for symmetry found a bare public field instead.
#[test]
fn the_edit_pinned_builder_matches_direct_field_assignment() {
    let span = ByteSpan { start: 10, len: 4 };
    let built = EditRequest::find_replace(0, "x", "y").pinned(span);
    let mut assigned = EditRequest::find_replace(0, "x", "y");
    assigned.pinned_span = Some(span);
    assert_eq!(built.pinned_span, assigned.pinned_span);
    assert_eq!(built.find, assigned.find);
    assert_eq!(built.replace, assigned.replace);
}

// ---------------------------------------------------------------------------
// 2. The footgun that must stay closed
// ---------------------------------------------------------------------------

#[test]
fn an_empty_find_with_no_pin_is_still_refused() {
    // THE criterion. Without this, a caller who forgot to pin silently
    // restyles whichever operator pdfcer happened to reach first.
    let d = doc("format_family.pdf");

    let err = set_format(
        &d,
        &FormatRequest::new(0, "").size(24.0),
        &FormatOptions::default(),
    )
    .expect_err("an unpinned empty find must be refused");
    let text = err.to_string();
    assert!(
        text.contains("empty find text"),
        "and refused by the same name it always was: {text}"
    );

    let err = pdfcer_core::text_edit::edit_text(
        &d,
        &EditRequest::find_replace(0, "", "x"),
        &EditOptions::default(),
    )
    .expect_err("same on the replace verb");
    assert!(matches!(err, EditError::Unsupported(ref m) if m.contains("empty find text")));
}

#[test]
fn a_pin_that_names_no_operator_is_still_a_pin_failure_not_a_find_failure() {
    // The two failures stay told apart (`Pass 118.0`): an empty find must not
    // turn a bad pin into "text not found", which would blame the operator's
    // own text for a span that named nothing.
    let d = doc("format_family.pdf");
    let err = set_format(
        &d,
        &FormatRequest::whole_operator(
            0,
            ByteSpan {
                start: 9_000,
                len: 2,
            },
        )
        .size(24.0),
        &FormatOptions::default(),
    )
    .expect_err("that span names no operator");
    assert!(
        matches!(err, FormatError::PinnedSpanNotFound { .. }),
        "expected a pin failure, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// 3. Disclosure (rule 4 — the extent was pdfcer's choice, not the operator's)
// ---------------------------------------------------------------------------

#[test]
fn the_chosen_extent_is_disclosed_and_names_the_multi_operator_case() {
    let d = doc("format_family.pdf");
    let span = first_operator_span(&d);
    let report = set_format(
        &d,
        &FormatRequest::whole_operator(0, span).size(24.0),
        &FormatOptions::default(),
    )
    .unwrap()
    .report;

    let note = report
        .disclosures
        .iter()
        .find(|d| d.starts_with("whole operator:"))
        .unwrap_or_else(|| panic!("no whole-operator disclosure in {:?}", report.disclosures));
    assert!(
        note.contains("11 character(s)"),
        "it states the extent it took: {note}"
    );
    assert!(
        note.contains("several show operators"),
        "and warns that a run can span more than one: {note}"
    );
}

#[test]
fn an_ordinary_find_gets_no_whole_operator_disclosure() {
    // A disclosure that fires on every edit is a disclosure nobody reads.
    let d = doc("format_family.pdf");
    let report = set_format(
        &d,
        &FormatRequest::new(0, "hello").size(24.0),
        &FormatOptions::default(),
    )
    .unwrap()
    .report;
    assert!(
        !report
            .disclosures
            .iter()
            .any(|d| d.starts_with("whole operator:")),
        "{:?}",
        report.disclosures
    );
}

// ---------------------------------------------------------------------------
// 4. The gate that would have opened silently
// ---------------------------------------------------------------------------

#[test]
fn the_font_coverage_gate_sees_the_resolved_text_not_the_empty_string() {
    // The subtle half of `Pass 145.0`, and the one a naive implementation
    // gets wrong. `plan_font` used to check coverage against `req.find`; on a
    // whole-operator request that is `""`, and EVERY face covers the empty
    // string. So a family change to `/F3` — which cannot encode the run's
    // `o` — would have been accepted, re-encoding eleven characters into a
    // face that has nowhere to put one of them.
    let d = doc("format_family.pdf");
    let span = first_operator_span(&d);

    let err = set_format(
        &d,
        &FormatRequest::whole_operator(0, span).font(FontSelector::new("F3")),
        &FormatOptions::default(),
    )
    .expect_err("/F3 cannot show the run, pinned or not");
    let text = err.to_string();
    assert!(
        text.contains("U+006F") || text.contains("'o'"),
        "refused for coverage of the RESOLVED text: {text}"
    );

    // …and the face that can show it still succeeds through the same path.
    let report = set_format(
        &d,
        &FormatRequest::whole_operator(0, span).font(FontSelector::new("F2")),
        &FormatOptions::default(),
    )
    .expect("/F2 covers the run")
    .report;
    assert_eq!(
        report.font_change,
        Some(("Times-Roman".to_owned(), "Calibri-Bold".to_owned()))
    );
    assert!(
        report
            .disclosures
            .iter()
            .any(|d| d.contains("11 character(s) were re-encoded")),
        "and the count is the resolved text's, not zero: {:?}",
        report.disclosures
    );
}
