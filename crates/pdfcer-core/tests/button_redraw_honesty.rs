//! An edit that redraws a button reports `Regenerated` only when the artwork
//! actually changed. A property pdfcer's check-box and radio artwork does not
//! draw from (`/DA`; a text field's `/MK` `/CA`) is recorded, not painted, and is
//! disclosed as such, with the appearance streams left untouched.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{
    AppearanceOutcome, EditSession, FieldAppearance, FieldEdit, NewCheckBox, NewRadioButton,
    NewTextField, WidgetEdit,
};
use pdfcer_core::fontdata::Std14;
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::Object;
use pdfcer_core::page_tree::Rect;
use pdfcer_core::vartext::TextColor;
use std::path::Path;

fn session() -> EditSession {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/dimension/plain-base.pdf");
    EditSession::new(Document::load(&p).unwrap())
}

fn rect() -> Rect {
    Rect {
        llx: 20.0,
        lly: 100.0,
        urx: 68.0,
        ury: 124.0,
    }
}

fn red_helv() -> FieldAppearance {
    FieldAppearance::standard(Std14::Helvetica, 10.0, TextColor::Rgb(1.0, 0.0, 0.0))
}

/// Where each `/AP` `/N` state stream of `name`'s widget 0 keeps its bytes:
/// a rewrite stages new bytes, so an unchanged list means nothing was redrawn.
fn ap_spans(s: &EditSession, name: &str) -> Vec<String> {
    let g = s.graph();
    let field = forms::parse_acroform(&g)
        .unwrap()
        .fields
        .into_iter()
        .find(|f| f.fully_qualified_name == name)
        .unwrap();
    let w = g.resolved(field.widgets[0].id).as_dict().cloned().unwrap();
    let ap = w.get(b"AP").map(|o| g.resolve(o).clone()).unwrap();
    let n = ap
        .as_dict()
        .and_then(|d| d.get(b"N"))
        .map(|o| g.resolve(o).clone());
    match n {
        Some(Object::Stream(st)) => vec![format!("{:?}", st.data_span)],
        Some(Object::Dict(states)) => states
            .0
            .iter()
            .map(|(_, v)| match g.resolve(v) {
                Object::Stream(st) => format!("{:?}", st.data_span),
                other => format!("{other:?}"),
            })
            .collect(),
        other => panic!("no /AP /N: {other:?}"),
    }
}

#[test]
fn a_check_box_da_edit_is_recorded_not_redrawn() {
    let mut s = session();
    s.add_check_box(&NewCheckBox::new(0, "Agree", rect()).declining_tooltip())
        .unwrap();
    let before = ap_spans(&s, "Agree");
    let out = s
        .edit_field("Agree", &FieldEdit::new().with_appearance(red_helv()))
        .unwrap();
    assert_eq!(
        ap_spans(&s, "Agree"),
        before,
        "identical artwork was rewritten"
    );
    assert!(
        !out.appearance_regenerated,
        "the check-box artwork does not draw from /DA, so nothing was redrawn"
    );
    assert!(
        out.appearance_stale.is_some(),
        "the /DA was written and the pixels did not change: that is owed a sentence"
    );
}

#[test]
fn a_radio_da_edit_is_recorded_not_redrawn() {
    let mut s = session();
    s.add_radio_button(&NewRadioButton::new(0, "Choice", rect(), "A").declining_tooltip())
        .unwrap();
    let out = s
        .edit_field("Choice", &FieldEdit::new().with_appearance(red_helv()))
        .unwrap();
    assert!(!out.appearance_regenerated);
    assert!(out.appearance_stale.is_some());
}

#[test]
fn a_radio_caption_edit_redraws_the_mark() {
    let mut s = session();
    s.add_radio_button(&NewRadioButton::new(0, "Choice", rect(), "A").declining_tooltip())
        .unwrap();
    let before = ap_spans(&s, "Choice");
    // `u` is the diamond's /MK /CA character.
    let out = s
        .edit_widget("Choice", 0, &WidgetEdit::new().with_caption("u"))
        .unwrap();
    assert_eq!(out.appearance, AppearanceOutcome::Regenerated);
    assert_ne!(ap_spans(&s, "Choice"), before);
}

#[test]
fn a_text_caption_edit_is_recorded_not_painted() {
    let mut s = session();
    s.add_text_field(&NewTextField::new(0, "Name", rect()).declining_tooltip())
        .unwrap();
    let out = s
        .edit_widget("Name", 0, &WidgetEdit::new().with_caption("X"))
        .unwrap();
    assert!(
        matches!(out.appearance, AppearanceOutcome::RecordedNotPainted(_)),
        "{:?}",
        out.appearance
    );
}

#[test]
fn a_real_redraw_still_reports_regenerated() {
    let mut s = session();
    s.add_check_box(&NewCheckBox::new(0, "Agree", rect()).declining_tooltip())
        .unwrap();
    let out = s
        .edit_widget(
            "Agree",
            0,
            &WidgetEdit::new().with_background(pdfcer_core::forms::MkColor::Gray(0.5)),
        )
        .unwrap();
    assert_eq!(out.appearance, AppearanceOutcome::Regenerated);
}
