//! `pdfcer_text::block_layout` (X095): a heading-sized top-of-page line
//! repeated with only case or spacing changed is running text; one whose
//! digits change is a heading sequence.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_model::document::Document;
use pdfcer_text::block_layout::{BlockKind, DocumentLayout, LayoutOptions, analyze_layout};
use pdfcer_text::text_extract::ExtractOptions;

/// A classic-xref PDF whose object `i + 1` is `bodies[i]`; a body starting
/// with `STREAM:` becomes a stream with that payload.
fn build_pdf(bodies: &[String]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        let body = match body.strip_prefix("STREAM:") {
            Some(data) => format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len()),
            None => body.clone(),
        };
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

/// One US Letter page per entry of `tops`: that text at 18 pt near the
/// top edge, then five 12 pt body lines.
fn pages_with_tops(tops: &[&str]) -> Vec<u8> {
    let n = tops.len();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
    let mut bodies = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
    ];
    for (i, top) in tops.iter().enumerate() {
        let mut content = format!("BT /F1 18 Tf 72 750 Td ({top}) Tj ET\n");
        for line in 0..5 {
            let y = 650 - 20 * line;
            content.push_str(&format!(
                "BT /F1 12 Tf 72 {y} Td (Body text line {line} on page {i}.) Tj ET\n"
            ));
        }
        bodies.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {} 0 R \
             /Resources << /Font << /F1 3 0 R >> >> >>",
            5 + 2 * i
        ));
        bodies.push(format!("STREAM:{content}"));
    }
    build_pdf(&bodies)
}

fn lay_out(tops: &[&str]) -> DocumentLayout {
    let doc = Document::from_bytes(pages_with_tops(tops)).unwrap();
    let options = LayoutOptions::default().with_running_min_fraction(0.5);
    analyze_layout(&doc.view(), &ExtractOptions::default(), &options).unwrap()
}

/// The kind of the block whose text starts with `prefix`, page by page.
fn kinds_of(layout: &DocumentLayout, prefix: &str) -> Vec<BlockKind> {
    layout
        .pages
        .iter()
        .map(|page| {
            page.blocks
                .iter()
                .find(|b| b.text(page).trim().to_lowercase().starts_with(prefix))
                .unwrap_or_else(|| panic!("no block starting {prefix:?}"))
                .kind
                .clone()
        })
        .collect()
}

#[test]
fn a_header_differing_only_in_case_and_spacing_is_running() {
    let layout = lay_out(&["ANNUAL REPORT", "Annual  report"]);
    assert_eq!(
        kinds_of(&layout, "annual"),
        [BlockKind::RunningHeader, BlockKind::RunningHeader]
    );
    assert_eq!(layout.diagnostics.running_headers, 2);
}

#[test]
fn numbered_chapter_titles_are_still_headings() {
    let layout = lay_out(&["Chapter 1", "Chapter 2"]);
    for kind in kinds_of(&layout, "chapter") {
        assert!(
            matches!(kind, BlockKind::Heading { .. }),
            "expected a heading, got {kind:?}"
        );
    }
    assert_eq!(layout.diagnostics.running_headers, 0);
}

/// One US Letter page per entry of `ys`: `Note` at 10 pt with its baseline
/// at that height, then five 12 pt body lines.
fn notes_at(ys: &[f32]) -> DocumentLayout {
    let n = ys.len();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 4 + 2 * i)).collect();
    let mut bodies = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
    ];
    for (i, y) in ys.iter().enumerate() {
        let mut content = format!("BT /F1 10 Tf 72 {y} Td (Note) Tj ET\n");
        for line in 0..5 {
            let by = 650 - 20 * line;
            content.push_str(&format!(
                "BT /F1 12 Tf 72 {by} Td (Body text line {line} on page {i}.) Tj ET\n"
            ));
        }
        bodies.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {} 0 R \
             /Resources << /Font << /F1 3 0 R >> >> >>",
            5 + 2 * i
        ));
        bodies.push(format!("STREAM:{content}"));
    }
    let doc = Document::from_bytes(build_pdf(&bodies)).unwrap();
    let options = LayoutOptions::default().with_running_min_fraction(0.4);
    analyze_layout(&doc.view(), &ExtractOptions::default(), &options).unwrap()
}

/// Would catch: one best window per group, leaving the second height body.
#[test]
fn running_text_at_two_disjoint_heights_is_running_at_both() {
    let layout = notes_at(&[772.0, 772.0, 772.0, 752.0, 752.0, 752.0]);
    assert_eq!(kinds_of(&layout, "note"), vec![BlockKind::RunningHeader; 6]);
}

/// Windows 20-24 and 24-28 pt below the top share the copy at 24; the
/// gaps are 3.9 pt, inside the 4 pt tolerance.
#[test]
fn overlapping_running_windows_mark_every_copy() {
    let layout = notes_at(&[772.0, 772.0, 768.1, 764.2, 764.2]);
    assert_eq!(kinds_of(&layout, "note"), vec![BlockKind::RunningHeader; 5]);
}
