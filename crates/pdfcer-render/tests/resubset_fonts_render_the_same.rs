//! A `/FontFile2` program cut by [`GLYPH_ZERO_RESUBSETTER`] draws the page
//! exactly as the whole program did, and the saved file is smaller.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::optimize::resubset::ResubsetFonts;
use pdfcer_core::optimize::{OptimizeSettings, OptimizedFile, RebuildOptions};
use pdfcer_core::writer::SaveOptions;
use pdfcer_render::font::resubset::GLYPH_ZERO_RESUBSETTER;
use pdfcer_render::render_page_view;

/// `.notdef A B C D E acute Eacute`; Eacute is E + acute.
const FACE: &[u8] = include_bytes!("../../../fixtures/synthetic/text/augment/face.ttf");

/// A page showing "AÉ" (WinAnsi 0x41 0xC9) in the embedded face.
fn pdf() -> Vec<u8> {
    let content = b"BT /F1 48 Tf 10 20 Td (A\\311) Tj ET";
    let mut program = format!("<< /Length {} /Length1 {0} >>\nstream\n", FACE.len()).into_bytes();
    program.extend_from_slice(FACE);
    program.extend_from_slice(b"\nendstream");
    let mut page_content = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    page_content.extend_from_slice(content);
    page_content.extend_from_slice(b"\nendstream");
    let bodies: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 140 80] /Contents 4 0 R \
           /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_vec(),
        page_content,
        b"<< /Type /Font /Subtype /TrueType /BaseFont /ABCDEF+Face \
           /Encoding /WinAnsiEncoding /FontDescriptor 6 0 R >>"
            .to_vec(),
        b"<< /Type /FontDescriptor /FontName /ABCDEF+Face /Flags 32 \
           /FontBBox [0 -200 1000 900] /ItalicAngle 0 /Ascent 800 /Descent -200 \
           /CapHeight 700 /StemV 80 /FontFile2 7 0 R >>"
            .to_vec(),
        program,
    ];
    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut at = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        at.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let n = bodies.len() + 1;
    out.extend_from_slice(format!("xref\n0 {n}\n0000000000 65535 f \n").as_bytes());
    for off in at {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

fn pixels(s: &EditSession) -> Vec<u8> {
    let page = s.pages().unwrap()[0].clone();
    render_page_view(&s.view(), &page, 2.0)
        .unwrap()
        .pixmap
        .data()
        .to_vec()
}

#[test]
fn a_cut_font_draws_the_page_unchanged_in_a_smaller_file() {
    let original = pdf();
    let mut s = EditSession::new(Document::from_bytes(original.clone()).unwrap());
    let before = pixels(&s);
    assert!(
        before
            .chunks(4)
            .filter(|p| p[..3] != [255, 255, 255])
            .count()
            > 500,
        "the fixture paints its text"
    );
    let mut settings = OptimizeSettings::new();
    settings.resubset = Some(ResubsetFonts::new(&GLYPH_ZERO_RESUBSETTER));
    let plan = s.plan_optimize(&settings).unwrap();
    let cut = plan.resubset.as_ref().expect("a re-subset plan");
    assert_eq!(cut.targets.len(), 1, "refused: {:?}", cut.refused);
    s.apply_optimize(&plan).unwrap();
    let (file, _) = s
        .save_optimized(&plan, &SaveOptions::default(), &RebuildOptions::none())
        .unwrap();
    let OptimizedFile::Smaller(bytes) = file else {
        panic!("expected a smaller file, got {file:?}");
    };
    assert!(bytes.len() < original.len());
    let reopened = EditSession::new(Document::from_bytes(bytes).unwrap());
    assert!(
        pixels(&reopened) == before,
        "the cut font draws differently"
    );
}
