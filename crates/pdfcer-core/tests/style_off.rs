//! `G086` / `G087` — taking bold or italic OFF through the style ladder, and
//! reading a synthetic style back from extraction provenance.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::text_edit::synth::{detect, detect_at, shear_into, shear_of};
use pdfcer_core::text_edit::{
    FormatError, FormatOptions, FormatRequest, StyleRung, StyleSynthesis, StyleTarget,
};
use pdfcer_core::text_extract::{ExtractOptions, GlyphProvenance, extract_page};
use pdfcer_core::writer::SaveOptions;

fn pdf(objects: &[(u32, String)]) -> Vec<u8> {
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0usize; objects.len() + 1];
    for (n, body) in objects {
        offsets[*n as usize] = out.len();
        out.extend_from_slice(format!("{n} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for off in &offsets[1..] {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn simple_font(base_font: &str) -> String {
    let widths = (0..95).map(|_| "500").collect::<Vec<_>>().join(" ");
    format!(
        "<< /Type /Font /Subtype /Type1 /BaseFont /{base_font} /Encoding /WinAnsiEncoding \
         /FirstChar 32 /LastChar 126 /Widths [{widths}] >>"
    )
}

/// One page showing `content`, with `/F1`, `/F2`, … set to `fonts` and
/// `extra` spliced into the resource dictionary.
fn page(fonts: &[&str], content: &str, extra: &str) -> Vec<u8> {
    let font_refs = (0..fonts.len())
        .map(|i| format!("/F{} {} 0 R", i + 1, 5 + i))
        .collect::<Vec<_>>()
        .join(" ");
    let mut objects = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()),
        (
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
                 /Resources << /Font << {font_refs} >> {extra} >> >>"
            ),
        ),
        (
            4,
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            ),
        ),
    ];
    for (i, base) in fonts.iter().enumerate() {
        objects.push((5 + i as u32, simple_font(base)));
    }
    pdf(&objects)
}

const PLAIN: &str = "BT /F1 12 Tf 72 700 Td (hello) Tj ET";

fn session(bytes: Vec<u8>) -> EditSession {
    EditSession::new(Document::from_bytes(bytes).unwrap())
}

fn style(bold: Option<bool>, italic: Option<bool>) -> FormatRequest {
    FormatRequest::new(0, "hello").style(StyleTarget::new(bold, italic))
}

fn saved(s: &EditSession) -> Vec<u8> {
    s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0
}

/// The provenance of the first glyph of "hello" on page 0 of `bytes`.
fn first_glyph(bytes: Vec<u8>) -> GlyphProvenance {
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    let opts = ExtractOptions::default().with_provenance(true);
    let page = extract_page(&doc, &pages[0], 0, &opts).unwrap();
    let run = page.runs.iter().find(|r| r.text.contains('h')).unwrap();
    run.glyphs[0].provenance.clone().unwrap()
}

#[test]
fn bold_off_on_helvetica_bold_binds_helvetica_at_rung_2() {
    let mut s = session(page(&["Helvetica-Bold"], PLAIN, ""));
    let r = s
        .format_text(&style(Some(false), None), &FormatOptions::default())
        .unwrap();
    let l = r.style_ladder.as_ref().unwrap();
    assert_eq!(l.rung, StyleRung::StandardFourteenSibling);
    assert_eq!(l.bound.as_deref(), Some("Helvetica"));
    assert_eq!(l.removed, StyleSynthesis::Bold);
    assert!(l.unsynthesised.is_none());
    assert!(
        r.disclosures
            .iter()
            .any(|d| d.starts_with("style: bold off")),
        "{:?}",
        r.disclosures
    );
    let out = String::from_utf8_lossy(&saved(&s)).into_owned();
    assert!(
        out.matches("/BaseFont /Helvetica").count()
            > out.matches("/BaseFont /Helvetica-Bold").count(),
        "a plain Helvetica resource was added"
    );
}

#[test]
fn bold_off_prefers_a_plain_face_already_on_the_page() {
    let mut s = session(page(&["Helvetica-Bold", "Helvetica"], PLAIN, ""));
    let r = s
        .format_text(&style(Some(false), None), &FormatOptions::default())
        .unwrap();
    let l = r.style_ladder.as_ref().unwrap();
    assert_eq!(l.rung, StyleRung::RealFaceOnPage);
    assert_eq!(l.bound.as_deref(), Some("Helvetica"));
}

#[test]
fn bold_off_keeps_the_italic_axis_the_face_carries() {
    let mut s = session(page(&["Helvetica-BoldOblique"], PLAIN, ""));
    let r = s
        .format_text(&style(Some(false), None), &FormatOptions::default())
        .unwrap();
    let l = r.style_ladder.as_ref().unwrap();
    assert_eq!(l.bound.as_deref(), Some("Helvetica-Oblique"));
}

#[test]
fn bold_off_with_no_plain_face_of_the_family_is_refused() {
    let mut s = session(page(&["Verdana-Bold"], PLAIN, ""));
    let err = s
        .format_text(&style(Some(false), None), &FormatOptions::default())
        .unwrap_err();
    assert!(
        matches!(err, FormatError::NoFaceWithoutStyle { style: "bold", .. }),
        "{err:?}"
    );
    assert_eq!(s.undo_depth(), 0);
}

#[test]
fn italic_off_removes_a_synthetic_shear_from_the_text_matrix() {
    let content = "BT /F1 12 Tf 1 0 0.212557 1 72 700 Tm (hello) Tj ET";
    let mut s = session(page(&["Helvetica"], content, ""));
    assert!(shear_of(first_glyph(saved(&s)).text_matrix.map(f64::from)) > 0.2);
    let r = s
        .format_text(&style(None, Some(false)), &FormatOptions::default())
        .unwrap();
    let l = r.style_ladder.as_ref().unwrap();
    assert_eq!(l.rung, StyleRung::SynthesisRemoved);
    assert_eq!(l.unsynthesised, StyleSynthesis::Italic);
    assert!(l.bound.is_none(), "the face is kept");
    let tm = first_glyph(saved(&s)).text_matrix.map(f64::from);
    assert!(shear_of(tm).abs() < 1e-4, "{tm:?}");
}

#[test]
fn italic_on_then_off_round_trips_to_an_unsheared_run() {
    let mut s = session(page(&["Verdana"], PLAIN, ""));
    let on = s
        .format_text(&style(None, Some(true)), &FormatOptions::default())
        .unwrap();
    assert_eq!(on.style_ladder.unwrap().rung, StyleRung::Synthetic);
    assert!(shear_of(first_glyph(saved(&s)).text_matrix.map(f64::from)) > 0.2);
    s.format_text(&style(None, Some(false)), &FormatOptions::default())
        .unwrap();
    let tm = first_glyph(saved(&s)).text_matrix.map(f64::from);
    assert!(shear_of(tm).abs() < 1e-4, "{tm:?}");
}

#[test]
fn bold_off_removes_a_synthetic_stroke() {
    let content = "BT /F1 12 Tf 2 Tr 0.264 w 72 700 Td (hello) Tj ET";
    let mut s = session(page(&["Verdana"], content, ""));
    let r = s
        .format_text(&style(Some(false), None), &FormatOptions::default())
        .unwrap();
    let l = r.style_ladder.as_ref().unwrap();
    assert_eq!(l.rung, StyleRung::SynthesisRemoved);
    assert_eq!(l.unsynthesised, StyleSynthesis::Bold);
    assert_eq!(first_glyph(saved(&s)).render_mode(), 0);
}

#[test]
fn a_rung_4_bold_reads_back_as_synthetic_bold() {
    let mut s = session(page(&["Verdana"], PLAIN, ""));
    let r = s
        .format_text(&style(Some(true), None), &FormatOptions::default())
        .unwrap();
    assert_eq!(r.style_ladder.unwrap().rung, StyleRung::Synthetic);
    let prov = first_glyph(saved(&s));
    assert_eq!(detect_at("Verdana", &prov), StyleSynthesis::Bold);
}

#[test]
fn an_outlined_hairline_run_is_not_synthetic_bold() {
    let content = "BT /F1 12 Tf 2 Tr 0.1 w 72 700 Td (hello) Tj ET";
    let prov = first_glyph(page(&["Verdana"], content, ""));
    assert_eq!(prov.render_mode(), 2);
    assert!((prov.line_width - 0.1).abs() < 1e-6);
    assert_eq!(detect_at("Verdana", &prov), StyleSynthesis::None);
}

#[test]
fn a_line_width_set_through_ext_gstate_is_read() {
    let content = "/GS1 gs BT /F1 12 Tf 2 Tr 72 700 Td (hello) Tj ET";
    let prov = first_glyph(page(
        &["Verdana"],
        content,
        "/ExtGState << /GS1 << /Type /ExtGState /LW 0.264 >> >>",
    ));
    assert!((prov.line_width - 0.264).abs() < 1e-6);
    assert_eq!(detect_at("Verdana", &prov), StyleSynthesis::Bold);
}

#[test]
fn a_shear_on_a_rotated_matrix_is_detected() {
    let tm = shear_into([0.0, 1.0, -1.0, 0.0, 72.0, 700.0]);
    assert!(shear_of(tm) > 0.2);
    assert_eq!(detect("Verdana", 0, 1.0, 12.0, tm), StyleSynthesis::Italic);
}
