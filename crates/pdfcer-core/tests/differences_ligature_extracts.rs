//! A simple font with no `/ToUnicode` whose `/Differences` maps a code to
//! the glyph name `/fi` extracts that code as U+FB01 (§9.10.2 step b: the
//! glyph name is looked up in the Adobe Glyph List, which names U+FB01).
//!
//! U+FB01 is the chosen output because it is lossless: the file names one
//! glyph, and expanding it to `f`+`i` is a normalisation a consumer can apply
//! (NFKC) but cannot undo. Changing this is a deliberate decision.

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_core::text_extract::{self, ExtractOptions};

fn fixture() -> Vec<u8> {
    let content = "BT /F1 12 Tf 72 300 Td <41804242> Tj ET";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_string(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
         /Encoding << /Type /Encoding /BaseEncoding /WinAnsiEncoding \
         /Differences [128 /fi] >> >>"
            .to_string(),
    ];
    let mut pdf = b"%PDF-1.7\n".to_vec();
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

#[test]
fn a_differences_fi_code_extracts_as_the_ligature_code_point() {
    let doc = Document::from_bytes(fixture()).expect("fixture loads");
    let pages = page_tree::pages(&doc).expect("page tree walks");
    let text = text_extract::extract_page(&doc, &pages[0], 0, &ExtractOptions::default())
        .expect("page extracts");
    let all: String = text.runs.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(all.trim(), "A\u{FB01}BB");
}
