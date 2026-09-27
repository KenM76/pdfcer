//! A field appearance redrawn as a side effect of another change reports what
//! its layout decided (rule 4), on every verb that redraws: `edit_field`,
//! `edit_widget`, `rotate_widget` and `reset_form`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, FieldEdit, LayoutDisclosure, WidgetEdit};
use pdfcer_core::forms;

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

/// One text field "t" holding (and defaulting to) "Ωx", whose Ω has no
/// `WinAnsi` code, drawn with `/DA` `da`.
fn session(da: &str) -> EditSession {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] \
         /DR << /Font << /Helv 5 0 R >> >> >> >>"
            .to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Annots [4 0 R] >>".to_owned(),
        format!(
            "<< /FT /Tx /T (t) /Type /Annot /Subtype /Widget /P 3 0 R \
             /Rect [20 50 200 72] /V <FEFF03A90078> /DV <FEFF03A90078> /DA ({da}) >>"
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
         /Encoding /WinAnsiEncoding >>"
            .to_owned(),
    ];
    EditSession::new(Document::from_bytes(assemble(&bodies)).expect("fixture parses"))
}

/// An auto-sized `/DA`: the redraw both auto-sizes and meets the Ω.
const AUTO: &str = "/Helv 0 Tf 0 g";

fn assert_auto_and_unencodable(l: &LayoutDisclosure, verb: &str) {
    assert!(
        l.applied_autosize.is_some(),
        "{verb}: auto-size not reported"
    );
    assert_eq!(l.unencodable_chars, 1, "{verb}: the Ω was not reported");
}

#[test]
fn edit_field_reports_its_redraw() {
    let mut s = session(AUTO);
    let out = s
        .edit_field("t", &FieldEdit::new().with_quadding(1))
        .unwrap();
    assert!(out.appearance_regenerated);
    assert_auto_and_unencodable(&out.layout, "edit_field");
}

#[test]
fn edit_widget_reports_its_redraw() {
    let mut s = session(AUTO);
    let out = s
        .edit_widget(
            "t",
            0,
            &WidgetEdit::new().with_background(forms::MkColor::Gray(0.5)),
        )
        .unwrap();
    assert!(out.appearance_regenerated);
    assert_auto_and_unencodable(&out.layout, "edit_widget");
}

#[test]
fn rotate_widget_reports_its_redraw() {
    let mut s = session(AUTO);
    let out = s.rotate_widget("t", 0, 90).unwrap();
    assert!(out.appearance_regenerated);
    assert_auto_and_unencodable(&out.layout, "rotate_widget");
}

#[test]
fn reset_form_reports_its_redraw() {
    let mut s = session(AUTO);
    let out = s.reset_form(None).unwrap();
    assert_eq!(out.fields_reset, 1);
    assert_auto_and_unencodable(&out.layout, "reset_form");
}

/// The colour narrowing is reported for a FIXED-size `/DA` too: it was once
/// recorded only alongside an auto-size.
#[test]
fn an_unmodelled_colour_is_reported_without_an_auto_size() {
    let mut s = session("/Helv 12 Tf /CS0 cs 1 scn");
    let out = s
        .edit_field("t", &FieldEdit::new().with_quadding(2))
        .unwrap();
    assert!(out.appearance_regenerated);
    assert_eq!(out.layout.applied_autosize, None);
    assert!(
        out.layout.da_colour_unmodelled,
        "a black stand-in for the file's colour went unreported"
    );
}
