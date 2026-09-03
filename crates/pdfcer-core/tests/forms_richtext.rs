//! Rich-text form fields (`/Ff` bit 26): the refusal that prevents a WRONG
//! VALUE, and the explicit downgrade that is the way through.
//!
//! ## Why the refusal is a correctness guard, not a fidelity one
//!
//! ISO 32000-1 §12.7.3.4 says the rich text string *"in addition to the `RV`
//! or `RC` entry, shall be used to generate the appearance"*, and §12.7.3.3
//! requires regeneration on every value change for these fields. Appearance
//! generation is therefore bound to `/RV`, **not** `/V`. Writing a fresh `/V`
//! while leaving `/RV` in place yields a field whose appearance a conforming
//! reader rebuilds from the OLD text — the document displays a value nobody
//! typed.
//!
//! The fixture makes that visible on purpose: its `/V` and `/RV` say
//! DIFFERENT things, so a test cannot pass by the two happening to agree.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::object::Object;
use std::path::{Path, PathBuf};

/// Fixture paths are resolved from `CARGO_MANIFEST_DIR`, not the CWD — an
/// integration test runs with the CRATE root as its working directory, not the
/// workspace root. Same helper shape as `add_text.rs`.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/forms")
        .join(name)
}

fn session() -> EditSession {
    EditSession::new(
        Document::load(&fixture("radio-choice-form.pdf")).expect("load the rich-text fixture"),
    )
}

fn notes_field(session: &EditSession) -> pdfcer_core::forms::Field {
    pdfcer_core::forms::parse_acroform(&session.graph())
        .expect("the fixture has an AcroForm")
        .fields
        .into_iter()
        .find(|f| f.fully_qualified_name == "Notes")
        .expect("the fixture has a Notes field")
}

/// The fixture is what the tests below claim it is. Asserted rather than
/// assumed: every test here is meaningless if bit 26 is not actually set.
#[test]
fn the_fixture_field_really_is_rich_text_with_a_disagreeing_plain_twin() {
    let s = session();
    let notes = notes_field(&s);
    assert!(notes.is_rich_text(), "Notes must be a rich-text field");
    let Some(Object::Dict(d)) = s.value(notes.id) else {
        panic!("Notes is not a dict");
    };
    assert!(d.get(b"RV").is_some(), "the fixture must carry /RV");
    assert!(d.get(b"DS").is_some(), "the fixture must carry /DS");
    // Decoded, not `{:?}`-formatted: `Object::String`'s Debug prints a byte
    // list, so a `contains("<b>")` against it fails while the markup is
    // present — which is exactly what happened on the first run of this test.
    let Some(Object::String(rv_bytes)) = d.get(b"RV") else {
        panic!("/RV is not a string");
    };
    let rv = String::from_utf8_lossy(rv_bytes);
    assert!(rv.contains("<b>"), "the /RV must carry real markup: {rv}");
    // The plain twin and the rich value disagree ON PURPOSE — see the module
    // doc. If they ever agree, the correctness bug these tests guard becomes
    // invisible and the suite silently stops proving anything.
    assert!(
        rv.contains("ORIGINAL") && notes.value.display_text().contains("ORIGINAL"),
        "fixture drift: /V and /RV must both mention ORIGINAL"
    );
}

/// A plain fill is REFUSED BY NAME, before anything is written.
#[test]
fn a_plain_fill_of_a_rich_text_field_is_refused_by_name() {
    let mut s = session();
    let err = s
        .fill_text_field("Notes", "plain replacement")
        .expect_err("a plain fill must not silently corrupt a rich-text field");
    assert!(
        matches!(err, EditError::FieldIsRichText { ref name } if name == "Notes"),
        "expected FieldIsRichText, got {err:?}"
    );
    assert!(
        !s.is_modified(),
        "a refusal must leave the session untouched — rule 4: refuse BEFORE mutating"
    );
}

/// The explicit downgrade accepts it, and leaves a CONSISTENT plain field.
///
/// All four postconditions matter together. Any one of them alone leaves the
/// document in a state that is either malformed or still wrong on screen:
/// clearing the flag but keeping `/RV` leaves the old text recoverable and the
/// dictionary self-contradictory; removing `/RV` but keeping the flag makes a
/// rich-text field with no rich value.
#[test]
fn the_explicit_downgrade_converts_the_field_and_removes_every_rich_entry() {
    let mut s = session();
    s.fill_text_field_downgrading_rich_text("Notes", "plain replacement")
        .expect("the explicit downgrade accepts a rich-text field");

    let notes = notes_field(&s);
    assert!(
        !notes.is_rich_text(),
        "1/4: the field must no longer be rich text"
    );
    assert_eq!(
        notes.value.display_text(),
        "plain replacement",
        "2/4: /V must hold exactly what was typed"
    );
    let Some(Object::Dict(d)) = s.value(notes.id) else {
        panic!("Notes is not a dict after the downgrade");
    };
    assert!(
        d.get(b"RV").is_none(),
        "3/4: /RV must be REMOVED — a stale rich value would still drive the appearance, \
         and would leave the old text recoverable from the dictionary"
    );
    assert!(
        d.get(b"DS").is_none(),
        "4/4: /DS styles the rich value and means nothing without one"
    );
}

/// One undo puts the whole downgrade back — flag, `/RV`, `/DS` and value —
/// because it is ONE command, not four.
#[test]
fn undoing_the_downgrade_restores_the_rich_text_field_whole() {
    let mut s = session();
    s.fill_text_field_downgrading_rich_text("Notes", "plain replacement")
        .unwrap();
    s.undo().expect("undo the downgrade");

    let notes = notes_field(&s);
    assert!(notes.is_rich_text(), "the RichText flag must come back");
    let Some(Object::Dict(d)) = s.value(notes.id) else {
        panic!("Notes is not a dict after undo");
    };
    assert!(d.get(b"RV").is_some(), "/RV must come back");
    assert!(d.get(b"DS").is_some(), "/DS must come back");
    assert!(!s.is_modified(), "one undo restores the pristine session");
}

/// The downgrade survives save-and-reopen — it is a real document change, not
/// a session-only view of one.
#[test]
fn the_downgrade_round_trips_through_a_save() {
    let mut s = session();
    s.fill_text_field_downgrading_rich_text("Notes", "plain replacement")
        .unwrap();
    let bytes = s
        .to_incremental_bytes(&pdfcer_core::writer::SaveOptions::identity())
        .unwrap()
        .0;

    let reopened = EditSession::new(Document::from_bytes(bytes).unwrap());
    let notes = notes_field(&reopened);
    assert!(!notes.is_rich_text());
    assert_eq!(notes.value.display_text(), "plain replacement");
    let Some(Object::Dict(d)) = reopened.value(notes.id) else {
        panic!("not a dict");
    };
    assert!(d.get(b"RV").is_none(), "/RV must stay gone after a save");
}

/// The downgrade does NOT touch an ordinary text field's flags — it is
/// conditional on the field actually being rich text, not applied blindly.
#[test]
fn the_downgrade_entry_point_is_harmless_on_a_plain_field() {
    let mut s = EditSession::new(
        Document::load(&fixture("demo-form.pdf")).expect("load the plain fixture"),
    );
    s.fill_text_field_downgrading_rich_text("FullName", "Ken")
        .expect("a plain field fills normally through this entry point too");
    let form = pdfcer_core::forms::parse_acroform(&s.graph()).unwrap();
    let f = form
        .fields
        .iter()
        .find(|f| f.fully_qualified_name == "FullName")
        .unwrap();
    assert_eq!(f.value.display_text(), "Ken");
    assert!(!f.is_rich_text());
}

// ---------------------------------------------------------------------------
// FDF/XFDF IMPORT meets a rich-text field
// ---------------------------------------------------------------------------
//
// `fill_text_field` refuses a rich-text field, and correctly: §12.7.3.4 makes
// `/DS` + `/RV` the inputs to appearance generation, so writing `/V` and
// leaving `/RV` stale makes every conforming reader regenerate from the OLD
// text. The spec RAG's own summary is blunt about it — "not a cosmetic loss,
// a wrong value on screen."
//
// `import_form_data` reaches that refusal through `?`. These tests pin what
// that means for a data file, which nothing covered before.

fn field_data(name: &str, value: &str) -> pdfcer_core::fdf::FieldData {
    pdfcer_core::fdf::FieldData {
        name: name.to_owned(),
        values: vec![value.to_owned()],
        rich_value: None,
    }
}

/// **A rich-text field in a data file is SKIPPED, not fatal.**
///
/// It is the same shape as the signature arm directly above it in the import
/// loop: an entry pdfcer cannot apply, counted in `skipped` so the caller can
/// see it did not land. Aborting instead would be a strictly worse answer to
/// the same question — see the partial-application test below for why.
#[test]
fn importing_into_a_rich_text_field_is_skipped_rather_than_fatal() {
    let mut s = session();
    let data = pdfcer_core::fdf::FormData {
        fields: vec![field_data("Notes", "plain text from a data file")],
    };

    let outcome = s
        .import_form_data(&data)
        .expect("a rich-text entry must not fail the import");
    assert_eq!(outcome.applied, 0, "nothing was applied");
    assert_eq!(
        outcome.skipped, 1,
        "and the operator is told one entry did not land"
    );

    // The field is untouched: still rich, still carrying its original /RV.
    let notes = notes_field(&s);
    assert!(notes.is_rich_text(), "the field is still rich text");
    let Some(Object::Dict(d)) = s.value(notes.id) else {
        panic!("Notes is not a dict");
    };
    assert!(
        d.get(b"RV").is_some(),
        "and its /RV survives — a skipped import writes nothing at all"
    );
}

/// **★ A rich-text entry must not abandon an import half-applied.**
///
/// This is why skipping beats aborting. The loop writes each entry to the
/// overlay as it goes, so a `?` on entry two leaves entry one already
/// written AND hands the caller an `Err` — a document that has been changed
/// by a call that reported failure, with nothing saying how far it got.
///
/// The data file is ordered deliberately: a field that CAN be applied first,
/// the rich-text one second.
#[test]
fn a_rich_text_entry_does_not_abandon_the_import_half_applied() {
    let mut s = session();
    let data = pdfcer_core::fdf::FormData {
        fields: vec![
            field_data("Country", "CA"),
            field_data("Notes", "plain text from a data file"),
        ],
    };

    let outcome = s
        .import_form_data(&data)
        .expect("the applicable entry must still be applied");
    assert_eq!(outcome.applied, 1, "Country landed");
    assert_eq!(outcome.skipped, 1, "Notes did not, and is counted");

    // And the one that landed really did.
    let form = pdfcer_core::forms::parse_acroform(&s.graph()).expect("AcroForm");
    let country = form.field_by_name("Country").expect("Country exists");
    assert_eq!(
        country.value.display_text(),
        "CA",
        "the applicable entry was applied, not rolled back"
    );
}

/// The refusal `import_form_data` routes around is still there for a DIRECT
/// caller — skipping is the importer's policy, not a weakening of the core
/// guard.
#[test]
fn fill_text_field_still_refuses_a_rich_text_field_directly() {
    let mut s = session();
    let err = s
        .fill_text_field("Notes", "plain text")
        .expect_err("the direct verb must still refuse");
    assert!(
        matches!(err, EditError::FieldIsRichText { .. }),
        "and refuse by NAME, so a caller can route on it; got {err:?}"
    );
}

/// **★ NO per-entry failure abandons the import half-applied — not just
/// rich text.**
///
/// The rich-text case was merely the reachable instance. Every verb the
/// import loop calls COMMITS, so a `?` on any of them wrote everything
/// before it and then reported failure. This drives a button entry with an
/// on-state the document does not have, which `set_button_state` refuses,
/// and asserts the entries either side of it still landed.
#[test]
fn a_refused_entry_of_any_kind_is_skipped_not_fatal() {
    let mut s = session();
    let data = pdfcer_core::fdf::FormData {
        fields: vec![
            field_data("Country", "CA"),
            // An on-state this check box does not have. Refused per-entry.
            field_data("Colour", "NoSuchStateExists"),
            field_data("Notes", "rich text, also skipped"),
        ],
    };

    let outcome = s
        .import_form_data(&data)
        .expect("a per-entry refusal must not fail the whole import");
    assert_eq!(
        outcome.applied + outcome.skipped,
        3,
        "every entry is accounted for: applied={} skipped={}",
        outcome.applied,
        outcome.skipped
    );
    assert!(
        outcome.applied >= 1,
        "the applicable entry still landed; applied={}",
        outcome.applied
    );

    let form = pdfcer_core::forms::parse_acroform(&s.graph()).expect("AcroForm");
    assert_eq!(
        form.field_by_name("Country")
            .expect("Country exists")
            .value
            .display_text(),
        "CA",
        "an entry BEFORE the refused one is not rolled back and not lost"
    );
}

// ---------------------------------------------------------------------------
// `/RV` survives EXPORT — the first slice of Pass 37.3
// ---------------------------------------------------------------------------
//
// Carrying the rich value OUT is safe and purely additive. Writing it back
// IN is deliberately not done yet: §12.7.3.3 makes `/DS` + `/RV` the inputs
// to appearance generation with an unconditional `shall` to regenerate on
// every value change (RT-M9, not gated by `/NeedAppearances` — RT-N7), and
// pdfcer cannot yet generate a rich-text appearance. Writing `/RV` without
// that would leave the stored value and the rendered one disagreeing, which
// is exactly what `fill_text_field` refuses for.

/// **The field's `/RV` reaches the exported FDF and XFDF.**
///
/// Before this, both formats have the slot and pdfcer wrote neither — so a
/// styled field exported and came back plain, and the operator found out on
/// the re-import.
#[test]
fn the_rich_value_reaches_both_export_formats() {
    let s = session();
    let form = pdfcer_core::forms::parse_acroform(&s.graph()).expect("AcroForm");

    let notes = form.field_by_name("Notes").expect("Notes exists");
    assert!(
        notes.rich_value.is_some(),
        "the fixture's Notes carries /RV — every assertion below is vacuous otherwise"
    );

    let data = pdfcer_core::fdf::FormData::from_acroform(&form);
    let entry = data
        .fields
        .iter()
        .find(|f| f.name == "Notes")
        .expect("Notes is exported");
    let rich = entry
        .rich_value
        .as_ref()
        .expect("and carries its rich value");

    let fdf = String::from_utf8_lossy(&data.to_fdf(None)).into_owned();
    assert!(fdf.contains("/RV"), "FDF Table 246's key is written: {fdf}");

    let xfdf = String::from_utf8_lossy(&data.to_xfdf(None)).into_owned();
    assert!(
        xfdf.contains("<value-richtext>"),
        "XFDF's slot is written: {xfdf}"
    );
    // The rich value is XML, and it is escaped as TEXT rather than embedded
    // raw — otherwise its own markup would merge into the XFDF's element
    // tree and a `<span>` in a field value would become an XFDF element.
    if rich.contains('<') {
        assert!(
            xfdf.contains("&lt;"),
            "the embedded XML is escaped, not merged into the tree: {xfdf}"
        );
    }
}

/// **And it survives a parse back — both formats round-trip.**
///
/// The export is only non-destructive if something can read it again. This
/// asserts the value that comes back is the value that went out, not merely
/// that a slot was populated.
#[test]
fn the_rich_value_round_trips_through_fdf_and_xfdf() {
    let s = session();
    let form = pdfcer_core::forms::parse_acroform(&s.graph()).expect("AcroForm");
    let data = pdfcer_core::fdf::FormData::from_acroform(&form);
    let original = data
        .fields
        .iter()
        .find(|f| f.name == "Notes")
        .and_then(|f| f.rich_value.clone())
        .expect("Notes has a rich value to round-trip");

    for (label, bytes) in [("FDF", data.to_fdf(None)), ("XFDF", data.to_xfdf(None))] {
        let parsed = if label == "FDF" {
            pdfcer_core::fdf::FormData::parse_fdf(&bytes)
        } else {
            pdfcer_core::fdf::FormData::parse_xfdf(&bytes)
        }
        .unwrap_or_else(|e| panic!("{label} re-parse failed: {e}"));

        let back = parsed
            .fields
            .iter()
            .find(|f| f.name == "Notes")
            .unwrap_or_else(|| panic!("{label}: Notes missing after re-parse"))
            .rich_value
            .clone()
            .unwrap_or_else(|| panic!("{label}: the rich value was dropped"));

        assert_eq!(back, original, "{label}: the rich value changed in transit");
    }
}

/// **A plain form's export is unchanged** — no empty slot, no new key.
///
/// `rich_value` is `None` for the overwhelming majority of fields, and this
/// pins that the feature is invisible on them. A `/RV` emitted as an empty
/// string would make every plain field look like a degenerate rich one.
#[test]
fn a_plain_form_gains_no_rich_text_markup() {
    let s = pdfcer_core::edit::EditSession::new(
        Document::load(&fixture("demo-form.pdf")).expect("load the plain fixture"),
    );
    let form = pdfcer_core::forms::parse_acroform(&s.graph()).expect("AcroForm");
    let data = pdfcer_core::fdf::FormData::from_acroform(&form);
    assert!(
        data.fields.iter().all(|f| f.rich_value.is_none()),
        "no plain field invents a rich value"
    );

    let fdf = String::from_utf8_lossy(&data.to_fdf(None)).into_owned();
    assert!(!fdf.contains("/RV"), "no /RV key on a plain form: {fdf}");
    let xfdf = String::from_utf8_lossy(&data.to_xfdf(None)).into_owned();
    assert!(
        !xfdf.contains("value-richtext"),
        "no rich-text element on a plain form: {xfdf}"
    );
}
