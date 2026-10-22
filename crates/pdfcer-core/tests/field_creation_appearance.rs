//! `/DA` stated at creation: `NewTextField::with_appearance` and
//! `NewChoiceField::with_appearance` write the font, size and colour the
//! value is drawn in, in the one command that creates the field.
//!
//! Pinned: the `/DA` string, the `/DR /Font` resource it names, an appearance
//! identical to the one `edit_field` would bake for the same `/DA`, the
//! refusals (an absent resource key, a name that would merge), and the
//! resource on the path that registers no new root field.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::{
    ChoiceOption, EditError, EditSession, FieldAppearance, FieldEdit, NewChoiceField, NewTextField,
};
use pdfcer_core::fontdata::Std14;
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::Object;
use pdfcer_core::page_tree::Rect;
use pdfcer_core::vartext::TextColor;

const BOX: Rect = Rect {
    llx: 40.0,
    lly: 700.0,
    urx: 240.0,
    ury: 720.0,
};

fn session() -> EditSession {
    let path: PathBuf =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/minimal.pdf");
    EditSession::new(Document::load(&path).expect("load minimal.pdf"))
}

fn courier_red() -> FieldAppearance {
    FieldAppearance::standard(Std14::Courier, 18.0, TextColor::Rgb(1.0, 0.0, 0.0))
}

fn field(s: &EditSession, name: &str) -> forms::Field {
    forms::parse_acroform(&s.graph())
        .expect("an AcroForm")
        .fields
        .iter()
        .find(|f| f.fully_qualified_name == name)
        .cloned()
        .unwrap_or_else(|| panic!("no field {name}"))
}

fn da(s: &EditSession, name: &str) -> String {
    String::from_utf8_lossy(&field(s, name).default_appearance.expect("a /DA")).into_owned()
}

fn dr_font_keys(s: &EditSession) -> Vec<String> {
    let g = s.graph();
    let dict = |o: Option<&Object>| {
        o.map(|o| g.resolve(o).clone())
            .and_then(|o| o.as_dict().cloned())
    };
    let root = dict(g.trailer_entry(b"Root"));
    let af = root.and_then(|r| dict(r.get(b"AcroForm")));
    let fonts = af
        .and_then(|af| dict(af.get(b"DR")))
        .and_then(|dr| dict(dr.get(b"Font")));
    fonts.map_or_else(Vec::new, |f| {
        f.0.iter()
            .map(|(k, _)| String::from_utf8_lossy(k.as_bytes()).into_owned())
            .collect()
    })
}

fn ap_bytes(s: &EditSession, name: &str) -> Vec<u8> {
    let g = s.graph();
    let widget = g
        .resolved(field(s, name).widgets[0].id)
        .as_dict()
        .cloned()
        .expect("widget");
    let ap = widget
        .get(b"AP")
        .map(|o| g.resolve(o).clone())
        .expect("/AP");
    let n = ap.as_dict().and_then(|d| d.get(b"N")).expect("/N");
    let Object::Stream(st) = g.resolve(n).clone() else {
        panic!("/N is not a stream");
    };
    s.view().slice(st.data_span).unwrap_or_default().to_vec()
}

#[test]
fn a_text_field_is_created_in_the_stated_face_size_and_colour() {
    let mut s = session();
    s.add_text_field(
        &NewTextField::new(0, "t", BOX)
            .declining_tooltip()
            .with_value("Hello")
            .with_appearance(courier_red()),
    )
    .expect("create");
    assert_eq!(
        da(&s, "t"),
        "/Cour 18 Tf
1 0 0 rg
"
    );
    assert!(
        dr_font_keys(&s).contains(&"Cour".to_owned()),
        "{:?}",
        dr_font_keys(&s)
    );
    let ap = String::from_utf8_lossy(&ap_bytes(&s, "t")).into_owned();
    assert!(ap.contains("/Cour 18 Tf"), "{ap}");
}

/// The created stream is the one `edit_field` bakes for the same `/DA`, so the
/// GUI's two-step and the one-step cannot draw differently.
#[test]
fn creating_with_an_appearance_draws_what_editing_to_it_draws() {
    let mut one = session();
    one.add_text_field(
        &NewTextField::new(0, "t", BOX)
            .declining_tooltip()
            .with_value("Hello")
            .with_appearance(courier_red()),
    )
    .expect("create styled");
    let mut two = session();
    two.add_text_field(
        &NewTextField::new(0, "t", BOX)
            .declining_tooltip()
            .with_value("Hello"),
    )
    .expect("create plain");
    two.edit_field("t", &FieldEdit::new().with_appearance(courier_red()))
        .expect("restyle");
    assert_eq!(da(&one, "t"), da(&two, "t"));
    assert_eq!(ap_bytes(&one, "t"), ap_bytes(&two, "t"));
}

#[test]
fn no_appearance_keeps_the_creation_default() {
    let mut s = session();
    s.add_text_field(&NewTextField::new(0, "t", BOX).declining_tooltip())
        .expect("create");
    assert_eq!(
        da(&s, "t"),
        "/Helv 0 Tf
0 g
"
    );
    assert_eq!(dr_font_keys(&s), ["Helv"]);
}

#[test]
fn a_choice_field_is_created_in_the_stated_face() {
    let mut s = session();
    let options = vec![ChoiceOption {
        export: "a".into(),
        display: "Apple".into(),
    }];
    s.add_choice_field(
        &NewChoiceField::new(0, "c", BOX, options)
            .declining_tooltip()
            .with_appearance(FieldAppearance::standard(
                Std14::TimesBold,
                12.0,
                TextColor::Gray(0.25),
            )),
    )
    .expect("create");
    assert_eq!(
        da(&s, "c"),
        "/TiBo 12 Tf
0.25 g
"
    );
    assert!(
        dr_font_keys(&s).contains(&"TiBo".to_owned()),
        "{:?}",
        dr_font_keys(&s)
    );
}

#[test]
fn an_absent_font_resource_is_refused_and_creates_nothing() {
    let mut s = session();
    let spec = NewTextField::new(0, "t", BOX)
        .declining_tooltip()
        .with_appearance(FieldAppearance::resource(
            b"Nope".to_vec(),
            10.0,
            TextColor::Gray(0.0),
        ));
    match s.add_text_field(&spec) {
        Err(EditError::FieldFontNotInResources { name, .. }) => assert_eq!(name, "Nope"),
        other => panic!("expected FieldFontNotInResources, got {other:?}"),
    }
    assert!(forms::parse_acroform(&s.graph()).is_none_or(|f| f.fields.is_empty()));
}

#[test]
fn a_present_font_resource_is_used() {
    let mut s = session();
    s.add_text_field(&NewTextField::new(0, "first", BOX).declining_tooltip())
        .expect("seed /DR with Helv");
    let r = Rect {
        lly: 600.0,
        ury: 620.0,
        ..BOX
    };
    s.add_text_field(
        &NewTextField::new(0, "t", r)
            .declining_tooltip()
            .with_appearance(FieldAppearance::resource(
                b"Helv".to_vec(),
                9.0,
                TextColor::Gray(0.5),
            )),
    )
    .expect("an existing key is accepted");
    assert_eq!(
        da(&s, "t"),
        "/Helv 9 Tf
0.5 g
"
    );
}

/// A second widget draws in its field's `/DA`, so an appearance on a merge is
/// refused rather than dropped.
#[test]
fn an_appearance_on_a_name_that_would_merge_is_refused() {
    let mut s = session();
    s.add_text_field(&NewTextField::new(0, "t", BOX).declining_tooltip())
        .expect("create");
    let r = Rect {
        lly: 600.0,
        ury: 620.0,
        ..BOX
    };
    match s.add_text_field(
        &NewTextField::new(0, "t", r)
            .declining_tooltip()
            .with_appearance(courier_red()),
    ) {
        Err(EditError::FieldAppearanceOnMerge { name }) => assert_eq!(name, "t"),
        other => panic!("expected FieldAppearanceOnMerge, got {other:?}"),
    }
    assert_eq!(field(&s, "t").widgets.len(), 1);
    assert_eq!(
        da(&s, "t"),
        "/Helv 0 Tf
0 g
"
    );
}

/// `a.c` hangs off the already-registered `a`, so no `/Fields` write carries
/// the font: it must still reach `/DR`.
#[test]
fn a_field_under_an_existing_parent_still_gains_its_font_resource() {
    let mut s = session();
    s.add_text_field(&NewTextField::new(0, "a.b", BOX).declining_tooltip())
        .expect("create a.b");
    let r = Rect {
        lly: 600.0,
        ury: 620.0,
        ..BOX
    };
    s.add_text_field(
        &NewTextField::new(0, "a.c", r)
            .declining_tooltip()
            .with_appearance(courier_red()),
    )
    .expect("create a.c");
    assert_eq!(
        da(&s, "a.c"),
        "/Cour 18 Tf
1 0 0 rg
"
    );
    assert!(
        dr_font_keys(&s).contains(&"Cour".to_owned()),
        "{:?}",
        dr_font_keys(&s)
    );
}

/// A form whose `/DR /Font /F1` (object 4) is a WinAnsi TrueType face.
fn session_with_embedded_dr_font() -> EditSession {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [] /DR << /Font << /F1 4 0 R >> >> >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 800] >>",
        "<< /Type /Font /Subtype /TrueType /BaseFont /Calibri /Encoding /WinAnsiEncoding \
         /FirstChar 72 /LastChar 72 /Widths [1000] /FontDescriptor << /MissingWidth 250 /Ascent 900 >> >>",
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = buf.len();
    buf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    EditSession::new(Document::from_bytes(buf).expect("fixture parses"))
}

/// A `/DR` font the document embeds is drawn with, not a standard-14 stand-in,
/// exactly as the regenerator binds it.
#[test]
fn a_created_field_draws_with_the_embedded_dr_font() {
    let mut s = session_with_embedded_dr_font();
    s.add_text_field(
        &NewTextField::new(0, "t", BOX)
            .declining_tooltip()
            .with_value("H")
            .with_appearance(FieldAppearance::resource(
                b"F1".to_vec(),
                12.0,
                TextColor::Gray(0.0),
            )),
    )
    .expect("create");
    let g = s.graph();
    let widget = g
        .resolved(field(&s, "t").widgets[0].id)
        .as_dict()
        .cloned()
        .expect("widget");
    let ap = widget
        .get(b"AP")
        .map(|o| g.resolve(o).clone())
        .expect("/AP");
    let Some(Object::Stream(n)) = ap
        .as_dict()
        .and_then(|d| d.get(b"N"))
        .map(|o| g.resolve(o).clone())
    else {
        panic!("no /AP /N stream");
    };
    let font = n
        .dict
        .get(b"Resources")
        .and_then(Object::as_dict)
        .and_then(|r| r.get(b"Font"))
        .and_then(Object::as_dict)
        .and_then(|f| f.get(b"F1"))
        .cloned();
    assert_eq!(
        font,
        Some(Object::Reference(pdfcer_core::object::ObjId::new(4, 0)))
    );
}
