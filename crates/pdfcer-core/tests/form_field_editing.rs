//! # Editing an existing field's properties (`Pass 134.0`)
//!
//! Every property the five `New*` specs accept at creation was, until this
//! Pass, settable **only** at creation. A shell that wanted to change one had
//! to delete the field and place a new one — losing its position, its name,
//! its place in the tab order and any value already filled into it.
//!
//! ## What these tests are defending, and it is not "the setter works"
//!
//! Three things, in order of how quietly they fail:
//!
//! 1. **The standard's producer gates are checked against the RESULT.** An
//!    edit holds half the answer and the file holds the other half, so
//!    clearing `/MaxLen` on a comb field breaks Table 228 without the request
//!    ever mentioning comb. A verb that validated its own arguments would
//!    accept that and write a file with no defined rendering.
//! 2. **The field/widget scope split.** `required` is one thing per field and
//!    `border` is one thing per placement. Getting it backwards is invisible
//!    on the ordinary one-widget field and wrong on every radio group.
//! 3. **A stored value that no longer fits is DISCLOSED, not repaired and not
//!    refused.** Acrobat does all three of these silently; see
//!    `forms__field_editing_after_creation.md`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{
    BorderSpec, BorderStyle, ChoiceOption, EditError, EditSession, FieldEdit, NewCheckBox,
    NewChoiceField, NewTextField, TooltipChoice, Visibility, WidgetEdit,
};
use pdfcer_core::forms::{self, FieldFlags};
use pdfcer_core::graph::ObjectGraph;
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

/// A text field with a value and a length limit, ready to be edited.
fn text_field(s: &mut EditSession, name: &str) {
    let mut spec = NewTextField::new(0, name, rect()).declining_tooltip();
    spec.value = "Hello".to_owned();
    s.add_text_field(&spec).expect("author a text field");
}

// ---------------------------------------------------------------------
// The headline: the five properties the GUI asked for first.
// ---------------------------------------------------------------------

#[test]
fn the_universal_properties_can_all_be_changed_after_placing() {
    let mut s = session("dimension/plain-base.pdf");
    text_field(&mut s, "Customer");
    let before = field_named(&s, "Customer").unwrap();
    assert!(!before.flags.has(FieldFlags::REQUIRED));

    let out = s
        .edit_field(
            "Customer",
            &FieldEdit::new()
                .with_required(true)
                .with_read_only(true)
                .with_tooltip(TooltipChoice::Text("Your full name".to_owned())),
        )
        .expect("edit the field");

    let after = field_named(&s, "Customer").unwrap();
    assert!(after.flags.has(FieldFlags::REQUIRED));
    assert!(after.flags.has(FieldFlags::READ_ONLY));
    assert_eq!(out.flags_before, before.flags.0);
    assert_eq!(out.flags_after, after.flags.0);
    assert_eq!(
        out.widgets_affected, 1,
        "the blast radius of a field-scope change is every widget the field \
         owns, and it is reported so a shell can say so"
    );
}

#[test]
fn a_property_can_be_turned_back_off() {
    // The pair. A setter that only ever ORs bits passes the test above and
    // leaves the operator unable to undo their own choice.
    let mut s = session("dimension/plain-base.pdf");
    text_field(&mut s, "Customer");
    s.edit_field("Customer", &FieldEdit::new().with_required(true))
        .unwrap();
    s.edit_field("Customer", &FieldEdit::new().with_required(false))
        .unwrap();
    assert!(
        !field_named(&s, "Customer")
            .unwrap()
            .flags
            .has(FieldFlags::REQUIRED)
    );
}

#[test]
fn an_untouched_property_is_left_alone() {
    // `None` means "leave it", which is what makes an edit composable with
    // what is already in the file. A struct of plain values would reset every
    // property the caller did not think about.
    let mut s = session("dimension/plain-base.pdf");
    let mut spec = NewTextField::new(0, "Customer", rect()).declining_tooltip();
    spec.multiline = true;
    s.add_text_field(&spec).unwrap();

    s.edit_field("Customer", &FieldEdit::new().with_required(true))
        .unwrap();
    assert!(
        field_named(&s, "Customer")
            .unwrap()
            .flags
            .has(FieldFlags::MULTILINE),
        "editing `required` must not clear `multiline`"
    );
}

// ---------------------------------------------------------------------
// ★ The gates, checked against the RESULT.
// ---------------------------------------------------------------------

#[test]
fn clearing_max_len_on_a_comb_field_is_refused_without_the_request_naming_comb() {
    // THE CASE A DELTA-VALIDATING VERB WOULD ACCEPT. Table 228 permits Comb
    // only when /MaxLen is present. This request says nothing about comb; the
    // FILE says the field is comb, and the two together break the gate.
    let mut s = session("dimension/plain-base.pdf");
    let mut spec = NewTextField::new(0, "Serial", rect()).declining_tooltip();
    spec.comb = true;
    spec.max_len = Some(10);
    s.add_text_field(&spec).unwrap();

    let err = s
        .edit_field("Serial", &FieldEdit::new().with_max_len(None))
        .expect_err("a comb field with no /MaxLen has no defined rendering");
    assert!(
        matches!(err, EditError::CombPreconditionUnmet { .. }),
        "{err:?}"
    );
    assert_eq!(
        field_named(&s, "Serial").unwrap().max_len,
        Some(10),
        "a refusal must stage nothing"
    );
}

#[test]
fn setting_comb_on_a_field_with_no_max_len_is_refused() {
    let mut s = session("dimension/plain-base.pdf");
    text_field(&mut s, "Serial");
    let err = s
        .edit_field("Serial", &FieldEdit::new().with_comb(true))
        .expect_err("comb needs /MaxLen");
    assert!(
        matches!(err, EditError::CombPreconditionUnmet { .. }),
        "{err:?}"
    );
}

#[test]
fn setting_comb_and_max_len_together_is_accepted() {
    // The other half: the gate must not refuse the legitimate way to get
    // there, which is to supply both in one edit. A verb that checked comb
    // against the FILE's /MaxLen rather than the RESULT's would refuse this.
    let mut s = session("dimension/plain-base.pdf");
    text_field(&mut s, "Serial");
    s.edit_field(
        "Serial",
        &FieldEdit::new().with_comb(true).with_max_len(Some(8)),
    )
    .expect("comb + /MaxLen in one edit is exactly Table 228's precondition met");
    let f = field_named(&s, "Serial").unwrap();
    assert!(f.flags.has(FieldFlags::COMB));
    assert_eq!(f.max_len, Some(8));
}

#[test]
fn clearing_combo_on_an_editable_drop_down_is_refused() {
    // Table 230 bit 19 — `Edit` "shall be used only if" `Combo` is set. Same
    // shape as the comb case and reachable from the other direction: the
    // request never mentions `editable`.
    let mut s = session("dimension/plain-base.pdf");
    let mut spec = NewChoiceField::new(
        0,
        "Country",
        rect(),
        vec![ChoiceOption::plain("UK"), ChoiceOption::plain("US")],
    )
    .declining_tooltip();
    spec.combo = true;
    spec.editable = true;
    s.add_choice_field(&spec).unwrap();

    let err = s
        .edit_field("Country", &FieldEdit::new().with_combo(false))
        .expect_err("an editable list box is not a thing the standard defines");
    assert!(
        matches!(err, EditError::ChoiceEditWithoutCombo { .. }),
        "{err:?}"
    );
}

// ---------------------------------------------------------------------
// Type mismatch.
// ---------------------------------------------------------------------

#[test]
fn a_text_property_on_a_check_box_is_refused_by_name() {
    let mut s = session("dimension/plain-base.pdf");
    s.add_check_box(&NewCheckBox::new(0, "Agree", rect()).declining_tooltip())
        .unwrap();
    let err = s
        .edit_field("Agree", &FieldEdit::new().with_multiline(true))
        .expect_err("bit 13 is not defined for /Btn");
    match err {
        EditError::FieldPropertyTypeMismatch {
            property,
            field_type,
            ..
        } => {
            assert_eq!(property, "multiline");
            assert_eq!(
                field_type, "check box",
                "the error must name the type in the OPERATOR's words — their \
                 mental model is usually wrong about which kind of field it is"
            );
        }
        other => panic!("wrong error: {other:?}"),
    }
}

#[test]
fn a_choice_property_on_a_text_field_is_refused() {
    let mut s = session("dimension/plain-base.pdf");
    text_field(&mut s, "Customer");
    let err = s
        .edit_field("Customer", &FieldEdit::new().with_multi_select(true))
        .expect_err("bit 22 is /Ch only");
    assert!(
        matches!(err, EditError::FieldPropertyTypeMismatch { .. }),
        "{err:?}"
    );
}

// ---------------------------------------------------------------------
// ★ Rule 4: a value that no longer fits is DISCLOSED.
// ---------------------------------------------------------------------

#[test]
fn shortening_max_len_below_the_stored_value_discloses_and_does_not_truncate() {
    let mut s = session("dimension/plain-base.pdf");
    text_field(&mut s, "Customer"); // value "Hello", 5 characters
    let out = s
        .edit_field("Customer", &FieldEdit::new().with_max_len(Some(3)))
        .expect("shortening a limit is a legitimate authoring act");

    let complaint = out
        .value_no_longer_fits
        .expect("the operator must be told the field is over its own limit");
    assert!(
        complaint.contains('5') && complaint.contains('3'),
        "{complaint}"
    );

    let f = field_named(&s, "Customer").unwrap();
    assert_eq!(
        f.value,
        forms::FieldValue::Text(b"Hello".to_vec()),
        "pdfcer must NOT truncate: the data is the operator's, and silently \
         shortening it is the Acrobat behaviour this disclosure exists to avoid"
    );
}

#[test]
fn removing_a_selected_choice_option_discloses_and_does_not_repoint() {
    let mut s = session("dimension/plain-base.pdf");
    let spec = NewChoiceField::new(
        0,
        "Country",
        rect(),
        vec![ChoiceOption::plain("UK"), ChoiceOption::plain("US")],
    )
    .declining_tooltip();
    s.add_choice_field(&spec).unwrap();
    s.set_choice_value("Country", &["US"]).unwrap();

    let out = s
        .edit_field(
            "Country",
            &FieldEdit::new()
                .with_options(vec![ChoiceOption::plain("UK"), ChoiceOption::plain("FR")]),
        )
        .expect("replacing the option list is legitimate");

    let complaint = out
        .value_no_longer_fits
        .expect("the selected value is no longer an option and must be said");
    assert!(complaint.contains("US"), "{complaint}");
}

#[test]
fn a_value_that_still_fits_produces_no_complaint() {
    // The pair, and it matters: a disclosure that always fires is one nobody
    // reads, which is the same reasoning `reaches_outside` uses one module
    // over.
    let mut s = session("dimension/plain-base.pdf");
    text_field(&mut s, "Customer");
    let out = s
        .edit_field("Customer", &FieldEdit::new().with_max_len(Some(50)))
        .unwrap();
    assert!(out.value_no_longer_fits.is_none());
}

// ---------------------------------------------------------------------
// Widget scope — geometry, border, visibility.
// ---------------------------------------------------------------------

#[test]
fn a_widget_can_be_resized_and_the_appearance_is_rebuilt() {
    let mut s = session("dimension/plain-base.pdf");
    text_field(&mut s, "Customer");
    let out = s
        .edit_widget(
            "Customer",
            0,
            &WidgetEdit::new().with_rect(Rect {
                llx: 20.0,
                lly: 100.0,
                urx: 420.0,
                ury: 148.0,
            }),
        )
        .expect("resize the widget");

    assert!(out.resized, "the extent changed, so this is a resize");
    assert!(
        out.appearance_regenerated,
        "§12.5.5 would SCALE the old stream into the new rectangle — a text \
         field made twice as wide would show text twice as wide rather than \
         room for more text"
    );
    let f = field_named(&s, "Customer").unwrap();
    let r = f.widgets[0].rect.unwrap();
    assert!((r.width() - 400.0).abs() < 1e-6);
}

#[test]
fn a_pure_translation_is_not_reported_as_a_resize() {
    // The distinction the whole geometry path turns on. A translation keeps
    // the baked artwork exact and needs no rebuild; reporting it as a resize
    // would make every drag rewrite a stream for nothing.
    let mut s = session("dimension/plain-base.pdf");
    text_field(&mut s, "Customer");
    let out = s
        .edit_widget(
            "Customer",
            0,
            &WidgetEdit::new().with_rect(Rect {
                llx: 50.0,
                lly: 130.0,
                urx: 250.0,
                ury: 154.0,
            }),
        )
        .unwrap();
    assert!(!out.resized, "same width and height, different position");
    assert!(!out.appearance_regenerated);
}

#[test]
fn a_widget_resized_to_nothing_is_refused() {
    let mut s = session("dimension/plain-base.pdf");
    text_field(&mut s, "Customer");
    let err = s
        .edit_widget(
            "Customer",
            0,
            &WidgetEdit::new().with_rect(Rect {
                llx: 20.0,
                lly: 100.0,
                urx: 20.0,
                ury: 100.0,
            }),
        )
        .expect_err("a field with no area exists, accepts a value, and can never be clicked");
    assert!(
        matches!(err, EditError::FieldRectDegenerate { .. }),
        "{err:?}"
    );
}

#[test]
fn border_and_visibility_are_widget_scope() {
    let mut s = session("dimension/plain-base.pdf");
    text_field(&mut s, "Customer");
    s.edit_widget(
        "Customer",
        0,
        &WidgetEdit::new()
            .with_border(BorderSpec {
                style: BorderStyle::Dashed,
                width: 2.0,
            })
            .with_visibility(Visibility::PrintOnly),
    )
    .expect("edit the widget");

    let graph = s.graph();
    let f = field_named(&s, "Customer").unwrap();
    let dict = graph.resolved(f.widgets[0].id).as_dict().cloned().unwrap();
    let bs = dict
        .get(b"BS")
        .and_then(pdfcer_core::object::Object::as_dict)
        .unwrap();
    assert_eq!(
        bs.get(b"S")
            .and_then(pdfcer_core::object::Object::as_name)
            .unwrap()
            .as_bytes(),
        b"D"
    );
    assert_eq!(
        dict.get(b"F").and_then(pdfcer_core::object::Object::as_int),
        Some(36),
        "PrintOnly is Print|NoView per Table 165"
    );
}

#[test]
fn an_out_of_range_widget_index_is_refused() {
    let mut s = session("dimension/plain-base.pdf");
    text_field(&mut s, "Customer");
    let err = s
        .edit_widget("Customer", 7, &WidgetEdit::new())
        .expect_err("there is one widget");
    assert!(
        matches!(err, EditError::WidgetIndexOutOfRange { .. }),
        "{err:?}"
    );
}

// ---------------------------------------------------------------------
// Undo, and the reason both verbs are ONE command each.
// ---------------------------------------------------------------------

#[test]
fn one_edit_is_one_undo_however_many_properties_it_touched() {
    // Not a convenience. Several of these properties are gated against each
    // other by the standard, so a per-property undo could step BACKWARDS
    // through a state the file may not be in — clearing /MaxLen before
    // clearing comb, for instance.
    let mut s = session("dimension/plain-base.pdf");
    text_field(&mut s, "Serial");
    s.edit_field(
        "Serial",
        &FieldEdit::new()
            .with_comb(true)
            .with_max_len(Some(8))
            .with_required(true),
    )
    .unwrap();
    assert!(s.undo().is_some(), "one command");
    let f = field_named(&s, "Serial").unwrap();
    assert!(!f.flags.has(FieldFlags::COMB));
    assert!(!f.flags.has(FieldFlags::REQUIRED));
    assert_eq!(f.max_len, None);
}

#[test]
fn a_widget_edit_undoes_geometry_and_artwork_together() {
    let mut s = session("dimension/plain-base.pdf");
    text_field(&mut s, "Customer");
    let before = field_named(&s, "Customer").unwrap().widgets[0]
        .rect
        .unwrap();
    s.edit_widget(
        "Customer",
        0,
        &WidgetEdit::new().with_rect(Rect {
            llx: 20.0,
            lly: 100.0,
            urx: 420.0,
            ury: 148.0,
        }),
    )
    .unwrap();
    assert!(s.undo().is_some());
    let after = field_named(&s, "Customer").unwrap().widgets[0]
        .rect
        .unwrap();
    assert!((after.width() - before.width()).abs() < 1e-6);
}
