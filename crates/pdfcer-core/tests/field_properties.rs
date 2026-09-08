//! # Three field properties that were readable and unwritable
//!
//! An audit of pdfcer's form surface (2026-09-08) listed the properties the
//! read model exposes with **no writer anywhere in the crate**. These three
//! are the ones an operator actually reaches for, and all three are on
//! Acrobat's own field-properties surface:
//!
//! | Key | Was | Now |
//! |---|---|---|
//! | `/Q` (Table 233) | `Field::quadding` readable, nothing wrote it | `FieldEdit::quadding` |
//! | `/DV` (Table 228) | `Field::default_value` readable, nothing wrote it | `FieldEdit::default_value` |
//! | `Ff` bit 3 NoExport | `FieldFlags::NO_EXPORT` defined and **referenced nowhere else in the workspace** | `FieldEdit::no_export` |
//!
//! ## Why `/DV` matters more than it looks
//!
//! `reset_form` reads `/DV` and removes `/V` where there is none — so with no
//! writer, **a reset could only ever restore defaults some OTHER application
//! had authored.** A form pdfcer built from scratch reset every field to
//! empty regardless of what its author intended, and there was no way to say
//! otherwise. That is the claim the round-trip test below actually pins.
//!
//! ## The three-state shape, and why it is not pedantry
//!
//! `/Q` and `/DV` are `Option<Option<T>>`: absent, explicitly-set, or
//! explicitly-removed. Table 233 defaults `/Q` to left, so `Some(Some(0))`
//! and `Some(None)` **render identically** — and are different facts about
//! the file. A round trip has to preserve which one it met, which is the same
//! contract `max_len` has carried since it shipped.
//!
//! ## And a refusal rather than a clamp
//!
//! Table 233 defines exactly three justifications. A fourth is refused by
//! name: clamping `7` to `2` would silently right-align a field the caller
//! meant to do something else with.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, FieldEdit, NewTextField};
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::Object;
use pdfcer_core::page_tree::Rect;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

const BOX: Rect = Rect {
    llx: 40.0,
    lly: 700.0,
    urx: 240.0,
    ury: 720.0,
};

/// A session with one text field named `t`.
fn with_text_field() -> EditSession {
    let doc = Document::load(&fixture("minimal.pdf")).expect("load minimal.pdf");
    let mut s = EditSession::new(doc);
    s.add_text_field(&NewTextField::new(0, "t", BOX).declining_tooltip())
        .expect("author the text field");
    s
}

fn field(s: &EditSession) -> forms::Field {
    let g = s.graph();
    let form = forms::parse_acroform(&g).expect("an AcroForm");
    form.fields
        .iter()
        .find(|f| f.fully_qualified_name == "t")
        .cloned()
        .expect("the field")
}

/// The field dictionary's raw value for `key`, so absence can be told from a
/// default. The read model resolves defaults; this does not.
fn raw(s: &EditSession, key: &[u8]) -> Option<Object> {
    let g = s.graph();
    let f = field(s);
    g.resolved(f.id)
        .as_dict()
        .and_then(|d| d.get(key))
        .map(|o| g.resolve(o).clone())
}

// ---------------------------------------------------------------------------
// /Q — quadding
// ---------------------------------------------------------------------------

/// Each of Table 233's three values is written and reads back.
#[test]
fn quadding_round_trips_for_all_three_table_233_values() {
    use pdfcer_core::vartext::Quadding;
    for (q, want) in [
        (0, Quadding::Left),
        (1, Quadding::Center),
        (2, Quadding::Right),
    ] {
        let mut s = with_text_field();
        s.edit_field("t", &FieldEdit::new().with_quadding(q))
            .expect("set quadding");
        assert_eq!(field(&s).quadding, want, "quadding {q} did not read back");
    }
}

/// ★ Removing `/Q` is NOT the same as setting it to 0.
///
/// Both render left-justified — Table 233's default is 0 — so a test that
/// only checked the rendered result could not tell them apart. This checks
/// the raw dictionary, which is the only place the difference exists.
#[test]
fn clearing_quadding_removes_the_key_rather_than_writing_zero() {
    let mut s = with_text_field();
    s.edit_field("t", &FieldEdit::new().with_quadding(2))
        .expect("set right-justified");
    assert!(raw(&s, b"Q").is_some(), "setup: /Q should be present");

    s.edit_field("t", &FieldEdit::new().clearing_quadding())
        .expect("clear quadding");
    assert!(
        raw(&s, b"Q").is_none(),
        "clearing must REMOVE /Q, not write 0 — both render left, and only \
         the removal survives a round trip as 'the file said nothing'"
    );

    let mut s2 = with_text_field();
    s2.edit_field("t", &FieldEdit::new().with_quadding(0))
        .expect("set left explicitly");
    assert_eq!(
        raw(&s2, b"Q"),
        Some(Object::Integer(0)),
        "an explicit 0 must be WRITTEN, not optimised away as the default"
    );
}

/// A value outside Table 233 is refused by name, and nothing is written.
///
/// The "nothing is written" half is the one worth having: a refusal that had
/// already mutated the dictionary would leave the field in a state the caller
/// never asked for and was told did not happen.
#[test]
fn an_out_of_range_quadding_is_refused_and_changes_nothing() {
    let mut s = with_text_field();
    s.edit_field("t", &FieldEdit::new().with_quadding(1))
        .expect("set centred");

    match s.edit_field("t", &FieldEdit::new().with_quadding(7)) {
        Err(EditError::QuaddingInvalid { given }) => assert_eq!(given, 7),
        other => panic!("expected QuaddingInvalid, got {other:?}"),
    }
    assert_eq!(
        field(&s).quadding,
        pdfcer_core::vartext::Quadding::Center,
        "a refused edit must leave the previous value intact"
    );
}

// ---------------------------------------------------------------------------
// /DV — the default value a reset restores
// ---------------------------------------------------------------------------

/// ★★ The claim that matters: a form pdfcer authored can now be reset to a
/// default pdfcer authored.
///
/// Before this, `/DV` was readable and unwritable, so `reset_form` could only
/// restore defaults another application had written — a pdfcer-built form
/// reset every field to empty whatever its author intended. This fills a
/// field, resets it, and asserts the DEFAULT comes back rather than emptiness.
#[test]
fn a_default_value_survives_a_reset_which_is_the_whole_point() {
    let mut s = with_text_field();
    s.edit_field("t", &FieldEdit::new().with_default_value("factory"))
        .expect("set /DV");
    assert_eq!(field(&s).default_value.display_text(), "factory");

    s.fill_text_field("t", "typed by the operator")
        .expect("fill");
    s.reset_form(None).expect("reset");

    let after = field(&s);
    assert_eq!(
        after.value.display_text(),
        "factory",
        "reset must restore the default pdfcer authored, not clear the field"
    );
}

/// Clearing `/DV` makes a reset CLEAR the field — the other half of the
/// contract, and the one a caller reaches for to undo the above.
#[test]
fn clearing_the_default_value_makes_a_reset_empty_the_field() {
    let mut s = with_text_field();
    s.edit_field("t", &FieldEdit::new().with_default_value("factory"))
        .expect("set /DV");
    s.edit_field("t", &FieldEdit::new().clearing_default_value())
        .expect("clear /DV");
    assert!(raw(&s, b"DV").is_none(), "/DV must be gone");

    s.fill_text_field("t", "typed").expect("fill");
    s.reset_form(None).expect("reset");
    assert!(
        field(&s).value.display_text().is_empty(),
        "with no /DV a reset clears the field"
    );
}

// ---------------------------------------------------------------------------
// Ff bit 3 — NoExport
// ---------------------------------------------------------------------------

/// The flag that was defined and referenced nowhere else in the workspace.
///
/// Asserted on the READ model rather than on the raw integer, so the test
/// would catch the bit being written at the wrong position — which is the
/// only interesting way this can be wrong.
#[test]
fn no_export_can_be_set_and_cleared() {
    let mut s = with_text_field();
    assert!(
        field(&s).flags.0 & forms::FieldFlags::NO_EXPORT == 0,
        "a freshly authored field does not carry NoExport"
    );

    s.edit_field("t", &FieldEdit::new().with_no_export(true))
        .expect("set NoExport");
    assert!(
        field(&s).flags.0 & forms::FieldFlags::NO_EXPORT != 0,
        "NoExport did not take — check the bit position (Table 226 bit 3)"
    );

    s.edit_field("t", &FieldEdit::new().with_no_export(false))
        .expect("clear NoExport");
    assert!(field(&s).flags.0 & forms::FieldFlags::NO_EXPORT == 0);
}

/// Setting one flag must not disturb its neighbours.
///
/// `Ff` is a single integer, so every flag edit is a read-modify-write of a
/// word shared by a dozen properties. The failure mode is silent and total:
/// a wrong mask clears Required and ReadOnly while appearing to work.
#[test]
fn setting_no_export_leaves_the_other_flags_alone() {
    let mut s = with_text_field();
    s.edit_field(
        "t",
        &FieldEdit::new()
            .with_required(true)
            .with_read_only(true)
            .with_multiline(true),
    )
    .expect("set three flags");

    s.edit_field("t", &FieldEdit::new().with_no_export(true))
        .expect("set NoExport");

    let f = field(&s);
    assert!(
        f.flags.0 & forms::FieldFlags::REQUIRED != 0,
        "Required lost"
    );
    assert!(
        f.flags.0 & forms::FieldFlags::READ_ONLY != 0,
        "ReadOnly lost"
    );
    assert!(
        f.flags.0 & forms::FieldFlags::MULTILINE != 0,
        "Multiline lost"
    );
    assert!(f.flags.0 & forms::FieldFlags::NO_EXPORT != 0);
}

// ---------------------------------------------------------------------------
// The four advisory flags and /TM — the residue, 2026-09-08
// ---------------------------------------------------------------------------

/// The four remaining `Ff` bits, each of which the read model exposed with no
/// writer — and three of which had **zero references** anywhere outside
/// `forms.rs`.
///
/// Table-driven and asserted on the READ model rather than the raw integer,
/// so a bit written at the wrong position fails here. That is the only
/// interesting way any of these can be wrong: each is a single bit with no
/// behaviour of pdfcer's own attached, so "does it round-trip at the right
/// position" is the whole contract.
#[test]
fn the_four_advisory_flags_round_trip_at_their_table_228_positions() {
    let cases: [(&str, fn(FieldEdit, bool) -> FieldEdit, u32); 4] = [
        (
            "FileSelect",
            FieldEdit::with_file_select,
            forms::FieldFlags::FILE_SELECT,
        ),
        (
            "DoNotSpellCheck",
            FieldEdit::with_no_spell_check,
            forms::FieldFlags::DO_NOT_SPELL_CHECK,
        ),
        (
            "DoNotScroll",
            FieldEdit::with_no_scroll,
            forms::FieldFlags::DO_NOT_SCROLL,
        ),
        (
            "CommitOnSelChange",
            FieldEdit::with_commit_on_sel_change,
            forms::FieldFlags::COMMIT_ON_SEL_CHANGE,
        ),
    ];
    for (name, set, bit) in cases {
        let mut s = with_text_field();
        assert!(
            field(&s).flags.0 & bit == 0,
            "{name}: a fresh field should not carry it"
        );
        s.edit_field("t", &set(FieldEdit::new(), true))
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert!(
            field(&s).flags.0 & bit != 0,
            "{name} did not take — check the bit position against Table 228/230"
        );
        s.edit_field("t", &set(FieldEdit::new(), false))
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert!(field(&s).flags.0 & bit == 0, "{name} did not clear");
    }
}

/// `/TM` is written and removed, and removing it is not the same as writing
/// an empty string.
///
/// It is what an EXPORT keys on, so a wrong value here changes the payload a
/// consumer receives while changing nothing visible on the page — the class
/// of defect nobody notices by looking.
#[test]
fn the_mapping_name_is_written_and_removed() {
    let mut s = with_text_field();
    assert!(raw(&s, b"TM").is_none(), "setup: no /TM yet");

    s.edit_field("t", &FieldEdit::new().with_mapping_name("total_due"))
        .expect("set /TM");
    assert!(raw(&s, b"TM").is_some(), "/TM must be written");

    s.edit_field("t", &FieldEdit::new().clearing_mapping_name())
        .expect("clear /TM");
    assert!(
        raw(&s, b"TM").is_none(),
        "clearing must REMOVE /TM so the export reverts to the field's own name, not write an empty one that exports as a blank key"
    );
}
