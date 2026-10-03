//! A decoration takes its position and thickness from the embedded font's
//! `post` / `OS/2` tables by default, and from fixed metrics when asked.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use pdfcer_core::content::ContentStream;
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::text_edit::decoration::{DecorationMetrics, DecorationSet};
use pdfcer_core::text_edit::{FormatOptions, FormatRequest};
use pdfcer_core::writer::SaveOptions;

const CONTENT: &str = "BT /F1 12 Tf 1 0 0 1 72 700 Tm (Hello) Tj ET";

/// `head` (unitsPerEm 1000), `post` (underline top −200, thickness 80) and
/// `OS/2` (strikeout top 340, size 60).
fn sfnt() -> Vec<u8> {
    let mut head = vec![0u8; 54];
    head[18..20].copy_from_slice(&1000u16.to_be_bytes());
    let mut post = vec![0u8; 32];
    post[8..10].copy_from_slice(&(-200i16).to_be_bytes());
    post[10..12].copy_from_slice(&80i16.to_be_bytes());
    let mut os2 = vec![0u8; 78];
    os2[26..28].copy_from_slice(&60i16.to_be_bytes());
    os2[28..30].copy_from_slice(&340i16.to_be_bytes());
    let tables: [(&[u8; 4], Vec<u8>); 3] = [(b"OS/2", os2), (b"head", head), (b"post", post)];
    let mut out = 0x0001_0000u32.to_be_bytes().to_vec();
    out.extend_from_slice(&3u16.to_be_bytes());
    out.extend_from_slice(&[0; 6]);
    let mut offset = 12 + 16 * tables.len();
    let mut body = Vec::new();
    for (tag, data) in &tables {
        out.extend_from_slice(*tag);
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&u32::try_from(offset).unwrap().to_be_bytes());
        out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
        offset += data.len();
        body.extend_from_slice(data);
    }
    out.extend_from_slice(&body);
    out
}

fn pdf() -> Vec<u8> {
    let widths = (0..95).map(|_| "500").collect::<Vec<_>>().join(" ");
    let program = sfnt();
    let objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
          /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_vec(),
        format!(
            "<< /Length {} >>\nstream\n{CONTENT}\nendstream",
            CONTENT.len()
        )
        .into_bytes(),
        format!(
            "<< /Type /Font /Subtype /TrueType /BaseFont /Sample \
             /Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 126 /Widths [{widths}] \
             /FontDescriptor 6 0 R >>"
        )
        .into_bytes(),
        b"<< /Type /FontDescriptor /FontName /Sample /Flags 32 \
          /FontBBox [0 -200 1000 800] /ItalicAngle 0 /Ascent 800 /Descent -200 \
          /CapHeight 700 /StemV 80 /FontFile2 7 0 R >>"
            .to_vec(),
        [
            format!("<< /Length {} >>\nstream\n", program.len()).into_bytes(),
            program,
            b"\nendstream".to_vec(),
        ]
        .concat(),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
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

/// The saved content after decorating "Hello" with `set` under `metrics`.
fn decorated(set: DecorationSet, metrics: Option<DecorationMetrics>) -> String {
    let mut s = EditSession::new(Document::from_bytes(pdf()).unwrap());
    let mut req = FormatRequest::new(0, "Hello").decoration(set);
    if let Some(m) = metrics {
        req = req.decoration_metrics(m);
    }
    s.format_text(&req, &FormatOptions::default()).unwrap();
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    let cs = ContentStream::from_page(&doc.view(), &pages[0]).unwrap();
    String::from_utf8(cs.buf).unwrap()
}

/// The `y height` operands of every rule's `re`.
fn rule_heights(text: &str) -> Vec<(f64, f64)> {
    text.match_indices("<</Rule")
        .map(|(at, _)| {
            let block = &text[at..];
            let parts: Vec<f64> = block[..block.find(" re ").unwrap()]
                .split_whitespace()
                .rev()
                .take(4)
                .map(|n| n.parse().unwrap())
                .collect();
            (parts[2], parts[0])
        })
        .collect()
}

fn close(got: &[(f64, f64)], want: &[(f64, f64)], text: &str) {
    assert_eq!(got.len(), want.len(), "{text}");
    for (g, w) in got.iter().zip(want) {
        assert!(
            (g.0 - w.0).abs() < 1e-6 && (g.1 - w.1).abs() < 1e-6,
            "{got:?} vs {want:?}\n{text}"
        );
    }
}

#[test]
fn the_default_uses_the_fonts_own_tables() {
    let text = decorated(DecorationSet::UNDERLINE, None);
    // Centre −240, thickness 80, at 12 pt: bottom −3.36, height 0.96.
    close(&rule_heights(&text), &[(-3.36, 0.96)], &text);
    assert!(!text.contains("/M /Standard"), "{text}");
    let text = decorated(DecorationSet::STRIKETHROUGH, None);
    // Centre 310, thickness 60: bottom 3.36, height 0.72.
    close(&rule_heights(&text), &[(3.36, 0.72)], &text);
}

#[test]
fn standard_metrics_ignore_the_tables_and_are_recorded() {
    let text = decorated(DecorationSet::UNDERLINE, Some(DecorationMetrics::Standard));
    // AFM: centre −100, thickness 50 → bottom −1.5, height 0.6.
    close(&rule_heights(&text), &[(-1.5, 0.6)], &text);
    assert!(text.contains("/M /Standard"), "{text}");
}
