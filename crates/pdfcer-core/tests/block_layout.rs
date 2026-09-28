//! Block layout end to end over synthetic in-memory PDFs: running text by
//! repetition, headings by size and weight, lists, captions, columns,
//! alignment, a tagged `/Artifact /Subtype` and a rotated page.

use pdfcer_core::block_layout::{
    Alignment, BlockKind, BlockSource, DocumentLayout, LayoutOptions, PageLayout, analyze_layout,
};
use pdfcer_core::document::Document;
use pdfcer_core::text_extract::ExtractOptions;

/// A PDF from object bodies (object 1 is the catalog); `STREAM:` bodies
/// become streams.
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

/// Pages from content streams, sharing fonts F1 Helvetica, F2
/// Helvetica-Bold, F3 Courier. Objects: 1 catalog, 2 pages, 3-5 fonts,
/// then (page, content) pairs.
fn doc_from_pages(contents: &[&str], rotate: u16) -> Document {
    let n = contents.len();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 6 + 2 * i)).collect();
    let mut bodies = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>"
            .to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".to_owned(),
    ];
    for (i, c) in contents.iter().enumerate() {
        bodies.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Rotate {rotate} \
             /Contents {} 0 R /Resources << /Font << /F1 3 0 R /F2 4 0 R /F3 5 0 R >> >> >>",
            7 + 2 * i
        ));
        bodies.push(format!("STREAM:{c}"));
    }
    Document::from_bytes(build_pdf(&bodies)).expect("loads")
}

fn layout(doc: &Document) -> DocumentLayout {
    analyze_layout(
        &doc.view(),
        &ExtractOptions::default(),
        &LayoutOptions::default(),
    )
    .expect("layout runs")
}

fn line(font: &str, size: u32, x: u32, y: u32, text: &str) -> String {
    format!("BT /{font} {size} Tf {x} {y} Td ({text}) Tj ET\n")
}

fn page_with_running(n: u32, body: &str) -> String {
    format!(
        "{}{body}{}",
        line("F1", 9, 72, 760, "Annual Report 2024"),
        line("F1", 9, 290, 30, &format!("Page {n}"))
    )
}

fn kinds_and_text(page: &PageLayout) -> Vec<(BlockKind, String)> {
    page.blocks
        .iter()
        .map(|b| (b.kind.clone(), b.text(page)))
        .collect()
}

fn three_page_doc() -> Document {
    let p1 = [
        line("F2", 24, 72, 700, "Introduction"),
        line(
            "F1",
            10,
            72,
            670,
            "The first paragraph opens with a long line of body text here.",
        ),
        line("F1", 10, 72, 658, "It continues, shorter."),
        line(
            "F1",
            10,
            72,
            646,
            "And it ends on a line of middling length.",
        ),
        line(
            "F1",
            10,
            90,
            626,
            "A second paragraph starts indented by eighteen points,",
        ),
        line("F1", 10, 72, 614, "then returns to the margin."),
        line("F2", 10, 72, 594, "Background"),
        line("F1", 10, 72, 582, "Some text follows the subheading."),
        "BT /F1 10 Tf 72 560 Td <95> Tj ( First item) Tj ET\n".to_owned(),
        "BT /F1 10 Tf 72 548 Td <95> Tj ( Second item) Tj ET\n".to_owned(),
        line("F1", 10, 82, 536, "wraps onto a second line"),
        line("F1", 10, 200, 510, "Figure 1: Growth over time"),
    ]
    .concat();
    let mut p2 = line(
        "F1",
        16,
        100,
        700,
        "A Heading That Crosses The Gutter Between Columns",
    );
    for k in 0..8u32 {
        p2.push_str(&line(
            "F1",
            10,
            72,
            670 - 12 * k,
            &format!("Left column line {k}"),
        ));
        p2.push_str(&line(
            "F1",
            10,
            320,
            670 - 12 * k,
            &format!("Right column line {k}"),
        ));
    }
    // Courier 10pt: every character advances exactly 6 pt.
    let p3 = [
        // Centred on x = 306: widths 60, 120, 84.
        line("F3", 10, 276, 700, "ten chars!"),
        line("F3", 10, 246, 688, "twenty characters!!!"),
        line("F3", 10, 264, 676, "fourteen chars"),
        // Right-aligned at x = 540.
        line("F3", 10, 480, 640, "ten chars!"),
        line("F3", 10, 420, 628, "twenty characters!!!"),
        line("F3", 10, 456, 616, "fourteen chars"),
        // Justified: forty characters each, flush both sides.
        line(
            "F3",
            10,
            72,
            580,
            "forty characters of text on each line ab",
        ),
        line(
            "F3",
            10,
            72,
            568,
            "forty characters of text on each line cd",
        ),
        line(
            "F3",
            10,
            72,
            556,
            "forty characters of text on each line ef",
        ),
        // Left: ragged right.
        line("F3", 10, 72, 520, "a short one"),
        line("F3", 10, 72, 508, "a considerably longer line of text"),
        line("F3", 10, 72, 496, "medium length"),
    ]
    .concat();
    doc_from_pages(
        &[
            &page_with_running(1, &p1),
            &page_with_running(2, &p2),
            &page_with_running(3, &p3),
        ],
        0,
    )
}

#[test]
fn running_text_is_found_by_repetition_not_position() {
    let doc = three_page_doc();
    let l = layout(&doc);
    for page in &l.pages {
        let first = page.blocks.first().expect("blocks");
        assert_eq!(first.kind, BlockKind::RunningHeader);
        assert_eq!(first.text(page), "Annual Report 2024");
        assert_eq!(first.source, BlockSource::Inferred);
        let last = page.blocks.last().expect("blocks");
        assert_eq!(last.kind, BlockKind::PageNumber);
    }
    // The page-1 title sits inside the top margin band but is not
    // repeated, so it stays a heading.
    let p1 = kinds_and_text(&l.pages[0]);
    assert_eq!(
        p1[1],
        (BlockKind::Heading { level: 1 }, "Introduction".to_owned())
    );
    let d = &l.diagnostics;
    assert_eq!(
        (d.running_headers, d.page_numbers, d.running_footers),
        (3, 3, 0)
    );
}

#[test]
fn headings_paragraphs_lists_and_captions_on_page_one() {
    let doc = three_page_doc();
    let l = layout(&doc);
    let page = &l.pages[0];
    let got = kinds_and_text(page);
    let kinds: Vec<&BlockKind> = got.iter().map(|(k, _)| k).collect();
    assert_eq!(
        kinds,
        [
            &BlockKind::RunningHeader,
            &BlockKind::Heading { level: 1 },
            &BlockKind::Paragraph,
            &BlockKind::Paragraph,
            &BlockKind::Heading { level: 3 },
            &BlockKind::Paragraph,
            &BlockKind::ListItem {
                marker: "\u{2022}".to_owned()
            },
            &BlockKind::ListItem {
                marker: "\u{2022}".to_owned()
            },
            &BlockKind::Caption,
            &BlockKind::PageNumber,
        ],
        "{got:#?}"
    );
    assert_eq!(got[7].1, "\u{2022} Second item wraps onto a second line");
    let second_para = &page.blocks[3];
    assert!((second_para.first_line_indent - 18.0).abs() < 0.5);
    assert_eq!(page.blocks[2].alignment, Alignment::Left);
    let d = &l.diagnostics;
    assert_eq!(d.body_font_size, Some(10.0));
    assert_eq!(
        (d.headings_from_size, d.headings_from_weight),
        (2, 1),
        "{d:?}"
    );
    assert_eq!((d.list_items, d.captions), (2, 1));
}

#[test]
fn two_columns_read_left_column_first() {
    let doc = three_page_doc();
    let l = layout(&doc);
    let page = &l.pages[1];
    assert_eq!(page.columns.len(), 2);
    let got = kinds_and_text(page);
    assert_eq!(got[1].0, BlockKind::Heading { level: 2 });
    assert!(got[2].1.starts_with("Left column line 0"), "{got:#?}");
    assert!(got[2].1.ends_with("Left column line 7"));
    assert!(got[3].1.starts_with("Right column line 0"));
    assert_eq!(page.blocks[2].column, Some(0));
    assert_eq!(page.blocks[3].column, Some(1));
    assert_eq!(l.diagnostics.multi_column_pages, 1);
    assert_eq!(l.diagnostics.spanning_lines, 1);
}

#[test]
fn alignment_is_measured_from_line_extents() {
    let doc = three_page_doc();
    let l = layout(&doc);
    let page = &l.pages[2];
    let aligned: Vec<Alignment> = page
        .blocks
        .iter()
        .filter(|b| b.kind == BlockKind::Paragraph)
        .map(|b| b.alignment)
        .collect();
    assert_eq!(
        aligned,
        [
            Alignment::Center,
            Alignment::Right,
            Alignment::Justified,
            Alignment::Left
        ]
    );
}

#[test]
fn a_tagged_header_wins_on_a_single_page() {
    let doc = doc_from_pages(
        &["/Artifact <</Type /Pagination /Subtype /Header>> BDC\n\
           BT /F1 9 Tf 72 760 Td (Confidential draft) Tj ET EMC\n\
           BT /F1 10 Tf 72 700 Td (Body text.) Tj ET\n\
           BT /F1 9 Tf 290 30 Td (1) Tj ET"],
        0,
    );
    let l = layout(&doc);
    let page = &l.pages[0];
    assert_eq!(page.blocks[0].kind, BlockKind::RunningHeader);
    assert_eq!(page.blocks[0].source, BlockSource::Tagged);
    // One page cannot show repetition, so the bare "1" is body text.
    assert_eq!(
        page.blocks.last().map(|b| &b.kind),
        Some(&BlockKind::Paragraph)
    );
    assert_eq!(l.diagnostics.tagged_artifact_blocks, 1);
    // Every untagged block is an inference, paragraphs included.
    assert_eq!(l.diagnostics.inferred(), 2);
    assert_eq!(
        l.diagnostics.inferred(),
        l.diagnostics.blocks - l.diagnostics.tagged_artifact_blocks
    );
}

#[test]
fn a_rotated_page_is_laid_out_as_displayed() {
    // /Rotate 90: text drawn running up the page reads left to right.
    let doc = doc_from_pages(
        &["BT /F1 10 Tf 0 1 -1 0 100 72 Tm (Rotated line one) Tj ET\n\
           BT /F1 10 Tf 0 1 -1 0 112 72 Tm (rotated line two) Tj ET\n\
           BT /F1 10 Tf 72 700 Td (Upright text is sideways here) Tj ET"],
        90,
    );
    let l = layout(&doc);
    let page = &l.pages[0];
    assert_eq!(page.blocks.len(), 1, "{:#?}", kinds_and_text(page));
    assert_eq!(
        page.blocks[0].text(page),
        "Rotated line one rotated line two"
    );
    assert_eq!(l.diagnostics.runs_not_horizontal, 1);
}

#[test]
fn an_indent_after_a_short_line_starts_a_paragraph_without_a_gap() {
    let doc = doc_from_pages(
        &[&[
            line("F3", 10, 72, 700, "a first paragraph line that runs long"),
            line("F3", 10, 72, 688, "a second line that also runs quite long"),
            line("F3", 10, 72, 676, "then it ends."),
            line("F3", 10, 90, 664, "The next one is indented, no gap"),
            line(
                "F3",
                10,
                72,
                652,
                "and carries on at the margin for a while",
            ),
        ]
        .concat()],
        0,
    );
    let l = layout(&doc);
    let page = &l.pages[0];
    let lines: Vec<&Vec<usize>> = page.blocks.iter().map(|b| &b.lines).collect();
    assert_eq!(lines, [&vec![0, 1, 2], &vec![3, 4]]);
    assert!((page.blocks[1].first_line_indent - 18.0).abs() < 0.5);
}
