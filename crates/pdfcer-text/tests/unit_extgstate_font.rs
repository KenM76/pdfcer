//! An ExtGState `/Font [font size]` selects a font as `Tf` does (ISO 32000-2
//! §8.4.5, X097).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use pdfcer_model::document::Document;
use pdfcer_text::text_extract::{ExtractOptions, extract_document};

/// `/GS1` selects Helvetica 7 0 R at 12; `/Bad` holds a malformed `/Font`;
/// `/F1` is Courier for `Tf`.
fn pdf(content: &str) -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R >> /ExtGState << /GS1 6 0 R \
         /Bad << /Font [/F1 12] >> >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".to_owned(),
        "<< /Type /ExtGState /Font [7 0 R 12] >>".to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

/// Each glyph-bearing run's text, first glyph x and total advance.
fn runs(content: &str) -> Vec<(String, f32, f32)> {
    let doc = Document::from_bytes(pdf(content)).unwrap();
    let text = extract_document(&doc, &ExtractOptions::default()).unwrap();
    text.pages[0]
        .runs
        .iter()
        .filter(|r| !r.glyphs.is_empty())
        .map(|r| {
            let x = r.glyphs.first().map_or(f32::NAN, |g| g.x);
            (r.text.clone(), x, r.glyphs.iter().map(|g| g.advance).sum())
        })
        .collect()
}

/// Helvetica "Cell" at 12: (722 + 556 + 222 + 222) / 1000 * 12.
const HELVETICA_CELL: f32 = 20.664;
/// Courier at 12: 600 / 1000 * 12 per glyph.
const COURIER_CELL: f32 = 28.8;

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn a_gs_font_selects_the_font_and_size() {
    let r = runs("/GS1 gs BT 100 700 Td (Cell) Tj ET");
    assert_eq!(r.len(), 1, "{r:?}");
    assert_eq!(r[0].0, "Cell");
    assert!(near(r[0].1, 100.0), "{r:?}");
    assert!(near(r[0].2, HELVETICA_CELL), "{r:?}");
}

#[test]
fn a_later_tf_overrides_and_q_restores_the_gs_font() {
    let r = runs("/GS1 gs BT 100 700 Td q /F1 12 Tf (Cell) Tj Q 0 -20 Td (Cell) Tj ET");
    assert_eq!(r.len(), 2, "{r:?}");
    assert!(near(r[0].2, COURIER_CELL), "{r:?}");
    assert!(near(r[1].2, HELVETICA_CELL), "{r:?}");
}

#[test]
fn a_malformed_gs_font_changes_nothing() {
    let r = runs("BT /F1 12 Tf /Bad gs 100 700 Td (Cell) Tj ET");
    assert_eq!(r.len(), 1, "{r:?}");
    assert!(near(r[0].2, COURIER_CELL), "{r:?}");
}
