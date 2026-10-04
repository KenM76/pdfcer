//! A widget's `/BS` `/D` dash pattern (ISO 32000-1 §12.5.4 Table 166) is
//! written when a dashed field is created, survives a redraw, and can be set
//! or removed on an existing widget.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::annot_author::{BorderDash, WidgetChrome};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{
    BorderSpec, BorderStyle, EditSession, NewCheckBox, NewTextField, WidgetEdit,
};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Dict, Object};
use pdfcer_core::page_tree::Rect;
use std::path::Path;

fn session() -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/dimension/plain-base.pdf");
    EditSession::new(Document::load(&path).expect("load fixture"))
}

fn rect() -> Rect {
    Rect {
        llx: 20.0,
        lly: 100.0,
        urx: 220.0,
        ury: 124.0,
    }
}

fn dashed(width: f64) -> BorderSpec {
    BorderSpec {
        style: BorderStyle::Dashed,
        width,
    }
}

fn dash(pattern: &[f64]) -> BorderDash {
    BorderDash::new(pattern.to_vec()).unwrap()
}

/// A black border colour: without `/MK /BC` the builders stroke no border.
fn chrome() -> WidgetChrome {
    WidgetChrome::new(None, Some(pdfcer_core::forms::MkColor::Gray(0.0)))
}

fn text_field(s: &mut EditSession, border: BorderSpec, chrome: WidgetChrome) {
    let mut spec = NewTextField::new(0, "Name", rect()).declining_tooltip();
    spec.border = border;
    spec.chrome = chrome;
    s.add_text_field(&spec).expect("author a text field");
}

fn widget_dict(s: &EditSession, name: &str) -> Dict {
    let fields = pdfcer_core::forms::parse_acroform(&s.graph())
        .unwrap()
        .fields;
    let field = fields
        .into_iter()
        .find(|f| f.fully_qualified_name == name)
        .unwrap();
    s.graph()
        .resolved(field.widgets[0].id)
        .as_dict()
        .cloned()
        .unwrap()
}

/// The widget's `/BS` `/D`, as numbers, or `None` when absent.
fn bs_dash(s: &EditSession, name: &str) -> Option<Vec<f64>> {
    let g = s.graph();
    let d = widget_dict(s, name);
    let Some(Object::Dict(bs)) = d.get(b"BS").map(|o| g.resolve(o).clone()) else {
        panic!("no /BS");
    };
    let Some(Object::Array(items)) = bs.get(b"D").map(|o| g.resolve(o).clone()) else {
        return None;
    };
    Some(items.iter().filter_map(Object::as_number).collect())
}

fn ap_text(s: &EditSession, name: &str) -> String {
    let g = s.graph();
    let d = widget_dict(s, name);
    let Some(Object::Dict(ap)) = d.get(b"AP").map(|o| g.resolve(o).clone()) else {
        panic!("no /AP");
    };
    let Some(Object::Stream(st)) = ap.get(b"N").map(|o| g.resolve(o).clone()) else {
        panic!("no /AP /N stream");
    };
    String::from_utf8_lossy(s.view().slice(st.data_span).unwrap_or_default()).into_owned()
}

#[test]
fn a_dashed_text_field_records_its_pattern_and_keeps_it_through_a_resize() {
    let mut s = session();
    text_field(
        &mut s,
        dashed(1.0),
        chrome().with_border_dash(dash(&[6.0, 2.0])),
    );
    assert_eq!(bs_dash(&s, "Name"), Some(vec![6.0, 2.0]));

    s.edit_widget(
        "Name",
        0,
        &WidgetEdit::new().with_rect(Rect {
            urx: 320.0,
            ..rect()
        }),
    )
    .unwrap();
    let ap = ap_text(&s, "Name");
    assert!(ap.contains("[6 2] 0 d"), "{ap}");
}

#[test]
fn a_dashed_check_box_records_its_pattern() {
    let mut s = session();
    let mut spec = NewCheckBox::new(0, "Agree", rect()).declining_tooltip();
    spec.border = dashed(1.0);
    spec.chrome = chrome().with_border_dash(dash(&[4.0, 1.0]));
    s.add_check_box(&spec).expect("author a check box");
    assert_eq!(bs_dash(&s, "Agree"), Some(vec![4.0, 1.0]));
}

#[test]
fn a_dash_on_a_solid_border_is_not_written() {
    let mut s = session();
    text_field(
        &mut s,
        BorderSpec::default(),
        chrome().with_border_dash(dash(&[6.0, 2.0])),
    );
    assert_eq!(bs_dash(&s, "Name"), None);
}

#[test]
fn a_dash_can_be_set_and_removed_on_an_existing_widget() {
    let mut s = session();
    text_field(&mut s, dashed(1.0), chrome());
    assert_eq!(bs_dash(&s, "Name"), None);

    let out = s
        .edit_widget(
            "Name",
            0,
            &WidgetEdit::new().with_border_dash(Some(dash(&[5.0, 3.0]))),
        )
        .unwrap();
    assert!(out.appearance_regenerated);
    assert_eq!(bs_dash(&s, "Name"), Some(vec![5.0, 3.0]));
    let ap = ap_text(&s, "Name");
    assert!(ap.contains("[5 3] 0 d"), "{ap}");

    s.edit_widget("Name", 0, &WidgetEdit::new().with_border_dash(None))
        .unwrap();
    assert_eq!(bs_dash(&s, "Name"), None);
    let ap = ap_text(&s, "Name");
    assert!(ap.contains("[3] 0 d"), "Table 166's default: {ap}");

    s.undo();
    assert_eq!(bs_dash(&s, "Name"), Some(vec![5.0, 3.0]));
}
