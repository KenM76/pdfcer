//! `ExtractOptions::word_gap_auto` (G209) against a synthetic page whose
//! word gaps are positioned (TJ), narrower than the fixed 0.20 em ratio.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use pdfcer_model::document::Document;
use pdfcer_text::text_extract::{ExtractOptions, extract_document};

fn pdf(content: &str) -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
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

/// Two lines of four-letter words separated by a 0.15 em TJ kern and no
/// space glyph: tight justification the fixed ratio cannot see.
const TIGHT: &str = "BT /F1 12 Tf 72 700 Td [(alfa)-150(brav)-150(char)-150(delt)-150(echo)] TJ \
0 -16 Td [(foxt)-150(golf)-150(hote)-150(indi)-150(juli)] TJ ET";

fn words(options: &ExtractOptions, content: &str) -> (String, f32, bool) {
    let doc = Document::from_bytes(pdf(content)).unwrap();
    let text = extract_document(&doc, options).unwrap();
    let page = &text.pages[0];
    (
        page.plain_text(),
        page.word_gap_ratio,
        page.word_gap_measured,
    )
}

#[test]
fn auto_splits_words_the_fixed_ratio_merges() {
    let (fixed, ratio, measured) = words(&ExtractOptions::default(), TIGHT);
    assert!(fixed.contains("alfabrav"), "{fixed:?}");
    assert!((ratio - 0.20).abs() < 1e-6 && !measured);

    let auto = ExtractOptions::default().with_word_gap_auto(true);
    let (text, ratio, measured) = words(&auto, TIGHT);
    assert!(measured, "{ratio}");
    assert!(ratio > 0.0 && ratio < 0.15, "{ratio}");
    assert!(text.contains("alfa brav char delt echo"), "{text:?}");
}

#[test]
fn a_page_without_a_valley_keeps_the_fixed_ratio() {
    let auto = ExtractOptions::default().with_word_gap_auto(true);
    let (text, ratio, measured) = words(&auto, "BT /F1 12 Tf 72 700 Td (one line) Tj ET");
    assert!(!measured);
    assert!((ratio - 0.20).abs() < 1e-6);
    assert!(text.contains("one line"), "{text:?}");
}
