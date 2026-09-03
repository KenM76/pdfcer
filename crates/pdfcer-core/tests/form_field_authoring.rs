//! Authoring NEW form fields (§12.7.2 registration + §12.5.6.19 widget).
//!
//! ## What these tests are really checking
//!
//! A created field is **three coordinated writes** — the merged field/widget
//! dictionary, the page's `/Annots` entry, and the `/AcroForm` `/Fields`
//! registration. Any two without the third produce a document that is broken
//! in a way nothing visibly reports: registered-but-not-annotated is
//! invisible, annotated-but-not-registered is not a form field at all.
//!
//! So the load-bearing assertion throughout is not "the bytes were written"
//! but **"`parse_acroform` reads it back as the field we meant"** — the same
//! parser the fill path, the CLI and the GUI all use. A field that only the
//! writer can see is not a field.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{
    ChoiceOption, EditError, EditSession, NewCheckBox, NewChoiceField, NewPushButton,
    NewRadioButton, NewTextField,
};
use pdfcer_core::forms::{self, ButtonKind, FieldFlags, FieldType, FieldValue};
use pdfcer_core::forms_author::FormAuthorError;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Name, Object};
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
        llx: 20.0,
        lly: 100.0,
        urx: 220.0,
        ury: 124.0,
    }
}

fn field_named(s: &EditSession, name: &str) -> Option<forms::Field> {
    forms::parse_acroform(&s.graph())?
        .fields
        .into_iter()
        .find(|f| f.fully_qualified_name == name)
}

/// The headline: a field created on a page that had NO form at all is read
/// back by the ordinary parser as a fillable text field.
#[test]
fn a_field_created_on_a_formless_page_parses_back_as_a_text_field() {
    let mut s = session("dimension/plain-base.pdf");
    assert!(
        forms::parse_acroform(&s.graph()).is_none(),
        "precondition: this fixture has no AcroForm, so the test proves \
         creation from nothing rather than appending to something"
    );

    s.add_text_field(&NewTextField::new(0, "Customer", rect()).declining_tooltip())
        .expect("author a text field");
    let f = field_named(&s, "Customer").expect("the field parses back");
    assert_eq!(f.field_type, Some(FieldType::Text));
    assert!(
        f.is_fillable(),
        "a field created to be typed in must be fillable"
    );
    assert_eq!(f.widgets.len(), 1, "one widget");
    assert!(
        f.widgets[0].merged,
        "single-widget fields use the §12.5.6.19 MERGED shape"
    );
}

/// All three writes land — and this is asserted at the OBJECT level, because
/// `parse_acroform` succeeding could in principle mask a missing `/Annots`
/// entry (the field would be registered but never drawn).
#[test]
fn the_field_is_registered_annotated_and_given_an_appearance() {
    let mut s = session("dimension/plain-base.pdf");
    let id = s
        .add_text_field(&NewTextField::new(0, "Customer", rect()).declining_tooltip())
        .unwrap()
        .field_id;
    let graph = s.graph();
    let d = graph
        .resolved(id)
        .as_dict()
        .expect("field is a dict")
        .clone();
    assert!(
        d.get(b"AP").is_some(),
        "1/3: an /AP, or it is invisible (R43/R51)"
    );

    // /AcroForm /Fields registration.
    let catalog = graph
        .resolved(graph.catalog_id().unwrap())
        .as_dict()
        .unwrap()
        .clone();
    let af = match catalog.get(b"AcroForm") {
        Some(Object::Dict(d)) => d.clone(),
        Some(Object::Reference(r)) => graph.resolved(*r).as_dict().unwrap().clone(),
        other => panic!("no /AcroForm: {other:?}"),
    };
    let Some(Object::Array(fields)) = af.get(b"Fields") else {
        panic!("/AcroForm has no /Fields array");
    };
    assert!(
        fields.contains(&Object::Reference(id)),
        "2/3: registered in /AcroForm /Fields"
    );
    // §12.7.3.3: the /DA names /Helv, so /DR /Font /Helv must resolve it or
    // another viewer regenerating from /DA cannot.
    let Some(Object::Dict(dr)) = af.get(b"DR") else {
        panic!("/AcroForm has no /DR");
    };
    let Some(Object::Dict(fonts)) = dr.get(b"Font") else {
        panic!("/DR has no /Font");
    };
    assert!(
        fonts.get(b"Helv").is_some(),
        "the /DA's font resolves in /DR"
    );

    // Page /Annots.
    let page_id = s.page_slots().unwrap()[0].id;
    let page = graph.resolved(page_id).as_dict().unwrap().clone();
    let annots = match page.get(b"Annots") {
        Some(Object::Array(a)) => a.clone(),
        Some(Object::Reference(r)) => match graph.resolved(*r) {
            Object::Array(a) => a.clone(),
            other => panic!("/Annots is not an array: {other:?}"),
        },
        other => panic!("page has no /Annots: {other:?}"),
    };
    assert!(
        annots.contains(&Object::Reference(id)),
        "3/3: present in the page's /Annots, or it is registered but never drawn"
    );
}

/// Additive (R46): every original byte survives, and the result round-trips.
#[test]
fn authoring_a_field_is_additive_and_the_result_reopens() {
    let original = std::fs::read(fixture("dimension/plain-base.pdf")).unwrap();
    let mut s = EditSession::new(Document::from_bytes(original.clone()).unwrap());
    s.add_text_field(&NewTextField::new(0, "Customer", rect()).declining_tooltip())
        .unwrap();
    let out = s
        .to_incremental_bytes(&pdfcer_core::writer::SaveOptions::identity())
        .unwrap()
        .0;

    assert!(
        out.starts_with(&original),
        "an additive author must not modify any original byte"
    );

    let reopened = EditSession::new(Document::from_bytes(out).unwrap());
    let f = field_named(&reopened, "Customer").expect("survives save and reopen");
    assert_eq!(f.field_type, Some(FieldType::Text));
}

/// The value and properties asked for are the ones stored.
#[test]
fn the_requested_value_and_properties_are_what_gets_written() {
    let mut s = session("dimension/plain-base.pdf");
    s.add_text_field(
        &NewTextField::new(0, "Notes", rect())
            .declining_tooltip()
            .with_value("hello")
            .with_max_len(40)
            .with_tooltip("Your notes")
            .with_flags(true, false, true),
    )
    .unwrap();
    let f = field_named(&s, "Notes").unwrap();
    assert_eq!(f.value.display_text(), "hello");
    assert_eq!(f.max_len, Some(40));
    assert_eq!(
        f.alternate_name.as_deref().map(String::from_utf8_lossy),
        Some("Your notes".into()),
        "/TU is what a screen reader announces, so it must survive verbatim"
    );
    assert!(f.flags.has(FieldFlags::MULTILINE));
    assert!(f.flags.has(FieldFlags::REQUIRED));
    assert!(!f.flags.has(FieldFlags::READ_ONLY));
}

/// A created field is immediately FILLABLE through the existing fill verb —
/// the proof that authoring produced a real field and not merely a
/// field-shaped dictionary.
#[test]
fn a_created_field_can_immediately_be_filled_by_the_existing_verb() {
    let mut s = session("dimension/plain-base.pdf");
    s.add_text_field(&NewTextField::new(0, "Customer", rect()).declining_tooltip())
        .unwrap();
    s.fill_text_field("Customer", "Ken Mantle")
        .expect("the ordinary fill path accepts a field pdfcer just authored");
    assert_eq!(
        field_named(&s, "Customer").unwrap().value.display_text(),
        "Ken Mantle"
    );
}

/// One undo removes the whole field — dictionary, annotation and
/// registration together, because it is ONE command.
#[test]
fn one_undo_removes_the_entire_field() {
    let original = std::fs::read(fixture("dimension/plain-base.pdf")).unwrap();
    let mut s = EditSession::new(Document::from_bytes(original.clone()).unwrap());
    s.add_text_field(&NewTextField::new(0, "Customer", rect()).declining_tooltip())
        .unwrap();
    assert!(s.is_modified());

    s.undo().expect("undo the authoring");
    assert!(!s.is_modified(), "undo restores the pristine session");
    assert!(
        forms::parse_acroform(&s.graph()).is_none(),
        "the /AcroForm created by the add is gone too, not left empty"
    );
    let out = s
        .to_incremental_bytes(&pdfcer_core::writer::SaveOptions::identity())
        .unwrap()
        .0;
    assert_eq!(out, original, "after undo the file is byte-identical");
}

/// A second field on a document that ALREADY has a form appends rather than
/// replacing — the existing fields must survive.
#[test]
fn adding_to_an_existing_form_appends_and_keeps_the_existing_fields() {
    let mut s = session("forms/demo-form.pdf");
    let before = forms::parse_acroform(&s.graph()).unwrap().fields.len();
    s.add_text_field(&NewTextField::new(0, "Extra", rect()).declining_tooltip())
        .unwrap();
    let form = forms::parse_acroform(&s.graph()).unwrap();
    assert_eq!(form.fields.len(), before + 1);
    assert!(
        form.fields
            .iter()
            .any(|f| f.fully_qualified_name == "FullName"),
        "the document's own fields are untouched"
    );
}

// -- Refusals ---------------------------------------------------------------

/// A name already used by a field of a DIFFERENT type is refused by name.
/// One `/V` cannot be both a text string and a button on-state.
#[test]
fn a_name_used_by_a_different_field_type_is_refused_by_name() {
    let mut s = session("forms/demo-form.pdf");
    // `Subscribe` is the fixture's check box.
    let err = s
        .add_text_field(&NewTextField::new(0, "Subscribe", rect()).declining_tooltip())
        .expect_err("a text field may not take a button's name");
    assert!(
        matches!(
            err,
            EditError::FieldAuthoring(FormAuthorError::FieldTypeCollision {
                ref fqn,
                existing,
                requested,
            }) if fqn == "Subscribe" && existing == "check box" && requested == "text"
        ),
        "expected FieldTypeCollision, got {err:?}"
    );
    assert!(!s.is_modified(), "a refusal writes nothing");
}

/// An empty name is refused: §12.7.3.2 builds the fully-qualified name from
/// `/T`, so a nameless field could never be filled or exported by name.
#[test]
fn an_empty_name_is_refused() {
    let mut s = session("dimension/plain-base.pdf");
    assert!(matches!(
        s.add_text_field(&NewTextField::new(0, "   ", rect()).declining_tooltip()),
        Err(EditError::FieldNameEmpty)
    ));
    assert!(!s.is_modified());
}

/// A zero-area rectangle is refused — it would create a field that exists,
/// accepts a value, and can never be seen or clicked.
#[test]
fn a_degenerate_rectangle_is_refused() {
    let mut s = session("dimension/plain-base.pdf");
    let flat = Rect {
        llx: 20.0,
        lly: 100.0,
        urx: 220.0,
        ury: 100.0,
    };
    assert!(matches!(
        s.add_text_field(&NewTextField::new(0, "Flat", flat).declining_tooltip()),
        Err(EditError::FieldRectDegenerate { .. })
    ));
    assert!(!s.is_modified());
}

/// A page index past the end is refused rather than silently landing on the
/// last page.
#[test]
fn a_page_out_of_range_is_refused() {
    let mut s = session("dimension/plain-base.pdf");
    assert!(
        s.add_text_field(&NewTextField::new(99, "Customer", rect()).declining_tooltip())
            .is_err()
    );
    assert!(!s.is_modified());
}

// ---------------------------------------------------------------------------
// Slice 2 — check boxes (ISO 32000-1 §12.7.4.2)
// ---------------------------------------------------------------------------

/// A second rectangle, so a test can place two fields without them
/// overlapping and make the "which one did I get back" question ambiguous.
fn rect2() -> Rect {
    Rect {
        llx: 20.0,
        lly: 200.0,
        urx: 44.0,
        ury: 224.0,
    }
}

/// The headline for check boxes: created on a page with no form at all, and
/// read back by the ORDINARY parser as a check box — not as some other kind
/// of button, and not as a text field that happens to hold a name.
#[test]
fn a_created_check_box_parses_back_as_a_check_box() {
    let mut s = session("dimension/plain-base.pdf");
    s.add_check_box(&NewCheckBox::new(0, "Agree", rect2()).declining_tooltip())
        .expect("add check box");
    let f = field_named(&s, "Agree").expect("field parses back");
    assert_eq!(f.field_type, Some(FieldType::Button));
    // The type test that matters: neither Radio nor Pushbutton is set, which
    // is what MAKES a /Btn field a check box (§12.7.4.2.1). A wrong flag
    // here produces a field that still parses, as a different type.
    assert_eq!(f.button_kind, Some(ButtonKind::Check));
    assert!(!f.flags.has(FieldFlags::RADIO));
    assert!(!f.flags.has(FieldFlags::PUSHBUTTON));
}

/// `/V` is a NAME for a check box, not a string — the single most common way
/// a hand-written box comes out unrecognisable. An unchecked box is `/Off`.
#[test]
fn a_check_box_value_is_a_name_and_defaults_to_off() {
    let mut s = session("dimension/plain-base.pdf");
    s.add_check_box(&NewCheckBox::new(0, "Agree", rect2()).declining_tooltip())
        .expect("add");
    let f = field_named(&s, "Agree").expect("parses");
    assert_eq!(f.value, FieldValue::Name(b"Off".to_vec()));

    let mut s2 = session("dimension/plain-base.pdf");
    s2.add_check_box(
        &NewCheckBox::new(0, "Agree", rect2())
            .declining_tooltip()
            .checked(true),
    )
    .expect("add");
    let f2 = field_named(&s2, "Agree").expect("parses");
    assert_eq!(f2.value, FieldValue::Name(b"Yes".to_vec()));
}

/// Both appearance states exist at creation, `/AP` `/N` is a sub-dictionary
/// keyed by state name, and `/AS` selects one of them (§12.7.4.2.3, §12.5.5).
///
/// This is the structural difference from every other field type, so it is
/// asserted against the raw dictionary rather than through the forms layer.
#[test]
fn both_appearance_states_exist_and_as_selects_one() {
    let mut s = session("dimension/plain-base.pdf");
    let id = s
        .add_check_box(
            &NewCheckBox::new(0, "Agree", rect2())
                .declining_tooltip()
                .checked(true),
        )
        .expect("add")
        .field_id;
    let g = s.graph();
    let d = g.resolved(id).as_dict().expect("field dict").clone();

    assert_eq!(
        d.get(b"AS").and_then(Object::as_name),
        Some(&Name::from(b"Yes")),
        "/AS must name the painted state"
    );
    let ap = d.get(b"AP").and_then(Object::as_dict).expect("/AP");
    let n = ap
        .get(b"N")
        .and_then(Object::as_dict)
        .expect("/AP /N is a DICTIONARY for a check box, not a stream");
    // Both states present, and the off state named exactly `Off` — §12.7.4.2.3
    // says it "shall" be, and a viewer that cannot find `Off` paints nothing.
    assert!(n.get(b"Yes").is_some(), "on state present");
    assert!(n.get(b"Off").is_some(), "off state present and named Off");
    // Each entry is a real stream with DRAWN content, not an empty
    // placeholder — a state that resolves to a zero-length stream paints
    // nothing, which for the off state is indistinguishable from no field.
    for state in [&b"Yes"[..], &b"Off"[..]] {
        let Some(Object::Reference(r)) = n.get(state) else {
            panic!("state {state:?} is not an indirect reference");
        };
        let Object::Stream(stream) = g.resolved(*r).clone() else {
            panic!("state {state:?} does not resolve to a stream");
        };
        let len = stream.dict.get(b"Length").and_then(Object::as_int);
        assert!(
            len.is_some_and(|n| n > 0),
            "state {state:?} has drawn content"
        );
    }
}

/// The on-state name is the exported value, so it is overridable — a form
/// that submits `Colour=Red` needs the on state named `Red`.
#[test]
fn the_on_state_name_is_overridable_and_reaches_both_v_and_ap() {
    let mut s = session("dimension/plain-base.pdf");
    let id = s
        .add_check_box(
            &NewCheckBox::new(0, "Colour", rect2())
                .declining_tooltip()
                .with_on_state("Red")
                .checked(true),
        )
        .expect("add")
        .field_id;
    let f = field_named(&s, "Colour").expect("parses");
    assert_eq!(f.value, FieldValue::Name(b"Red".to_vec()));

    let g = s.graph();
    let d = g.resolved(id).as_dict().expect("dict").clone();
    assert_eq!(
        d.get(b"AS").and_then(Object::as_name),
        Some(&Name::from(b"Red"))
    );
    let n = d
        .get(b"AP")
        .and_then(Object::as_dict)
        .and_then(|ap| ap.get(b"N"))
        .and_then(Object::as_dict)
        .expect("/AP /N");
    assert!(
        n.get(b"Red").is_some(),
        "the appearance sub-dictionary must be keyed by the SAME name /AS uses"
    );
}

/// THE PROOF THAT IT IS A REAL FIELD: the existing, unmodified
/// `set_button_state` accepts a box this created and toggles it.
///
/// A dictionary that merely parses is not a field. This is what separates
/// the two — the fill verb was written against Acrobat-authored files and
/// knows nothing about the creation path.
#[test]
fn a_created_check_box_can_immediately_be_toggled_by_the_existing_verb() {
    let mut s = session("dimension/plain-base.pdf");
    s.add_check_box(&NewCheckBox::new(0, "Agree", rect2()).declining_tooltip())
        .expect("add");
    s.set_button_state("Agree", "Yes").expect("tick it");
    assert_eq!(
        field_named(&s, "Agree").expect("parses").value,
        FieldValue::Name(b"Yes".to_vec())
    );

    s.set_button_state("Agree", "Off").expect("untick it");
    assert_eq!(
        field_named(&s, "Agree").expect("parses").value,
        FieldValue::Name(b"Off".to_vec())
    );
}

/// `Off` cannot name the on state: §12.7.4.2.3 reserves it, and a box whose
/// two states share a name cannot express "checked".
#[test]
fn off_is_refused_as_an_on_state_name() {
    let mut s = session("dimension/plain-base.pdf");
    let err = s
        .add_check_box(
            &NewCheckBox::new(0, "Agree", rect2())
                .declining_tooltip()
                .with_on_state("Off"),
        )
        .expect_err("must refuse");
    assert!(matches!(err, EditError::CheckBoxOnStateInvalid { .. }));

    let err = s
        .add_check_box(
            &NewCheckBox::new(0, "Agree", rect2())
                .declining_tooltip()
                .with_on_state("  "),
        )
        .expect_err("must refuse");
    assert!(matches!(err, EditError::CheckBoxOnStateInvalid { .. }));
}

/// One undo removes the whole box — both appearance streams, the field, the
/// `/Annots` entry and the `/AcroForm` registration — and leaves the document
/// byte-identical to before.
#[test]
fn one_undo_removes_the_entire_check_box() {
    let mut s = session("dimension/plain-base.pdf");
    let before = std::fs::read(fixture("dimension/plain-base.pdf")).unwrap();
    s.add_check_box(&NewCheckBox::new(0, "Agree", rect2()).declining_tooltip())
        .expect("add");
    assert!(field_named(&s, "Agree").is_some());
    s.undo().expect("undo");
    assert!(field_named(&s, "Agree").is_none(), "field is gone");
    assert_eq!(
        s.to_incremental_bytes(&pdfcer_core::writer::SaveOptions::identity())
            .unwrap()
            .0,
        before,
        "undo must be byte-identical, not merely equivalent"
    );
}

// ---------------------------------------------------------------------------
// Slice 2 — choice fields (ISO 32000-1 §12.7.4.4)
// ---------------------------------------------------------------------------

fn countries() -> Vec<ChoiceOption> {
    vec![
        ChoiceOption::new("CA", "Canada"),
        ChoiceOption::new("MX", "Mexico"),
        ChoiceOption::plain("Other"),
    ]
}

/// A created choice field parses back as one, with its options intact and the
/// export/display split preserved.
#[test]
fn a_created_choice_field_parses_back_with_its_options() {
    let mut s = session("dimension/plain-base.pdf");
    s.add_choice_field(&NewChoiceField::new(0, "Country", rect(), countries()).declining_tooltip())
        .expect("add choice");
    let f = field_named(&s, "Country").expect("parses back");
    assert_eq!(f.field_type, Some(FieldType::Choice));
    assert_eq!(f.options.len(), 3);
    // THE SPLIT IS THE POINT. Collapsing export into display would leave the
    // drop-down reading correctly and the submitted data wrong — a defect
    // with no visible symptom.
    assert_eq!(f.options[0].export, b"CA".to_vec());
    assert_eq!(f.options[0].display, b"Canada".to_vec());
    assert_eq!(f.options[1].export, b"MX".to_vec());
    assert_eq!(f.options[1].display, b"Mexico".to_vec());
    // A plain option round-trips with both halves equal.
    assert_eq!(f.options[2].export, b"Other".to_vec());
    assert_eq!(f.options[2].display, b"Other".to_vec());
}

/// An option whose export and display coincide is written as a BARE STRING,
/// not as a two-element array — the shape §12.7.4.4 intends and the shape a
/// hand-written file would have.
#[test]
fn a_plain_option_is_written_as_a_bare_string() {
    let mut s = session("dimension/plain-base.pdf");
    let id = s
        .add_choice_field(
            &NewChoiceField::new(0, "Country", rect(), countries()).declining_tooltip(),
        )
        .expect("add")
        .field_id;
    let g = s.graph();
    let d = g.resolved(id).as_dict().expect("dict").clone();
    let opt = d.get(b"Opt").and_then(Object::as_array).expect("/Opt");
    assert!(
        matches!(opt[0], Object::Array(_)),
        "an export != display option is a two-element array"
    );
    assert!(
        matches!(opt[2], Object::String(_)),
        "an export == display option is a bare string"
    );
}

/// Created UNSELECTED: §12.7.4.4 defaults `/V` to null, and this constructor
/// deliberately takes no position on the export-vs-display `/V` convention.
#[test]
fn a_created_choice_field_has_no_selection() {
    let mut s = session("dimension/plain-base.pdf");
    let id = s
        .add_choice_field(
            &NewChoiceField::new(0, "Country", rect(), countries()).declining_tooltip(),
        )
        .expect("add")
        .field_id;
    assert_eq!(
        field_named(&s, "Country").expect("parses").value,
        FieldValue::Absent
    );
    let g = s.graph();
    let d = g.resolved(id).as_dict().expect("dict").clone();
    assert!(d.get(b"V").is_none(), "/V is absent, not an empty string");
}

/// THE PROOF, again: the existing unmodified `set_choice_value` accepts a
/// field this created, and resolves the display string the operator sees.
#[test]
fn a_created_choice_field_can_immediately_be_filled_by_the_existing_verb() {
    let mut s = session("dimension/plain-base.pdf");
    s.add_choice_field(&NewChoiceField::new(0, "Country", rect(), countries()).declining_tooltip())
        .expect("add");
    s.set_choice_value("Country", &["Mexico"])
        .expect("the existing fill verb must accept a field we created");
    assert!(
        field_named(&s, "Country")
            .expect("parses")
            .value
            .is_present(),
        "the fill landed"
    );

    // And a value that is not an option is still refused — creation did not
    // weaken the fill verb's own guard.
    let err = s
        .set_choice_value("Country", &["Atlantis"])
        .expect_err("not an option");
    assert!(matches!(err, EditError::ChoiceValueNotInOptions { .. }));
}

/// Combo, editable, multi-select and sort each reach `/Ff` (§12.7.4.4
/// Table 230). Bit 26 is NOT set — on a `/Tx` field that bit is `RichText`,
/// and the overload is a documented trap.
#[test]
fn the_choice_flags_reach_ff() {
    let mut s = session("dimension/plain-base.pdf");
    s.add_choice_field(
        &NewChoiceField::new(0, "Country", rect(), countries())
            .declining_tooltip()
            .as_combo(true)
            .multi_select(true)
            .sorted(true),
    )
    .expect("add");
    let f = field_named(&s, "Country").expect("parses");
    assert!(f.flags.has(FieldFlags::COMBO));
    assert!(f.flags.has(FieldFlags::EDIT));
    assert!(f.flags.has(FieldFlags::MULTI_SELECT));
    assert!(f.flags.has(FieldFlags::SORT));
}

/// `sorted` SORTS THE ARRAY, it does not merely set a flag. §12.7.4.4: a
/// reader "shall display the options in the order in which they occur in the
/// Opt array", so a flag alone would change nothing an operator can see.
#[test]
fn sorting_reorders_the_options_rather_than_only_flagging_them() {
    let mut s = session("dimension/plain-base.pdf");
    s.add_choice_field(
        &NewChoiceField::new(0, "Country", rect(), countries())
            .declining_tooltip()
            .sorted(true),
    )
    .expect("add");
    let f = field_named(&s, "Country").expect("parses");
    let display: Vec<_> = f
        .options
        .iter()
        .map(|o| String::from_utf8_lossy(&o.display).into_owned())
        .collect();
    assert_eq!(display, vec!["Canada", "Mexico", "Other"]);

    // And unsorted preserves the caller's order exactly.
    let mut s2 = session("dimension/plain-base.pdf");
    let reversed = vec![ChoiceOption::plain("Zulu"), ChoiceOption::plain("Alpha")];
    s2.add_choice_field(&NewChoiceField::new(0, "C", rect(), reversed).declining_tooltip())
        .expect("add");
    let f2 = field_named(&s2, "C").expect("parses");
    assert_eq!(f2.options[0].display, b"Zulu".to_vec());
}

/// An empty option list is ALLOWED and DISCLOSED: the field saves, and the
/// outcome says it cannot be filled until options are added.
///
/// A form under construction legitimately passes through the empty state, so
/// refusing would block a real workflow; but a field nothing can fill is
/// exactly what R4 says must not be discovered later.
#[test]
fn a_choice_field_with_no_options_is_allowed_and_disclosed() {
    let mut s = session("dimension/plain-base.pdf");
    let out = s
        .add_choice_field(
            &NewChoiceField::new(0, "Country", rect(), Vec::new()).declining_tooltip(),
        )
        .expect("an empty option list is legal");
    assert!(
        out.disclosures.has_no_options,
        "the un-fillable state must be disclosed, not silent"
    );
    let f = field_named(&s, "Country").expect("the field still exists");
    assert_eq!(f.field_type, Some(FieldType::Choice));
    assert!(f.options.is_empty());

    // And it really is un-fillable — the disclosure is not merely cosmetic.
    assert!(matches!(
        s.set_choice_value("Country", &["anything"]),
        Err(EditError::ChoiceValueNotInOptions { .. })
    ));
}

/// A populated choice field reports NO disclosure — the flag tracks the real
/// condition rather than being set unconditionally.
#[test]
fn a_populated_choice_field_discloses_nothing() {
    let mut s = session("dimension/plain-base.pdf");
    let out = s
        .add_choice_field(
            &NewChoiceField::new(0, "Country", rect(), countries()).declining_tooltip(),
        )
        .expect("add");
    assert!(!out.disclosures.has_no_options);
}

/// `Edit` without `Combo` is impossible (§12.7.4.4 Table 230) and is refused
/// rather than silently dropped.
#[test]
fn an_editable_list_box_is_refused() {
    let mut s = session("dimension/plain-base.pdf");
    let mut spec = NewChoiceField::new(0, "Country", rect(), countries()).declining_tooltip();
    spec.editable = true; // without `combo`
    let err = s.add_choice_field(&spec).expect_err("must refuse");
    assert!(matches!(err, EditError::ChoiceEditRequiresCombo));
}

/// A duplicated export value would be unselectable, because the fill verb
/// resolves to the first match.
#[test]
fn a_duplicate_export_value_is_refused() {
    let mut s = session("dimension/plain-base.pdf");
    let dupes = vec![
        ChoiceOption::new("CA", "Canada"),
        ChoiceOption::new("CA", "Canada (again)"),
    ];
    let err = s
        .add_choice_field(&NewChoiceField::new(0, "Country", rect(), dupes).declining_tooltip())
        .expect_err("must refuse");
    assert!(matches!(err, EditError::ChoiceOptionDuplicate { .. }));
}

/// One undo removes the whole choice field, byte-identically.
#[test]
fn one_undo_removes_the_entire_choice_field() {
    let mut s = session("dimension/plain-base.pdf");
    let before = std::fs::read(fixture("dimension/plain-base.pdf")).unwrap();
    s.add_choice_field(&NewChoiceField::new(0, "Country", rect(), countries()).declining_tooltip())
        .expect("add");
    s.undo().expect("undo");
    assert!(field_named(&s, "Country").is_none());
    assert_eq!(
        s.to_incremental_bytes(&pdfcer_core::writer::SaveOptions::identity())
            .unwrap()
            .0,
        before
    );
}

/// A name already used by the SAME type MERGES, for all three verbs.
///
/// # This test used to assert the opposite, and the change is the point
///
/// It previously asserted `FieldNameAlreadyUsed` for each of the three verbs.
/// That refusal was correct while it stood: §12.7.3.2 makes the
/// fully-qualified name a field's IDENTITY, so appending a second same-named
/// field emits a document with two fields and one identity and no
/// disambiguator — which cannot be un-authored, because nothing records which
/// of the two the operator meant.
///
/// The write-side resolver now exists, so the same-name same-type add does
/// what §12.7.3.2 says it means: it MERGES, attaching another widget to the
/// one field. That is not a loosening of a safety rule — the duplicate-FQN
/// document is now unreachable by CONSTRUCTION (every authoring write
/// resolves the name against the graph before deciding what to write) rather
/// than merely refused by a guard.
///
/// The identity assertion the old pair of tests made is kept verbatim and
/// strengthened: still exactly ONE field of that name, now with TWO widgets
/// rather than one.
#[test]
fn a_name_already_used_by_the_same_type_merges_into_one_field() {
    // TEXT.
    let mut s = session("dimension/plain-base.pdf");
    s.add_text_field(&NewTextField::new(0, "Dup", rect()).declining_tooltip())
        .expect("first text field");
    s.add_text_field(&NewTextField::new(0, "Dup", rect2()).declining_tooltip())
        .expect("the second text field must MERGE, not be refused");
    assert_merged(&s, "Dup", "text");

    // CHECK BOX.
    let mut s = session("dimension/plain-base.pdf");
    s.add_check_box(&NewCheckBox::new(0, "Dup", rect2()).declining_tooltip())
        .expect("first check box");
    s.add_check_box(&NewCheckBox::new(0, "Dup", rect()).declining_tooltip())
        .expect("the second check box must MERGE");
    assert_merged(&s, "Dup", "check box");

    // CHOICE.
    let mut s = session("dimension/plain-base.pdf");
    s.add_choice_field(&NewChoiceField::new(0, "Dup", rect(), countries()).declining_tooltip())
        .expect("first choice field");
    s.add_choice_field(&NewChoiceField::new(0, "Dup", rect2(), countries()).declining_tooltip())
        .expect("the second choice field must MERGE");
    assert_merged(&s, "Dup", "choice");
}

/// Exactly ONE field of this name, carrying TWO widgets.
///
/// Both halves are load-bearing and they fail differently: two fields means
/// the duplicate-identity document the resolver exists to prevent, and one
/// widget means the merge silently discarded the placement the operator just
/// asked for.
fn assert_merged(s: &EditSession, fqn: &str, label: &str) {
    let form = forms::parse_acroform(&s.graph()).expect("form");
    let matching: Vec<_> = form
        .fields
        .iter()
        .filter(|f| f.fully_qualified_name == fqn)
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "{label}: a merge must leave ONE field, not two identities",
    );
    assert_eq!(
        matching[0].widgets.len(),
        2,
        "{label}: the merged field must carry both widgets",
    );
}

/// A name already used by a field of a DIFFERENT type is refused, in both
/// directions — the shared preflight is what makes this uniform across the
/// three creation verbs.
#[test]
fn a_name_used_by_another_field_type_is_refused_for_both_new_types() {
    let mut s = session("dimension/plain-base.pdf");
    s.add_text_field(&NewTextField::new(0, "Shared", rect()).declining_tooltip())
        .expect("text field");
    let err = s
        .add_check_box(&NewCheckBox::new(0, "Shared", rect2()).declining_tooltip())
        .expect_err("check box must not steal a text field's name");
    assert!(matches!(
        err,
        EditError::FieldAuthoring(FormAuthorError::FieldTypeCollision { .. })
    ));

    let err = s
        .add_choice_field(
            &NewChoiceField::new(0, "Shared", rect2(), countries()).declining_tooltip(),
        )
        .expect_err("choice must not steal a text field's name");
    assert!(matches!(
        err,
        EditError::FieldAuthoring(FormAuthorError::FieldTypeCollision { .. })
    ));
}

/// Both new types run the SAME preflight as slice 1, so the structural
/// refusals hold for them without being re-implemented.
#[test]
fn the_shared_preflight_refusals_apply_to_both_new_types() {
    let degenerate = Rect {
        llx: 10.0,
        lly: 10.0,
        urx: 10.0,
        ury: 40.0,
    };
    let mut s = session("dimension/plain-base.pdf");
    assert!(matches!(
        s.add_check_box(&NewCheckBox::new(0, "A", degenerate).declining_tooltip()),
        Err(EditError::FieldRectDegenerate { .. })
    ));
    assert!(matches!(
        s.add_choice_field(
            &NewChoiceField::new(0, "A", degenerate, countries()).declining_tooltip()
        ),
        Err(EditError::FieldRectDegenerate { .. })
    ));
    assert!(matches!(
        s.add_check_box(&NewCheckBox::new(0, "   ", rect2()).declining_tooltip()),
        Err(EditError::FieldNameEmpty)
    ));
    assert!(matches!(
        s.add_choice_field(&NewChoiceField::new(0, "", rect(), countries()).declining_tooltip()),
        Err(EditError::FieldNameEmpty)
    ));
    assert!(matches!(
        s.add_check_box(&NewCheckBox::new(9, "A", rect2()).declining_tooltip()),
        Err(EditError::PageOutOfRange { .. })
    ));
    assert!(matches!(
        s.add_choice_field(&NewChoiceField::new(9, "A", rect(), countries()).declining_tooltip()),
        Err(EditError::PageOutOfRange { .. })
    ));
}

/// All three field types coexist on one page, and the document reopens with
/// every one of them intact — the additive-authoring check (R46) extended to
/// slice 2.
#[test]
fn all_three_field_types_coexist_and_the_result_reopens() {
    let mut s = session("dimension/plain-base.pdf");
    s.add_text_field(
        &NewTextField::new(0, "Name", rect())
            .declining_tooltip()
            .with_value("Ken Mantle"),
    )
    .expect("text");
    s.add_check_box(
        &NewCheckBox::new(0, "Agree", rect2())
            .declining_tooltip()
            .checked(true),
    )
    .expect("check");
    s.add_choice_field(
        &NewChoiceField::new(
            0,
            "Country",
            Rect {
                llx: 250.0,
                lly: 200.0,
                urx: 400.0,
                ury: 224.0,
            },
            countries(),
        )
        .declining_tooltip()
        .as_combo(false),
    )
    .expect("choice");
    let bytes = s
        .to_incremental_bytes(&pdfcer_core::writer::SaveOptions::identity())
        .unwrap()
        .0;
    let doc = pdfcer_core::document::Document::from_bytes(bytes).expect("reopen");
    let reopened = EditSession::new(doc);
    let form = forms::parse_acroform(&reopened.graph()).expect("form survives the round trip");
    let mut names: Vec<_> = form
        .fields
        .iter()
        .map(|f| f.fully_qualified_name.clone())
        .collect();
    names.sort();
    assert_eq!(names, vec!["Agree", "Country", "Name"]);

    let agree = field_named(&reopened, "Agree").expect("check box survives");
    assert_eq!(agree.button_kind, Some(ButtonKind::Check));
    assert_eq!(agree.value, FieldValue::Name(b"Yes".to_vec()));
    let country = field_named(&reopened, "Country").expect("choice survives");
    assert_eq!(country.options.len(), 3);
}

// ===========================================================================
// F6 — `--defaults-from`
// ===========================================================================

/// Saving and re-reading, so an assertion is about the FILE and not the model.
///
/// R159: `parse_acroform` would report a copied option list just as happily
/// if the copy never reached the bytes, because it reads the same in-memory
/// graph the edit wrote to. Round-tripping is what makes the claim about the
/// document.
fn saved_bytes(session: &mut EditSession) -> Vec<u8> {
    session
        .to_incremental_bytes(&pdfcer_core::writer::SaveOptions::identity())
        .expect("save")
        .0
}

/// The flagship case: a choice field's `/Opt` copies, export≠display intact,
/// and it is in the saved bytes rather than merely in the model.
#[test]
fn a_choice_option_list_copies_into_the_saved_bytes() {
    let mut s = session("forms/radio-choice-form.pdf");
    let defaults = s
        .field_defaults("Country")
        .expect("Country is a choice field");
    assert_eq!(
        defaults.options.len(),
        3,
        "the template's own list is the thing being copied; if this is 0 the \
         test proves nothing about copying",
    );

    let mut spec = NewChoiceField::new(0, "Country2", rect(), Vec::new()).declining_tooltip();
    let applied = spec.apply_defaults(&defaults);
    assert!(!applied.type_mismatch, "choice -> choice is a match");
    s.add_choice_field(&spec).expect("author");

    let bytes = saved_bytes(&mut s);
    let text = String::from_utf8_lossy(&bytes);
    // Export and display DIFFER for these entries, so finding the pair in the
    // file proves both halves survived — a copy that collapsed them to one
    // string would still contain "Canada" and would still pass a weaker test.
    assert!(
        text.contains("(CA) (Canada)"),
        "the copied /Opt must reach the file with its export/display pair intact",
    );
    assert!(text.contains("(MX) (Mexico)"), "second pair");
    assert!(text.contains("(AR) (Argentina)"), "third pair");
}

/// A template of a different type copies NOTHING, and says so.
///
/// Not a partial copy: every property the four specs share is a boolean and
/// no boolean copies, so there is no common subset left to transfer.
#[test]
fn a_different_type_copies_nothing_and_discloses_it() {
    let s = session("forms/radio-choice-form.pdf");
    let from_text = s.field_defaults("Notes").expect("Notes is a text field");
    assert_eq!(from_text.field_type, Some(FieldType::Text));

    let mut spec = NewChoiceField::new(0, "Mismatched", rect(), Vec::new()).declining_tooltip();
    let applied = spec.apply_defaults(&from_text);

    assert!(
        applied.type_mismatch,
        "a text template must report that it contributed nothing to a choice field",
    );
    assert!(
        spec.options.is_empty(),
        "nothing may transfer across a type boundary",
    );
}

/// A radio template contributes nothing even to another radio field.
///
/// The types MATCH here; the copyable set is simply empty, because a radio
/// field's only non-boolean property is a per-widget export value and
/// `field_defaults` names a field. Reported the same way, because the fact
/// the operator needs — "you asked for defaults and got none" — is the same.
#[test]
fn a_radio_template_contributes_nothing() {
    let s = session("forms/radio-choice-form.pdf");
    let defaults = s.field_defaults("Colour").expect("Colour is a radio group");
    assert_eq!(defaults.button_kind, Some(ButtonKind::Radio));

    let mut spec = NewChoiceField::new(0, "FromRadio", rect(), Vec::new()).declining_tooltip();
    assert!(spec.apply_defaults(&defaults).type_mismatch);
    assert!(spec.options.is_empty());
}

/// The accessibility name is NEVER copied — R105's purpose, not just its
/// mechanism.
///
/// A copied `/TU` would leave the operator with an accessibility name they
/// never chose while every mechanical check still passed. The template here
/// HAS a tooltip precondition-checked below, so a failure to copy cannot be
/// mistaken for the template having nothing to give.
#[test]
fn the_accessibility_name_is_never_copied() {
    let mut s = session("forms/demo-form.pdf");
    let mut source = NewTextField::new(0, "Template", rect()).with_tooltip("Announce me");
    source.max_len = Some(42);
    s.add_text_field(&source).expect("author the template");

    let template = field_named(&s, "Template").expect("template exists");
    assert!(
        template.alternate_name.is_some(),
        "precondition: the template must HAVE a /TU, or this test cannot \
         distinguish 'not copied' from 'nothing to copy'",
    );

    let defaults = s.field_defaults("Template").expect("read defaults");
    let mut spec = NewTextField::new(0, "Copy", rect()).declining_tooltip();
    spec.apply_defaults(&defaults);
    s.add_text_field(&spec).expect("author the copy");

    let copy = field_named(&s, "Copy").expect("copy exists");
    assert_eq!(
        copy.alternate_name, None,
        "R105: a copied tooltip satisfies the mechanism while defeating the \
         purpose — the operator would carry an accessibility name they never \
         decided",
    );
    assert_eq!(copy.max_len, Some(42), "but /MaxLen does copy");
}

/// Explicit arguments beat the template — it fills gaps, it does not overrule.
#[test]
fn an_explicit_value_is_not_overwritten_by_the_template() {
    let mut s = session("forms/demo-form.pdf");
    let mut source = NewTextField::new(0, "T", rect()).declining_tooltip();
    source.max_len = Some(10);
    s.add_text_field(&source).expect("author");

    let defaults = s.field_defaults("T").expect("read");
    let mut spec = NewTextField::new(0, "Explicit", rect()).declining_tooltip();
    spec.max_len = Some(99);
    spec.apply_defaults(&defaults);
    assert_eq!(
        spec.max_len,
        Some(99),
        "the operator spoke about THIS field; a template must not overrule it",
    );
}

/// A template that does not exist is refused, not treated as empty.
#[test]
fn a_missing_template_is_refused() {
    let s = session("forms/demo-form.pdf");
    assert!(matches!(
        s.field_defaults("NoSuchField"),
        Err(EditError::FieldNotFound { .. })
    ));
}

/// **Every authored form field is marked printable.**
///
/// §12.5.3 Table 165 bit 3. `/F` defaults to 0 — every flag clear — so a
/// widget without it is one a conforming reader may show on screen and
/// leave off the paper, and the operator would not find out until they
/// printed.
///
/// # ★ This test exists because a reported defect turned out not to be one
///
/// An Acrobat-parity audit reported that `add_radio_button`,
/// `add_push_button` and `add_choice_field` never set `/F`, while
/// `add_text_field` and `add_check_box` did — concluding that
/// pdfcer-authored dropdowns and radio groups would not print. The
/// evidence was a grep: five direct `Name::from(b"F")` writes in
/// `edit.rs`, none of them inside those three functions.
///
/// The grep was accurate and the conclusion was wrong. Those three build
/// their widget through `widget_base_dict`, which sets `/F` for all of
/// them. The property held the whole time.
///
/// This test is what established that. Written to catch the reported
/// defect, it passed — and then passed again with the "fix" removed,
/// which is the only reason the fix was not committed along with a
/// commit message describing a bug that never existed.
///
/// The lesson is narrow and worth keeping: **a grep for direct writes
/// cannot see a shared helper.** Absence of a call site is not absence
/// of the behaviour, and the cheap way to tell the difference is to
/// assert the OUTCOME rather than the code shape.
///
/// So the test stays. It guards a real property — one no existing test
/// covered — and it now guards it against a future refactor that moves a
/// field type off the shared builder, which is the way this could still
/// become true.
///
/// # Why one test over all five rather than five assertions
///
/// A sixth field type is how this would regress. As five separate tests,
/// a new verb is simply uncovered and nothing says so. As one list,
/// adding a verb without adding it here is a visible omission in the
/// same file.
#[test]
fn every_authored_field_type_is_marked_printable() {
    /// Table 165 bit 3.
    const PRINT: i64 = 4;

    let mut s = session("forms/demo-form.pdf");
    // Built as one literal rather than pushed: clippy's `vec_init_then_push`
    // is right that the pushes were a list wearing a loop's clothes.
    let authored: Vec<(&str, pdfcer_core::object::ObjId)> = vec![
        (
            "text",
            s.add_text_field(&NewTextField::new(0, "T", rect()).declining_tooltip())
                .expect("text field")
                .field_id,
        ),
        (
            "check box",
            s.add_check_box(&NewCheckBox::new(0, "C", rect()).declining_tooltip())
                .expect("check box")
                .field_id,
        ),
        (
            "radio button",
            s.add_radio_button(&NewRadioButton::new(0, "R", rect(), "on").declining_tooltip())
                .expect("radio button")
                .field_id,
        ),
        (
            "push button",
            s.add_push_button(&NewPushButton::new(0, "P", rect(), "Go").declining_tooltip())
                .expect("push button")
                .field_id,
        ),
        (
            "choice field",
            s.add_choice_field(
                &NewChoiceField::new(0, "L", rect(), countries()).declining_tooltip(),
            )
            .expect("choice field")
            .field_id,
        ),
    ];

    let graph = s.graph();
    for (name, id) in authored {
        let dict = graph
            .resolved(id)
            .as_dict()
            .unwrap_or_else(|| panic!("{name}: the authored field must be a dictionary"));
        let flags = graph
            .resolve(dict.get(b"F").unwrap_or(&Object::Null))
            .as_int()
            .unwrap_or_else(|| {
                panic!("{name}: has no /F at all, so it defaults to every flag clear — unprintable")
            });
        assert_eq!(
            flags & PRINT,
            PRINT,
            "{name}: /F is {flags}, so bit 3 (Print) is clear — this field would be on \
             screen and absent from the paper"
        );
    }
}
