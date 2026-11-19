//! `ExtractedGlyph::width_guessed`: set for a `/MissingWidth` code and for
//! a non-standard font with no `/Widths`; clear for stated and AFM widths.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use pdfcer_model::document::Document;
use pdfcer_text::text_extract::{ExtractOptions, extract_document};

fn pdf(content: &str) -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R /F2 7 0 R /F3 8 0 R >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Stated-Face /FirstChar 65 \
         /LastChar 65 /Widths [600] /FontDescriptor 6 0 R >>"
            .to_owned(),
        "<< /Type /FontDescriptor /FontName /Stated-Face /Flags 32 \
         /FontBBox [0 0 1000 1000] /ItalicAngle 0 /Ascent 800 /Descent -200 \
         /CapHeight 700 /StemV 80 /MissingWidth 300 >>"
            .to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Unlisted-Face >>".to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".to_owned(),
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

fn guessed(content: &str) -> Vec<bool> {
    let doc = Document::from_bytes(pdf(content)).unwrap();
    let text = extract_document(&doc, &ExtractOptions::default()).unwrap();
    text.pages[0]
        .runs
        .iter()
        .flat_map(|r| r.glyphs.iter().map(|g| g.width_guessed))
        .collect()
}

#[test]
fn a_stated_width_is_measured_and_missing_width_is_a_guess() {
    // A is in /Widths; B falls to /MissingWidth.
    assert_eq!(guessed("BT /F1 10 Tf 72 700 Td (AB) Tj ET"), [false, true]);
}

#[test]
fn a_font_with_no_widths_is_guessed_and_a_standard_face_is_not() {
    assert_eq!(guessed("BT /F2 10 Tf 72 700 Td (AB) Tj ET"), [true, true]);
    assert_eq!(guessed("BT /F3 10 Tf 72 700 Td (AB) Tj ET"), [false, false]);
}
