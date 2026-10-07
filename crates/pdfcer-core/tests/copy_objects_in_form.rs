//! `EditSession::copy_objects_in_form` (pdfcer-gui request G145): copy leaves
//! inside a form XObject to a clip that pastes as page content where the leaf
//! was drawn.

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::vector::{Bounds, Matrix, VectorObject};
use pdfcer_core::writer::SaveOptions;

/// `Fm0` (own `/Resources` with a font) placed at `2 0 0 2 10 10 cm`:
/// leaf 0 a square, leaf 1 a line, leaf 2 text in `/F1`. `Fm1` (no
/// `/Resources`) placed at `1 0 0 1 60 60 cm`: leaf 3 a square.
fn fixture() -> Vec<u8> {
    let page = "q 2 0 0 2 10 10 cm /Fm0 Do Q\nq 1 0 0 1 60 60 cm /Fm1 Do Q\n";
    let fm0 = "0 0 10 10 re S\n12 0 m 18 6 l S\nBT /F1 6 Tf 0 14 Td (A) Tj ET\n";
    let fm1 = "0 0 5 5 re f\n";
    let stream = |dict: &str, body: &str| {
        format!(
            "<< {dict} /Length {} >>\nstream\n{body}endstream",
            body.len()
        )
    };
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /XObject \
         << /Fm0 5 0 R /Fm1 6 0 R >> >> /Contents 4 0 R >>"
            .to_owned(),
        stream("", page),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 40 40] /Resources << /Font << /F1 7 0 R \
             >> >>",
            fm0,
        ),
        stream("/Type /XObject /Subtype /Form /BBox [0 0 10 10]", fm1),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objs.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = offsets.len() + 1;
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

fn session(bytes: Vec<u8>) -> EditSession {
    EditSession::new(Document::from_bytes(bytes).unwrap())
}

fn leaf_bbox(s: &mut EditSession, i: usize) -> Bounds {
    s.page_objects(0).unwrap().leaves[i].object.page_bbox()
}

fn close(a: Bounds, b: Bounds) -> bool {
    let d = |x: f64, y: f64| (x - y).abs() < 1e-6;
    d(a.min.x, b.min.x) && d(a.min.y, b.min.y) && d(a.max.x, b.max.x) && d(a.max.y, b.max.y)
}

#[test]
fn a_leaf_pastes_as_page_content_where_and_as_large_as_it_was_drawn() {
    let mut s = session(fixture());
    let drawn = leaf_bbox(&mut s, 0);
    let clip = s.copy_objects_in_form(0, &[0, 1]).unwrap();
    assert_eq!(clip.items.len(), 2);
    assert!(s.undo_kind().is_none(), "copy commits nothing");

    let before = s.page_objects(0).unwrap().objects.len();
    s.paste_objects(0, &clip, Matrix::IDENTITY).unwrap();
    let saved = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let mut r = session(saved);
    let model = r.page_objects(0).unwrap();
    assert_eq!(
        model.objects.len(),
        before + 2,
        "pasted onto the page itself"
    );
    let pasted = model
        .objects
        .iter()
        .find(|o| matches!(o, VectorObject::Path(_)))
        .unwrap();
    assert!(
        close(pasted.page_bbox(), drawn),
        "{:?} vs {drawn:?}",
        pasted.page_bbox()
    );
    assert!(
        (drawn.max.x - drawn.min.x - 20.0).abs() < 1e-6,
        "the 2x placement is in the drawn size"
    );
}

#[test]
fn text_carries_the_forms_font_and_a_resourceless_form_copies_too() {
    let mut s = session(fixture());
    let clip = s.copy_objects_in_form(0, &[2]).unwrap();
    assert_eq!(clip.items.len(), 1);
    assert!(
        clip.items[0].bindings.iter().any(|b| b.name == b"F1"),
        "{:?}",
        clip.items[0].bindings
    );
    assert!(!clip.objects.is_empty(), "the font travels with the clip");
    s.paste_objects(0, &clip, Matrix::IDENTITY).unwrap();

    let clip = s.copy_objects_in_form(0, &[3]).unwrap();
    assert_eq!(clip.items.len(), 1);
    assert!(close(clip.bbox, leaf_bbox(&mut s, 3)));
}

#[test]
fn bad_selections_are_refused() {
    let mut s = session(fixture());
    let err = s.copy_objects_in_form(0, &[]).unwrap_err();
    assert!(
        matches!(err, EditError::FormLeafOutOfRange { .. }),
        "{err:?}"
    );
    let err = s.copy_objects_in_form(0, &[9]).unwrap_err();
    assert!(
        matches!(err, EditError::FormLeafOutOfRange { .. }),
        "{err:?}"
    );
    let err = s.copy_objects_in_form(0, &[0, 3]).unwrap_err();
    assert!(
        matches!(err, EditError::FormLeafSelectionSpansForms { .. }),
        "{err:?}"
    );
}
