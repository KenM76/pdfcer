//! `FieldEdit::with_default_selections` writes a choice field's `/DV` as a
//! selection: export values, an array on a MultiSelect field (ISO 32000-1
//! §12.7.4.4 Table 231), and `reset_form` restores it with `/I` to match.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, FieldEdit};
use pdfcer_core::forms;
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::{Dict, Object};

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

/// A list box "L" with flags `ff`, options exported as A/B/C and labelled
/// Alpha/Beta/Gamma, holding `/V [(B)]`.
fn session(ff: u32) -> EditSession {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] \
         /DA (/Helv 10 Tf 0 g) /DR << /Font << /Helv 5 0 R >> >> >> >>"
            .to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Annots [4 0 R] >>".to_owned(),
        format!(
            "<< /FT /Ch /Ff {ff} /T (L) /Type /Annot /Subtype /Widget /P 3 0 R \
             /Rect [20 50 200 110] /Opt [[(A) (Alpha)] [(B) (Beta)] [(C) (Gamma)]] \
             /V (B) >>"
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
         /Encoding /WinAnsiEncoding >>"
            .to_owned(),
    ];
    EditSession::new(Document::from_bytes(assemble(&bodies)).expect("fixture parses"))
}

const MULTI: u32 = 2_097_152;

fn dict(s: &EditSession) -> Dict {
    let g = s.graph();
    let id = forms::parse_acroform(&g)
        .unwrap()
        .fields
        .into_iter()
        .find(|f| f.fully_qualified_name == "L")
        .unwrap()
        .id;
    g.resolved(id).as_dict().cloned().unwrap()
}

fn strings(o: Option<&Object>) -> Vec<Vec<u8>> {
    match o {
        Some(Object::Array(a)) => a
            .iter()
            .map(|x| match x {
                Object::String(b) => b.clone(),
                other => panic!("{other:?}"),
            })
            .collect(),
        Some(Object::String(b)) => vec![b.clone()],
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_multi_select_default_is_written_as_an_array_of_exports() {
    let mut s = session(MULTI);
    s.edit_field(
        "L",
        &FieldEdit::new().with_default_selections(["Alpha", "C"]),
    )
    .unwrap();
    let d = dict(&s);
    assert!(matches!(d.get(b"DV"), Some(Object::Array(_))), "{d:?}");
    assert_eq!(strings(d.get(b"DV")), vec![b"A".to_vec(), b"C".to_vec()]);
}

#[test]
fn a_reset_restores_the_multi_value_default_with_its_indices() {
    let mut s = session(MULTI);
    s.edit_field(
        "L",
        &FieldEdit::new().with_default_selections(["Alpha", "Gamma"]),
    )
    .unwrap();
    s.reset_form(None).unwrap();
    let d = dict(&s);
    assert_eq!(strings(d.get(b"V")), vec![b"A".to_vec(), b"C".to_vec()]);
    assert_eq!(
        d.get(b"I"),
        Some(&Object::Array(vec![Object::Integer(0), Object::Integer(2)]))
    );
}

#[test]
fn a_single_select_default_is_a_string() {
    let mut s = session(0);
    s.edit_field("L", &FieldEdit::new().with_default_selections(["Gamma"]))
        .unwrap();
    assert_eq!(dict(&s).get(b"DV"), Some(&Object::String(b"C".to_vec())));
}

#[test]
fn several_defaults_on_a_single_select_field_are_refused_before_writing() {
    let mut s = session(0);
    let before = dict(&s);
    let err = s
        .edit_field("L", &FieldEdit::new().with_default_selections(["A", "B"]))
        .unwrap_err();
    assert!(
        matches!(err, EditError::ChoiceRequiresMultiSelect { count: 2, .. }),
        "{err:?}"
    );
    assert_eq!(dict(&s), before);
}

#[test]
fn a_default_no_option_matches_is_refused() {
    let mut s = session(MULTI);
    let err = s
        .edit_field("L", &FieldEdit::new().with_default_selections(["Delta"]))
        .unwrap_err();
    assert!(
        matches!(err, EditError::ChoiceValueNotInOptions { .. }),
        "{err:?}"
    );
}

#[test]
fn an_empty_selection_removes_the_default() {
    let mut s = session(MULTI);
    s.edit_field("L", &FieldEdit::new().with_default_selections(["A"]))
        .unwrap();
    s.edit_field(
        "L",
        &FieldEdit::new().with_default_selections(Vec::<String>::new()),
    )
    .unwrap();
    assert_eq!(dict(&s).get(b"DV"), None);
}

#[test]
fn the_later_builder_wins() {
    let e = FieldEdit::new()
        .with_default_value("x")
        .with_default_selections(["A"]);
    assert_eq!(e.default_value, None);
    let e = e.with_default_value("y");
    assert_eq!(e.default_selections, None);
}
