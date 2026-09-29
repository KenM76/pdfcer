//! `pdfcer_text::tagged_layout` (G066) against synthetic tagged files.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_model::document::Document;
use pdfcer_model::page_tree::pages_in;
use pdfcer_text::block_layout::{BlockKind, BlockSource, LayoutOptions, PageGeometry};
use pdfcer_text::structure_tree::read_structure_tree;
use pdfcer_text::tagged_layout::{
    FallbackReason, LayoutSourceUsed, StructureUse, TaggedLayout, TaggedLayoutOptions,
    layout_from_structure,
};
use pdfcer_text::text_extract::ExtractOptions;

/// A classic-xref PDF whose object `i + 1` is `bodies[i]`; a body starting
/// with `STREAM:` becomes a stream with that payload.
fn build_pdf(bodies: &[&str]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        let body = match body.strip_prefix("STREAM:") {
            Some(data) => format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len()),
            None => (*body).to_owned(),
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

/// One page: a heading, a paragraph, a one-item list, a paragraph in a
/// non-standard type, a 3x2 table (header row, a two-column total) and an
/// artifact page number. MCIDs 0-9.
const CONTENT: &str = "STREAM:\
/H1 <</MCID 0>> BDC BT /F1 18 Tf 72 720 Td (Quarterly Report) Tj ET EMC\n\
/P <</MCID 1>> BDC BT /F1 11 Tf 72 690 Td (Sales rose in every region.) Tj ET EMC\n\
/Lbl <</MCID 2>> BDC BT /F1 11 Tf 72 665 Td (1.) Tj ET EMC\n\
/LBody <</MCID 3>> BDC BT /F1 11 Tf 90 665 Td (First item) Tj ET EMC\n\
/Blurb <</MCID 4>> BDC BT /F1 11 Tf 72 640 Td (A styled note) Tj ET EMC\n\
/TH <</MCID 5>> BDC BT /F1 11 Tf 72 600 Td (Name) Tj ET EMC\n\
/TH <</MCID 6>> BDC BT /F1 11 Tf 250 600 Td (Qty) Tj ET EMC\n\
/TD <</MCID 7>> BDC BT /F1 11 Tf 72 580 Td (Widget) Tj ET EMC\n\
/TD <</MCID 8>> BDC BT /F1 11 Tf 250 580 Td (12) Tj ET EMC\n\
/TD <</MCID 9>> BDC BT /F1 11 Tf 72 560 Td (Total 12) Tj ET EMC\n\
/Artifact BMC BT /F1 9 Tf 300 40 Td (Page 1) Tj ET EMC";

const PAGE: &str = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
                    /Resources << /Font << /F1 5 0 R >> >> >>";
const FONT: &str = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>";

/// Catalog + page + content + font, then the structure objects from 6 on.
fn tagged(structure: &[&str]) -> Vec<u8> {
    let mut bodies = vec![
        "<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 6 0 R \
         /MarkInfo << /Marked true >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        PAGE,
        CONTENT,
        FONT,
    ];
    bodies.extend_from_slice(structure);
    build_pdf(&bodies)
}

const REPORT: &[&str] = &[
    // 6
    "<< /Type /StructTreeRoot /K 7 0 R >>",
    // 7
    "<< /S /Document /P 6 0 R /K [8 0 R 9 0 R 10 0 R 14 0 R 15 0 R] >>",
    // 8, 9
    "<< /S /H1 /P 7 0 R /Pg 3 0 R /K 0 >>",
    "<< /S /P /P 7 0 R /Pg 3 0 R /K 1 >>",
    // 10-13: L > LI > (Lbl, LBody)
    "<< /S /L /P 7 0 R /K 11 0 R >>",
    "<< /S /LI /P 10 0 R /K [12 0 R 13 0 R] >>",
    "<< /S /Lbl /P 11 0 R /Pg 3 0 R /K 2 >>",
    "<< /S /LBody /P 11 0 R /Pg 3 0 R /K 3 >>",
    // 14: a producer's own type with no /RoleMap entry
    "<< /S /Blurb /P 7 0 R /Pg 3 0 R /K 4 >>",
    // 15-25: Table > THead > TR > TH TH; TBody > TR > TD TD; TR > TD(ColSpan 2)
    "<< /S /Table /P 7 0 R /K [16 0 R 20 0 R] >>",
    "<< /S /THead /P 15 0 R /K 17 0 R >>",
    "<< /S /TR /P 16 0 R /K [18 0 R 19 0 R] >>",
    "<< /S /TH /P 17 0 R /Pg 3 0 R /K 5 >>",
    "<< /S /TH /P 17 0 R /Pg 3 0 R /K 6 >>",
    "<< /S /TBody /P 15 0 R /K [21 0 R 24 0 R] >>",
    "<< /S /TR /P 20 0 R /K [22 0 R 23 0 R] >>",
    "<< /S /TD /P 21 0 R /Pg 3 0 R /K 7 >>",
    "<< /S /TD /P 21 0 R /Pg 3 0 R /K 8 >>",
    "<< /S /TR /P 20 0 R /K 25 0 R >>",
    "<< /S /TD /P 24 0 R /Pg 3 0 R /A << /O /Table /ColSpan 2 >> /K 9 >>",
];

fn lay_out(bytes: Vec<u8>, options: &TaggedLayoutOptions) -> TaggedLayout {
    let doc = Document::from_bytes(bytes).unwrap();
    let view = doc.view();
    let tree = read_structure_tree(&view, &ExtractOptions::default()).unwrap();
    let pages = pages_in(&view).unwrap();
    let geometry: Vec<PageGeometry> = tree
        .text
        .pages
        .iter()
        .map(|p| PageGeometry::new(pages[p.page_index].crop_box, pages[p.page_index].rotate))
        .collect();
    layout_from_structure(&tree, &geometry, &LayoutOptions::default(), options)
}

fn report_layout() -> TaggedLayout {
    lay_out(tagged(REPORT), &TaggedLayoutOptions::default())
}

#[test]
fn blocks_follow_the_tags_in_logical_order() {
    let t = report_layout();
    let page = &t.layout.pages[0];
    let blocks: Vec<(BlockKind, BlockSource, String)> = page
        .blocks
        .iter()
        .map(|b| (b.kind.clone(), b.source, b.text(page)))
        .collect();
    assert_eq!(
        blocks,
        [
            (
                BlockKind::Heading { level: 1 },
                BlockSource::Structure,
                "Quarterly Report".to_owned()
            ),
            (
                BlockKind::Paragraph,
                BlockSource::Structure,
                "Sales rose in every region.".to_owned()
            ),
            (
                BlockKind::ListItem {
                    marker: "1.".to_owned()
                },
                BlockSource::Structure,
                "1. First item".to_owned()
            ),
            (
                BlockKind::Paragraph,
                BlockSource::Structure,
                "A styled note".to_owned()
            ),
            // The artifact page number is not the tree's: its inferred
            // block is kept, after the structure block before it.
            (
                BlockKind::Paragraph,
                BlockSource::Inferred,
                "Page 1".to_owned()
            ),
        ]
    );
    let r = &t.report;
    assert_eq!(r.source, LayoutSourceUsed::StructureTree);
    assert_eq!(r.fallback, None);
    assert!((r.coverage - 1.0).abs() < 1e-6, "{}", r.coverage);
    assert_eq!(r.structure_blocks, 4);
    assert_eq!(r.non_standard_as_paragraph, 1, "Blurb has no role map");
    assert_eq!(r.untyped_as_paragraph, 0);
    assert_eq!(r.inferred_blocks_kept, 1);
    assert_eq!(r.broken_references, 0);
}

#[test]
fn a_table_element_becomes_a_grid_with_spans_and_a_header() {
    let t = report_layout();
    assert_eq!(t.tables.len(), 1);
    let table = &t.tables[0];
    assert_eq!((table.rows.len(), table.columns.len()), (3, 2));
    assert_eq!(table.header_rows, 1);
    let cells: Vec<(usize, usize, usize, &str, bool)> = table
        .cells
        .iter()
        .map(|c| (c.row, c.col, c.col_span, c.text.as_str(), c.header))
        .collect();
    assert_eq!(
        cells,
        [
            (0, 0, 1, "Name", true),
            (0, 1, 1, "Qty", true),
            (1, 0, 1, "Widget", false),
            (1, 1, 1, "12", false),
            (2, 0, 2, "Total 12", false),
        ]
    );
    // Cell text is the table's, not a paragraph's.
    let page = &t.layout.pages[0];
    assert!(page.blocks.iter().all(|b| !b.text(page).contains("Widget")));
    assert_eq!((t.report.tables, t.report.table_cells), (1, 5));
}

#[test]
fn an_untagged_file_falls_back_and_says_why() {
    let bytes = build_pdf(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        PAGE,
        CONTENT,
        FONT,
    ]);
    let t = lay_out(bytes, &TaggedLayoutOptions::default());
    assert_eq!(t.report.source, LayoutSourceUsed::Inferred);
    assert_eq!(t.report.fallback, Some(FallbackReason::NoStructureTree));
    assert!(t.tables.is_empty());
    let page = &t.layout.pages[0];
    assert!(
        page.blocks
            .iter()
            .all(|b| b.source == BlockSource::Inferred)
    );
    assert_eq!(t.report.inferred_blocks_kept, page.blocks.len());
}

#[test]
fn structure_use_never_ignores_a_good_tree() {
    let options = TaggedLayoutOptions::default().with_use_structure(StructureUse::Never);
    let t = lay_out(tagged(REPORT), &options);
    assert_eq!(t.report.source, LayoutSourceUsed::Inferred);
    assert_eq!(t.report.fallback, Some(FallbackReason::Disabled));
}

/// A tree that tags only the heading claims under half the text.
const HEADING_ONLY: &[&str] = &[
    "<< /Type /StructTreeRoot /K 7 0 R >>",
    "<< /S /H1 /P 6 0 R /Pg 3 0 R /K 0 >>",
];

#[test]
fn a_thin_tree_falls_back_under_auto_and_is_used_under_always() {
    let auto = lay_out(tagged(HEADING_ONLY), &TaggedLayoutOptions::default());
    assert_eq!(auto.report.fallback, Some(FallbackReason::LowCoverage));
    assert!(auto.report.coverage > 0.0 && auto.report.coverage < 0.5);

    let always = TaggedLayoutOptions::default().with_use_structure(StructureUse::Always);
    let t = lay_out(tagged(HEADING_ONLY), &always);
    assert_eq!(t.report.source, LayoutSourceUsed::StructureTree);
    assert_eq!(t.report.structure_blocks, 1);
    let page = &t.layout.pages[0];
    assert_eq!(page.blocks[0].kind, BlockKind::Heading { level: 1 });
    assert_eq!(page.blocks[0].source, BlockSource::Structure);
    assert!(
        page.blocks[1..]
            .iter()
            .all(|b| b.source == BlockSource::Inferred)
    );
}

#[test]
fn retain_pages_recounts() {
    let mut t = report_layout();
    t.retain_pages(&[]);
    assert!(t.layout.pages.is_empty() && t.tables.is_empty());
    let r = &t.report;
    assert_eq!(
        (
            r.structure_blocks,
            r.inferred_blocks_kept,
            r.tables,
            r.table_cells
        ),
        (0, 0, 0, 0)
    );
}

#[test]
fn a_paragraph_nested_in_a_paragraph_is_its_own_block_but_a_list_items_is_not() {
    const NESTED: &[&str] = &[
        // 6, 7
        "<< /Type /StructTreeRoot /K 7 0 R >>",
        "<< /S /Document /P 6 0 R /K [8 0 R 10 0 R] >>",
        // 8, 9: a producer's heading P holding its body P
        "<< /S /P /P 7 0 R /Pg 3 0 R /K [0 9 0 R] >>",
        "<< /S /P /P 8 0 R /Pg 3 0 R /K 1 >>",
        // 10-14: L > LI > (Lbl, LBody > P)
        "<< /S /L /P 7 0 R /K 11 0 R >>",
        "<< /S /LI /P 10 0 R /K [12 0 R 13 0 R] >>",
        "<< /S /Lbl /P 11 0 R /Pg 3 0 R /K 2 >>",
        "<< /S /LBody /P 11 0 R /K 14 0 R >>",
        "<< /S /P /P 13 0 R /Pg 3 0 R /K 3 >>",
    ];
    let t = lay_out(
        tagged(NESTED),
        &TaggedLayoutOptions::default().with_use_structure(StructureUse::Always),
    );
    let page = &t.layout.pages[0];
    let tagged_blocks: Vec<(BlockKind, String)> = page
        .blocks
        .iter()
        .filter(|b| b.source == BlockSource::Structure)
        .map(|b| (b.kind.clone(), b.text(page)))
        .collect();
    assert_eq!(
        tagged_blocks,
        [
            (BlockKind::Paragraph, "Quarterly Report".to_owned()),
            (
                BlockKind::Paragraph,
                "Sales rose in every region.".to_owned()
            ),
            (
                BlockKind::ListItem {
                    marker: "1.".to_owned()
                },
                "1. First item".to_owned()
            ),
        ]
    );
    assert_eq!(t.report.structure_blocks, 3);
}
