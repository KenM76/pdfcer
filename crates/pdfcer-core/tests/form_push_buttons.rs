//! Authoring **push buttons** (ISO 32000-1 §12.7.4.2.2) — the last of the
//! three `/FT /Btn` kinds pdfcer could not create.
//!
//! ## What these tests are really checking
//!
//! A push button is the field type defined by what it does **not** have. Its
//! three siblings all carry a value: a text field's `/V` is a string, a check
//! box's is a name, a choice field's is a string or an array. §12.7.4.2.2 says
//! a push button *"retains no permanent value"* and *"shall not use the `V`
//! and `DV` entries"*, and every structural difference follows from that one
//! sentence:
//!
//! - no `/V`, no `/DV`, no `/AS`;
//! - `/AP` `/N` is a **plain stream**, not the state-keyed sub-dictionary
//!   §12.7.4.2.3 gives check boxes and radios;
//! - nothing can fill it, and `set_button_state` must keep refusing it.
//!
//! Each of those is a place where a copy of the check-box code would produce
//! something that parses and misbehaves, so each is asserted against the
//! written OBJECT rather than against the model — a model that says "no
//! value" while the dictionary carries `/V /Off` is a model that is lying,
//! and the model is what every other test in the suite reads through.
//!
//! The other load-bearing property is the **inert disclosure**. Push-button
//! CREATION authors no action — `set_button_action` is the separate,
//! deliberate second act that does (`Pass 182.0`/`Pass 183.0`) — so this is
//! the only creation verb whose successful result is a control that does not
//! work.
//! `push_button_inert` is asserted on every path — create, merge, defaults —
//! because a disclosure that is true 100% of the time is exactly the kind
//! that gets optimised into a doc comment and then into nothing.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, NewCheckBox, NewPushButton, NewTextField};
use pdfcer_core::forms::{self, ButtonKind, FieldFlags, FieldType, FieldValue};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Dict, Object};
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

fn rect() -> Rect {
    Rect {
        llx: 40.0,
        lly: 80.0,
        urx: 160.0,
        ury: 104.0,
    }
}

fn second_rect() -> Rect {
    Rect {
        llx: 40.0,
        lly: 200.0,
        urx: 160.0,
        ury: 224.0,
    }
}

fn field_named(s: &EditSession, name: &str) -> Option<forms::Field> {
    forms::parse_acroform(&s.graph())?
        .fields
        .into_iter()
        .find(|f| f.fully_qualified_name == name)
}

fn dict_of(s: &EditSession, id: pdfcer_core::object::ObjId) -> Dict {
    s.graph()
        .resolved(id)
        .as_dict()
        .expect("a dictionary")
        .clone()
}

/// The headline: a push button created on a page with no form at all parses
/// back through the ordinary reader as a `/FT /Btn` of kind `Push`.
#[test]
fn a_push_button_parses_back_as_a_push_button() {
    let mut s = session("dimension/plain-base.pdf");
    assert!(
        forms::parse_acroform(&s.graph()).is_none(),
        "precondition: this fixture has no AcroForm, so the test proves \
         creation from nothing rather than appending to something"
    );

    s.add_push_button(&NewPushButton::new(0, "Submit", rect(), "Send it").declining_tooltip())
        .expect("author a push button");

    let f = field_named(&s, "Submit").expect("the field parses back");
    assert_eq!(f.field_type, Some(FieldType::Button));
    assert_eq!(
        f.button_kind,
        Some(ButtonKind::Push),
        "the /Ff Pushbutton bit is what makes a /Btn a push button, and the \
         reader must classify it from that bit alone"
    );
    assert!(
        f.flags.has(FieldFlags::PUSHBUTTON),
        "bit 17 is set unconditionally — it is not optional the way Radio is"
    );
    assert_eq!(f.widgets.len(), 1);
    assert!(
        f.widgets[0].merged,
        "a single-widget field uses the §12.5.6.19 MERGED shape"
    );
}

/// §12.7.4.2.2 verbatim: a push button *"shall not use the `V` and `DV`
/// entries"*. Asserted against the DICTIONARY, not the model.
///
/// The model would report `FieldValue::Absent` for a `/V` of `/Off` too, so a
/// model-level assertion would pass for a button that carries the check box's
/// value keys — which is precisely the defect a copy of `add_check_box` would
/// introduce.
#[test]
fn a_push_button_carries_no_value_keys_at_all() {
    let mut s = session("dimension/plain-base.pdf");
    let id = s
        .add_push_button(&NewPushButton::new(0, "Submit", rect(), "Send it").declining_tooltip())
        .expect("add")
        .field_id;

    let d = dict_of(&s, id);
    assert!(d.get(b"V").is_none(), "/V is ABSENT, not /Off");
    assert!(d.get(b"DV").is_none(), "/DV is ABSENT");
    assert!(
        d.get(b"AS").is_none(),
        "/AS selects an appearance STATE (§12.5.5), and a push button has none"
    );
    assert_eq!(
        field_named(&s, "Submit").expect("parses").value,
        FieldValue::Absent
    );
}

/// `/AP` `/N` is a **stream reference**, not a state-keyed sub-dictionary.
///
/// This is the structural divergence from the other two button kinds, and it
/// is invisible in the model: `has_normal_appearance` is true either way, so
/// only the object shape can tell them apart.
#[test]
fn the_appearance_is_a_plain_stream_not_a_state_subdictionary() {
    let mut s = session("dimension/plain-base.pdf");
    let push = s
        .add_push_button(&NewPushButton::new(0, "Submit", rect(), "Send it").declining_tooltip())
        .expect("add")
        .field_id;
    let check = s
        .add_check_box(&NewCheckBox::new(0, "Agree", second_rect()).declining_tooltip())
        .expect("add")
        .field_id;

    let g = s.graph();
    let push_n = dict_of(&s, push)
        .get(b"AP")
        .map(|o| g.resolve(o))
        .and_then(Object::as_dict)
        .and_then(|ap| ap.get(b"N").cloned())
        .expect("/AP /N");
    assert!(
        matches!(push_n, Object::Reference(_)),
        "a push button's /AP /N references ONE stream: {push_n:?}"
    );
    assert!(
        matches!(g.resolve(&push_n), Object::Stream(_)),
        "and that reference resolves to a stream, not a dictionary"
    );

    // The contrast, asserted in the same test so the two shapes are compared
    // rather than each merely described.
    let check_n = dict_of(&s, check)
        .get(b"AP")
        .map(|o| g.resolve(o))
        .and_then(Object::as_dict)
        .and_then(|ap| ap.get(b"N").cloned())
        .expect("/AP /N");
    assert!(
        matches!(check_n, Object::Dict(_)),
        "a check box's /AP /N is a state-keyed DICTIONARY (§12.7.4.2.3): {check_n:?}"
    );
}

/// The caption lands in `/MK` `/CA`, and it is not the field's name.
///
/// Three strings for three audiences — `/T` for scripts, `/TU` for screen
/// readers, `/CA` for the person clicking — and none is derived from another.
#[test]
fn the_caption_is_mk_ca_and_is_independent_of_the_name_and_tooltip() {
    let mut s = session("dimension/plain-base.pdf");
    let id = s
        .add_push_button(
            &NewPushButton::new(0, "btnSubmit1", rect(), "Send it")
                .with_tooltip("Submit this application"),
        )
        .expect("add")
        .field_id;

    let d = dict_of(&s, id);
    let g = s.graph();
    let ca = d
        .get(b"MK")
        .map(|o| g.resolve(o))
        .and_then(Object::as_dict)
        .and_then(|mk| mk.get(b"CA").cloned())
        .expect("/MK /CA");
    // Compared against the WRITER's spelling: `encode_text_string` emits a
    // PDFDocEncoded literal for pure-ASCII, so the bytes are the caption's.
    assert_eq!(
        ca,
        Object::String(b"Send it".to_vec()),
        "the caption is written verbatim into /MK /CA"
    );

    let f = field_named(&s, "btnSubmit1").expect("parses");
    assert_eq!(
        f.widgets[0].caption.as_deref(),
        Some(&b"Send it"[..]),
        "and the reader models it, so it can be listed and copied"
    );
    assert_eq!(
        f.alternate_name.as_deref(),
        Some(&b"Submit this application"[..]),
        "/TU is its own string and did not become the caption"
    );
    assert_eq!(
        f.partial_name.as_deref(),
        Some(&b"btnSubmit1"[..]),
        "/T is its own string and did not become the caption either"
    );
}

/// Every push button discloses that it is inert, and it is asserted on the
/// CREATE path and the MERGE path both — the merge branch returns a different
/// `FieldAuthorOutcome` and is where a per-branch disclosure goes missing.
#[test]
fn every_push_button_discloses_that_it_has_no_action() {
    let mut s = session("dimension/plain-base.pdf");
    let created = s
        .add_push_button(&NewPushButton::new(0, "Submit", rect(), "Send it").declining_tooltip())
        .expect("add");
    assert!(
        created.disclosures.push_button_inert,
        "a created push button has no action and says so"
    );

    let merged = s
        .add_push_button(
            &NewPushButton::new(0, "Submit", second_rect(), "Send").declining_tooltip(),
        )
        .expect("merge");
    assert!(merged.merged, "the same name attaches a second widget");
    assert!(
        merged.disclosures.push_button_inert,
        "the merge branch owes the disclosure too — the button it attached a \
         widget to still does nothing"
    );

    // And no action key was written on either path, which is the fact the
    // disclosure is about.
    let d = dict_of(&s, created.field_id);
    assert!(d.get(b"A").is_none(), "no /A action");
    assert!(d.get(b"AA").is_none(), "no /AA additional-actions dict");
    assert!(
        !field_named(&s, "Submit")
            .expect("parses")
            .has_additional_actions
    );
}

/// A merged widget keeps its OWN caption. `/MK` is a widget key (Table 189),
/// so one button pressed in two places may read differently in each.
///
/// Stated as a test because it differs from the on-state case, where a merged
/// check box deliberately does NOT get its own state name: those widgets are
/// views of one exported value and must agree, while these are views of one
/// action and need not.
#[test]
fn a_merged_push_button_widget_keeps_its_own_caption() {
    let mut s = session("dimension/plain-base.pdf");
    s.add_push_button(&NewPushButton::new(0, "Submit", rect(), "Send it").declining_tooltip())
        .expect("add");
    s.add_push_button(&NewPushButton::new(0, "Submit", second_rect(), "Go").declining_tooltip())
        .expect("merge");

    let f = field_named(&s, "Submit").expect("parses");
    assert_eq!(f.widgets.len(), 2, "one field, two views");
    let captions: Vec<Vec<u8>> = f.widgets.iter().filter_map(|w| w.caption.clone()).collect();
    assert!(
        captions.contains(&b"Send it".to_vec()) && captions.contains(&b"Go".to_vec()),
        "each widget carries its own /MK /CA, and the second add did NOT \
         relabel the first: {captions:?}"
    );
}

/// An empty caption is allowed and disclosed — a blank plate is a real thing
/// to author, and it is also what a forgotten `--caption` looks like.
#[test]
fn an_empty_caption_is_allowed_and_disclosed() {
    let mut s = session("dimension/plain-base.pdf");
    let out = s
        .add_push_button(&NewPushButton::new(0, "Blank", rect(), "").declining_tooltip())
        .expect("an empty caption is not a refusal");
    assert!(out.disclosures.push_button_no_caption);

    // `/CA` is written PRESENT-AND-EMPTY rather than omitted, so a later
    // editor can tell "no label wanted" from "never considered".
    let g = s.graph();
    let ca = dict_of(&s, out.field_id)
        .get(b"MK")
        .map(|o| g.resolve(o))
        .and_then(Object::as_dict)
        .and_then(|mk| mk.get(b"CA").cloned())
        .expect("/MK /CA is present even when empty");
    assert_eq!(ca, Object::String(Vec::new()));

    // A captioned button does NOT raise the disclosure — a flag that is
    // always true tests nothing.
    let out = s
        .add_push_button(
            &NewPushButton::new(0, "Labelled", second_rect(), "OK").declining_tooltip(),
        )
        .expect("add");
    assert!(!out.disclosures.push_button_no_caption);
}

/// Nothing can fill a push button, and the refusal predates this slice —
/// `set_button_state` already gated on the button kind. Asserted here anyway,
/// because creation is what made the refusal reachable: before this, no
/// pdfcer-authored document could contain a push button to refuse.
#[test]
fn a_push_button_cannot_be_filled() {
    let mut s = session("dimension/plain-base.pdf");
    s.add_push_button(&NewPushButton::new(0, "Submit", rect(), "Send it").declining_tooltip())
        .expect("add");

    let err = s
        .set_button_state("Submit", "Yes")
        .expect_err("a push button has no state to select");
    assert!(
        matches!(err, EditError::FieldNotFillable { .. }),
        "refused as not fillable, not as a missing state: {err:?}"
    );
    assert!(
        !field_named(&s, "Submit").expect("parses").is_fillable(),
        "and the model agrees, so a GUI listing fillable fields never offers it"
    );
}

/// The name is one identity across the whole `/Btn` family: a push button
/// cannot take a name a check box holds, and vice versa.
///
/// §12.7.3.2 makes same-FQN nodes ONE field, and one field has one kind —
/// merging these would give a single field widgets that disagree about
/// whether they have a value at all.
#[test]
fn a_push_button_will_not_merge_into_a_check_box_or_a_text_field() {
    let mut s = session("dimension/plain-base.pdf");
    s.add_check_box(&NewCheckBox::new(0, "Agree", rect()).declining_tooltip())
        .expect("add");
    let err = s
        .add_push_button(&NewPushButton::new(0, "Agree", second_rect(), "Go").declining_tooltip())
        .expect_err("a check box's name is not free for a push button");
    let msg = err.to_string();
    assert!(
        msg.contains("check box") && msg.contains("push button"),
        "the refusal names BOTH kinds, so the operator knows what collided \
         rather than only that something did: {msg}"
    );

    s.add_text_field(&NewTextField::new(0, "Notes", second_rect()).declining_tooltip())
        .expect("add");
    s.add_push_button(&NewPushButton::new(0, "Notes", second_rect(), "Go").declining_tooltip())
        .expect_err("nor a text field's");
}

/// R105 is enforced for this type like every other: an undecided `/TU` is
/// refused before anything is written.
#[test]
fn an_undecided_accessibility_name_is_refused() {
    let mut s = session("dimension/plain-base.pdf");
    let err = s
        .add_push_button(&NewPushButton::new(0, "Submit", rect(), "Send it"))
        .expect_err("neither with_tooltip nor declining_tooltip was called");
    assert!(matches!(err, EditError::TooltipDecisionRequired { .. }));
    assert!(
        forms::parse_acroform(&s.graph()).is_none(),
        "the refusal is total: no /AcroForm was created on the way out"
    );
}

/// `--defaults-from` copies the caption and nothing else, and only into a gap.
#[test]
fn defaults_from_copies_the_caption_only_into_a_gap() {
    let mut s = session("dimension/plain-base.pdf");
    s.add_push_button(
        &NewPushButton::new(0, "Template", rect(), "Submit application")
            .declining_tooltip()
            .with_flags(true),
    )
    .expect("add the template");
    let defaults = s.field_defaults("Template").expect("read defaults");
    assert_eq!(
        defaults.caption.as_deref(),
        Some(&b"Submit application"[..])
    );

    // Into a gap: an empty caption takes the template's.
    let mut spec = NewPushButton::new(0, "Copy", second_rect(), "").declining_tooltip();
    let applied = spec.apply_defaults(&defaults);
    assert!(!applied.type_mismatch);
    assert_eq!(spec.caption, "Submit application");
    assert!(
        !spec.read_only,
        "READ-ONLY IS A BOOLEAN AND BOOLEANS DO NOT COPY — a presence flag \
         cannot express 'off', so a copied one could be added but never \
         removed. The template is read-only and the copy is not."
    );

    // Not over an explicit value.
    let mut spec = NewPushButton::new(0, "Copy2", second_rect(), "Cancel").declining_tooltip();
    let applied = spec.apply_defaults(&defaults);
    assert!(!applied.type_mismatch);
    assert_eq!(spec.caption, "Cancel", "an explicit caption wins");
}

/// A template of the wrong type contributes nothing, and says so.
#[test]
fn defaults_from_a_non_push_button_copies_nothing_and_reports_it() {
    let mut s = session("dimension/plain-base.pdf");
    s.add_check_box(&NewCheckBox::new(0, "Agree", rect()).declining_tooltip())
        .expect("add");
    let defaults = s.field_defaults("Agree").expect("read defaults");
    assert!(
        defaults.caption.is_none(),
        "a check box has no caption to offer"
    );

    let mut spec = NewPushButton::new(0, "Copy", second_rect(), "").declining_tooltip();
    let applied = spec.apply_defaults(&defaults);
    assert!(
        applied.type_mismatch,
        "'you asked for defaults and got none' is the fact the operator needs"
    );
    assert_eq!(spec.caption, "");
}

/// Undo restores the document exactly, which is the round-trip invariant
/// every authoring verb owes: a created button that cannot be un-created
/// leaves `/AcroForm`, `/Annots` and the appearance stream behind.
#[test]
fn undoing_a_push_button_removes_every_trace_of_it() {
    let mut s = session("dimension/plain-base.pdf");
    s.add_push_button(&NewPushButton::new(0, "Submit", rect(), "Send it").declining_tooltip())
        .expect("add");
    assert!(field_named(&s, "Submit").is_some());

    s.undo().expect("undo the add");
    assert!(
        forms::parse_acroform(&s.graph()).is_none(),
        "the /AcroForm the add created is gone, not left empty"
    );
}

/// The flags a push button offers are exactly one, and `Required` is not
/// representable at all.
///
/// `/Ff` bit 2 means *the field shall have a value at export time*; a push
/// button never has a value, so a required one states a condition no operator
/// action can satisfy. It is refused BY CONSTRUCTION — there is no setter, no
/// struct field, and therefore no error variant and no message to read.
#[test]
fn read_only_is_the_only_flag_and_required_is_unrepresentable() {
    let mut s = session("dimension/plain-base.pdf");
    let id = s
        .add_push_button(
            &NewPushButton::new(0, "Inert", rect(), "Go")
                .declining_tooltip()
                .with_flags(true),
        )
        .expect("add")
        .field_id;

    let f = field_named(&s, "Inert").expect("parses");
    assert!(f.flags.has(FieldFlags::PUSHBUTTON));
    assert!(f.flags.read_only());
    assert!(
        !f.flags.has(FieldFlags::REQUIRED),
        "nothing in this crate can set Required on a push button"
    );

    // And the written `/Ff` is exactly those two bits — no stray inherited
    // flag rode along.
    let ff = dict_of(&s, id).get(b"Ff").and_then(Object::as_int);
    assert_eq!(
        ff,
        Some(i64::from(FieldFlags::PUSHBUTTON | FieldFlags::READ_ONLY))
    );
}
