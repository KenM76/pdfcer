//! # Radio group authoring (decision 020 F2)
//!
//! ## What this file is asserting, and why it is a separate file
//!
//! A radio group is the first authored field whose identity is spread across
//! several objects that must agree. A text field or a check box can be wrong
//! in one dictionary; a radio group can be wrong in the *relationship between*
//! four of them — the field, and one widget per member — while every
//! individual dictionary is impeccable.
//!
//! The specific failure that shape invites, and that these tests exist to
//! catch: **three `add_radio_button` calls producing three FIELDS rather than
//! one field with three widgets.** Such a document parses. `list-fields`
//! reports plausible output. Every widget renders. And it is the duplicate-FQN
//! document §12.7.3.2 forbids — three independent groups sharing one name,
//! with no disambiguator, which nothing downstream can repair because the file
//! format records no fact to repair it *to*.
//!
//! So the load-bearing assertion in this file is `fields == 1`, and several
//! tests below restate it rather than assuming an earlier one established it.
//!
//! ## The verification standard: use the UNMODIFIED fill path
//!
//! `add_radio_button` contains no code implementing mutual exclusion. It sets
//! `/Ff` bit 16, draws a round widget, and merges. Everything that makes a
//! radio group *behave* like one comes from the already-shipped
//! [`EditSession::set_button_state`], which sets each widget's `/AS` to the
//! requested state when that widget offers it and `/Off` otherwise.
//!
//! That is deliberate and it is what
//! [`selecting_one_member_clears_the_others_through_the_shipped_fill_path`]
//! proves: if an untouched consumer written before radio authoring existed
//! accepts these fields and behaves correctly, the authored thing is a real
//! radio group rather than a dictionary that happens to parse. A test that
//! called a radio-specific helper would prove only that the helper agrees with
//! the writer that produced it.
//!
//! ## Spec basis
//!
//! * §12.7.4.2.1 (Table 226) — `Radio` bit 16, `NoToggleToOff` bit 15,
//!   `RadiosInUnison` bit 26; members distinguished by on-state name.
//! * §12.7.4.2.3 — `Off` is the reserved off-state name.
//! * §12.7.3.2 — the fully-qualified name is the field's identity.
//! * §12.5.5 — `/AS` selects which `/AP /N` sub-stream paints.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, NewCheckBox, NewRadioButton};
use pdfcer_core::forms::{self, ButtonKind, FieldFlags, FieldType, FieldValue};
use pdfcer_core::forms_author::FormAuthorError;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::Object;
use pdfcer_core::page_tree::Rect;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(name)
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(rel)).expect("load fixture"))
}

/// Member `i`'s rectangle, stacked down the page so no two overlap.
fn member_rect(i: usize) -> Rect {
    let top = 720.0 - (i as f64) * 30.0;
    Rect::from_corners(72.0, top - 20.0, 92.0, top)
}

/// Build a group of `members` in one session, on a formless fixture.
///
/// The fixture has no `/AcroForm` at all, so every test here proves creation
/// from nothing rather than appending to a form somebody else built.
fn group_of(members: &[&str]) -> EditSession {
    let mut s = session("dimension/plain-base.pdf");
    for (i, v) in members.iter().enumerate() {
        s.add_radio_button(
            &NewRadioButton::new(0, "Colour", member_rect(i), *v).declining_tooltip(),
        )
        .unwrap_or_else(|e| panic!("member {v} joins the group: {e}"));
    }
    s
}

/// The single named field, or a panic naming what was found instead.
fn the_group(s: &EditSession, fqn: &str) -> forms::Field {
    let form = forms::parse_acroform(&s.graph()).expect("the form parses");
    let named: Vec<_> = form
        .fields
        .iter()
        .filter(|f| f.fully_qualified_name == fqn)
        .collect();
    assert_eq!(
        named.len(),
        1,
        "`{fqn}` must be exactly ONE field; {} were found, which is the \
         duplicate-FQN document §12.7.3.2 forbids",
        named.len()
    );
    named[0].clone()
}

/// Three calls build ONE field with THREE widgets — F2's acceptance criterion.
#[test]
fn three_calls_build_one_group_with_three_members() {
    let s = group_of(&["Red", "Green", "Blue"]);
    let g = the_group(&s, "Colour");

    assert_eq!(g.widgets.len(), 3, "one field, three widgets");
    assert_eq!(g.field_type, Some(FieldType::Button));
    assert_eq!(
        g.button_kind,
        Some(ButtonKind::Radio),
        "`/Ff` bit 16 is the entire radio type declaration — with it clear a \
         `/Btn` field IS a check box (§12.7.4.2.1)"
    );
    assert!(
        !g.widgets[0].merged,
        "a multi-member group is Shape B: the field is a PARENT, not its own \
         widget. Shape A here would mean the merge never promoted."
    );
}

/// Every member offers its own export value and `Off`, and no other state.
#[test]
fn each_member_offers_exactly_its_own_state_and_off() {
    let s = group_of(&["Red", "Green", "Blue"]);
    let g = the_group(&s, "Colour");

    let mut seen: Vec<String> = Vec::new();
    for w in &g.widgets {
        let mut states: Vec<String> = w
            .on_states
            .iter()
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect();
        states.sort();
        assert_eq!(
            states.len(),
            1,
            "a member offers exactly ONE on state; {states:?} means its `/AP /N` \
             keys are wrong"
        );
        seen.push(states.remove(0));
    }
    seen.sort();
    assert_eq!(
        seen,
        vec!["Blue".to_owned(), "Green".to_owned(), "Red".to_owned()],
        "the three members export the three requested values"
    );
}

/// Selecting a member through the shipped, unmodified fill path lights
/// exactly one widget.
///
/// See this file's header: nothing in `add_radio_button` implements mutual
/// exclusion, so this passing is what proves the grouping is genuine.
#[test]
fn selecting_one_member_clears_the_others_through_the_shipped_fill_path() {
    let mut s = group_of(&["Red", "Green", "Blue"]);
    s.set_button_state("Colour", "Green")
        .expect("the shipped fill path accepts an authored group");

    let g = the_group(&s, "Colour");
    assert_eq!(
        g.value,
        FieldValue::Name(b"Green".to_vec()),
        "`/V` names the chosen member"
    );

    let graph = s.graph();
    let mut lit = Vec::new();
    for w in &g.widgets {
        let appearance_state = graph
            .resolved(w.id)
            .as_dict()
            .and_then(|d| d.get(b"AS"))
            .and_then(Object::as_name)
            .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned())
            .expect("every widget carries `/AS` (§12.5.5)");
        if appearance_state != "Off" {
            lit.push(appearance_state);
        }
    }
    assert_eq!(
        lit,
        vec!["Green".to_owned()],
        "exactly one widget may paint as selected; {lit:?} is what a group \
         that is not really one looks like"
    );
}

/// Clearing the group to `Off` turns every member off.
#[test]
fn clearing_the_group_turns_every_member_off() {
    let mut s = group_of(&["Red", "Green", "Blue"]);
    s.set_button_state("Colour", "Red").unwrap();
    s.set_button_state("Colour", "Off").unwrap();

    let g = the_group(&s, "Colour");
    let graph = s.graph();
    for w in &g.widgets {
        let appearance_state = graph
            .resolved(w.id)
            .as_dict()
            .and_then(|d| d.get(b"AS"))
            .and_then(Object::as_name)
            .map(|n| n.as_bytes().to_vec())
            .unwrap();
        assert_eq!(
            appearance_state, b"Off",
            "no member stays lit after a clear"
        );
    }
}

/// A member created `selected` is the group's value, and adding a later
/// member does not disturb it.
#[test]
fn an_initial_selection_survives_a_later_member_joining() {
    let mut s = session("dimension/plain-base.pdf");
    s.add_radio_button(
        &NewRadioButton::new(0, "Colour", member_rect(0), "Red")
            .declining_tooltip()
            .selected(true),
    )
    .unwrap();
    s.add_radio_button(
        &NewRadioButton::new(0, "Colour", member_rect(1), "Green").declining_tooltip(),
    )
    .unwrap();

    let g = the_group(&s, "Colour");
    assert_eq!(
        g.value,
        FieldValue::Name(b"Red".to_vec()),
        "the second member must not silently re-point a selection the first made"
    );
}

/// Two members cannot share an export value — unless the group asked to
/// select in unison, which is exactly what `/Ff` bit 26 requests.
#[test]
fn a_duplicate_export_value_is_refused_unless_radios_in_unison() {
    let mut s = session("dimension/plain-base.pdf");
    s.add_radio_button(
        &NewRadioButton::new(0, "Colour", member_rect(0), "Red").declining_tooltip(),
    )
    .unwrap();
    let err = s
        .add_radio_button(
            &NewRadioButton::new(0, "Colour", member_rect(1), "Red").declining_tooltip(),
        )
        .expect_err("a second member exporting Red must be refused");
    assert!(
        matches!(err, EditError::RadioExportValueTaken { .. }),
        "expected RadioExportValueTaken, got {err:?}"
    );

    // The same pair in a unison group is the flag's entire purpose.
    let mut unison = session("dimension/plain-base.pdf");
    for i in 0..2 {
        unison
            .add_radio_button(
                &NewRadioButton::new(0, "Unison", member_rect(i), "Red")
                    .declining_tooltip()
                    .with_group_flags(false, true),
            )
            .expect("radios-in-unison permits a shared export value");
    }
    let g = the_group(&unison, "Unison");
    assert_eq!(g.widgets.len(), 2);
    assert!(
        g.radios_in_unison(),
        "the TYPE-GATED predicate must report bit 26 on a radio group — the \
         same bit means RichText on a text field, so the raw bit is never \
         tested directly"
    );
}

/// A check box cannot join a radio group: buttons compare KIND, not just
/// `/FT`.
///
/// Both are `/FT /Btn`, so a type comparison alone would MERGE them — giving
/// one field widgets that disagree about whether they toggle independently
/// or exclusively.
#[test]
fn a_check_box_cannot_join_a_radio_group() {
    let mut s = session("dimension/plain-base.pdf");
    s.add_radio_button(
        &NewRadioButton::new(0, "Colour", member_rect(0), "Red").declining_tooltip(),
    )
    .unwrap();
    let err = s
        .add_check_box(&NewCheckBox::new(0, "Colour", member_rect(1)).declining_tooltip())
        .expect_err("a check box merging into a radio group must be refused");
    assert!(
        matches!(
            err,
            EditError::FieldAuthoring(FormAuthorError::FieldTypeCollision { .. })
        ),
        "expected FieldTypeCollision, got {err:?}"
    );
}

/// The reverse direction: a radio member cannot join a check box.
#[test]
fn a_radio_member_cannot_join_a_check_box() {
    let mut s = session("dimension/plain-base.pdf");
    s.add_check_box(&NewCheckBox::new(0, "Agree", member_rect(0)).declining_tooltip())
        .unwrap();
    let err = s
        .add_radio_button(
            &NewRadioButton::new(0, "Agree", member_rect(1), "Red").declining_tooltip(),
        )
        .expect_err("a radio member merging into a check box must be refused");
    assert!(
        matches!(
            err,
            EditError::FieldAuthoring(FormAuthorError::FieldTypeCollision { .. })
        ),
        "expected FieldTypeCollision, got {err:?}"
    );
}

/// Group-behaviour flags belong to the GROUP; a merge that disagrees is
/// disclosed, not silently applied and not refused (rule 4).
#[test]
fn disagreeing_group_flags_on_a_merge_are_disclosed_not_applied() {
    let mut s = session("dimension/plain-base.pdf");
    s.add_radio_button(
        &NewRadioButton::new(0, "Colour", member_rect(0), "Red")
            .declining_tooltip()
            .with_group_flags(true, false),
    )
    .unwrap();
    let out = s
        .add_radio_button(
            &NewRadioButton::new(0, "Colour", member_rect(1), "Green")
                .declining_tooltip()
                .with_group_flags(false, false),
        )
        .unwrap();

    assert!(
        out.merged,
        "precondition: this is the merge branch, not a second field"
    );
    assert!(
        out.disclosures.group_flags_ignored,
        "a member whose flags disagree with its group's must SAY so — \
         silently dropping what the operator passed is the sneaky outcome"
    );

    let g = the_group(&s, "Colour");
    assert!(
        g.flags.has(FieldFlags::NO_TOGGLE_TO_OFF),
        "the GROUP's flag stands: the second member does not get to rewrite \
         how the first one behaves"
    );
}

/// `group_flags_ignored` ALONE must make [`FieldAuthorDisclosures::any`]
/// report true.
///
/// # Why this needs its own test when the one above already sets the flag
///
/// [`disagreeing_group_flags_on_a_merge_are_disclosed_not_applied`] calls
/// `.declining_tooltip()`, so `tooltip_declined` is also true there and
/// `any()` came out true **through a different field**. `any()` in fact
/// omitted `group_flags_ignored` entirely, and no test noticed, because no
/// test had ever produced it as the SOLE disclosure — which is exactly the
/// condition under which the omission bites.
///
/// So this supplies a real `/TU`, on an untagged document, for a non-choice
/// type: every other disclosure is structurally false and the assertion is
/// carried by the one field under test. A GUI gating its disclosure block on
/// `any()` — the predicate's whole purpose — would otherwise have told the
/// operator nothing about a flag override pdfcer performed.
///
/// **R162**: the negative control below is not decoration. It establishes
/// that this session and this call shape CAN produce `any() == false`, so the
/// positive assertion is capable of failing rather than true by construction.
#[test]
fn group_flags_ignored_alone_is_reported_by_any() {
    let mut s = session("dimension/plain-base.pdf");

    // The negative control, FIRST: same fixture, same type, agreeing flags,
    // a real tooltip. If this came out true, the positive case below would
    // prove nothing about `group_flags_ignored` specifically.
    let quiet = s
        .add_radio_button(
            &NewRadioButton::new(0, "Colour", member_rect(0), "Red")
                .with_tooltip("Colour choice")
                .with_group_flags(true, false),
        )
        .unwrap();
    assert!(
        !quiet.disclosures.any(),
        "control: a first member with a supplied /TU on an untagged document \
         has nothing to disclose — if this is true, the case below cannot \
         isolate group_flags_ignored"
    );

    let out = s
        .add_radio_button(
            &NewRadioButton::new(0, "Colour", member_rect(1), "Green")
                .with_tooltip("Colour choice")
                .with_group_flags(false, false),
        )
        .unwrap();

    assert!(out.merged, "precondition: the merge branch");
    assert!(
        out.disclosures.group_flags_ignored,
        "precondition: the flag under test is the one that fired"
    );
    assert!(
        !out.disclosures.tooltip_declined
            && !out.disclosures.tagged_document
            && !out.disclosures.structure_tab_order
            && !out.disclosures.has_no_options,
        "precondition: group_flags_ignored is the SOLE disclosure, so any() \
         cannot be carried by a sibling field"
    );

    assert!(
        out.disclosures.any(),
        "any() must see group_flags_ignored. It did not, and a caller gating \
         its disclosure block on any() would have silently dropped a choice \
         pdfcer made that the operator did not specify (rule 4)"
    );
}

/// Agreeing flags produce no disclosure — the common scripted case, where
/// every call in a loop passes the same flags, must stay quiet.
#[test]
fn agreeing_group_flags_produce_no_disclosure() {
    let mut s = session("dimension/plain-base.pdf");
    for (i, v) in ["Red", "Green"].iter().enumerate() {
        let out = s
            .add_radio_button(
                &NewRadioButton::new(0, "Colour", member_rect(i), *v)
                    .declining_tooltip()
                    .with_group_flags(true, false),
            )
            .unwrap();
        assert!(
            !out.disclosures.group_flags_ignored,
            "member {v} passed the group's own flags and must not be warned \
             about them"
        );
    }
}

/// `Off` cannot be a member's export value (§12.7.4.2.3 reserves it).
#[test]
fn off_is_refused_as_an_export_value() {
    let mut s = session("dimension/plain-base.pdf");
    let err = s
        .add_radio_button(
            &NewRadioButton::new(0, "Colour", member_rect(0), "Off").declining_tooltip(),
        )
        .expect_err("`Off` names the off state and cannot name a member");
    assert!(
        matches!(err, EditError::CheckBoxOnStateInvalid { .. }),
        "expected the shared reserved-name refusal, got {err:?}"
    );
}

/// R105 applies to radio members as to every other authored field: an
/// undecided `/TU` is refused before anything is written.
#[test]
fn an_undecided_tooltip_is_refused() {
    let mut s = session("dimension/plain-base.pdf");
    let err = s
        .add_radio_button(&NewRadioButton::new(0, "Colour", member_rect(0), "Red"))
        .expect_err("R105: the accessibility name is a decision, not an option");
    assert!(
        matches!(err, EditError::TooltipDecisionRequired { .. }),
        "expected TooltipDecisionRequired, got {err:?}"
    );
}

/// A group survives save-and-reload as one field with all its members.
///
/// Everything above reads the LIVE session. This reads the file, which is a
/// different lookup: the merge writes a promoted parent, new widget objects
/// and a rewritten page `/Annots`, and any of those failing to reach the
/// bytes would leave the session correct and the document wrong.
#[test]
fn a_group_round_trips_through_save_and_reload() {
    let s = group_of(&["Red", "Green", "Blue"]);
    let (bytes, _report) = s
        .to_incremental_bytes(&pdfcer_core::writer::SaveOptions::identity())
        .expect("save");
    let reloaded = Document::from_bytes(bytes).expect("the saved group reloads");
    let session = EditSession::new(reloaded);

    let g = the_group(&session, "Colour");
    assert_eq!(
        g.widgets.len(),
        3,
        "all three members must survive the save — a widget present in the \
         session and absent from the file is the defect this catches"
    );
    assert_eq!(g.button_kind, Some(ButtonKind::Radio));
}

// =====================================================================
// Deletion (decision 020 F2 / §3.6.3)
// =====================================================================

/// Mid-group deletion: the group survives, one member lighter, still Shape B.
#[test]
fn deleting_a_mid_group_member_leaves_the_rest_untouched() {
    let mut s = group_of(&["Red", "Green", "Blue"]);
    let out = s.delete_widget("Colour", 1).expect("delete member 1");

    assert_eq!(out.widgets_removed, 1);
    assert!(
        !out.field_removed,
        "two members remain, so the field remains"
    );
    assert!(
        !out.selection_cleared,
        "nothing was selected, so nothing could be cleared"
    );

    let g = the_group(&s, "Colour");
    assert_eq!(g.widgets.len(), 2, "3 - 1 = 2");
    let mut left: Vec<String> = g
        .widgets
        .iter()
        .flat_map(|w| w.on_states.iter())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    left.sort();
    assert_eq!(
        left,
        vec!["Blue".to_owned(), "Red".to_owned()],
        "the deleted member is the one that went"
    );
}

/// R102: a group falling to ONE member stays Shape B rather than collapsing.
///
/// Collapsing would rewrite object identities the operator never asked to
/// change, and both shapes are legal, so the deletion has no business
/// choosing.
#[test]
fn a_group_reduced_to_one_member_does_not_collapse_to_shape_a() {
    let mut s = group_of(&["Red", "Green", "Blue"]);
    s.delete_widget("Colour", 2).unwrap();
    s.delete_widget("Colour", 1).unwrap();

    let g = the_group(&s, "Colour");
    assert_eq!(g.widgets.len(), 1);
    assert!(
        !g.widgets[0].merged,
        "R102: 3 -> 1 keeps the /Kids parent; it does not become its own widget"
    );
}

/// §3.6.3's disclosure: deleting the SELECTED member clears the value, and
/// says so.
///
/// Without this the field's `/V` would name a state no remaining widget can
/// display — a malformed field that still parses.
#[test]
fn deleting_the_selected_member_clears_the_value_and_discloses_it() {
    let mut s = group_of(&["Red", "Green", "Blue"]);
    s.set_button_state("Colour", "Green").unwrap();

    // Find which widget offers Green, rather than assuming widget order.
    let g = the_group(&s, "Colour");
    let idx = g
        .widgets
        .iter()
        .position(|w| w.on_states.iter().any(|st| st == b"Green"))
        .expect("some widget offers Green");

    let out = s
        .delete_widget("Colour", idx)
        .expect("delete the selection");
    assert!(
        out.selection_cleared,
        "the group's selection went with the widget, and that must be REPORTED"
    );

    let g = the_group(&s, "Colour");
    assert_eq!(
        g.value,
        FieldValue::Name(b"Off".to_vec()),
        "/V must not keep naming a state no remaining widget can display"
    );
    let graph = s.graph();
    for w in &g.widgets {
        let appearance_state = graph
            .resolved(w.id)
            .as_dict()
            .and_then(|d| d.get(b"AS"))
            .and_then(Object::as_name)
            .map(|n| n.as_bytes().to_vec())
            .unwrap();
        assert_eq!(
            appearance_state, b"Off",
            "every survivor goes to Off with the value they were agreeing with"
        );
    }
}

/// Deleting a member that did NOT hold the selection leaves it alone.
#[test]
fn deleting_an_unselected_member_keeps_the_selection() {
    let mut s = group_of(&["Red", "Green", "Blue"]);
    s.set_button_state("Colour", "Red").unwrap();
    let g = the_group(&s, "Colour");
    let idx = g
        .widgets
        .iter()
        .position(|w| w.on_states.iter().any(|st| st == b"Blue"))
        .unwrap();

    let out = s.delete_widget("Colour", idx).unwrap();
    assert!(!out.selection_cleared);
    assert_eq!(
        the_group(&s, "Colour").value,
        FieldValue::Name(b"Red".to_vec()),
        "an unrelated member leaving must not disturb the choice"
    );
}

/// Last-member deletion takes the field with it (§3.6.3 rule 3), by the same
/// rule for any field type rather than a radio special case.
#[test]
fn deleting_the_last_member_removes_the_field() {
    let mut s = group_of(&["Red"]);
    let out = s
        .delete_widget("Colour", 0)
        .expect("delete the only member");
    assert!(out.field_removed, "no members left means no field left");

    let form = forms::parse_acroform(&s.graph());
    let still_there = form
        .iter()
        .flat_map(|f| f.fields.iter())
        .any(|f| f.fully_qualified_name == "Colour");
    assert!(!still_there, "the field must be gone from the form");
}

/// An index past the end is refused by name, with the count.
#[test]
fn a_widget_index_past_the_end_is_refused() {
    let mut s = group_of(&["Red", "Green"]);
    let err = s
        .delete_widget("Colour", 7)
        .expect_err("there is no widget 7");
    assert!(
        matches!(err, EditError::WidgetIndexOutOfRange { widgets: 2, .. }),
        "the refusal must carry the real count; got {err:?}"
    );
}

/// Deleting a whole group removes every widget AND leaves no dangling
/// reference in the saved BYTES.
///
/// # Why this asserts on bytes rather than through the parser
///
/// This is F0's lesson, and it was learned from a shipped defect: flatten
/// left `/AcroForm /Fields` naming deleted objects, and every forms test
/// passed anyway because they all asserted through `parse_acroform`, which
/// silently drops entries that no longer resolve. The MODEL looked right
/// while the FILE was wrong. A parser that repairs on the way past cannot be
/// the witness for a write path.
#[test]
fn deleting_a_group_leaves_no_dangling_reference_in_the_bytes() {
    let mut s = group_of(&["Red", "Green", "Blue"]);
    let widget_ids: Vec<u32> = the_group(&s, "Colour")
        .widgets
        .iter()
        .map(|w| w.id.num)
        .collect();
    let field_id = the_group(&s, "Colour").id.num;

    let out = s.delete_field("Colour").expect("delete the whole group");
    assert_eq!(out.widgets_removed, 3);
    assert!(out.field_removed);

    let (bytes, _) = s
        .to_incremental_bytes(&pdfcer_core::writer::SaveOptions::identity())
        .expect("save");
    let reloaded = EditSession::new(Document::from_bytes(bytes).expect("reload"));
    let graph = reloaded.graph();

    // Read the CONTAINERS RAW. Not through `parse_acroform` — that is the
    // repairing reader whose leniency hid the shipped flatten defect, so it
    // cannot be the witness here. `/Fields` and `/Annots` are read straight
    // off the dictionaries and inspected for references by number.
    let gone: Vec<u32> = widget_ids
        .iter()
        .copied()
        .chain(std::iter::once(field_id))
        .collect();

    /// Object numbers an array-valued key of `dict` references.
    fn refs_of<G: ObjectGraph + ?Sized>(graph: &G, dict: &Object, key: &[u8]) -> Vec<u32> {
        let Some(v) = dict.as_dict().and_then(|d| d.get(key)) else {
            return Vec::new();
        };
        match graph.resolve(v) {
            Object::Array(a) => a
                .iter()
                .filter_map(|o| o.as_reference().map(|r| r.num))
                .collect(),
            _ => Vec::new(),
        }
    }

    // FIRST, PROVE THE INSTRUMENT WORKS. A loop over an empty array asserts
    // nothing, so "no dangling reference found" is only meaningful once
    // `refs_of` has been shown to find references at all. Re-derived from the
    // PRE-deletion document, where all four objects are certainly present.
    {
        let before = EditSession::new(
            Document::from_bytes(
                group_of(&["Red", "Green", "Blue"])
                    .to_incremental_bytes(&pdfcer_core::writer::SaveOptions::identity())
                    .expect("save the undeleted group")
                    .0,
            )
            .expect("reload"),
        );
        let bg = before.graph();
        let found: Vec<u32> = pdfcer_core::page_tree::pages_in(&bg)
            .expect("page tree")
            .iter()
            .flat_map(|p| refs_of(&bg, &bg.resolved(p.id).clone(), b"Annots"))
            .collect();
        assert_eq!(
            found.len(),
            3,
            "precondition: before deletion the page's /Annots must name the \
             three widgets, or the checks below are looking at nothing"
        );
    }

    // `/AcroForm /Fields` — the container the shipped flatten got wrong.
    let acroform = graph
        .catalog_dict()
        .and_then(|c| c.get(b"AcroForm"))
        .map(|a| graph.resolve(a).clone())
        .unwrap_or(Object::Null);
    for num in refs_of(&graph, &acroform, b"Fields") {
        assert!(
            !gone.contains(&num),
            "/AcroForm /Fields still names deleted object {num} — the exact \
             shape of the shipped flatten defect, and invisible to any test \
             that reads through parse_acroform"
        );
    }

    // Every page's `/Annots`.
    for page in pdfcer_core::page_tree::pages_in(&graph).expect("page tree") {
        let page_obj = graph.resolved(page.id).clone();
        for num in refs_of(&graph, &page_obj, b"Annots") {
            assert!(
                !gone.contains(&num),
                "a page's /Annots still names deleted object {num} — the page \
                 would paint a widget belonging to no field"
            );
        }
    }

    assert!(
        !forms::parse_acroform(&graph)
            .iter()
            .flat_map(|f| f.fields.iter())
            .any(|f| f.fully_qualified_name == "Colour"),
        "the group must be gone after a reload too"
    );
}
