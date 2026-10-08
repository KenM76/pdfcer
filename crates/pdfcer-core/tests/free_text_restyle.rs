//! `set_text_annot_style` on a `/FreeText` (pdfcer-gui request G148): a box
//! pdfcer cannot redraw faithfully is refused unless the caller opts into a
//! plain redraw, and `font_size` reaches `/DA`.

use pdfcer_core::annot_author::{Color, TextAnnotSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, TextAnnotStyle};
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::writer::SaveOptions;

/// A `/FreeText` at object 4. `ap` is its `/AP /N` content (object 5), or
/// `None` for no appearance; `extra` is spliced into the dictionary.
fn doc_with_free_text(ap: Option<&str>, extra: &str) -> EditSession {
    let ap_ref = if ap.is_some() {
        "/AP << /N 5 0 R >>"
    } else {
        ""
    };
    let mut pdf = format!(
        "%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
         2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
         3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Annots [4 0 R] >> endobj\n\
         4 0 obj << /Type /Annot /Subtype /FreeText /Rect [20 20 120 90] \
         /Contents (alpha beta gamma delta epsilon zeta) /DA (/Helv 12 Tf 0 g) {ap_ref} {extra} >> endobj\n"
    );
    if let Some(body) = ap {
        pdf.push_str(&format!(
            "5 0 obj << /Type /XObject /Subtype /Form /BBox [0 0 100 70] /Length {} >>\nstream\n{body}\nendstream\nendobj\n",
            body.len() + 1
        ));
    }
    pdf.push_str("trailer << /Size 6 /Root 1 0 R >>\n");
    EditSession::new(Document::from_bytes(pdf.into_bytes()).expect("rebuildable"))
}

const ID: ObjId = ObjId::new(4, 0);

fn recolour(redraw_as_plain: bool) -> TextAnnotStyle {
    TextAnnotStyle {
        color: Some(Color::Rgb(1.0, 0.0, 0.0)),
        redraw_as_plain,
        ..Default::default()
    }
}

/// The saved annotation dictionary and its `/AP /N` bytes.
fn saved(s: &EditSession) -> (pdfcer_core::object::Dict, String) {
    let bytes = s.to_full_bytes(&SaveOptions::identity()).expect("save").0;
    let doc = Document::from_bytes(bytes).expect("re-parse");
    let Object::Dict(annot) = doc.get(ID).expect("annotation").value.clone() else {
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
    (annot, painted)
}

#[test]
fn a_foreign_text_box_is_refused_and_left_unchanged() {
    let mut s = doc_with_free_text(Some("0 0 1 rg 0 0 100 70 re f"), "");
    let err = s.set_text_annot_style(ID, &recolour(false)).unwrap_err();
    assert!(
        matches!(err, EditError::FreeTextAppearanceForeign { id } if id == ID),
        "{err:?}"
    );
    assert!(s.undo_kind().is_none(), "nothing committed");
    assert_eq!(saved(&s).1, "0 0 1 rg 0 0 100 70 re f\n");
}

#[test]
fn a_foreign_text_box_redraws_wrapped_when_asked() {
    let mut s = doc_with_free_text(Some("0 0 1 rg 0 0 100 70 re f"), "");
    let change = s.set_text_annot_style(ID, &recolour(true)).expect("redraw");
    assert!(change.appearance_was_foreign);
    let painted = saved(&s).1;
    assert!(
        painted.matches("Tj").count() >= 2,
        "wrapped within the 100pt box: {painted}"
    );
}

#[test]
fn rich_text_is_refused_then_dropped_when_asked() {
    let rc = "/RC (<body><p><b>alpha</b></p></body>) /DS (font: 12pt Helvetica)";
    let mut s = doc_with_free_text(None, rc);
    let err = s.set_text_annot_style(ID, &recolour(false)).unwrap_err();
    assert!(
        matches!(err, EditError::FreeTextIsRichText { id } if id == ID),
        "{err:?}"
    );
    assert!(s.undo_kind().is_none());

    let change = s.set_text_annot_style(ID, &recolour(true)).expect("redraw");
    assert_eq!(change.rich_text_dropped, ["RC", "DS"]);
    let (annot, painted) = saved(&s);
    assert!(!annot.contains_key(b"RC") && !annot.contains_key(b"DS"));
    assert!(painted.contains("alpha"), "{painted}");
}

#[test]
fn font_size_reaches_the_da_and_the_appearance() {
    let mut s = EditSession::new(
        Document::from_bytes(
            b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] >> endobj\n\
trailer << /Size 4 /Root 1 0 R >>\n"
                .to_vec(),
        )
        .expect("rebuildable"),
    );
    let id = s
        .add_text_annotation(
            0,
            &TextAnnotSpec::FreeText {
                rect: Rect {
                    llx: 20.0,
                    lly: 20.0,
                    urx: 220.0,
                    ury: 90.0,
                },
                text: "short".to_owned(),
                font: pdfcer_core::fontdata::Std14::Helvetica,
                font_size: 12.0,
                color: pdfcer_core::vartext::TextColor::Gray(0.0),
                quadding: pdfcer_core::vartext::Quadding::Left,
                multiline: false,
                border: None,
                border_width: 1.0,
                frame: Default::default(),
            },
        )
        .expect("place");
    let change = s
        .set_text_annot_style(
            id,
            &TextAnnotStyle {
                font_size: Some(20.0),
                ..Default::default()
            },
        )
        .expect("resize the text");
    assert!(change.font_size_written && !change.appearance_was_foreign);
    let bytes = s.to_full_bytes(&SaveOptions::identity()).unwrap().0;
    let doc = Document::from_bytes(bytes).unwrap();
    let Object::Dict(annot) = &doc.get(id).unwrap().value else {
        panic!()
    };
    let Some(Object::String(da)) = annot.get(b"DA") else {
        panic!("no /DA")
    };
    assert!(
        String::from_utf8_lossy(da).contains("20 Tf"),
        "{:?}",
        String::from_utf8_lossy(da)
    );
}
