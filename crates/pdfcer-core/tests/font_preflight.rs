//! # `EditSession::preview_font_resources` — the font-resource pre-flight
//! (`Pass 142.1`)
//!
//! **Out-of-crate on purpose.** Every type this query returns is
//! `#[non_exhaustive]`, which means a consuming crate cannot match
//! [`FontAcceptance`] exhaustively and cannot construct one of these structs
//! at all. An in-crate test sees none of that: inside `pdfcer-core` the
//! attribute is inert, so an in-crate test would happily write a `match` a
//! real consumer cannot compile. These tests are written the way `pdfcer-gui`
//! must write them — accessor plus `if let` — so the ergonomics being shipped
//! are the ergonomics being tested.
//!
//! ## What is being pinned, and why each one earns a test
//!
//! 1. **Acceptance is per RUN, not per page.** `format_family.pdf` carries
//!    `/F3 /Times-Bold` whose `/Encoding /Differences` reassigns code 111 to
//!    `/bullet`. It refuses `"hello world"` (which contains `o`) and accepts
//!    `"hell"` (which does not) — *the same face, the same page, opposite
//!    answers.* A pre-flight that answered per page would be wrong for one of
//!    those two runs and could not say which.
//!
//! 2. **A name claim is not an acceptance.** `/F3`'s `/BaseFont` says `Bold`,
//!    so every name-based test in the codebase says "a real bold face is
//!    available here". The pre-flight says `real_bold = None` for the run's
//!    family on `"hello world"`, because the only Times-Bold on the page
//!    cannot show the run. **That gap is `Pass 144.0`**, and this test is what
//!    keeps the two answers from drifting apart again.
//!
//! 3. **A `/BaseFont` is not always a usable selector.** `format_twins.pdf`
//!    carries two resources with the identical `/BaseFont /Times-Bold` — the
//!    shape a real embedding producer emits routinely, found in 87 % of
//!    embedding files by `pdfcer-gui`'s own survey. `set_font`'s name match
//!    reaches exactly one of them, and on that fixture it reaches the one that
//!    *refuses*. The pre-flight must hand back the resource key instead and
//!    say why.
//!
//! Fixture provenance: `fixtures/synthetic/textedit/PROVENANCE.md`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::span::ByteSpan;
use pdfcer_core::text_edit::{FontAcceptance, FontPreflight};
use pdfcer_core::text_extract::{ExtractOptions, extract_page};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/textedit")
        .join(name)
}

fn preflight(name: &str, find: &str) -> FontPreflight {
    let bytes = std::fs::read(fixture(name)).expect("the fixture is readable");
    let doc = Document::from_bytes(bytes).expect("the fixture parses");
    let session = EditSession::new(doc);
    session
        .preview_font_resources(0, find, None)
        .expect("the run is locatable on page 1")
}

/// The refusal message for one resource, or `None` when it was accepted.
///
/// Written the way a downstream crate must write it: `#[non_exhaustive]`
/// forbids an exhaustive `match`, so the shape is an accessor plus an
/// `if let`. Deliberately not a helper inside `pdfcer-core`, where the
/// attribute would be inert and this constraint invisible.
fn refusal_of(p: &FontPreflight, resource: &str) -> Option<String> {
    let e = p
        .entries
        .iter()
        .find(|e| e.resource == resource)
        .unwrap_or_else(|| panic!("resource /{resource} is on the page"));
    if let FontAcceptance::Refused { message, .. } = &e.acceptance {
        return Some(message.clone());
    }
    assert!(e.acceptance.is_accepted(), "the enum has only two states");
    None
}

// ---------------------------------------------------------------------------
// 1. Every page font resource is surveyed, and the run is identified
// ---------------------------------------------------------------------------

#[test]
fn every_page_font_resource_is_surveyed_and_the_run_is_named() {
    let p = preflight("format_family.pdf", "hello world");

    assert_eq!(p.text, "hello world");
    assert_eq!(p.run_resource, "F1");
    assert_eq!(p.run_font, "Times-Roman");

    let keys: Vec<&str> = p.entries.iter().map(|e| e.resource.as_str()).collect();
    assert_eq!(
        keys,
        vec!["F1", "F2", "F3"],
        "all three /Font resources are reported, not just the interesting ones"
    );
}

// ---------------------------------------------------------------------------
// 2. Acceptance is the real `set_font` answer, per resource
// ---------------------------------------------------------------------------

#[test]
fn acceptance_matches_what_set_font_actually_does() {
    let p = preflight("format_family.pdf", "hello world");

    // /F1 is the run's own face; re-encoding into itself is trivially fine.
    assert_eq!(refusal_of(&p, "F1"), None);
    // /F2 (Calibri-Bold) covers the run — this is the face `--set-font F2`
    // succeeds on today.
    assert_eq!(refusal_of(&p, "F2"), None);

    // /F3 (Times-Bold) does NOT, and the message is the one `--set-font F3`
    // itself prints, verbatim — not a paraphrase invented by the pre-flight.
    let f3 = refusal_of(&p, "F3").expect("/F3 refuses this run");
    assert!(
        f3.contains("U+006F") && f3.contains("Times-Bold"),
        "the refusal names the character and the face: {f3}"
    );

    let accepted: Vec<&str> = p.accepted().map(|e| e.resource.as_str()).collect();
    assert_eq!(accepted, vec!["F1", "F2"]);
}

#[test]
fn the_refused_character_is_reported_structurally_not_only_in_prose() {
    let p = preflight("format_family.pdf", "hello world");
    let f3 = p.entries.iter().find(|e| e.resource == "F3").unwrap();
    let mut got = None;
    if let FontAcceptance::Refused { character, .. } = &f3.acceptance {
        got = *character;
    }
    assert_eq!(
        got,
        Some('o'),
        "a shell should not have to parse the message to know which character failed"
    );
}

// ---------------------------------------------------------------------------
// 3. THE PIN FOR `Pass 144.0` — a name claim is not an acceptance
// ---------------------------------------------------------------------------

#[test]
fn a_face_that_claims_bold_but_cannot_cover_the_run_is_not_a_real_bold() {
    let p = preflight("format_family.pdf", "hello world");

    let f3 = p.entries.iter().find(|e| e.resource == "F3").unwrap();
    assert!(
        f3.claims_bold,
        "/F3's /BaseFont literally says Bold — every name-based test says yes here"
    );
    assert!(
        !f3.acceptance.is_accepted(),
        "and it would nonetheless refuse the run"
    );

    assert!(
        p.real_bold().is_none(),
        "so there is NO real bold face of the run's family usable on this run — \
         which is exactly what `Pass 144.0`'s gate got wrong by asking the name"
    );
    assert!(p.real_italic().is_none(), "the page carries no italic face");
}

#[test]
fn the_same_face_accepts_a_run_without_the_uncovered_character() {
    // The whole argument for a per-RUN answer, in one pair of asserts.
    let p = preflight("format_family.pdf", "hell");
    assert_eq!(refusal_of(&p, "F3"), None, "'hell' has no 'o' to lose");

    let bold = p.real_bold().expect("now a real Times-Bold IS usable");
    assert_eq!(bold.resource, "F3");
    assert_eq!(bold.base_font, "Times-Bold");
    assert_eq!(
        bold.selector, "Times-Bold",
        "unambiguous on this page, so the /BaseFont is a safe selector"
    );
}

// ---------------------------------------------------------------------------
// 4. The join problem: two resources, one /BaseFont
// ---------------------------------------------------------------------------

#[test]
fn two_resources_with_one_base_font_fall_back_to_the_resource_key() {
    let p = preflight("format_twins.pdf", "hello world");

    for key in ["FB1", "FB2"] {
        let e = p.entries.iter().find(|e| e.resource == key).unwrap();
        assert_eq!(e.base_font, "Times-Bold");
        assert!(
            e.base_font_ambiguous,
            "/{key} shares its /BaseFont with another resource on this page"
        );
        assert_eq!(
            e.selector, key,
            "so the only selector that reaches /{key} is its resource key"
        );
    }

    // The run's own face is NOT ambiguous, so it keeps the readable selector.
    let f1 = p.entries.iter().find(|e| e.resource == "F1").unwrap();
    assert!(!f1.base_font_ambiguous);
    assert_eq!(f1.selector, "Times-Roman");
}

#[test]
fn the_real_bold_offered_is_the_twin_that_accepts_not_the_one_the_name_reaches() {
    let p = preflight("format_twins.pdf", "hello world");

    assert!(
        refusal_of(&p, "FB1").is_some(),
        "/FB1 remaps 'o' and must refuse"
    );
    assert_eq!(refusal_of(&p, "FB2"), None, "/FB2 covers the run");

    let bold = p
        .real_bold()
        .expect("a real, usable Times-Bold exists on this page");
    assert_eq!(
        bold.resource, "FB2",
        "the pre-flight offers the twin that WORKS"
    );
    assert_eq!(
        bold.selector, "FB2",
        "and offers it by resource key, because 'Times-Bold' is ambiguous here \
         and would reach the other twin"
    );
}

#[test]
fn an_entry_that_is_the_real_bold_reports_itself() {
    let p = preflight("format_twins.pdf", "hello world");

    let fb2 = p.entries.iter().find(|e| e.resource == "FB2").unwrap();
    let own = fb2
        .real_bold
        .as_ref()
        .expect("/FB2 is an accepted real bold of its own family");
    assert_eq!(
        own.resource, "FB2",
        "an entry that IS the family's real bold must say so about itself — \
         answering None here would tell a shell to fake a weight on top of a \
         genuine one"
    );

    // …and the refusing twin still points at the working one, not at itself.
    let fb1 = p.entries.iter().find(|e| e.resource == "FB1").unwrap();
    assert_eq!(fb1.real_bold.as_ref().unwrap().resource, "FB2");
}

// ---------------------------------------------------------------------------
// 5. It is a query: calling it changes nothing
// ---------------------------------------------------------------------------

#[test]
fn the_preflight_stages_nothing() {
    let bytes = std::fs::read(fixture("format_family.pdf")).unwrap();
    let doc = Document::from_bytes(bytes).unwrap();
    let session = EditSession::new(doc);

    for _ in 0..5 {
        let _ = session
            .preview_font_resources(0, "hello world", None)
            .unwrap();
    }
    assert_eq!(
        session.undo_depth(),
        0,
        "a &self query must not stage or commit a command"
    );
    assert!(
        session.dirty_set().is_empty(),
        "…and must not mark a single object dirty"
    );
}

// ---------------------------------------------------------------------------
// 6. Location failures are errors; refusals are not
// ---------------------------------------------------------------------------

#[test]
fn text_that_is_not_on_the_page_is_an_error_but_a_refusing_font_is_not() {
    let bytes = std::fs::read(fixture("format_family.pdf")).unwrap();
    let doc = Document::from_bytes(bytes).unwrap();
    let session = EditSession::new(doc);

    assert!(
        session
            .preview_font_resources(0, "not on this page", None)
            .is_err(),
        "the run must be locatable for the question to mean anything"
    );
    assert!(
        session
            .preview_font_resources(9, "hello world", None)
            .is_err(),
        "a page index past the end is an error, not an empty answer"
    );
    // …whereas a page on which a font refuses answers successfully.
    assert!(
        session
            .preview_font_resources(0, "hello world", None)
            .is_ok()
    );
}

// ---------------------------------------------------------------------------
// 7. `Pass 147.0` — an empty `find` with a pin means the WHOLE OPERATOR here
//    too, and the failure it replaces was silent and INVERTED
// ---------------------------------------------------------------------------

/// The first show operator's byte span, the way a shell obtains one.
fn first_operator_span(doc: &Document) -> ByteSpan {
    let pages = pdfcer_core::page_tree::pages(doc).expect("pages");
    let opts = ExtractOptions::default().with_provenance(true);
    let page = extract_page(doc, &pages[0], 0, &opts).expect("extract");
    page.runs
        .iter()
        .flat_map(|r| r.glyphs.iter())
        .find_map(|g| g.provenance.as_ref().map(|p| p.operator_span))
        .expect("the fixture has a glyph with provenance")
}

#[test]
fn an_empty_find_with_a_pin_surveys_the_whole_operator_not_zero_characters() {
    // THE DEFECT, reported by `pdfcer-gui` after they consumed `Pass 145.0` and
    // took the obvious next step. `find_anchor` never reads `find` when a pin
    // is set, so an empty one located the right operator and then tested
    // coverage against `"".chars()` — which yields nothing, so no character
    // could fail to encode and EVERY face came back accepted.
    //
    // ★ The failure was SILENT and INVERTED: the list looked richer, not
    // broken. A query written to stop a shell offering unusable faces became
    // an unconditional yes — strictly worse than the `fontinfo` name-join
    // superset it exists to replace.
    let bytes = std::fs::read(fixture("format_family.pdf")).unwrap();
    let doc = Document::from_bytes(bytes).unwrap();
    let span = first_operator_span(&doc);
    let session = EditSession::new(doc);

    let p = session
        .preview_font_resources(0, "", Some(span))
        .expect("a pinned empty find locates the operator");

    assert_eq!(
        p.text, "hello world",
        "`text` reports the RESOLVED characters, so a caller can read back \
         what was actually tested"
    );
    assert!(
        refusal_of(&p, "F3").is_some(),
        "/F3 cannot show this operator's text and must say so"
    );
    assert_eq!(p.accepted().count(), 2);
}

#[test]
fn the_preflight_and_format_text_agree_about_an_empty_find() {
    // The point of the fix is not that one function got better — it is that
    // the two stopped disagreeing about what the same two operands mean.
    // `FormatRequest::whole_operator` and this query take a page, a pin and no
    // text; if they ever resolve that differently again, a shell's preview
    // and its commit describe different characters.
    let bytes = std::fs::read(fixture("format_family.pdf")).unwrap();
    let doc = Document::from_bytes(bytes).unwrap();
    let span = first_operator_span(&doc);

    let pre = EditSession::new(
        Document::from_bytes(std::fs::read(fixture("format_family.pdf")).unwrap()).unwrap(),
    )
    .preview_font_resources(0, "", Some(span))
    .unwrap();

    // What the pre-flight says /F3 would do…
    assert!(refusal_of(&pre, "F3").is_some());
    // …and what `set_font` actually does, through the same pinned request.
    let err = pdfcer_core::text_edit::set_format(
        &doc,
        &pdfcer_core::text_edit::FormatRequest::whole_operator(0, span)
            .font(pdfcer_core::text_edit::FontSelector::new("F3")),
        &pdfcer_core::text_edit::FormatOptions::default(),
    )
    .expect_err("the commit path refuses too");
    assert!(
        err.to_string().contains("U+006F"),
        "and for the same reason: {err}"
    );
}

#[test]
fn an_empty_find_with_no_pin_is_refused_by_the_same_name_the_commit_path_uses() {
    // ★ THIS TEST CAUGHT A WRONG ASSUMPTION IN THE FIX ABOVE, and that is why
    // it is written as its own case rather than folded in.
    //
    // The first cut of `Pass 147.0` fixed the PINNED half and assumed the
    // unpinned one already errored. It did not: `find_anchor` with no pin runs
    // `s.text.contains(find)`, and EVERY STRING CONTAINS THE EMPTY STRING — so
    // an unpinned empty `find` silently matched the first show operator on the
    // page and surveyed against zero characters. Same silent, inverted
    // failure, about an operator the caller never named.
    //
    // It is refused with the sentence `match_run` has used since Pass 14.1,
    // not a second spelling of it.
    let bytes = std::fs::read(fixture("format_family.pdf")).unwrap();
    let doc = Document::from_bytes(bytes).unwrap();
    let session = EditSession::new(doc);
    let err = session
        .preview_font_resources(0, "", None)
        .expect_err("an unpinned empty find must be refused");
    assert!(
        err.to_string().contains("empty find text"),
        "and by the same name the commit path uses: {err}"
    );
}
