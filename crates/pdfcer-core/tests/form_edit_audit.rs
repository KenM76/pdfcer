//! Field and widget edits must change only what they name, and whatever they
//! change must reach the file the way a reader will see it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, Visibility, WidgetEdit};
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Dict, ObjId, Object};

fn assemble(bodies: &[String]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
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

fn session(bodies: &[&str]) -> EditSession {
    let owned: Vec<String> = bodies.iter().map(|s| (*s).to_owned()).collect();
    EditSession::new(Document::from_bytes(assemble(&owned)).expect("fixture parses"))
}

fn obj_dict(s: &EditSession, n: u32) -> Dict {
    let g = s.graph();
    match g.resolve(&Object::Reference(ObjId::new(n, 0))) {
        Object::Dict(d) => d.clone(),
        other => panic!("not a dictionary: {other:?}"),
    }
}

fn field(s: &EditSession, name: &str) -> forms::Field {
    forms::parse_acroform(&s.graph())
        .unwrap()
        .fields
        .into_iter()
        .find(|f| f.fully_qualified_name == name)
        .unwrap()
}

/// One text field, one widget (object 4), carrying `/F` = `flags`.
fn widget_with_flags(flags: i64) -> EditSession {
    let widget = format!(
        "<< /Type /Annot /Subtype /Widget /FT /Tx /T (Name) /P 3 0 R \
         /Rect [20 300 220 324] /F {flags} >>"
    );
    session(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Annots [4 0 R] >>",
        &widget,
    ])
}

// ---------------------------------------------------------------------------
// Visibility owns three `/F` bits, not the word
// ---------------------------------------------------------------------------

#[test]
fn setting_visibility_keeps_the_widgets_other_annotation_flags() {
    // Print | Locked | NoZoom | ReadOnly(annot).
    let mut s = widget_with_flags(4 | 128 | 8 | 64);
    s.edit_widget(
        "Name",
        0,
        &WidgetEdit::new().with_visibility(Visibility::PrintOnly),
    )
    .unwrap();
    assert_eq!(
        obj_dict(&s, 4).get(b"F").and_then(Object::as_int),
        Some(36 | 128 | 8 | 64),
        "only Hidden/Print/NoView change; Locked, NoZoom and ReadOnly survive"
    );
}

#[test]
fn visibility_reads_through_flags_it_does_not_own() {
    let s = widget_with_flags(4 | 8);
    assert_eq!(
        field(&s, "Name").widgets[0].visibility,
        Some(Visibility::VisibleAndPrints),
        "Print | NoZoom is visible-and-prints; NoZoom is not a visibility bit"
    );
    let s = widget_with_flags(4 | 2);
    assert_eq!(
        field(&s, "Name").widgets[0].visibility,
        None,
        "Print | Hidden is not one of the four and still reads as None"
    );
}
