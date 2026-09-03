//! Composite (`/Type0` / CIDFont) runs: what edits, and what is refused.
//!
//! # What this file used to pin, and why it changed
//!
//! It asserted that editing ANY composite run refused by name rather than
//! reporting `NoMatch`. That was the right test while composite runs were
//! uneditable: the defect it caught was `edit-text` classifying the font
//! *after* `match_run`, so an operator whose text was plainly on the page was
//! told it was absent — two different problems, two different next actions,
//! and only the wrong one on offer.
//!
//! Composite runs are editable as of Pass 29.0, so "every composite refuses"
//! is no longer true and the test would have been asserting a limitation
//! instead of a guarantee. The guarantees worth pinning now are two:
//!
//! 1. a composite font whose `/ToUnicode` **inverts** is genuinely editable,
//!    end to end, verified by reading the text back rather than by a success
//!    return;
//! 2. a composite font whose map **cannot** be inverted is refused BY NAME,
//!    with the obstruction stated — because that is a property of the font
//!    that no amount of pdfcer work will fix, and an operator needs to know
//!    which kind of "no" they are looking at (R110).
//!
//! # The fixture that had to be built for this
//!
//! Neither existing composite fixture could carry test 2 once editing worked.
//! The injective one now EDITS — it is test 1. The no-`/ToUnicode` one cannot
//! serve either, and the reason is worth stating because it is not obvious:
//! its text does not decode at all, so no anchor is ever found and `NoMatch`
//! is the honest answer — the test would pass without ever reaching the
//! refusal. So `cidfonttype2-noninjective-tounicode.pdf` exists: two CIDs
//! mapping to the same character, a map that is present and decodable and
//! still not a function.

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::text_edit::{EditError, EditOptions, EditRequest, RInvTrigger, edit_text};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/text")
        .join(name)
}

fn load(name: &str) -> Document {
    let path = fixture(name);
    let bytes =
        std::fs::read(&path).unwrap_or_else(|e| panic!("missing fixture {}: {e}", path.display()));
    Document::from_bytes(bytes).expect("fixture parses")
}

/// **The capability.** An invertible composite font edits, end to end.
///
/// Asserted by reading the text back out of the SAVED document, not by the
/// call returning `Ok`. A composite edit that wrote the wrong CIDs would
/// return `Ok` and render confidently wrong glyphs — the failure mode this
/// whole encoding path exists to avoid — and only a round trip catches it.
#[test]
fn an_invertible_composite_run_is_editable_end_to_end() {
    let doc = load("composite-editable.pdf");
    let out = edit_text(
        &doc,
        &EditRequest::find_replace(0, "ABC", "CBA"),
        &EditOptions::default(),
    )
    .expect("an invertible composite font must be editable");

    let edited = Document::from_bytes(out.bytes).expect("the edited document re-parses");
    let pages = pdfcer_core::page_tree::pages(&edited).expect("pages");
    let page = pages.first().expect("one page");
    let text = pdfcer_core::text_extract::extract_page(
        &edited,
        page,
        0,
        &pdfcer_core::text_extract::ExtractOptions::default(),
    )
    .expect("the edited page still yields text")
    .runs
    .iter()
    .map(|r| r.text.clone())
    .collect::<String>();
    assert!(
        text.contains("CBA"),
        "the replacement must be readable back out of the saved file; got {text:?}"
    );
    assert!(
        !text.contains("ABC"),
        "the original must be gone, not merely overdrawn; got {text:?}"
    );
}

/// **The refusal that remains, and must.** A non-injective map is refused by
/// name, naming the obstruction.
///
/// Two codes map to the same character, so writing that character back has no
/// single answer. Guessing would emit a real, wrong glyph — indistinguishable
/// from correct output on screen, which is exactly why this refuses instead.
#[test]
fn a_non_injective_composite_font_is_refused_by_name() {
    let doc = load("cidfonttype2-noninjective-tounicode.pdf");
    let err = edit_text(
        &doc,
        &EditRequest::find_replace(0, "A", "A"),
        &EditOptions::default(),
    )
    .expect_err("a font whose map is not a function must not be edited");

    match err {
        EditError::Refused(r) => {
            assert_eq!(r.trigger, RInvTrigger::Composite);
            let msg = r.message;
            assert!(
                msg.contains("cannot be inverted"),
                "the refusal must name the obstruction: {msg}"
            );
            // R110: the operator has to be able to tell "this font can never
            // be edited" from "pdfcer cannot do it yet". This is the first.
            assert!(
                !msg.contains("does not handle"),
                "this refusal is about the FONT, and must not read as a pdfcer \
                 limitation the operator could wait out: {msg}"
            );
            // A message with runs of spaces has been shipped three times in
            // this project from `\`-continuations in format strings.
            assert!(
                !msg.contains("  "),
                "the message must not contain collapsed-continuation whitespace: {msg}"
            );
        }
        other => panic!("expected the composite refusal, got {other:?}"),
    }
}

/// The positive control: a SIMPLE embedded font still edits.
///
/// The change reorders and rewrites a shared code path, so the way it could go
/// wrong is by breaking the path that has always worked — which would satisfy
/// both assertions above while breaking in-place editing for every document
/// that has ever worked.
#[test]
fn a_simple_font_run_still_edits() {
    let doc = load("subset-simple-embedded.pdf");
    let out = edit_text(
        &doc,
        &EditRequest::find_replace(0, "ABC", "ACB"),
        &EditOptions::default(),
    )
    .expect("a simple embedded font must still be editable");
    assert!(!out.bytes.is_empty());
    Document::from_bytes(out.bytes).expect("the edited document must re-parse");
}
