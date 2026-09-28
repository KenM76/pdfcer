//! Table detection end to end over synthetic in-memory PDFs: grids drawn
//! as lines, as stroked cell rectangles and as thin fills; merged cells;
//! the three header-row evidences; frames and white rules that are not
//! tables; and a rotated page.

use pdfcer_core::document::Document;
use pdfcer_core::table_detect::{
    BoundarySource, DocumentTables, HeaderEvidence, TableError, TableOptions, detect_tables,
    detect_tables_in_pages,
};
use pdfcer_core::text_extract::{ExtractError, ExtractOptions};

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

/// One page; fonts F1 Helvetica and F2 Helvetica-Bold.
fn doc(content: &str, rotate: u16) -> Document {
    let bodies = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Rotate {rotate} \
             /Contents 4 0 R /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> >>"
        ),
        format!("STREAM:{content}"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>"
            .to_owned(),
    ];
    Document::from_bytes(build_pdf(&bodies)).expect("loads")
}

fn detect(content: &str, rotate: u16) -> DocumentTables {
    let d = doc(content, rotate);
    detect_tables(
        &d.view(),
        &ExtractOptions::default(),
        &TableOptions::default(),
    )
    .expect("detects")
}

const XS: [f32; 4] = [72.0, 200.0, 328.0, 456.0];
const YS: [f32; 4] = [700.0, 680.0, 660.0, 640.0];

/// Every grid line as its own `m l S` segment.
fn grid_lines(xs: &[f32], ys: &[f32]) -> String {
    let (x0, x1) = (xs[0], xs[xs.len() - 1]);
    let (y0, y1) = (ys[ys.len() - 1], ys[0]);
    let mut s = String::from("0 G 0.5 w\n");
    for y in ys {
        s.push_str(&format!("{x0} {y} m {x1} {y} l S\n"));
    }
    for x in xs {
        s.push_str(&format!("{x} {y0} m {x} {y1} l S\n"));
    }
    s
}

/// Text in cell (row, col) of the `XS`/`YS` grid.
fn cell_text(row: usize, col: usize, font: &str, text: &str) -> String {
    format!(
        "BT /{font} 10 Tf {} {} Td ({text}) Tj ET\n",
        XS[col] + 4.0,
        YS[row + 1] + 6.0
    )
}

fn body_rows(font_header: &str) -> String {
    let mut s = String::new();
    for (c, t) in ["Part", "Qty", "Price"].iter().enumerate() {
        s.push_str(&cell_text(0, c, font_header, t));
    }
    for (c, t) in ["Bolt M6", "12", "0.40"].iter().enumerate() {
        s.push_str(&cell_text(1, c, "F1", t));
    }
    for (c, t) in ["Nut M6", "30", "0.10"].iter().enumerate() {
        s.push_str(&cell_text(2, c, "F1", t));
    }
    s
}

#[test]
fn a_line_drawn_grid_is_a_table_with_a_bold_header() {
    let found = detect(&(grid_lines(&XS, &YS) + &body_rows("F2")), 0);
    assert_eq!(found.tables.len(), 1);
    let t = &found.tables[0];
    assert_eq!(t.source, BoundarySource::Ruled);
    assert_eq!((t.rows.len(), t.columns.len(), t.cells.len()), (3, 3, 9));
    assert_eq!(t.cell(0, 0).unwrap().text, "Part");
    assert_eq!(t.cell(1, 0).unwrap().text, "Bolt M6");
    assert_eq!(t.cell(2, 2).unwrap().text, "0.10");
    assert_eq!(t.header_rows, 1);
    assert_eq!(t.header_evidence, Some(HeaderEvidence::Bold));
    let bbox = t.bbox;
    assert!((bbox.llx - 72.0).abs() < 0.5 && (bbox.ury - 700.0).abs() < 0.5);
    // Glyph refs point at the extraction the result carries.
    let c = t.cell(1, 1).unwrap();
    assert_eq!(c.glyphs.len(), 2);
    let page = &found.text.pages[0];
    let g = &page.runs[c.glyphs[0].run].glyphs[c.glyphs[0].glyph];
    assert!(g.x > 200.0 && g.x < 328.0);
    let d = &found.diagnostics;
    assert_eq!((d.tables_ruled, d.cells, d.header_rows_inferred), (1, 9, 1));
    assert_eq!(d.inferred(), 2);
    assert!(d.rules_from_strokes >= 8);
}

#[test]
fn stroked_cell_rectangles_and_a_merged_title_row() {
    // A title row spanning all three columns, then two plain rows.
    let mut s = String::from("0 G 1 w\n");
    s.push_str("72 680 384 20 re S\n");
    for y in &YS[2..] {
        for x in &XS[..3] {
            s.push_str(&format!("{x} {y} 128 20 re S\n"));
        }
    }
    s.push_str("BT /F1 10 Tf 76 686 Td (Fasteners) Tj ET\n");
    s.push_str(&cell_text(1, 1, "F1", "12"));
    let found = detect(&s, 0);
    assert_eq!(found.tables.len(), 1);
    let t = &found.tables[0];
    assert_eq!((t.rows.len(), t.columns.len(), t.cells.len()), (3, 3, 7));
    let title = t.cell(0, 0).unwrap();
    assert_eq!((title.row_span, title.col_span), (1, 3));
    assert_eq!(title.text, "Fasteners");
    assert!(t.cell(0, 1).is_none());
    assert_eq!(t.cell(1, 1).unwrap().text, "12");
    assert_eq!(found.diagnostics.merged_cells, 1);
    assert_eq!(t.header_rows, 0);
}

#[test]
fn a_shaded_first_row_is_a_header_and_thin_fills_are_rules() {
    // Rules drawn as 0.5 pt filled bars; the header row is grey-filled.
    let (x0, x1, y0, y1) = (XS[0], XS[3], YS[3], YS[0]);
    let mut s = String::from("0.85 g 72 680 384 20 re f\n0 g\n");
    for y in YS {
        s.push_str(&format!("{x0} {} {} 0.5 re f\n", y - 0.25, x1 - x0));
    }
    for x in XS {
        s.push_str(&format!("{} {y0} 0.5 {} re f\n", x - 0.25, y1 - y0));
    }
    s.push_str(&body_rows("F1"));
    let found = detect(&s, 0);
    assert_eq!(found.tables.len(), 1);
    let t = &found.tables[0];
    assert_eq!(t.cells.len(), 9);
    assert_eq!(t.header_evidence, Some(HeaderEvidence::Filled));
    assert_eq!(found.diagnostics.rules_from_fills, 8);
    assert_eq!(found.diagnostics.rules_from_strokes, 0);
}

#[test]
fn a_heavy_rule_under_the_first_row_marks_a_header() {
    let mut s = grid_lines(&XS, &YS);
    s.push_str(&format!("2 w {} 680 m {} 680 l S\n", XS[0], XS[3]));
    s.push_str(&body_rows("F1"));
    let found = detect(&s, 0);
    let t = &found.tables[0];
    assert_eq!(t.header_evidence, Some(HeaderEvidence::HeavyRule));
}

#[test]
fn a_frame_white_rules_and_curves_are_not_tables() {
    let mut s = String::from("0 G 1 w 50 50 500 700 re S\n");
    // A white grid: invisible, so not a table.
    s.push_str("1 G\n");
    s.push_str(&grid_lines(&XS, &YS).replace("0 G ", ""));
    // A circle-ish curve inside the frame.
    s.push_str("0 G 300 300 m 300 350 350 350 350 300 c S\n");
    let found = detect(&s, 0);
    assert!(found.tables.is_empty(), "{:?}", found.tables);
    assert_eq!(found.diagnostics.single_cell_frames, 1);
    assert_eq!(found.diagnostics.inferred(), 0);
}

#[test]
fn a_rotated_page_reads_rows_as_displayed() {
    // Three user-space columns, two user-space rows; /Rotate 90 shows it
    // as three rows of two columns.
    let (xs, ys) = (&XS, &YS[..3]);
    let mut s = grid_lines(xs, ys);
    s.push_str("BT /F1 10 Tf 76 666 Td (A) Tj ET\n");
    s.push_str("BT /F1 10 Tf 76 686 Td (B) Tj ET\n");
    let found = detect(&s, 90);
    let t = &found.tables[0];
    assert_eq!((t.rows.len(), t.columns.len()), (3, 2));
    // Display top-left: lowest user x, lowest user y.
    assert_eq!(t.cell(0, 0).unwrap().text, "A");
    assert_eq!(t.cell(0, 1).unwrap().text, "B");
    // Row bands come back in user space: the first spans user x 72..200.
    let r0 = t.rows[0];
    assert!((r0.llx - 72.0).abs() < 0.5 && (r0.urx - 200.0).abs() < 0.5);
}

#[test]
fn a_cell_with_two_lines_keeps_both() {
    let mut s = grid_lines(&XS, &YS);
    s.push_str("BT /F1 6 Tf 76 673 Td (upper) Tj ET\n");
    s.push_str("BT /F1 6 Tf 76 664 Td (lower words) Tj ET\n");
    let found = detect(&s, 0);
    assert_eq!(
        found.tables[0].cell(1, 0).unwrap().text,
        "upper\nlower words"
    );
}

/// Unruled rows at y = 500, 486, 472, ... with columns at x = 72, 200, 328.
fn aligned_rows(rows: &[&[&str]], header_font: &str) -> String {
    let mut s = String::new();
    for (r, cells) in rows.iter().enumerate() {
        let font = if r == 0 { header_font } else { "F1" };
        #[allow(clippy::cast_precision_loss)]
        let y = 500.0 - 14.0 * r as f32;
        for (c, t) in cells.iter().enumerate() {
            if t.is_empty() {
                continue;
            }
            #[allow(clippy::cast_precision_loss)]
            let x = 72.0 + 128.0 * c as f32;
            s.push_str(&format!("BT /{font} 10 Tf {x} {y} Td ({t}) Tj ET\n"));
        }
    }
    s
}

const PARTS: [&[&str]; 4] = [
    &["Part", "Qty", "Price"],
    &["Bolt M6", "12", "0.40"],
    &["Nut M6", "30", "0.10"],
    &["Washer", "", "0.05"],
];

#[test]
fn whitespace_aligned_rows_are_a_table() {
    // A one-phrase caption line just above is not a row of the table.
    let caption = "BT /F1 10 Tf 72 514 Td (Parts list) Tj ET\n";
    let found = detect(&(caption.to_owned() + &aligned_rows(&PARTS, "F2")), 0);
    assert_eq!(found.tables.len(), 1, "{:?}", found.diagnostics);
    let t = &found.tables[0];
    assert_eq!(t.source, BoundarySource::Aligned);
    assert_eq!((t.rows.len(), t.columns.len(), t.cells.len()), (4, 3, 12));
    assert_eq!(t.cell(1, 0).unwrap().text, "Bolt M6");
    assert_eq!(t.cell(2, 2).unwrap().text, "0.10");
    // The empty cell is still in the grid.
    assert_eq!(t.cell(3, 1).unwrap().text, "");
    assert_eq!(t.header_evidence, Some(HeaderEvidence::Bold));
    // Column edges sit in the gutters, row edges between baselines.
    let c1 = t.columns[1];
    assert!(c1.llx > 110.0 && c1.llx < 200.0, "{c1:?}");
    let r1 = t.rows[1];
    assert!(r1.ury > 490.0 && r1.ury < 504.0, "{r1:?}");
    let d = &found.diagnostics;
    assert_eq!((d.tables_aligned, d.tables_ruled, d.cells), (1, 0, 12));
    assert_eq!(d.inferred(), 2);
}

#[test]
fn two_columns_of_prose_are_not_a_table() {
    let left = "The quick brown fox jumps over the lazy dog again";
    let right = "Pack my box with five dozen liquor jugs once more";
    let rows: Vec<[&str; 2]> = (0..5).map(|_| [left, right]).collect();
    let mut s = String::new();
    for (r, [a, b]) in rows.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let y = 500.0 - 12.0 * r as f32;
        s.push_str(&format!("BT /F1 8 Tf 72 {y} Td ({a}) Tj ET\n"));
        s.push_str(&format!("BT /F1 8 Tf 320 {y} Td ({b}) Tj ET\n"));
    }
    let found = detect(&s, 0);
    assert!(found.tables.is_empty(), "{:?}", found.tables);
    assert_eq!(found.diagnostics.aligned_blocks_rejected, 1);
    assert_eq!(found.diagnostics.inferred(), 0);
}

#[test]
fn a_column_used_by_one_row_rejects_the_block() {
    let rows: [&[&str]; 3] = [&["A", "1"], &["B", "2"], &["C", "3", "stray"]];
    let found = detect(&aligned_rows(&rows, "F1"), 0);
    assert!(found.tables.is_empty(), "{:?}", found.tables);
    assert_eq!(found.diagnostics.aligned_blocks_rejected, 1);
}

#[test]
fn two_aligned_rows_are_too_few() {
    let found = detect(&aligned_rows(&PARTS[..2], "F1"), 0);
    assert!(found.tables.is_empty());
    assert_eq!(found.diagnostics.aligned_blocks_rejected, 0);
}

#[test]
fn a_booktabs_rule_under_the_first_row_marks_a_header() {
    let mut s = String::from("0 G 1 w 72 512 m 400 512 l S\n");
    s.push_str("0.5 w 72 494 m 400 494 l S\n");
    s.push_str("1 w 72 452 m 400 452 l S\n");
    s.push_str(&aligned_rows(&PARTS, "F1"));
    let found = detect(&s, 0);
    assert_eq!(found.tables.len(), 1, "{:?}", found.diagnostics);
    let t = &found.tables[0];
    assert_eq!(t.source, BoundarySource::Aligned);
    assert_eq!(t.header_evidence, Some(HeaderEvidence::RuleBelow));
    // A second interior rule makes it a ruled-rows table, not booktabs.
    let found = detect(&(s + "0.5 w 72 480 m 400 480 l S\n"), 0);
    assert_eq!(found.tables[0].header_evidence, None);
}

#[test]
fn a_ruled_and_an_aligned_table_share_a_page() {
    let mut s = grid_lines(&XS, &YS);
    s.push_str(&body_rows("F1"));
    s.push_str(&aligned_rows(&PARTS, "F1"));
    let found = detect(&s, 0);
    let sources: Vec<_> = found.tables.iter().map(|t| t.source).collect();
    assert_eq!(sources, [BoundarySource::Ruled, BoundarySource::Aligned]);
    // The ruled table's text is not offered to the aligned pass again.
    assert_eq!(found.tables[1].rows.len(), 4);
    let d = &found.diagnostics;
    assert_eq!((d.tables_ruled, d.tables_aligned), (1, 1));
}

/// Three pages: a 3x3 grid on pages 1 and 3, a 2x2 grid on page 2.
fn three_pages() -> Document {
    let big = grid_lines(&XS, &YS) + &body_rows("F2");
    let small = grid_lines(&XS[..3], &YS[..3]);
    let page = |contents: usize| {
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {contents} 0 R              /Resources << /Font << /F1 9 0 R /F2 10 0 R >> >> >>"
        )
    };
    let bodies = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>".to_owned(),
        page(6),
        page(7),
        page(8),
        format!("STREAM:{big}"),
        format!("STREAM:{small}"),
        format!("STREAM:{big}"),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>"
            .to_owned(),
    ];
    Document::from_bytes(build_pdf(&bodies)).expect("loads")
}

#[test]
fn a_page_subset_reads_and_counts_only_those_pages() {
    let d = three_pages();
    let (x, o) = (ExtractOptions::default(), TableOptions::default());
    let all = detect_tables(&d.view(), &x, &o).expect("detects");
    assert_eq!(all.diagnostics.pages, 3);
    assert_eq!(all.tables.len(), 3);

    let one = detect_tables_in_pages(&d.view(), &[1], &x, &o).expect("detects");
    assert_eq!(one.diagnostics.pages, 1);
    assert_eq!(one.text.pages.len(), 1);
    assert_eq!(one.tables.len(), 1);
    assert_eq!(one.tables[0].page_index, 1);
    assert_eq!(
        (one.tables[0].rows.len(), one.tables[0].columns.len()),
        (2, 2)
    );
    assert_eq!(one.diagnostics.cells, 4);

    let two = detect_tables_in_pages(&d.view(), &[2, 0], &x, &o).expect("detects");
    let order: Vec<usize> = two.tables.iter().map(|t| t.page_index).collect();
    assert_eq!(order, [2, 0]);
    assert_eq!(two.diagnostics.cells, 18);
}

#[test]
fn a_page_past_the_end_is_refused() {
    let d = three_pages();
    let err = detect_tables_in_pages(
        &d.view(),
        &[3],
        &ExtractOptions::default(),
        &TableOptions::default(),
    )
    .expect_err("page 4 of 3");
    assert!(matches!(
        err,
        TableError::Extract(ExtractError::NoSuchPage { index: 3, count: 3 })
    ));
}
