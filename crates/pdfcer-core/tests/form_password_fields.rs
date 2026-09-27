//! A `Password` text field (§12.7.4.3 Table 228, bit 14) is drawn masked and,
//! by default, its value never reaches the saved file.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, NewTextField};
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::Object;
use pdfcer_core::page_tree::Rect;
use pdfcer_core::writer::SaveOptions;

const BOX: Rect = Rect {
    llx: 40.0,
    lly: 700.0,
    urx: 240.0,
    ury: 720.0,
};

const SECRET: &str = "hunter2";

fn with_password_field() -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/minimal.pdf");
    let mut s = EditSession::new(Document::load(&path).expect("load minimal.pdf"));
    s.add_text_field(
        &NewTextField::new(0, "pw", BOX)
            .with_password(true)
            .declining_tooltip(),
    )
    .expect("author the password field");
    s
}

fn field(s: &EditSession) -> forms::Field {
    forms::parse_acroform(&s.graph())
        .expect("an AcroForm")
        .fields
        .into_iter()
        .find(|f| f.fully_qualified_name == "pw")
        .expect("the field")
}

fn stored_v(s: &EditSession) -> Option<Object> {
    let g = s.graph();
    g.resolved(field(s).id)
        .as_dict()
        .and_then(|d| d.get(b"V"))
        .map(|o| g.resolve(o).clone())
}

fn ap_bytes(s: &EditSession) -> String {
    let g = s.graph();
    let w = &field(s).widgets[0];
    let Some(Object::Dict(ap)) = g
        .resolved(w.id)
        .as_dict()
        .and_then(|d| d.get(b"AP"))
        .map(|o| g.resolve(o).clone())
    else {
        panic!("no /AP");
    };
    let Some(Object::Stream(st)) = ap.get(b"N").map(|o| g.resolve(o).clone()) else {
        panic!("no /AP /N stream");
    };
    String::from_utf8_lossy(s.view().slice(st.data_span).unwrap_or_default()).into_owned()
}

fn saved(s: &EditSession) -> Vec<u8> {
    s.to_incremental_bytes(&SaveOptions::identity())
        .expect("the session saves")
        .0
}

fn contains(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len()).any(|w| w == needle.as_bytes())
}

#[test]
fn a_password_fill_is_drawn_masked_and_never_saved() {
    let mut s = with_password_field();
    let out = s.fill_text_field("pw", SECRET).expect("fill");
    assert!(
        out.password_value_withheld,
        "the withholding was not disclosed"
    );
    assert_eq!(stored_v(&s), None, "the password was stored in /V");

    let ap = ap_bytes(&s);
    assert!(ap.contains("(*******)"), "not masked: {ap}");
    assert!(!ap.contains(SECRET), "plaintext drawn: {ap}");

    assert!(
        !contains(&saved(&s), SECRET),
        "the password reached the saved file"
    );
}

#[test]
fn storing_a_password_is_explicit_and_still_masks_the_appearance() {
    let mut s = with_password_field();
    let out = s
        .fill_text_field_storing_password("pw", SECRET)
        .expect("fill");
    assert!(!out.password_value_withheld);
    assert!(matches!(stored_v(&s), Some(Object::String(b)) if b == SECRET.as_bytes()));
    assert!(!ap_bytes(&s).contains(SECRET), "plaintext drawn");

    // A later default fill removes the stored value rather than keeping it.
    s.fill_text_field("pw", "x").expect("refill");
    assert_eq!(stored_v(&s), None, "a stale stored password survived");
}

#[test]
fn a_password_field_created_with_a_value_is_masked_and_keeps_no_value() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/minimal.pdf");
    let mut s = EditSession::new(Document::load(&path).expect("load minimal.pdf"));
    s.add_text_field(
        &NewTextField::new(0, "pw", BOX)
            .with_password(true)
            .with_value(SECRET)
            .declining_tooltip(),
    )
    .expect("author");
    assert_eq!(stored_v(&s), None);
    assert!(ap_bytes(&s).contains("(*******)"));
    assert!(!contains(&saved(&s), SECRET));
}

#[test]
fn turning_password_on_removes_the_stored_value_and_masks_it() {
    use pdfcer_core::edit::FieldEdit;
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/minimal.pdf");
    let mut s = EditSession::new(Document::load(&path).expect("load minimal.pdf"));
    s.add_text_field(&NewTextField::new(0, "pw", BOX).declining_tooltip())
        .expect("author");
    s.fill_text_field("pw", SECRET).expect("fill");
    assert!(stored_v(&s).is_some());

    let out = s
        .edit_field("pw", &FieldEdit::new().with_password(true))
        .expect("edit");
    assert!(out.password_value_removed);
    assert!(out.appearance_regenerated);
    assert_eq!(stored_v(&s), None);
    assert!(ap_bytes(&s).contains("(*******)"), "{}", ap_bytes(&s));
    assert!(
        !contains(&saved(&s), SECRET),
        "the password reached the saved file"
    );

    // One undo restores the stored value and the drawn text together.
    s.undo().expect("undo");
    assert!(stored_v(&s).is_some());
    assert!(ap_bytes(&s).contains(SECRET));
}
