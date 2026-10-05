//! A radio button draws the mark the operator picked, and keeps it.
//!
//! Radio buttons share the check box's six `/MK` `/CA` styles. The default is
//! Circle — the centre dot pdfcer always drew — and writes no `/CA`, so an
//! existing radio is byte-identical. Any other style is recorded in `/MK`
//! `/CA` and drawn as vector artwork, and survives a resize because the
//! rebuild reads the style back from that key.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use pdfcer_core::annot_author::CheckStyle;
use pdfcer_core::document::Document;
use pdfcer_core::edit::{AppearanceOutcome, EditSession, NewRadioButton, WidgetEdit};
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::Object;
use pdfcer_core::page_tree::Rect;

fn session() -> EditSession {
    let p: PathBuf =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/minimal.pdf");
    EditSession::new(Document::load(&p).expect("load minimal.pdf"))
}

const BOX: Rect = Rect {
    llx: 40.0,
    lly: 40.0,
    urx: 60.0,
    ury: 60.0,
};

const BIG: Rect = Rect {
    llx: 40.0,
    lly: 40.0,
    urx: 90.0,
    ury: 90.0,
};

fn widget_dict(s: &EditSession) -> pdfcer_core::object::Dict {
    let g = s.graph();
    let form = forms::parse_acroform(&g).expect("an AcroForm");
    let field = form.fields.first().expect("one field");
    g.resolved(field.widgets.first().expect("one widget").id)
        .as_dict()
        .cloned()
        .expect("widget dict")
}

/// The ON-state appearance stream's bytes, as a viewer finds them.
fn on_state_bytes(s: &EditSession) -> Vec<u8> {
    let g = s.graph();
    let dict = widget_dict(s);
    let Some(Object::Dict(ap)) = dict.get(b"AP").map(|o| g.resolve(o).clone()) else {
        return Vec::new();
    };
    let Some(Object::Dict(n)) = ap.get(b"N").map(|o| g.resolve(o).clone()) else {
        return Vec::new();
    };
    let Some(entry) = n.get(b"A".as_slice()) else {
        return Vec::new();
    };
    let Object::Stream(st) = g.resolve(entry).clone() else {
        return Vec::new();
    };
    s.view().slice(st.data_span).unwrap_or_default().to_vec()
}

fn mk_ca(s: &EditSession) -> Option<Vec<u8>> {
    let g = s.graph();
    let dict = widget_dict(s);
    let Some(Object::Dict(mk)) = dict.get(b"MK").map(|o| g.resolve(o).clone()) else {
        return None;
    };
    match mk.get(b"CA").map(|o| g.resolve(o).clone()) {
        Some(Object::String(b)) => Some(b),
        _ => None,
    }
}

fn authored(style: CheckStyle, rect: Rect) -> EditSession {
    let mut s = session();
    let mut spec = NewRadioButton::new(0, "rb", rect, "A").declining_tooltip();
    spec.style = style;
    s.add_radio_button(&spec).expect("author the radio button");
    s
}

#[test]
fn every_style_draws_different_artwork() {
    let mut seen: HashMap<Vec<u8>, CheckStyle> = HashMap::new();
    for style in CheckStyle::all().iter().copied() {
        let content = on_state_bytes(&authored(style, BOX));
        assert!(!content.is_empty(), "{style:?} drew an empty ON state");
        if let Some(prev) = seen.insert(content, style) {
            panic!("{style:?} and {prev:?} drew identical artwork: the style is ignored");
        }
    }
}

#[test]
fn a_non_default_style_is_recorded_in_mk_ca() {
    assert_eq!(mk_ca(&authored(CheckStyle::Star, BOX)), Some(b"H".to_vec()));
    assert_eq!(
        mk_ca(&authored(CheckStyle::Check, BOX)),
        Some(b"4".to_vec())
    );
}

/// The default is Circle, drawn as the centre dot with no `/CA` — so a
/// radio authored without a style is exactly what it always was.
#[test]
fn the_default_is_the_dot_with_no_caption() {
    let mut s = session();
    s.add_radio_button(&NewRadioButton::new(0, "rb", BOX, "A").declining_tooltip())
        .expect("author with no style set");
    assert_eq!(mk_ca(&s), None, "the default writes no /MK /CA");
    let on = on_state_bytes(&s);
    assert_eq!(on, on_state_bytes(&authored(CheckStyle::Circle, BOX)));
    assert!(
        !String::from_utf8_lossy(&on).contains(" J"),
        "the dot is a fill, not a stroked mark: {}",
        String::from_utf8_lossy(&on)
    );
}

#[test]
fn the_style_survives_a_resize() {
    let mut s = authored(CheckStyle::Star, BOX);
    let out = s
        .edit_widget("rb", 0, &WidgetEdit::new().with_rect(BIG))
        .expect("resize the widget");
    assert_eq!(out.appearance, AppearanceOutcome::Regenerated);

    let star_at_big = on_state_bytes(&authored(CheckStyle::Star, BIG));
    assert_ne!(
        star_at_big,
        on_state_bytes(&authored(CheckStyle::Circle, BIG))
    );
    assert_eq!(
        on_state_bytes(&s),
        star_at_big,
        "after a resize the radio must still draw a star at the new size"
    );
}
