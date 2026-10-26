//! `q`/`Q` edge cases at the pixel level: a form's surplus `Q`s cannot pop
//! the page's saved states (§8.4.2, §8.10.1), and the pen survives `Q`
//! inside a text object (§8.4.1 Table 51, §9.4.1). Each subject must render
//! pixel-identical to a twin that needs no `q`/`Q` behaviour. The same
//! fixtures are checked for extraction and redaction in `pdfcer-core`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::render_page;

/// A one-page PDF drawing `content`, with Helvetica as `/F1`, Times as `/F2`
/// and each `(name, body)` of `forms` as a form XObject.
fn page_pdf(content: &str, forms: &[(&str, &str)]) -> Vec<u8> {
    let mut xobjects = String::new();
    let mut bodies = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        String::new(),
        stream("", content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman >>".to_string(),
    ];
    for (name, body) in forms {
        xobjects.push_str(&format!("/{name} {} 0 R ", bodies.len() + 1));
        bodies.push(stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 612 792] /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> ",
            body,
        ));
    }
    bodies[2] = format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R /F2 6 0 R >> /XObject << {xobjects}>> >> >>"
    );
    let mut pdf = b"%PDF-2.0\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = pdf.len();
    let n = bodies.len() + 1;
    pdf.extend_from_slice(format!("xref\n0 {n}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        pdf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    pdf
}

fn stream(dict: &str, data: &str) -> String {
    format!(
        "<< {dict}/Length {} >>\nstream\n{data}\nendstream",
        data.len()
    )
}

fn pixels(content: &str, forms: &[(&str, &str)]) -> Vec<u8> {
    let doc = Document::from_bytes(page_pdf(content, forms)).expect("fixture loads");
    let pages = page_tree::pages(&doc).expect("page tree walks");
    render_page(&doc, &pages[0], 0.5)
        .expect("page renders")
        .pixmap
        .data()
        .to_vec()
}

fn assert_renders_like(subject: (&str, &[(&str, &str)]), twin: (&str, &[(&str, &str)])) {
    let got = pixels(subject.0, subject.1);
    assert!(got.iter().any(|&b| b < 128), "the fixture paints ink");
    assert!(
        got == pixels(twin.0, twin.1),
        "subject renders differently from its twin"
    );
}

const STRAY_OUT: (&str, &str) = ("X", "BT /F1 12 Tf 200 100 Td (FF) Tj ET Q Q Q");
const STRAY_IN: (&str, &str) = ("X", "BT /F1 12 Tf 200 100 Td (FF) Tj Q Q Q ET");
const BALANCED: (&str, &str) = ("X", "BT /F1 12 Tf 200 100 Td (FF) Tj ET");
const PAGE: &str = "q 2 0 0 2 0 0 cm BT /F1 12 Tf 10 10 Td (AA) Tj ET q /X Do Q Q \
                    BT /F1 12 Tf 72 300 Td (SECRET) Tj ET";

#[test]
fn a_forms_surplus_q_outside_bt_cannot_pop_the_pages_state() {
    assert_renders_like((PAGE, &[STRAY_OUT]), (PAGE, &[BALANCED]));
}

#[test]
fn a_forms_surplus_q_inside_bt_cannot_pop_the_pages_state() {
    assert_renders_like((PAGE, &[STRAY_IN]), (PAGE, &[BALANCED]));
}

#[test]
fn the_pen_survives_q_inside_bt_after_a_font_change() {
    assert_renders_like(
        (
            "BT /F1 12 Tf 72 300 Td (AAAA) Tj q /F2 24 Tf (B) Tj Q (SECRET) Tj ET",
            &[],
        ),
        (
            "BT /F1 12 Tf 72 300 Td (AAAA) Tj /F2 24 Tf (B) Tj /F1 12 Tf (SECRET) Tj ET",
            &[],
        ),
    );
}

#[test]
fn a_td_made_inside_q_is_kept_after_q() {
    assert_renders_like(
        (
            "BT /F1 12 Tf 72 300 Td (AA) Tj q 100 0 Td Q (SECRET) Tj ET",
            &[],
        ),
        (
            "BT /F1 12 Tf 72 300 Td (AA) Tj 100 0 Td (SECRET) Tj ET",
            &[],
        ),
    );
}
