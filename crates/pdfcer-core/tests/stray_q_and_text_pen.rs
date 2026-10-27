//! `q`/`Q` edge cases in text extraction and redaction.
//!
//! - A form with surplus `Q`s cannot pop the page's saved states: each
//!   content stream balances its own `q`/`Q` (§8.4.2, §8.10.1).
//! - Inside a text object, `Q` restores Tm and Tlm saved by its `q`
//!   (ISO 32000-2:2020 errata to §9.4.1), as Acrobat does: a `Td` made
//!   inside `q`/`Q` is undone.
//!
//! Each case is compared with a twin page that needs no `q`/`Q` behaviour,
//! and a search-and-redact of `SECRET` must remove exactly `SECRET`.

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree;
use pdfcer_core::text_extract::{self, ExtractOptions};
use pdfcer_core::vector::{Matrix, VectorObject, decompose_page};
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

/// Every glyph origin on the page in content order, regardless of how the
/// glyphs are grouped into runs.
fn all_glyphs(pdf: Vec<u8>) -> (String, Vec<(i64, i64)>) {
    let mut text = String::new();
    let mut pos = Vec::new();
    for (t, p) in glyphs(pdf) {
        text.push_str(&t);
        pos.extend(p);
    }
    (text, pos)
}

fn squash(t: &str) -> String {
    t.chars()
        .filter(|c| !c.is_whitespace() && *c != '|')
        .collect()
}

/// `subject` places its glyphs exactly where `twin` does, and redacting
/// `SECRET` from each leaves the same text. Returns `(before, after)`.
fn assert_same_redaction(
    subject: (&str, &[(&str, &str)]),
    twin: (&str, &[(&str, &str)]),
) -> (String, String) {
    let pdf = || page_pdf(subject.0, subject.1);
    assert_eq!(all_glyphs(pdf()), all_glyphs(page_pdf(twin.0, twin.1)));
    let before = page_text(pdf());
    assert!(before.contains("SECRET"), "{before}");
    let redacted = redact_secret(pdf());
    let after = squash(&page_text(redacted.clone()));
    let twin_after = squash(&page_text(redact_secret(page_pdf(twin.0, twin.1))));
    assert_eq!(after, twin_after, "{}", String::from_utf8_lossy(&redacted));
    (squash(&before), after)
}

/// As [`assert_same_redaction`], and the redaction removes exactly `SECRET`.
fn assert_same_and_redacts(subject: (&str, &[(&str, &str)]), twin: (&str, &[(&str, &str)])) {
    let (before, after) = assert_same_redaction(subject, twin);
    assert_eq!(after, before.replacen("SECRET", "", 1));
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

/// `SECRET` starts where `B` started, the pen before `q`: four Helvetica
/// `A`s (667/1000 em each) at 12 pt put it at x = 72 + 32.016. `SECRET`
/// is then drawn over the 24 pt `B`, so the redaction removes both.
#[test]
fn q_inside_bt_restores_the_pen_after_a_font_change() {
    let (_, after) = assert_same_redaction(
        (
            "BT /F1 12 Tf 72 300 Td (AAAA) Tj q /F2 24 Tf (B) Tj Q (SECRET) Tj ET",
            &[],
        ),
        (
            "BT /F1 12 Tf 72 300 Td (AAAA) Tj /F2 24 Tf (B) Tj \
             /F1 12 Tf 1 0 0 1 104.016 300 Tm (SECRET) Tj ET",
            &[],
        ),
    );
    assert_eq!(after, "AAAA");
}

#[test]
fn a_td_made_inside_q_is_undone_by_q() {
    assert_same_and_redacts(
        (
            "BT /F1 12 Tf 72 300 Td (AA) Tj q 100 0 Td Q (SECRET) Tj ET",
            &[],
        ),
        ("BT /F1 12 Tf 72 300 Td (AA) Tj (SECRET) Tj ET", &[]),
    );
}

/// A pen saved in one text object is not restored into the next: the
/// mis-nested `q BT … ET BT … Q … ET` keeps the second object's own pen.
#[test]
fn a_q_made_outside_bt_does_not_restore_a_pen() {
    assert_same_and_redacts(
        (
            "/F1 12 Tf q BT 72 300 Td (AA) Tj ET BT 72 200 Td (BB) Tj Q (SECRET) Tj ET",
            &[],
        ),
        (
            "/F1 12 Tf BT 72 300 Td (AA) Tj ET BT 72 200 Td (BB) Tj (SECRET) Tj ET",
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

/// Each text object's page bounds, as object selection sees them.
fn text_bounds(pdf: Vec<u8>) -> Vec<String> {
    let doc = Document::from_bytes(pdf).expect("fixture loads");
    let pages = page_tree::pages(&doc).expect("page tree walks");
    decompose_page(&doc.view(), &pages[0], Matrix::IDENTITY)
        .expect("decomposes")
        .objects
        .iter()
        .filter_map(|o| match o {
            VectorObject::Text(t) => Some(format!("{:?}", t.page_bbox)),
            _ => None,
        })
        .collect()
}

const TD_IN_Q: &str = "BT /F1 12 Tf 72 300 Td (AA) Tj q 100 0 Td Q (SECRET) Tj ET";
const TD_TWIN: &str = "BT /F1 12 Tf 72 300 Td (AA) Tj (SECRET) Tj ET";

#[test]
fn object_selection_bounds_follow_the_restored_pen() {
    assert_eq!(
        text_bounds(page_pdf(TD_IN_Q, &[])),
        text_bounds(page_pdf(TD_TWIN, &[]))
    );
}
