//! # ODS export — detected tables written as an OpenDocument spreadsheet
//!
//! Writes an OpenDocument spreadsheet package (OASIS ODF v1.3 Part 2
//! Packages, Part 3 Schema) from [`crate::table_detect::Table`]s, with the
//! same sheet grouping and the same number rule as
//! [`super::xlsx::write_xlsx`]: a cell is a number in the `.ods` exactly
//! when it is one in the `.xlsx` (see that module's *Numbers*).
//!
//! Package layout: `mimetype` first and stored (P2 §3.3, P3 §2.2.4 B),
//! `content.xml`, and `META-INF/manifest.xml` with `manifest:version` on the
//! root (P2 §4.16.14.2). No `styles.xml` or `meta.xml` (P3 §2.2.1 A.2).
//!
//! Cell text keeps its spacing: a run of spaces and a leading or trailing
//! space are written as `<text:s>` (P3 §6.1.2–6.1.3), a tab as
//! `<text:tab/>`, and each line of a multi-line cell as its own `<text:p>`.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::ooxml_zip::{PackageError, ZipWriter, escape_into};
use super::xlsx::{
    MAX_COLS, MAX_ROWS, NumberLocale, Parsed, SheetLayout, group_sheets, parse_number,
};
use crate::table_detect::Table;

/// Options for [`write_ods`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct OdsOptions {
    /// Which tables share a sheet.
    pub sheets: SheetLayout,
    /// How numbers are read.
    pub numbers: NumberLocale,
}

impl OdsOptions {
    /// Sets [`Self::sheets`].
    #[must_use]
    pub const fn with_sheets(mut self, sheets: SheetLayout) -> Self {
        self.sheets = sheets;
        self
    }

    /// Sets [`Self::numbers`].
    #[must_use]
    pub const fn with_numbers(mut self, numbers: NumberLocale) -> Self {
        self.numbers = numbers;
        self
    }
}

/// What [`write_ods`] wrote, and what it changed or left out. The fields
/// mean what [`super::xlsx::XlsxReport`]'s do; ODF has no cell-length
/// limit, so nothing is truncated.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct OdsReport {
    /// Sheets written (at least 1; an empty one when there are no tables).
    pub sheets: usize,
    /// Tables written.
    pub tables: usize,
    /// Cells written.
    pub cells: usize,
    /// Cells written as a merged range.
    pub merged_cells: usize,
    /// Header rows written bold.
    pub header_rows: usize,
    /// Cells written as numbers.
    pub numbers: usize,
    /// Cells that parse as a number with a value depending on the locale,
    /// kept as text ([`NumberLocale::Auto`] only).
    pub ambiguous_numbers: usize,
    /// Characters XML cannot carry (control characters), dropped.
    pub characters_dropped: usize,
    /// Cells left out because they fall past LibreOffice Calc's last row
    /// (1 048 576) or column (16 384).
    pub cells_beyond_limits: usize,
}

/// A written spreadsheet.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OdsOutput {
    /// The `.ods` file.
    pub bytes: Vec<u8>,
    /// What was written.
    pub report: OdsReport,
}

const MIMETYPE: &str = "application/vnd.oasis.opendocument.spreadsheet";
const XML_DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n";
/// Column width bounds in points, matching the `.xlsx` export's 3–80
/// character widths.
const MIN_WIDTH_PT: f64 = 15.75;
const MAX_WIDTH_PT: f64 = 420.0;

/// Writes `tables` as an `.ods` spreadsheet.
///
/// # Errors
///
/// [`PackageError`] when the package exceeds what a zip without ZIP64 can
/// hold.
///
/// # Examples
///
/// ```no_run
/// use pdfcer_core::document::Document;
/// use pdfcer_core::export::ods::{OdsOptions, write_ods};
/// use pdfcer_core::table_detect::{TableOptions, detect_tables};
/// use pdfcer_core::text_extract::ExtractOptions;
///
/// # fn demo(doc: &Document) -> Result<(), Box<dyn std::error::Error>> {
/// let found = detect_tables(&doc.view(), &ExtractOptions::default(), &TableOptions::default())?;
/// let out = write_ods(&found.tables, &OdsOptions::default())?;
/// std::fs::write("tables.ods", &out.bytes)?;
/// println!("{} ambiguous numbers kept as text", out.report.ambiguous_numbers);
/// # Ok(()) }
/// ```
pub fn write_ods(tables: &[Table], options: &OdsOptions) -> Result<OdsOutput, PackageError> {
    let mut report = OdsReport::default();
    let sheets = group_sheets(tables, options.sheets);
    report.sheets = sheets.len();

    let mut body = String::new();
    let mut widths: Vec<f64> = Vec::new();
    for (name, group) in &sheets {
        sheet(
            &mut body,
            name,
            group,
            options.numbers,
            &mut widths,
            &mut report,
        );
    }

    let mut x = String::from(XML_DECL);
    x.push_str(
        "<office:document-content \
xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" \
xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" \
xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" \
xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" \
xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" \
office:version=\"1.3\"><office:automatic-styles>",
    );
    for (i, w) in widths.iter().enumerate() {
        let _ = write!(
            x,
            "<style:style style:name=\"co{i}\" style:family=\"table-column\">\
<style:table-column-properties style:column-width=\"{w:.2}pt\"/></style:style>"
        );
    }
    // ce1 bold, ce2 wrap, ce3 both; property order per P3 §17.
    for (name, bold, wrap) in [
        ("ce1", true, false),
        ("ce2", false, true),
        ("ce3", true, true),
    ] {
        let _ = write!(
            x,
            "<style:style style:name=\"{name}\" style:family=\"table-cell\">"
        );
        if wrap {
            x.push_str("<style:table-cell-properties fo:wrap-option=\"wrap\"/>");
        }
        if bold {
            x.push_str(
                "<style:text-properties fo:font-weight=\"bold\" \
style:font-weight-asian=\"bold\" style:font-weight-complex=\"bold\"/>",
            );
        }
        x.push_str("</style:style>");
    }
    x.push_str("</office:automatic-styles><office:body><office:spreadsheet>");
    x.push_str(&body);
    x.push_str("</office:spreadsheet></office:body></office:document-content>");

    let mut zip = ZipWriter::new();
    zip.add_stored("mimetype", MIMETYPE.as_bytes())?;
    zip.add("content.xml", x.as_bytes())?;
    zip.add("META-INF/manifest.xml", MANIFEST.as_bytes())?;
    Ok(OdsOutput {
        bytes: zip.finish()?,
        report,
    })
}

const MANIFEST: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\">\
<manifest:file-entry manifest:full-path=\"/\" manifest:version=\"1.3\" manifest:media-type=\"application/vnd.oasis.opendocument.spreadsheet\"/>\
<manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/>\
</manifest:manifest>";

enum Slot {
    Cell {
        paragraphs: String,
        number: Option<String>,
        style: usize,
        rows: usize,
        cols: usize,
    },
    Covered,
}

fn sheet(
    out: &mut String,
    name: &str,
    group: &[&Table],
    locale: NumberLocale,
    widths: &mut Vec<f64>,
    report: &mut OdsReport,
) {
    let mut grid: BTreeMap<(usize, usize), Slot> = BTreeMap::new();
    let mut col_width: BTreeMap<usize, f64> = BTreeMap::new();
    let mut base = 0usize;
    for table in group {
        report.tables += 1;
        report.header_rows += table.header_rows;
        for (c, band) in table.columns.iter().enumerate() {
            let w = band.width().clamp(MIN_WIDTH_PT, MAX_WIDTH_PT);
            let e = col_width.entry(c).or_insert(w);
            *e = e.max(w);
        }
        for cell in &table.cells {
            let row = base + cell.row;
            let rows = cell.row_span.max(1);
            let cols = cell.col_span.max(1);
            if row + rows > MAX_ROWS || cell.col + cols > MAX_COLS {
                report.cells_beyond_limits += 1;
                continue;
            }
            let mut paragraphs = String::new();
            for line in cell.text.split('\n') {
                paragraphs.push_str("<text:p>");
                report.characters_dropped += paragraph_into(&mut paragraphs, line);
                paragraphs.push_str("</text:p>");
            }
            let number = match parse_number(cell.text.trim(), locale) {
                Parsed::Number(v) => {
                    report.numbers += 1;
                    Some(v)
                }
                Parsed::Ambiguous => {
                    report.ambiguous_numbers += 1;
                    None
                }
                Parsed::Text => None,
            };
            if rows > 1 || cols > 1 {
                report.merged_cells += 1;
                for r in row..row + rows {
                    for c in cell.col..cell.col + cols {
                        grid.insert((r, c), Slot::Covered);
                    }
                }
            }
            report.cells += 1;
            let style = usize::from(cell.row < table.header_rows)
                + 2 * usize::from(cell.text.contains('\n'));
            grid.insert(
                (row, cell.col),
                Slot::Cell {
                    paragraphs,
                    number,
                    style,
                    rows,
                    cols,
                },
            );
        }
        base += table.rows.len() + 1;
    }

    let mut n = String::new();
    escape_into(&mut n, name);
    let _ = write!(out, "<table:table table:name=\"{n}\">");
    if col_width.is_empty() {
        out.push_str("<table:table-column/>");
    }
    let last_col = col_width.keys().next_back().copied().unwrap_or(0);
    for c in 0..=last_col {
        if col_width.is_empty() {
            break;
        }
        let w = col_width.get(&c).copied().unwrap_or(MIN_WIDTH_PT);
        let _ = write!(
            out,
            "<table:table-column table:style-name=\"co{}\"/>",
            widths.len()
        );
        widths.push(w);
    }
    let last_row = grid.keys().next_back().map(|&(r, _)| r);
    let Some(last_row) = last_row else {
        out.push_str("<table:table-row><table:table-cell/></table:table-row></table:table>");
        return;
    };
    let mut slots = grid.into_iter().peekable();
    for r in 0..=last_row {
        out.push_str("<table:table-row>");
        let mut next_col = 0usize;
        while let Some(((sr, c), slot)) = slots.next_if(|((sr, _), _)| *sr == r) {
            debug_assert_eq!(sr, r);
            if c > next_col {
                let _ = write!(
                    out,
                    "<table:table-cell table:number-columns-repeated=\"{}\"/>",
                    c - next_col
                );
            }
            next_col = c + 1;
            match slot {
                Slot::Covered => out.push_str("<table:covered-table-cell/>"),
                Slot::Cell {
                    paragraphs,
                    number,
                    style,
                    rows,
                    cols,
                } => {
                    out.push_str("<table:table-cell");
                    if style > 0 {
                        let _ = write!(out, " table:style-name=\"ce{style}\"");
                    }
                    match &number {
                        Some(v) => {
                            let _ =
                                write!(out, " office:value-type=\"float\" office:value=\"{v}\"");
                        }
                        None => out.push_str(" office:value-type=\"string\""),
                    }
                    if cols > 1 {
                        let _ = write!(out, " table:number-columns-spanned=\"{cols}\"");
                    }
                    if rows > 1 {
                        let _ = write!(out, " table:number-rows-spanned=\"{rows}\"");
                    }
                    out.push('>');
                    out.push_str(&paragraphs);
                    out.push_str("</table:table-cell>");
                }
            }
        }
        if next_col == 0 {
            out.push_str("<table:table-cell/>");
        }
        out.push_str("</table:table-row>");
    }
    out.push_str("</table:table>");
}

/// Writes one line of cell text as `text:p` content: XML-escaped, every
/// space ODF would collapse or strip as `<text:s>`, tabs as `<text:tab/>`.
/// Returns how many characters XML cannot carry were dropped.
fn paragraph_into(out: &mut String, line: &str) -> usize {
    let line = line.strip_suffix('\r').unwrap_or(line);
    let mut dropped = 0;
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while let Some(&c) = chars.get(i) {
        if c == ' ' {
            let run = chars.iter().skip(i).take_while(|&&c| c == ' ').count();
            let edge = i == 0 || i + run == chars.len();
            // One literal space survives inside a line; at an edge none do.
            let literal = usize::from(!edge);
            if literal == 1 {
                out.push(' ');
            }
            let escaped = run - literal;
            match escaped {
                0 => {}
                1 => out.push_str("<text:s/>"),
                k => {
                    let _ = write!(out, "<text:s text:c=\"{k}\"/>");
                }
            }
            i += run;
        } else if c == '\t' {
            out.push_str("<text:tab/>");
            i += 1;
        } else {
            let mut buf = [0u8; 4];
            dropped += escape_into(out, c.encode_utf8(&mut buf));
            i += 1;
        }
    }
    dropped
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn para(s: &str) -> String {
        let mut o = String::new();
        paragraph_into(&mut o, s);
        o
    }

    use crate::export::xlsx::tests::table;

    fn content(bytes: &[u8]) -> String {
        let parts = crate::export::ooxml_zip::tests::read_zip(bytes);
        assert_eq!(
            parts[0],
            ("mimetype".to_owned(), MIMETYPE.as_bytes().to_vec())
        );
        assert_eq!(&bytes[8..10], &[0, 0], "mimetype is stored");
        assert!(parts.iter().any(|(n, d)| n == "META-INF/manifest.xml"
            && String::from_utf8_lossy(d).contains("<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\">")));
        let (_, data) = parts.iter().find(|(n, _)| n == "content.xml").unwrap();
        String::from_utf8(data.clone()).unwrap()
    }

    #[test]
    fn a_table_becomes_a_sheet_with_the_xlsx_number_rule() {
        let t = table(
            0,
            &[
                (0, 0, 1, 2, "Parts & list"),
                (1, 0, 1, 1, "Part"),
                (1, 1, 1, 1, "Qty"),
                (
                    2, 0, 1, 1, "Bolt
M6",
                ),
                (2, 1, 1, 1, "1,234"),
                (3, 0, 1, 1, "Nut"),
                (3, 1, 1, 1, "(12.5)"),
            ],
            2,
        );
        let out = write_ods(std::slice::from_ref(&t), &OdsOptions::default()).unwrap();
        let r = out.report;
        assert_eq!((r.sheets, r.tables, r.cells), (1, 1, 7));
        assert_eq!((r.merged_cells, r.header_rows), (1, 2));
        assert_eq!((r.numbers, r.ambiguous_numbers), (1, 1));
        let x = content(&out.bytes);
        assert!(x.contains("<table:table table:name=\"Table 1\"><table:table-column table:style-name=\"co0\"/><table:table-column table:style-name=\"co1\"/><table:table-row>"), "{x}");
        assert!(x.contains("style:column-width=\"105.00pt\""), "{x}");
        assert!(x.contains(
            "<table:table-row><table:table-cell table:style-name=\"ce1\" office:value-type=\"string\" table:number-columns-spanned=\"2\"><text:p>Parts &amp; list</text:p></table:table-cell><table:covered-table-cell/></table:table-row>"
        ), "{x}");
        assert!(x.contains(
            "<table:table-cell table:style-name=\"ce2\" office:value-type=\"string\"><text:p>Bolt</text:p><text:p>M6</text:p>"
        ), "{x}");
        assert!(
            x.contains("<table:table-cell office:value-type=\"string\"><text:p>1,234</text:p>"),
            "{x}"
        );
        assert!(x.contains(
            "<table:table-cell office:value-type=\"float\" office:value=\"-12.5\"><text:p>(12.5)</text:p>"
        ), "{x}");
        let us = write_ods(&[t], &OdsOptions::default().with_numbers(NumberLocale::Us)).unwrap();
        assert_eq!((us.report.numbers, us.report.ambiguous_numbers), (2, 0));
        assert!(content(&us.bytes).contains("office:value=\"1234\""));
    }

    #[test]
    fn a_row_merge_covers_the_rows_below_and_gaps_are_filled() {
        let t = table(
            0,
            &[(0, 0, 2, 1, "tall"), (0, 2, 1, 1, "c"), (1, 1, 1, 1, "b")],
            0,
        );
        let x = content(&write_ods(&[t], &OdsOptions::default()).unwrap().bytes);
        assert!(x.contains(
            "<table:table-row><table:table-cell office:value-type=\"string\" table:number-rows-spanned=\"2\"><text:p>tall</text:p></table:table-cell><table:table-cell table:number-columns-repeated=\"1\"/><table:table-cell office:value-type=\"string\"><text:p>c</text:p></table:table-cell></table:table-row><table:table-row><table:covered-table-cell/><table:table-cell office:value-type=\"string\"><text:p>b</text:p>"
        ), "{x}");
    }

    #[test]
    fn sheet_layouts_group_tables_and_stack_with_a_blank_row() {
        let a = table(0, &[(0, 0, 1, 1, "a")], 0);
        let b = table(0, &[(0, 0, 1, 1, "b")], 0);
        let c = table(2, &[(0, 0, 1, 1, "c")], 0);
        let all = [a, b, c];
        assert_eq!(
            write_ods(&all, &OdsOptions::default())
                .unwrap()
                .report
                .sheets,
            3
        );
        let per_page = write_ods(
            &all,
            &OdsOptions::default().with_sheets(SheetLayout::PerPage),
        )
        .unwrap();
        assert_eq!(per_page.report.sheets, 2);
        let x = content(&per_page.bytes);
        assert!(
            x.contains("table:name=\"Page 1\"") && x.contains("table:name=\"Page 3\""),
            "{x}"
        );
        assert!(x.contains(
            "<text:p>a</text:p></table:table-cell></table:table-row><table:table-row><table:table-cell/></table:table-row><table:table-row><table:table-cell office:value-type=\"string\"><text:p>b</text:p>"
        ), "{x}");
        let none = write_ods(&[], &OdsOptions::default()).unwrap();
        assert_eq!((none.report.sheets, none.report.tables), (1, 0));
        assert!(content(&none.bytes).contains(
            "<table:table table:name=\"Tables\"><table:table-column/><table:table-row><table:table-cell/></table:table-row></table:table>"
        ));
    }

    #[test]
    fn output_is_deterministic() {
        let t = table(0, &[(0, 0, 1, 1, "x")], 0);
        let a = write_ods(std::slice::from_ref(&t), &OdsOptions::default()).unwrap();
        assert_eq!(
            a.bytes,
            write_ods(&[t], &OdsOptions::default()).unwrap().bytes
        );
    }

    #[test]
    fn spaces_that_odf_would_collapse_or_strip_are_escaped() {
        assert_eq!(para("a b"), "a b");
        assert_eq!(para("a   b"), "a <text:s text:c=\"2\"/>b"); // string-gap-exempt: a run of spaces is the input under test
        assert_eq!(para(" a "), "<text:s/>a<text:s/>");
        assert_eq!(para("a\tb<"), "a<text:tab/>b&lt;");
        assert_eq!(para("  "), "<text:s text:c=\"2\"/>");
    }
}
