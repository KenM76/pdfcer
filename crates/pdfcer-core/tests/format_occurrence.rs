//! `G084` — `FormatRequest::occurrence` restyles the n-th match of `find`
//! inside the pinned operator, not the first.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::text_edit::{FormatError, FormatOptions, FormatRequest, StyleTarget};
use pdfcer_core::text_extract::{ExtractOptions, extract_page};
use pdfcer_core::writer::SaveOptions;

const CONTENT: &str = "BT /F1 12 Tf 72 700 Td (the cat and the dog) Tj ET";

fn pdf() -> Vec<u8> {
    let widths = (0..95).map(|_| "500").collect::<Vec<_>>().join(" ");
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{CONTENT}\nendstream",
            CONTENT.len()
        ),
        format!(
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
             /Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 126 /Widths [{widths}] >>"
        ),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for off in offsets {
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

/// Every glyph on page 0 as `(character, font resource name)`, in show order.
fn glyph_fonts(bytes: Vec<u8>) -> Vec<(char, Vec<u8>)> {
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    let opts = ExtractOptions::default().with_provenance(true);
    let page = extract_page(&doc, &pages[0], 0, &opts).unwrap();
    let mut out = Vec::new();
    for run in &page.runs {
        for g in &run.glyphs {
            let start = g.text_start as usize;
            let ch = run.text[start..].chars().next().unwrap();
            let p = g.provenance.as_ref().unwrap();
            out.push((ch, p.font_resource.clone().unwrap()));
        }
    }
    out.retain(|(c, _)| *c != ' ');
    out
}

fn bold_the(occurrence: usize) -> Result<Vec<u8>, FormatError> {
    let mut s = EditSession::new(Document::from_bytes(pdf()).unwrap());
    let req = FormatRequest::new(0, "the")
        .occurrence(occurrence)
        .style(StyleTarget::new(Some(true), None));
    s.format_text(&req, &FormatOptions::default())?;
    Ok(s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0)
}

#[test]
fn occurrence_one_restyles_the_second_match_only() {
    let glyphs = glyph_fonts(bold_the(1).unwrap());
    let text: String = glyphs.iter().map(|(c, _)| c).collect();
    assert_eq!(text, "thecatandthedog");
    let original = &glyphs[0].1;
    assert_eq!(original.as_slice(), b"F1");
    for (c, font) in &glyphs[..9] {
        assert_eq!(font, original, "{c} before the second `the` kept /F1");
    }
    for (c, font) in &glyphs[9..12] {
        assert_ne!(font, original, "{c} of the second `the` was restyled");
    }
    for (c, font) in &glyphs[12..] {
        assert_eq!(font, original, "{c} after it kept /F1");
    }
}

#[test]
fn occurrence_zero_is_the_first_match() {
    let glyphs = glyph_fonts(bold_the(0).unwrap());
    assert!(glyphs[..3].iter().all(|(_, f)| f.as_slice() != b"F1"));
    assert!(glyphs[3..].iter().all(|(_, f)| f.as_slice() == b"F1"));
}

#[test]
fn an_occurrence_past_the_last_match_is_no_match() {
    let err = bold_the(2).unwrap_err();
    assert!(
        matches!(&err, FormatError::NoMatch(f) if f == "the"),
        "{err:?}"
    );
    assert!(bold_the(1).is_ok());
}
