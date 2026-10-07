//! The `/AcroForm` `/Q` is the document-wide default quadding (ISO 32000-1
//! §12.7.2 Table 218): a field with no `/Q` of its own or on any ancestor
//! reads back, and is drawn, with it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, FieldEdit};
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::Object;
use pdfcer_core::vartext::Quadding;

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

/// A text field "t" under an `/AcroForm` carrying `acro_q`, with `field_q` of
/// its own.
fn session(acro_q: &str, field_q: &str) -> EditSession {
    let bodies = [
        format!(
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] {acro_q} \
             /DA (/Helv 10 Tf 0 g) /DR << /Font << /Helv 5 0 R >> >> >> >>"
        ),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Annots [4 0 R] >>".to_owned(),
        format!(
            "<< /FT /Tx /T (t) /Type /Annot /Subtype /Widget /P 3 0 R \
             /Rect [20 50 200 72] {field_q} >>"
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
         /Encoding /WinAnsiEncoding >>"
            .to_owned(),
    ];
    EditSession::new(Document::from_bytes(assemble(&bodies)).expect("fixture parses"))
}

fn field(s: &EditSession) -> forms::Field {
    forms::parse_acroform(&s.graph())
        .unwrap()
        .fields
        .into_iter()
        .find(|f| f.fully_qualified_name == "t")
        .unwrap()
}

fn normal_ap(s: &EditSession) -> Vec<u8> {
    let g = s.graph();
    let w = g
        .resolved(field(s).widgets[0].id)
        .as_dict()
        .cloned()
        .unwrap();
    let ap = w.get(b"AP").map(|o| g.resolve(o).clone()).unwrap();
    let n = ap
        .as_dict()
        .and_then(|d| d.get(b"N"))
        .map(|o| g.resolve(o).clone());
    let Some(Object::Stream(st)) = n else {
        panic!("no /AP /N stream: {n:?}");
    };
    s.view().slice(st.data_span).unwrap().to_vec()
}

#[test]
fn a_field_with_no_q_reads_back_the_acroform_q() {
    assert_eq!(field(&session("/Q 1", "")).quadding, Quadding::Center);
    assert_eq!(field(&session("/Q 2", "")).quadding, Quadding::Right);
    assert_eq!(field(&session("", "")).quadding, Quadding::Left);
    assert_eq!(field(&session("/Q 2", "/Q 0")).quadding, Quadding::Left);
}

#[test]
fn a_fill_under_an_acroform_q_is_drawn_with_it() {
    let mut inherited = session("/Q 1", "");
    let mut own = session("", "/Q 1");
    inherited.fill_text_field("t", "Hi").unwrap();
    own.fill_text_field("t", "Hi").unwrap();
    assert_eq!(
        String::from_utf8_lossy(&normal_ap(&inherited)),
        String::from_utf8_lossy(&normal_ap(&own)),
        "the form-wide centring was not drawn"
    );
}

#[test]
fn clearing_a_fields_q_reads_back_the_acroform_q() {
    let mut s = session("/Q 2", "/Q 0");
    s.edit_field("t", &FieldEdit::new().clearing_quadding())
        .unwrap();
    assert_eq!(field(&s).quadding, Quadding::Right);
}

#[test]
fn own_quadding_follows_setting_and_clearing_the_fields_q() {
    let mut s = session("/Q 1", "");
    assert_eq!(field(&s).own_quadding, None);
    s.edit_field("t", &FieldEdit::new().with_quadding(1))
        .unwrap();
    let f = field(&s);
    assert_eq!(
        (f.quadding, f.own_quadding),
        (Quadding::Center, Some(Quadding::Center))
    );
    s.edit_field("t", &FieldEdit::new().clearing_quadding())
        .unwrap();
    let f = field(&s);
    assert_eq!((f.quadding, f.own_quadding), (Quadding::Center, None));
}
