//! `set_text_annot_style` reaches a `/FreeText`'s frame and text (pdfcer-gui
//! request G149): fill, border width, dash, opacity, text colour and face
//! each survive save, reopen and `text_spec_from_dict`, and the frame-only
//! properties are refused by name on a sticky note or a stamp.

use pdfcer_core::annot_author::{
    BorderDash, Color, StickyIcon, TextAnnotSpec, text_spec_from_dict,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, StyleEdit, TextAnnotStyle};
use pdfcer_core::fontdata::Std14;
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::vartext::{Quadding, TextColor};
use pdfcer_core::writer::SaveOptions;

const BLANK: &[u8] = b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] >> endobj\n\
trailer << /Size 4 /Root 1 0 R >>\n";

const RECT: Rect = Rect {
    llx: 20.0,
    lly: 20.0,
    urx: 220.0,
    ury: 90.0,
};

fn session() -> EditSession {
    EditSession::new(Document::from_bytes(BLANK.to_vec()).expect("rebuildable"))
}

fn free_text(border: Option<Color>) -> TextAnnotSpec {
    TextAnnotSpec::FreeText {
        rect: RECT,
        text: "frame me".to_owned(),
        font: Std14::Helvetica,
        font_size: 12.0,
        color: TextColor::Gray(0.0),
        quadding: Quadding::Left,
        multiline: false,
        border,
        border_width: 1.0,
        frame: Default::default(),
    }
}

/// A session holding one placed `/FreeText` with a red border.
fn placed() -> (EditSession, ObjId) {
    let mut s = session();
    let id = s
        .add_text_annotation(0, &free_text(Some(Color::Rgb(1.0, 0.0, 0.0))))
        .expect("place");
    (s, id)
}

/// The saved annotation dictionary, its `/AP /N` bytes and the spec read
/// back from the reopened file.
fn reopened(s: &EditSession, id: ObjId) -> (Dict, String, TextAnnotSpec) {
    let bytes = s.to_full_bytes(&SaveOptions::identity()).expect("save").0;
    let doc = Document::from_bytes(bytes).expect("re-parse");
    let Object::Dict(annot) = doc.get(id).expect("annotation").value.clone() else {
        panic!("not a dict");
    };
    let painted = match annot.get(b"AP").map(|o| doc.resolve(o)) {
        Some(Object::Dict(ap)) => match doc.resolve(ap.get(b"N").expect("/N")) {
            Object::Stream(st) => {
                String::from_utf8_lossy(st.data_span.slice(doc.bytes()).expect("bytes"))
                    .into_owned()
            }
            _ => String::new(),
        },
        _ => String::new(),
    };
    let graph = EditSession::new(doc);
    let spec = text_spec_from_dict(&graph, &annot).expect("spec reads back");
    (annot, painted, spec)
}

fn frame_of(spec: &TextAnnotSpec) -> (Option<Color>, f64, Option<Color>, Option<Vec<f64>>) {
    let TextAnnotSpec::FreeText {
        border,
        border_width,
        frame,
        ..
    } = spec
    else {
        panic!("not a FreeText: {spec:?}");
    };
    (
        *border,
        *border_width,
        frame.fill,
        frame.dash.as_ref().map(|d| d.pattern().to_vec()),
    )
}

fn dash(p: &[f64]) -> BorderDash {
    BorderDash::new(p.to_vec()).expect("valid dash")
}

#[test]
fn fill_border_width_and_dash_round_trip() {
    let (mut s, id) = placed();
    let change = s
        .set_text_annot_style(
            id,
            &TextAnnotStyle {
                fill: Some(StyleEdit::Set(Color::Rgb(1.0, 1.0, 0.0))),
                border_width: Some(2.5),
                dash: Some(StyleEdit::Set(dash(&[4.0, 2.0]))),
                ..Default::default()
            },
        )
        .expect("restyle");
    assert!(change.frame_written && !change.text_style_written && !change.opacity_written);
    let (annot, painted, spec) = reopened(&s, id);
    assert!(annot.contains_key(b"IC"), "fill recorded for a re-bake");
    let (border, width, fill, d) = frame_of(&spec);
    assert_eq!(
        border,
        Some(Color::Rgb(1.0, 0.0, 0.0)),
        "border colour kept"
    );
    assert_eq!(width, 2.5);
    assert_eq!(fill, Some(Color::Rgb(1.0, 1.0, 0.0)));
    assert_eq!(d, Some(vec![4.0, 2.0]));
    assert!(painted.contains("1 1 0 rg"), "fill painted: {painted}");
    assert!(painted.contains("[4 2] 0 d"), "dash painted: {painted}");
}

#[test]
fn a_colour_only_restyle_keeps_the_fill_and_dash() {
    let (mut s, id) = placed();
    s.set_text_annot_style(
        id,
        &TextAnnotStyle {
            fill: Some(StyleEdit::Set(Color::Gray(0.9))),
            dash: Some(StyleEdit::Set(dash(&[3.0]))),
            ..Default::default()
        },
    )
    .expect("frame");
    s.set_text_annot_style(
        id,
        &TextAnnotStyle {
            color: Some(Color::Rgb(0.0, 0.0, 1.0)),
            ..Default::default()
        },
    )
    .expect("recolour");
    let (border, _, fill, d) = frame_of(&reopened(&s, id).2);
    assert_eq!(border, Some(Color::Rgb(0.0, 0.0, 1.0)));
    assert_eq!(fill, Some(Color::Gray(0.9)), "fill survived the re-bake");
    assert_eq!(d, Some(vec![3.0]), "dash survived the re-bake");
}

#[test]
fn clear_removes_fill_and_dash_and_zero_width_removes_the_border() {
    let (mut s, id) = placed();
    s.set_text_annot_style(
        id,
        &TextAnnotStyle {
            fill: Some(StyleEdit::Set(Color::Gray(0.5))),
            dash: Some(StyleEdit::Set(dash(&[2.0]))),
            ..Default::default()
        },
    )
    .expect("frame");
    s.set_text_annot_style(
        id,
        &TextAnnotStyle {
            fill: Some(StyleEdit::Clear),
            dash: Some(StyleEdit::Clear),
            border_width: Some(0.0),
            ..Default::default()
        },
    )
    .expect("clear");
    let (annot, painted, spec) = reopened(&s, id);
    assert!(!annot.contains_key(b"IC"));
    let (border, _, fill, d) = frame_of(&spec);
    assert_eq!((border, fill, d), (None, None, None));
    assert!(!painted.contains(" re S"), "no border stroked: {painted}");
}

#[test]
fn a_width_on_a_borderless_box_draws_a_border() {
    let mut s = session();
    let id = s.add_text_annotation(0, &free_text(None)).expect("place");
    s.set_text_annot_style(
        id,
        &TextAnnotStyle {
            border_width: Some(3.0),
            ..Default::default()
        },
    )
    .expect("border");
    let (border, width, _, _) = frame_of(&reopened(&s, id).2);
    assert_eq!(border, Some(Color::Gray(0.0)), "black when no colour given");
    assert_eq!(width, 3.0);
}

#[test]
fn opacity_text_colour_and_face_round_trip() {
    let (mut s, id) = placed();
    let change = s
        .set_text_annot_style(
            id,
            &TextAnnotStyle {
                opacity: Some(StyleEdit::Set(0.4)),
                text_color: Some(TextColor::Rgb(0.0, 0.5, 0.0)),
                font: Some(Std14::TimesBold),
                ..Default::default()
            },
        )
        .expect("restyle");
    assert!(change.opacity_written && change.text_style_written);
    let (annot, _, spec) = reopened(&s, id);
    assert_eq!(annot.get(b"CA"), Some(&Object::Real(0.4)));
    let TextAnnotSpec::FreeText { color, font, .. } = spec else {
        panic!()
    };
    assert_eq!(color, TextColor::Rgb(0.0, 0.5, 0.0));
    assert_eq!(font, Std14::TimesBold);

    s.set_text_annot_style(
        id,
        &TextAnnotStyle {
            opacity: Some(StyleEdit::Clear),
            ..Default::default()
        },
    )
    .expect("opaque");
    assert!(!reopened(&s, id).0.contains_key(b"CA"));
}

#[test]
fn one_restyle_is_one_undo() {
    let (mut s, id) = placed();
    let before = reopened(&s, id).0;
    s.set_text_annot_style(
        id,
        &TextAnnotStyle {
            fill: Some(StyleEdit::Set(Color::Gray(0.8))),
            opacity: Some(StyleEdit::Set(0.5)),
            border_width: Some(4.0),
            ..Default::default()
        },
    )
    .expect("restyle");
    s.undo().expect("undo");
    assert_eq!(reopened(&s, id).0, before);
}

#[test]
fn opacity_out_of_range_is_refused() {
    let (mut s, id) = placed();
    for bad in [1.5, -0.1, f64::NAN] {
        let err = s
            .set_text_annot_style(
                id,
                &TextAnnotStyle {
                    opacity: Some(StyleEdit::Set(bad)),
                    ..Default::default()
                },
            )
            .unwrap_err();
        assert!(
            matches!(err, EditError::MarkupOpacityOutOfRange { .. }),
            "{err:?}"
        );
    }
    assert_eq!(
        s.undo_kinds().len(),
        1,
        "only the placement is on the stack"
    );
}

#[test]
fn frame_and_text_properties_are_refused_on_a_note_and_a_stamp() {
    let mut s = session();
    let note = s
        .add_text_annotation(
            0,
            &TextAnnotSpec::Sticky {
                rect: RECT,
                icon: StickyIcon::Note,
                contents: "note".to_owned(),
                color: Color::Rgb(1.0, 1.0, 0.0),
                open: false,
            },
        )
        .expect("note");
    let cases: [(TextAnnotStyle, &str); 5] = [
        (
            TextAnnotStyle {
                fill: Some(StyleEdit::Set(Color::Gray(0.5))),
                ..Default::default()
            },
            "a text-box fill",
        ),
        (
            TextAnnotStyle {
                border_width: Some(1.0),
                ..Default::default()
            },
            "a border width",
        ),
        (
            TextAnnotStyle {
                dash: Some(StyleEdit::Clear),
                ..Default::default()
            },
            "a border dash",
        ),
        (
            TextAnnotStyle {
                text_color: Some(TextColor::Gray(0.2)),
                ..Default::default()
            },
            "a text colour",
        ),
        (
            TextAnnotStyle {
                font: Some(Std14::Courier),
                ..Default::default()
            },
            "a text face",
        ),
    ];
    for (style, want) in &cases {
        let err = s.set_text_annot_style(note, style).unwrap_err();
        assert!(
            matches!(&err, EditError::StylePropertyNotApplicable { property, .. } if property == want),
            "{err:?}"
        );
    }
    // Opacity applies to every subtype.
    let change = s
        .set_text_annot_style(
            note,
            &TextAnnotStyle {
                opacity: Some(StyleEdit::Set(0.5)),
                ..Default::default()
            },
        )
        .expect("a note fades");
    assert!(change.opacity_written);
}
