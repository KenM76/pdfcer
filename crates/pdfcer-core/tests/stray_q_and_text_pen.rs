//! `q`/`Q` edge cases in text extraction and redaction.
//!
//! - A form with surplus `Q`s cannot pop the page's saved states: each
//!   content stream balances its own `q`/`Q` (§8.4.2, §8.10.1).
//! - The pen position survives `Q` inside a text object: Tm and Tlm are not
//!   graphics state (§8.4.1 Table 51, §9.4.1), and a `Td` made inside
//!   `q`/`Q` is kept.
//!
//! Each case is compared with a twin page that needs no `q`/`Q` behaviour,
//! and a search-and-redact of `SECRET` must remove exactly `SECRET`.

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree;
use pdfcer_core::text_extract::{self, ExtractOptions};
use pdfcer_core::writer::SaveOptions;

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
            "/Type /XObject /Subtype /Form /BBox [0 0 612 792] \
             /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> ",
            body,
        ));
    }
    bodies[2] = format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R /F2 6 0 R >> /XObject << {xobjects}>> >> >>"
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

/// Every run's text with its glyph origins, rounded to 0.01 pt.
fn glyphs(pdf: Vec<u8>) -> Vec<(String, Vec<(i64, i64)>)> {
    let doc = Document::from_bytes(pdf).expect("fixture loads");
    let pages = page_tree::pages(&doc).expect("page tree walks");
    let text = text_extract::extract_page(&doc, &pages[0], 0, &ExtractOptions::default())
        .expect("page extracts");
    text.runs
        .iter()
        .map(|r| {
            let pos = r
                .glyphs
                .iter()
                .map(|g| ((g.x * 100.0).round() as i64, (g.y * 100.0).round() as i64))
                .collect();
            (r.text.clone(), pos)
        })
        .collect()
}

fn page_text(pdf: Vec<u8>) -> String {
    glyphs(pdf)
        .into_iter()
        .map(|(t, _)| t)
        .collect::<Vec<_>>()
        .join("|")
}

/// `pdf` with every occurrence of `SECRET` redacted.
fn redact_secret(pdf: Vec<u8>) -> Vec<u8> {
    let mut session = EditSession::new(Document::from_bytes(pdf).expect("fixture loads"));
    let marks = session
        .mark_redactions_by_search("SECRET", false)
        .expect("search marks");
    assert!(!marks.is_empty(), "SECRET is found");
    session.apply_redactions().expect("redaction applies");
    session
        .to_full_bytes(&SaveOptions::default())
        .expect("saves")
        .0
}

/// `subject` places its glyphs exactly where `twin` does, and redacting
/// `SECRET` from it removes exactly `SECRET`.
fn assert_same_and_redacts(subject: (&str, &[(&str, &str)]), twin: (&str, &[(&str, &str)])) {
    let pdf = || page_pdf(subject.0, subject.1);
    assert_eq!(glyphs(pdf()), glyphs(page_pdf(twin.0, twin.1)));
    let before = page_text(pdf());
    assert!(before.contains("SECRET"), "{before}");
    let redacted = redact_secret(pdf());
    let after = page_text(redacted.clone());
    let squash = |t: &str| {
        t.chars()
            .filter(|c| !c.is_whitespace() && *c != '|')
            .collect::<String>()
    };
    assert_eq!(
        squash(&after),
        squash(&before.replacen("SECRET", "", 1)),
        "{}",
        String::from_utf8_lossy(&redacted)
    );
}

const STRAY_OUT: (&str, &str) = ("X", "BT /F1 12 Tf 200 100 Td (FF) Tj ET Q Q Q");
const STRAY_IN: (&str, &str) = ("X", "BT /F1 12 Tf 200 100 Td (FF) Tj Q Q Q ET");
const BALANCED: (&str, &str) = ("X", "BT /F1 12 Tf 200 100 Td (FF) Tj ET");

/// The form runs under a scaled CTM saved by the page; with the bug, its
/// surplus `Q`s consume that frame and the page's own `Q` restores nothing,
/// so `SECRET` lands at twice its coordinates.
const PAGE: &str = "q 2 0 0 2 0 0 cm BT /F1 12 Tf 10 10 Td (AA) Tj ET q /X Do Q Q \
                    BT /F1 12 Tf 72 300 Td (SECRET) Tj ET";

#[test]
fn a_forms_surplus_q_outside_bt_cannot_pop_the_pages_state() {
    assert_same_and_redacts((PAGE, &[STRAY_OUT]), (PAGE, &[BALANCED]));
}

#[test]
fn a_forms_surplus_q_inside_bt_cannot_pop_the_pages_state() {
    assert_same_and_redacts((PAGE, &[STRAY_IN]), (PAGE, &[BALANCED]));
}

#[test]
fn the_pen_survives_q_inside_bt_after_a_font_change() {
    assert_same_and_redacts(
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
    assert_same_and_redacts(
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

/// A match's neighbours touch its quad; redacting it must keep them.
#[test]
fn a_search_redaction_mid_word_keeps_its_neighbours() {
    let pdf = page_pdf("BT /F1 12 Tf 72 300 Td (XXSECRETYY) Tj ET", &[]);
    let after = page_text(redact_secret(pdf));
    let squashed: String = after
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '|')
        .collect();
    assert_eq!(squashed, "XXYY");
}
