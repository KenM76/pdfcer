//! Table detection end to end over synthetic in-memory PDFs: grids drawn
//! as lines, as stroked cell rectangles and as thin fills; merged cells;
//! the three header-row evidences; frames and white rules that are not
//! tables; and a rotated page.

use pdfcer_core::document::Document;
use pdfcer_core::table_detect::{
    BoundarySource, DocumentTables, HeaderEvidence, TableOptions, detect_tables,
};
use pdfcer_core::text_extract::ExtractOptions;

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
    s.push_str(&body_rows("F1"));
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
