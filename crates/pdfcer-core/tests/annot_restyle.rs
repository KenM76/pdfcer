//! `/CA` on any annotation and `/C` on the marker subtypes (pdfcer-gui
//! request G150).

use pdfcer_core::annot_author::{
    AttachmentIcon, CaretSpec, Color, FileAttachmentSpec, ScreenSpec, SoundSpec,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{
    AppearanceWrite, EditError, EditSession, MarkerStyle, MarkupOptions, MarkupStyle, StyleEdit,
};
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::sound::{SoundData, WavImportOptions};
use pdfcer_core::writer::SaveOptions;
use std::path::Path;

const RECT: Rect = Rect {
    llx: 100.0,
    lly: 100.0,
    urx: 124.0,
    ury: 124.0,
};

fn session() -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/annot/no-ap-circle.pdf");
    EditSession::new(Document::load(&path).expect("load fixture"))
}

/// The saved annotation dictionary and its `/AP /N` content.
fn saved(s: &EditSession, id: ObjId) -> (Dict, String) {
    let bytes = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save")
        .0;
    saved_from(&Document::from_bytes(bytes).expect("re-parse"), id)
}

fn saved_from(doc: &Document, id: ObjId) -> (Dict, String) {
    let Object::Dict(annot) = doc.get(id).expect("annotation").value.clone() else {
        panic!("not a dict");
    };
    let Some(Object::Dict(ap)) = annot.get(b"AP").map(|o| doc.resolve(o)) else {
        return (annot, String::new());
    };
    let Object::Stream(st) = doc.resolve(ap.get(b"N").expect("/N")) else {
        panic!("no /N stream");
    };
    let text =
        String::from_utf8_lossy(st.data_span.slice(doc.bytes()).expect("bytes")).into_owned();
    (annot, text)
}

fn ca(annot: &Dict) -> Option<f64> {
    annot.get(b"CA").and_then(Object::as_number)
}

/// A 16-sample, 8 kHz, 8-bit mono WAV.
fn wav() -> Vec<u8> {
    let data = [128u8; 16];
    let mut w = b"RIFF".to_vec();
    w.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&8000u32.to_le_bytes());
    w.extend_from_slice(&8000u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&8u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(data.len() as u32).to_le_bytes());
    w.extend_from_slice(&data);
    w
}

/// One of each marker subtype, with a non-default icon on the attachment.
fn markers(s: &mut EditSession) -> Vec<ObjId> {
    let opts = MarkupOptions::default();
    let mut attach = FileAttachmentSpec::new(RECT, "a.txt", b"hi".to_vec());
    attach.icon = AttachmentIcon::Paperclip;
    let sound = SoundData::from_wav(&wav(), &WavImportOptions::default())
        .expect("wav")
        .sound;
    vec![
        s.add_caret_annotation(0, &CaretSpec::new(RECT), &opts)
            .unwrap(),
        s.add_file_attachment_annotation(0, &attach, &opts).unwrap(),
        s.add_sound_annotation(0, &SoundSpec::new(RECT, sound), &opts)
            .unwrap(),
        s.add_screen_annotation(
            0,
            &ScreenSpec::new(RECT, "c.mp4", "video/mp4", vec![0; 4]),
            &opts,
        )
        .unwrap(),
    ]
}

#[test]
fn opacity_sets_clamps_and_clears_ca_without_touching_the_appearance() {
    let mut s = session();
    let id = markers(&mut s)[0];
    let (_, ap_before) = saved(&s, id);

    let change = s.set_annot_opacity(id, StyleEdit::Set(0.4)).unwrap();
    assert_eq!(
        (change.previous, change.current, change.clamped),
        (None, Some(0.4), false)
    );
    let (annot, ap) = saved(&s, id);
    assert_eq!(ca(&annot), Some(0.4));
    assert_eq!(ap, ap_before, "the appearance is not re-baked");
    assert!(!ap.contains(" gs"), "/CA stays out of the stream: {ap}");

    let change = s.set_annot_opacity(id, StyleEdit::Set(1.5)).unwrap();
    assert!(change.clamped);
    assert_eq!(ca(&saved(&s, id).0), Some(1.0));

    s.set_annot_opacity(id, StyleEdit::Clear).unwrap();
    assert_eq!(ca(&saved(&s, id).0), None);
}

#[test]
fn opacity_reaches_a_subtype_set_markup_style_refuses() {
    let mut s = session();
    let ids = markers(&mut s);
    // The screen is not a markup annotation; `set_markup_style` refuses it.
    let screen = ids[3];
    assert!(
        s.set_markup_style(
            screen,
            &MarkupStyle {
                opacity: Some(StyleEdit::Set(0.5)),
                ..MarkupStyle::default()
            }
        )
        .is_err()
    );
    s.set_annot_opacity(screen, StyleEdit::Set(0.5)).unwrap();
    assert_eq!(ca(&saved(&s, screen).0), Some(0.5));
}

#[test]
fn an_unchanged_opacity_commits_nothing_and_nan_is_refused() {
    let mut s = session();
    let id = markers(&mut s)[0];
    let depth = s.undo_depth();
    s.set_annot_opacity(id, StyleEdit::Clear).unwrap();
    assert_eq!(s.undo_depth(), depth);
    assert!(matches!(
        s.set_annot_opacity(id, StyleEdit::Set(f64::NAN)),
        Err(EditError::MarkupOpacityOutOfRange { .. })
    ));
    assert_eq!(s.undo_depth(), depth);
}

#[test]
fn every_marker_subtype_recolours_and_keeps_its_icon() {
    let mut s = session();
    let ids = markers(&mut s);
    for &id in &ids {
        let change = s
            .set_marker_style(id, &MarkerStyle::new(Color::Rgb(0.0, 1.0, 0.0)))
            .unwrap_or_else(|e| panic!("{id:?}: {e}"));
        assert!(!change.appearance_was_foreign, "{}", change.subtype);
        assert!(matches!(change.appearance, AppearanceWrite::InPlace(_)));
        let (annot, ap) = saved(&s, id);
        let Some(Object::Array(c)) = annot.get(b"C") else {
            panic!("{}: no /C", change.subtype);
        };
        assert_eq!(c.len(), 3);
        assert!(ap.contains("0 1 0 rg"), "{}: {ap}", change.subtype);
    }
    let (attach, _) = saved(&s, ids[1]);
    assert_eq!(
        attach.get(b"Name"),
        Some(&Object::Name(b"Paperclip".as_slice().into()))
    );
    assert!(attach.contains_key(b"FS"), "the attachment keeps its file");
}

#[test]
fn a_recolour_is_one_undo_entry() {
    let mut s = session();
    let id = markers(&mut s)[0];
    let (before, _) = saved(&s, id);
    let depth = s.undo_depth();
    s.set_marker_style(id, &MarkerStyle::new(Color::Gray(0.0)))
        .unwrap();
    assert_eq!(s.undo_depth(), depth + 1);
    s.undo();
    assert_eq!(saved(&s, id).0, before);
}

#[test]
fn a_non_marker_is_refused_by_name() {
    let mut s = session();
    let circle = markers(&mut s)[0];
    // The fixture's own circle.
    let doc_circle = ObjId::new(4, 0);
    assert!(circle != doc_circle);
    assert!(matches!(
        s.set_marker_style(doc_circle, &MarkerStyle::new(Color::Gray(0.0))),
        Err(EditError::StylePropertyNotApplicable { .. })
    ));
}

/// A page whose caret draws a blue square another program drew.
fn foreign_caret() -> Vec<u8> {
    let content = "0 0 1 rg 0 0 10 10 re f";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Annots [4 0 R] >>".to_owned(),
        "<< /Type /Annot /Subtype /Caret /Rect [100 100 110 110] /C [0 0 1] \
         /AP << /N 5 0 R >> >>"
            .to_owned(),
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

#[test]
fn a_foreign_marker_is_refused_unless_redrawn() {
    let id = ObjId::new(4, 0);
    let mut s = EditSession::new(Document::from_bytes(foreign_caret()).unwrap());
    let green = Color::Rgb(0.0, 1.0, 0.0);
    assert!(matches!(
        s.set_marker_style(id, &MarkerStyle::new(green)),
        Err(EditError::MarkerAppearanceForeign { .. })
    ));
    assert!(!s.can_undo());

    let mut style = MarkerStyle::new(green);
    style.redraw_as_plain = true;
    let change = s.set_marker_style(id, &style).unwrap();
    assert!(change.appearance_was_foreign);
    assert!(saved(&s, id).1.contains("0 1 0 rg"));
}

#[test]
fn a_locked_marker_is_refused() {
    let mut s = session();
    let id = markers(&mut s)[0];
    s.set_annotation_flags(
        id,
        pdfcer_core::annot::AnnotFlags(
            pdfcer_core::annot::AnnotFlags::LOCKED | pdfcer_core::annot::AnnotFlags::PRINT,
        ),
    )
    .unwrap();
    assert!(matches!(
        s.set_annot_opacity(id, StyleEdit::Set(0.5)),
        Err(EditError::AnnotationLocked { .. })
    ));
    assert!(matches!(
        s.set_marker_style(id, &MarkerStyle::new(Color::Gray(0.0))),
        Err(EditError::AnnotationLocked { .. })
    ));
}

#[test]
fn set_markup_style_refuses_a_nan_opacity() {
    let mut s = session();
    let circle = ObjId::new(4, 0);
    assert!(matches!(
        s.set_markup_style(
            circle,
            &MarkupStyle {
                opacity: Some(StyleEdit::Set(f64::NAN)),
                ..MarkupStyle::default()
            }
        ),
        Err(EditError::MarkupOpacityOutOfRange { .. })
    ));
}
