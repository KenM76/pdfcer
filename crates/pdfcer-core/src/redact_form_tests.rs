//! Form XObject surgery: text inside a form under a mark is removed, in
//! the form's own coordinate system, without touching other placements.

use super::*;
use crate::text_extract::{self, ExtractOptions};
use crate::writer::SaveOptions;

/// A one-page file. `page_res` is the page's `/Resources` body, `content`
/// its content, `rect` the `/Redact` mark, and `extra` further objects
/// numbered from 6 (5 is the Helvetica font, 4 the mark).
fn pdf(page_res: &str, content: &str, rect: [f64; 4], extra: &[String]) -> Vec<u8> {
    let [x0, y0, x1, y1] = rect;
    let mut bodies = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 400] /Resources {page_res} \
             /Contents 6 0 R /Annots [4 0 R] >>"
        ),
        format!("<< /Type /Annot /Subtype /Redact /Rect [{x0} {y0} {x1} {y1}] >>"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        stream("", content),
    ];
    bodies.extend_from_slice(extra);
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let n = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {n}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    buf
}

fn stream(dict_extra: &str, content: &str) -> String {
    format!(
        "<< {dict_extra} /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

fn form(extra: &str, content: &str) -> String {
    stream(&format!("/Type /XObject /Subtype /Form {extra}"), content)
}

/// Apply, then return (extracted text, every decoded stream concatenated,
/// the report).
fn apply(bytes: Vec<u8>) -> (String, Vec<u8>, RedactionReport) {
    let doc = Document::from_bytes(bytes).unwrap();
    let (out, report) = apply_redactions(&doc, &SaveOptions::identity()).unwrap();
    let back = Document::from_bytes(out).unwrap();
    let text = text_extract::extract_document(&back, &ExtractOptions::default())
        .unwrap()
        .pages
        .iter()
        .flat_map(|p| p.runs.iter().map(|r| r.text.clone()))
        .collect::<Vec<_>>()
        .join(" ");
    let view = back.view();
    let mut all = Vec::new();
    for io in back.objects() {
        if let Object::Stream(s) = &io.value
            && let Some(raw) = view.slice(s.data_span)
            && let Ok(d) = crate::filters::decode_stream(&s.dict, raw)
        {
            all.extend_from_slice(&d);
        }
    }
    (text, all, report)
}

fn has(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

const RES: &str = "<< /Font << /F1 5 0 R >> /XObject << /Fm0 7 0 R >> >>";

/// Text inside a form placed by `/Matrix` is measured in page space and
/// removed from the file, not only from the page's rendering.
#[test]
fn text_in_a_translated_form_is_removed_from_the_file() {
    let bytes = pdf(
        RES,
        "/Fm0 Do",
        [153.0, 290.0, 206.0, 320.0],
        &[form(
            "/BBox [0 0 300 50] /Matrix [1 0 0 1 100 280] /Resources << /Font << /F1 5 0 R >> >>",
            "BT /F1 12 Tf 10 20 Td (KEEP    SECRET    TWO) Tj ET",
        )],
    );
    let (text, all, report) = apply(bytes);
    assert!(
        !has(&all, b"SECRET"),
        "SECRET survives somewhere in the file"
    );
    assert!(text.contains("KEEP") && text.contains("TWO"), "{text:?}");
    assert!(report.glyphs_removed >= 6, "{report:?}");
    assert!(
        report.notes.iter().any(|n| n.contains("inside 1 form")),
        "{:?}",
        report.notes
    );
}

/// One form painted twice; the mark covers only the second placement.
/// The first still shows its text, and the original form is kept for it.
#[test]
fn a_shared_form_is_copied_not_edited() {
    let bytes = pdf(
        RES,
        "q 1 0 0 1 0 0 cm /Fm0 Do Q q 1 0 0 1 0 200 cm /Fm0 Do Q",
        [0.0, 200.0, 600.0, 400.0],
        &[form(
            "/BBox [0 0 300 50] /Resources << /Font << /F1 5 0 R >> >>",
            "BT /F1 12 Tf 10 20 Td (SECRET) Tj ET",
        )],
    );
    let (text, _all, report) = apply(bytes);
    assert_eq!(text.matches("SECRET").count(), 1, "{text:?}");
    assert_eq!(report.glyphs_removed, 6, "{report:?}");
}

/// A form inside a form, the inner one with no `/Resources` (falls back to
/// the page's font).
#[test]
fn text_in_a_nested_form_is_removed() {
    let res = "<< /Font << /F1 5 0 R >> /XObject << /Fm0 7 0 R >> >>";
    let bytes = pdf(
        res,
        "/Fm0 Do",
        [0.0, 0.0, 600.0, 400.0],
        &[
            form(
                "/BBox [0 0 600 400] /Resources << /XObject << /In 8 0 R >> >>",
                "q 1 0 0 1 50 50 cm /In Do Q",
            ),
            form("/BBox [0 0 300 50]", "BT /F1 12 Tf 10 20 Td (SECRET) Tj ET"),
        ],
    );
    let (_text, all, report) = apply(bytes);
    assert!(
        !has(&all, b"SECRET"),
        "SECRET survives somewhere in the file"
    );
    assert_eq!(report.glyphs_removed, 6, "{report:?}");
}

/// A form that paints itself is not descended twice, and the miss is
/// disclosed rather than claimed.
#[test]
fn a_self_painting_form_is_disclosed() {
    let res = "<< /Font << /F1 5 0 R >> /XObject << /Fm0 7 0 R >> >>";
    let bytes = pdf(
        res,
        "/Fm0 Do",
        [0.0, 0.0, 600.0, 400.0],
        &[form(
            "/BBox [0 0 600 400] /Resources << /Font << /F1 5 0 R >> /XObject << /Fm0 7 0 R >> >>",
            "BT /F1 12 Tf 10 20 Td (SECRET) Tj ET /Fm0 Do",
        )],
    );
    let (_text, _all, report) = apply(bytes);
    assert_eq!(report.glyphs_removed, 6, "{report:?}");
    assert!(
        report
            .notes
            .iter()
            .any(|n| n.contains("form XObject overlaps")),
        "{:?}",
        report.notes
    );
}

/// A form whose `/BBox` misses the mark is left byte-identical.
#[test]
fn a_form_clipped_away_from_the_mark_is_untouched() {
    let bytes = pdf(
        RES,
        "/Fm0 Do BT /F1 12 Tf 400 380 Td (X) Tj ET",
        [390.0, 370.0, 420.0, 395.0],
        &[form(
            "/BBox [0 0 100 50] /Resources << /Font << /F1 5 0 R >> >>",
            "BT /F1 12 Tf 10 20 Td (SECRET) Tj ET",
        )],
    );
    let (text, _all, report) = apply(bytes);
    assert!(text.contains("SECRET"), "{text:?}");
    assert!(
        !report.notes.iter().any(|n| n.contains("form XObject")),
        "{:?}",
        report.notes
    );
}
