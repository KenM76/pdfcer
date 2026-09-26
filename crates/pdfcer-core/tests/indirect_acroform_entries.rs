//! `/AcroForm` entries stored as their own objects (`/Fields 5 0 R`,
//! `/DR 7 0 R`, `/CO 6 0 R`) must be extended in place, not replaced.
//! §7.3.10 lets any of them be indirect.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, NewTextField};
use pdfcer_core::form_script::{CalcHelper, SimpleOp};
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::page_tree::Rect;

fn assemble(bodies: &[&str]) -> Vec<u8> {
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

/// One page with one text field `Total` (object 8), registered through an
/// indirect `/Fields` (5), calculated through an indirect `/CO` (6), with an
/// indirect `/DR` (7) carrying a font `/Cour` the new field does not use.
fn indirect_form() -> EditSession {
    let bytes = assemble(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm 4 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Annots [8 0 R] >>",
        "<< /Fields 5 0 R /CO 6 0 R /DR 7 0 R /DA (/Helv 0 Tf 0 g) >>",
        "[8 0 R]",
        "[8 0 R]",
        "<< /Font << /Helv 9 0 R /Cour 10 0 R >> >>",
        "<< /FT /Tx /T (Total) /Type /Annot /Subtype /Widget /Rect [20 300 220 324] \
         /P 3 0 R /AA << /C << /S /JavaScript /JS (event.value = 1;) >> >> >>",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>",
    ]);
    EditSession::new(Document::from_bytes(bytes).expect("fixture parses"))
}

fn rect(y: f64) -> Rect {
    Rect {
        llx: 20.0,
        lly: y,
        urx: 220.0,
        ury: y + 24.0,
    }
}

fn names(s: &EditSession) -> Vec<String> {
    let mut v: Vec<String> = forms::parse_acroform(&s.graph())
        .expect("a form")
        .fields
        .into_iter()
        .map(|f| f.fully_qualified_name)
        .collect();
    v.sort();
    v
}

fn acroform_entry(s: &EditSession, key: &[u8]) -> Object {
    let g = s.graph();
    let af = g
        .resolved(ObjId::new(4, 0))
        .as_dict()
        .expect("AcroForm")
        .clone();
    g.resolve(af.get(key).expect("entry present")).clone()
}

#[test]
fn adding_a_field_keeps_every_field_in_an_indirect_fields_array() {
    let mut s = indirect_form();
    s.add_text_field(&NewTextField::new(0, "Extra", rect(200.0)).declining_tooltip())
        .expect("add a field");
    assert_eq!(names(&s), ["Extra", "Total"]);
    let g = s.graph();
    let af = g
        .resolved(ObjId::new(4, 0))
        .as_dict()
        .expect("AcroForm")
        .clone();
    assert_eq!(
        af.get(b"Fields").and_then(Object::as_reference),
        Some(ObjId::new(5, 0)),
        "/Fields is still its own object, not inlined"
    );
}

#[test]
fn adding_a_field_keeps_the_fonts_in_an_indirect_dr() {
    let mut s = indirect_form();
    s.add_text_field(&NewTextField::new(0, "Extra", rect(200.0)).declining_tooltip())
        .expect("add a field");
    let dr = acroform_entry(&s, b"DR");
    let font = dr
        .as_dict()
        .and_then(|d| d.get(b"Font"))
        .expect("/DR /Font");
    let font = s
        .graph()
        .resolve(font)
        .as_dict()
        .expect("font dict")
        .clone();
    assert!(font.get(b"Cour").is_some(), "/Cour survived: {font:?}");
}

#[test]
fn a_calculation_appends_to_an_indirect_co_array() {
    let mut s = indirect_form();
    s.add_text_field(&NewTextField::new(0, "A", rect(200.0)).declining_tooltip())
        .expect("add a field");
    s.add_text_field(&NewTextField::new(0, "B", rect(150.0)).declining_tooltip())
        .expect("add a field");
    s.set_field_calculation(
        "B",
        Some(CalcHelper::Simple {
            op: SimpleOp::Sum,
            operands: vec![b"A".to_vec()],
        }),
    )
    .expect("set a calculation");
    let co = acroform_entry(&s, b"CO");
    let co = co.as_array().expect("/CO array");
    assert_eq!(co.len(), 2, "Total kept, B appended: {co:?}");
    assert_eq!(co[0].as_reference(), Some(ObjId::new(8, 0)));
}
