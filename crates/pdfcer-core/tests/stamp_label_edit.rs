//! `set_stamp_label` and `TextAnnotStyle::label` change the words on a placed
//! stamp's face (pdfcer-gui request G151), keeping its size, colour, comment
//! and object identity, and survive save and reopen.

use pdfcer_core::annot_author::{
    Color, StampFit, StampLabelFit, StampName, StampStyle, TextAnnotSpec,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{
    CommandKind, EditError, EditSession, MarkupNote, StyleEdit, TextAnnotStyle,
};
use pdfcer_core::fontdata::Std14;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::vartext::{Quadding, TextColor};
use pdfcer_core::writer::SaveOptions;

const BLANK: &[u8] = b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >> endobj\n\
trailer << /Size 4 /Root 1 0 R >>\n";

const RECT: Rect = Rect {
    llx: 50.0,
    lly: 50.0,
    urx: 250.0,
    ury: 90.0,
};

/// A session holding one red `DRAFT`-named stamp reading `CHECKED` at 14 pt,
/// with a comment.
fn placed() -> (EditSession, ObjId) {
    let mut s = EditSession::new(Document::from_bytes(BLANK.to_vec()).expect("rebuildable"));
    let id = s
        .add_text_annotation(
            0,
            &TextAnnotSpec::Stamp {
                rect: RECT,
                name: StampName::Draft,
                label: Some("CHECKED".to_owned()),
                color: Color::Rgb(1.0, 0.0, 0.0),
                style: StampStyle::points(14.0),
            },
        )
        .expect("place");
    s.set_markup_note(id, &MarkupNote::new("see sheet 2"))
        .expect("comment");
    (s, id)
}

fn reopened(s: &EditSession) -> EditSession {
    let bytes = s.to_full_bytes(&SaveOptions::identity()).expect("save").0;
    EditSession::new(Document::from_bytes(bytes).expect("re-parse"))
}

fn label_and_size(s: &EditSession, id: ObjId) -> (String, f64) {
    let p = s
        .stamp_label_parameters(id)
        .expect("exists")
        .expect("pdfcer drew it");
    (p.label, p.size)
}

fn key(s: &EditSession, id: ObjId, k: &[u8]) -> Option<Object> {
    let Some(Object::Dict(d)) = s.value(id) else {
        panic!("not a dict")
    };
    d.get(k).cloned()
}

#[test]
fn a_new_label_keeps_size_colour_comment_and_identity() {
    let (mut s, id) = placed();
    let colour = key(&s, id, b"C");
    let change = s.set_stamp_label(id, "APPROVED").expect("relabel");
    assert!(change.label_written && !change.color_written);
    assert_eq!(change.annot_id, id);
    let back = reopened(&s);
    assert_eq!(label_and_size(&back, id), ("APPROVED".to_owned(), 14.0));
    assert_eq!(key(&back, id, b"C"), colour);
    assert_eq!(
        key(&back, id, b"Contents"),
        Some(Object::String(b"see sheet 2".to_vec()))
    );
}

#[test]
fn a_relabel_is_one_undo() {
    let (mut s, id) = placed();
    let depth = s.undo_kinds().len();
    s.set_stamp_label(id, "VOID").expect("relabel");
    assert_eq!(s.undo_kinds().len(), depth + 1);
    assert_eq!(s.undo(), Some(CommandKind::SetTextAnnotStyle));
    assert_eq!(label_and_size(&s, id).0, "CHECKED");
}

#[test]
fn a_longer_label_grows_the_box_unless_the_caller_shrinks_it() {
    let (mut s, id) = placed();
    let long = "APPROVED FOR CONSTRUCTION SUBJECT TO COMMENTS";
    let grown = s.set_stamp_label(id, long).expect("grow");
    assert!(
        matches!(grown.stamp_label_fit, Some(StampLabelFit::BoxGrown { .. })),
        "{:?}",
        grown.stamp_label_fit
    );
    assert!(grown.rect_after.urx > RECT.urx);

    let (mut s, id) = placed();
    let shrunk = s
        .set_text_annot_style(
            id,
            &TextAnnotStyle {
                label: Some(StyleEdit::Set(long.to_owned())),
                stamp_fit: Some(StampFit::ShrinkToBox),
                ..Default::default()
            },
        )
        .expect("shrink");
    assert!(
        matches!(
            shrunk.stamp_label_fit,
            Some(StampLabelFit::LabelShrunk { .. })
        ),
        "{:?}",
        shrunk.stamp_label_fit
    );
    assert_eq!(shrunk.rect_after.urx, RECT.urx);
}

#[test]
fn clear_restores_the_stamp_names_default_words() {
    let (mut s, id) = placed();
    s.set_text_annot_style(
        id,
        &TextAnnotStyle {
            label: Some(StyleEdit::Clear),
            ..Default::default()
        },
    )
    .expect("reset");
    assert_eq!(
        label_and_size(&reopened(&s), id),
        ("DRAFT".to_owned(), 14.0)
    );
}

#[test]
fn a_blank_label_is_refused_and_nothing_changes() {
    let (mut s, id) = placed();
    let depth = s.undo_kinds().len();
    for blank in ["", "   "] {
        let err = s.set_stamp_label(id, blank).unwrap_err();
        assert!(matches!(err, EditError::StampLabelEmpty { .. }), "{err:?}");
    }
    assert_eq!(s.undo_kinds().len(), depth);
}

#[test]
fn a_label_is_refused_on_a_text_box() {
    let mut s = EditSession::new(Document::from_bytes(BLANK.to_vec()).expect("rebuildable"));
    let id = s
        .add_text_annotation(
            0,
            &TextAnnotSpec::FreeText {
                rect: RECT,
                text: "box".to_owned(),
                font: Std14::Helvetica,
                font_size: 12.0,
                color: TextColor::Gray(0.0),
                quadding: Quadding::Left,
                multiline: false,
                border: None,
                border_width: 1.0,
                frame: Default::default(),
            },
        )
        .expect("place");
    let err = s.set_stamp_label(id, "NOPE").unwrap_err();
    assert!(
        matches!(&err, EditError::StylePropertyNotApplicable { property, .. } if *property == "stamp label"),
        "{err:?}"
    );
}
