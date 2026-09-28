//! # XLSX export — detected tables written as an Excel workbook
//!
//! Writes a SpreadsheetML package (ECMA-376 Part 1 / ISO/IEC 29500-1 §18)
//! from [`crate::table_detect::Table`]s: one cell per detected cell, merged
//! cells as `<mergeCells>` with the text in the top-left cell, the header
//! row bold, multi-line cells wrapped, column widths from the column bands.
//!
//! ## Numbers
//!
//! A cell becomes a number only when its reading is certain. The failure
//! this guards against is a silent wrong value: `1.234` is 1.234 in the US
//! and 1234 in most of Europe, and nothing in the cell says which.
//! Under [`NumberLocale::Auto`] a cell whose value depends on the locale
//! stays text and is counted in [`XlsxReport::ambiguous_numbers`]; pass
//! [`NumberLocale::Us`] or [`NumberLocale::European`] to decide. Text that
//! a number would change is never converted, under any locale: a leading
//! zero (`007`) or more than 15 significant digits (Excel's precision).

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::ooxml_zip::{PackageError, ZipWriter, escape_into};
use crate::table_detect::Table;

/// Which tables share a worksheet.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SheetLayout {
    /// One worksheet per table.
    #[default]
    PerTable,
    /// One worksheet per page with a table; its tables stacked with a
    /// blank row between.
    PerPage,
    /// Every table on one worksheet, stacked with a blank row between.
    Single,
}

/// How a cell's digits and separators are read as a number.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NumberLocale {
    /// A number only when every locale reads it the same way.
    #[default]
    Auto,
    /// `,` groups thousands, `.` is the decimal point.
    Us,
    /// `.` groups thousands, `,` is the decimal point.
    European,
    /// Every cell is text.
    Off,
}

/// Options for [`write_xlsx`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct XlsxOptions {
    /// Which tables share a worksheet.
    pub sheets: SheetLayout,
    /// How numbers are read.
    pub numbers: NumberLocale,
}

impl XlsxOptions {
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

/// What [`write_xlsx`] wrote, and what it changed or left out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct XlsxReport {
    /// Worksheets written (at least 1; an empty one when there are no
    /// tables).
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
    /// Cells cut to Excel's 32 767-character limit.
    pub cells_truncated: usize,
    /// Cells left out because they fall past Excel's last row or column.
    pub cells_beyond_limits: usize,
}

/// A written workbook.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct XlsxOutput {
    /// The `.xlsx` file.
    pub bytes: Vec<u8>,
    /// What was written.
    pub report: XlsxReport,
}

const MAX_ROWS: usize = 1_048_576;
const MAX_COLS: usize = 16_384;
const MAX_CELL_CHARS: usize = 32_767;
/// Points per unit of Excel column width (one digit of 11pt Calibri).
const POINTS_PER_WIDTH: f64 = 5.25;

const NS_MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const NS_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const XML_DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";

/// Writes `tables` as an `.xlsx` workbook.
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
/// use pdfcer_core::export::xlsx::{XlsxOptions, write_xlsx};
/// use pdfcer_core::table_detect::{TableOptions, detect_tables};
/// use pdfcer_core::text_extract::ExtractOptions;
///
/// # fn demo(doc: &Document) -> Result<(), Box<dyn std::error::Error>> {
/// let found = detect_tables(&doc.view(), &ExtractOptions::default(), &TableOptions::default())?;
/// let out = write_xlsx(&found.tables, &XlsxOptions::default())?;
/// std::fs::write("tables.xlsx", &out.bytes)?;
/// println!("{} ambiguous numbers kept as text", out.report.ambiguous_numbers);
/// # Ok(()) }
/// ```
pub fn write_xlsx(tables: &[Table], options: &XlsxOptions) -> Result<XlsxOutput, PackageError> {
    let mut report = XlsxReport::default();
    let mut sheets: Vec<(String, Vec<&Table>)> = Vec::new();
    for (i, t) in tables.iter().enumerate() {
        match options.sheets {
            SheetLayout::PerTable => sheets.push((format!("Table {}", i + 1), vec![t])),
            SheetLayout::PerPage => {
                let name = format!("Page {}", t.page_index + 1);
                match sheets.last_mut() {
                    Some((n, group)) if *n == name => group.push(t),
                    _ => sheets.push((name, vec![t])),
                }
            }
            SheetLayout::Single => match sheets.last_mut() {
                Some((_, group)) => group.push(t),
                None => sheets.push(("Tables".to_owned(), vec![t])),
            },
        }
    }
    if sheets.is_empty() {
        sheets.push(("Tables".to_owned(), Vec::new()));
    }
    report.sheets = sheets.len();

    let mut zip = ZipWriter::new();
    zip.add(
        "[Content_Types].xml",
        content_types(sheets.len()).as_bytes(),
    )?;
    zip.add("_rels/.rels", ROOT_RELS.as_bytes())?;
    zip.add("xl/workbook.xml", workbook(&sheets).as_bytes())?;
    zip.add(
        "xl/_rels/workbook.xml.rels",
        workbook_rels(sheets.len()).as_bytes(),
    )?;
    zip.add("xl/styles.xml", STYLES.as_bytes())?;
    for (i, (_, group)) in sheets.iter().enumerate() {
        let xml = worksheet(group, options.numbers, &mut report);
        zip.add(&format!("xl/worksheets/sheet{}.xml", i + 1), xml.as_bytes())?;
    }
    Ok(XlsxOutput {
        bytes: zip.finish()?,
        report,
    })
}

struct CellOut {
    text: String,
    number: Option<String>,
    bold: bool,
    wrap: bool,
}

fn worksheet(group: &[&Table], locale: NumberLocale, report: &mut XlsxReport) -> String {
    let mut cells: BTreeMap<(usize, usize), CellOut> = BTreeMap::new();
    let mut merges: Vec<String> = Vec::new();
    let mut widths: BTreeMap<usize, f64> = BTreeMap::new();
    let mut base = 0usize;
    for table in group {
        report.tables += 1;
        report.header_rows += table.header_rows;
        for (c, band) in table.columns.iter().enumerate() {
            let w = (band.width() / POINTS_PER_WIDTH).clamp(3.0, 80.0);
            let e = widths.entry(c).or_insert(w);
            *e = e.max(w);
        }
        for cell in &table.cells {
            let row = base + cell.row;
            let last_row = row + cell.row_span.max(1) - 1;
            let last_col = cell.col + cell.col_span.max(1) - 1;
            if last_row >= MAX_ROWS || last_col >= MAX_COLS {
                report.cells_beyond_limits += 1;
                continue;
            }
            let mut text = String::new();
            let raw: String = cell.text.chars().take(MAX_CELL_CHARS).collect();
            if raw.len() < cell.text.len() {
                report.cells_truncated += 1;
            }
            report.characters_dropped += escape_into(&mut text, &raw);
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
            if last_row > row || last_col > cell.col {
                report.merged_cells += 1;
                merges.push(format!(
                    "{}:{}",
                    cell_ref(row, cell.col),
                    cell_ref(last_row, last_col)
                ));
            }
            report.cells += 1;
            cells.insert(
                (row, cell.col),
                CellOut {
                    wrap: cell.text.contains('\n'),
                    bold: cell.row < table.header_rows,
                    text,
                    number,
                },
            );
        }
        base += table.rows.len() + 1;
    }

    let mut x = String::from(XML_DECL);
    let _ = write!(x, "<worksheet xmlns=\"{NS_MAIN}\">");
    if !widths.is_empty() {
        x.push_str("<cols>");
        for (c, w) in &widths {
            let _ = write!(
                x,
                "<col min=\"{n}\" max=\"{n}\" width=\"{w:.2}\" customWidth=\"1\"/>",
                n = c + 1
            );
        }
        x.push_str("</cols>");
    }
    x.push_str("<sheetData>");
    let mut open_row: Option<usize> = None;
    for (&(r, c), cell) in &cells {
        if open_row != Some(r) {
            if open_row.is_some() {
                x.push_str("</row>");
            }
            let _ = write!(x, "<row r=\"{}\">", r + 1);
            open_row = Some(r);
        }
        let style = usize::from(cell.bold) + 2 * usize::from(cell.wrap);
        let s = if style == 0 {
            String::new()
        } else {
            format!(" s=\"{style}\"")
        };
        match &cell.number {
            Some(v) => {
                let _ = write!(x, "<c r=\"{}\"{s}><v>{v}</v></c>", cell_ref(r, c));
            }
            None => {
                let _ = write!(
                    x,
                    "<c r=\"{}\"{s} t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>",
                    cell_ref(r, c),
                    cell.text
                );
            }
        }
    }
    if open_row.is_some() {
        x.push_str("</row>");
    }
    x.push_str("</sheetData>");
    if !merges.is_empty() {
        let _ = write!(x, "<mergeCells count=\"{}\">", merges.len());
        for m in &merges {
            let _ = write!(x, "<mergeCell ref=\"{m}\"/>");
        }
        x.push_str("</mergeCells>");
    }
    x.push_str("</worksheet>");
    x
}

/// `A1`-style reference, zero-based inputs.
fn cell_ref(row: usize, col: usize) -> String {
    let mut letters = Vec::new();
    let mut n = col + 1;
    while n > 0 {
        let rem = (n - 1) % 26;
        letters.push(char::from(b'A' + u8::try_from(rem).unwrap_or(0)));
        n = (n - 1) / 26;
    }
    letters.iter().rev().collect::<String>() + &(row + 1).to_string()
}

#[derive(Debug, PartialEq, Eq)]
enum Parsed {
    /// The canonical `xsd:double` lexical form.
    Number(String),
    Ambiguous,
    Text,
}

/// Reads `s` as a number. `(1,234.00)` and a leading `-`/`+` give the sign.
fn parse_number(s: &str, locale: NumberLocale) -> Parsed {
    if locale == NumberLocale::Off || s.is_empty() {
        return Parsed::Text;
    }
    let (negative, body) =
        if let Some(inner) = s.strip_prefix('(').and_then(|r| r.strip_suffix(')')) {
            (true, inner)
        } else if let Some(rest) = s.strip_prefix('-') {
            (true, rest)
        } else {
            (false, s.strip_prefix('+').unwrap_or(s))
        };
    let bytes = body.as_bytes();
    let (Some(first), Some(last)) = (bytes.first(), bytes.last()) else {
        return Parsed::Text;
    };
    if !first.is_ascii_digit()
        || !last.is_ascii_digit()
        || !bytes
            .iter()
            .all(|b| b.is_ascii_digit() || *b == b'.' || *b == b',')
    {
        return Parsed::Text;
    }
    let dots = body.matches('.').count();
    let commas = body.matches(',').count();
    // (group separator, decimal separator) under which to validate.
    let (group, decimal) = match locale {
        NumberLocale::Us => (',', '.'),
        NumberLocale::European => ('.', ','),
        NumberLocale::Off => return Parsed::Text,
        NumberLocale::Auto => {
            if dots > 0 && commas > 0 {
                // Whichever comes last is the decimal separator.
                if body.rfind('.') > body.rfind(',') {
                    (',', '.')
                } else {
                    ('.', ',')
                }
            } else if dots + commas == 0 {
                (',', '.')
            } else if dots + commas > 1 {
                // A separator used twice can only group.
                if dots > 0 { ('.', ',') } else { (',', '.') }
            } else {
                let sep = if dots == 1 { '.' } else { ',' };
                let (int, frac) = body.split_once(sep).unwrap_or((body, ""));
                if frac.len() == 3 && int.len() <= 3 && !int.starts_with('0') {
                    return Parsed::Ambiguous;
                }
                if sep == '.' { (',', '.') } else { ('.', ',') }
            }
        }
    };
    let (int, frac) = match body.split_once(decimal) {
        Some((i, f)) => (i, Some(f)),
        None => (body, None),
    };
    if frac.is_some_and(|f| f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit())) {
        return Parsed::Text;
    }
    let groups: Vec<&str> = int.split(group).collect();
    if groups.len() > 1 {
        let Some((head, rest)) = groups.split_first() else {
            return Parsed::Text;
        };
        if head.is_empty() || head.len() > 3 || rest.iter().any(|g| g.len() != 3) {
            return Parsed::Text;
        }
    }
    let digits: String = groups.concat();
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Parsed::Text;
    }
    if digits.len() > 1 && digits.starts_with('0') {
        return Parsed::Text;
    }
    let significant = digits.trim_start_matches('0').len() + frac.map_or(0, str::len);
    if significant > 15 {
        return Parsed::Text;
    }
    let mut v = String::new();
    if negative {
        v.push('-');
    }
    v.push_str(&digits);
    if let Some(f) = frac {
        v.push('.');
        v.push_str(f);
    }
    Parsed::Number(v)
}

fn content_types(sheets: usize) -> String {
    let mut x = String::from(XML_DECL);
    x.push_str(
        "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
<Default Extension=\"xml\" ContentType=\"application/xml\"/>\
<Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>\
<Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/>",
    );
    for i in 1..=sheets {
        let _ = write!(
            x,
            "<Override PartName=\"/xl/worksheets/sheet{i}.xml\" \
ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>"
        );
    }
    x.push_str("</Types>");
    x
}

const ROOT_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/>\
</Relationships>";

fn workbook(sheets: &[(String, Vec<&Table>)]) -> String {
    let mut x = String::from(XML_DECL);
    let _ = write!(
        x,
        "<workbook xmlns=\"{NS_MAIN}\" xmlns:r=\"{NS_REL}\"><sheets>"
    );
    for (i, (name, _)) in sheets.iter().enumerate() {
        let _ = write!(
            x,
            "<sheet name=\"{name}\" sheetId=\"{n}\" r:id=\"rId{n}\"/>",
            n = i + 1
        );
    }
    x.push_str("</sheets></workbook>");
    x
}

fn workbook_rels(sheets: usize) -> String {
    let mut x = String::from(XML_DECL);
    x.push_str(
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">",
    );
    for i in 1..=sheets {
        let _ = write!(
            x,
            "<Relationship Id=\"rId{i}\" Type=\"{NS_REL}/worksheet\" Target=\"worksheets/sheet{i}.xml\"/>"
        );
    }
    let _ = write!(
        x,
        "<Relationship Id=\"rId{}\" Type=\"{NS_REL}/styles\" Target=\"styles.xml\"/></Relationships>",
        sheets + 1
    );
    x
}

/// Cell formats: 0 plain, 1 bold, 2 wrapped, 3 bold and wrapped.
const STYLES: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">\
<fonts count=\"2\"><font><sz val=\"11\"/><name val=\"Calibri\"/></font>\
<font><b/><sz val=\"11\"/><name val=\"Calibri\"/></font></fonts>\
<fills count=\"2\"><fill><patternFill patternType=\"none\"/></fill>\
<fill><patternFill patternType=\"gray125\"/></fill></fills>\
<borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders>\
<cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs>\
<cellXfs count=\"4\">\
<xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/>\
<xf numFmtId=\"0\" fontId=\"1\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyFont=\"1\"/>\
<xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyAlignment=\"1\"><alignment vertical=\"top\" wrapText=\"1\"/></xf>\
<xf numFmtId=\"0\" fontId=\"1\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyFont=\"1\" applyAlignment=\"1\"><alignment vertical=\"top\" wrapText=\"1\"/></xf>\
</cellXfs>\
<cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles>\
</styleSheet>";

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn n(s: &str) -> Parsed {
        Parsed::Number(s.to_owned())
    }

    #[test]
    fn auto_parses_only_locale_independent_numbers() {
        let a = NumberLocale::Auto;
        assert_eq!(parse_number("42", a), n("42"));
        assert_eq!(parse_number("3.5", a), n("3.5"));
        assert_eq!(parse_number("3,5", a), n("3.5"));
        assert_eq!(parse_number("1,234.56", a), n("1234.56"));
        assert_eq!(parse_number("1.234,56", a), n("1234.56"));
        assert_eq!(parse_number("1,234,567", a), n("1234567"));
        assert_eq!(parse_number("1.234.567", a), n("1234567"));
        assert_eq!(parse_number("0.125", a), n("0.125"));
        assert_eq!(parse_number("(12.50)", a), n("-12.50"));
        assert_eq!(parse_number("-7", a), n("-7"));
        assert_eq!(parse_number("1,234", a), Parsed::Ambiguous);
        assert_eq!(parse_number("1.234", a), Parsed::Ambiguous);
    }

    #[test]
    fn a_locale_decides_the_ambiguous_case() {
        assert_eq!(parse_number("1.234", NumberLocale::Us), n("1.234"));
        assert_eq!(parse_number("1.234", NumberLocale::European), n("1234"));
        assert_eq!(parse_number("1,234", NumberLocale::Us), n("1234"));
        assert_eq!(parse_number("1,234", NumberLocale::European), n("1.234"));
        assert_eq!(
            parse_number("1,234.5", NumberLocale::European),
            Parsed::Text
        );
        assert_eq!(parse_number("42", NumberLocale::Off), Parsed::Text);
    }

    #[test]
    fn text_a_number_would_change_stays_text() {
        let a = NumberLocale::Auto;
        assert_eq!(parse_number("007", a), Parsed::Text);
        assert_eq!(parse_number("1234567890123456", a), Parsed::Text);
        assert_eq!(parse_number("12,34", NumberLocale::Us), Parsed::Text);
        assert_eq!(parse_number("$5", a), Parsed::Text);
        assert_eq!(parse_number("1.", a), Parsed::Text);
        assert_eq!(parse_number("A1", a), Parsed::Text);
    }

    fn table(
        page: usize,
        cells: &[(usize, usize, usize, usize, &str)],
        header_rows: usize,
    ) -> Table {
        use crate::table_detect::{BoundarySource, TableCell};
        use pdfcer_model::page_tree::Rect;
        let rows = cells.iter().map(|c| c.0 + c.2).max().unwrap_or(0);
        let cols = cells.iter().map(|c| c.1 + c.3).max().unwrap_or(0);
        let r = Rect::from_corners(0.0, 0.0, 10.0, 10.0);
        Table {
            page_index: page,
            bbox: r,
            source: BoundarySource::Ruled,
            rows: vec![r; rows],
            columns: vec![Rect::from_corners(0.0, 0.0, 105.0, 10.0); cols],
            cells: cells
                .iter()
                .map(|&(row, col, row_span, col_span, text)| TableCell {
                    row,
                    col,
                    row_span,
                    col_span,
                    bbox: r,
                    glyphs: Vec::new(),
                    text: text.to_owned(),
                })
                .collect(),
            header_rows,
            header_evidence: None,
        }
    }

    fn part(bytes: &[u8], name: &str) -> String {
        let parts = crate::export::ooxml_zip::tests::read_zip(bytes);
        let (_, data) = parts.iter().find(|(n, _)| n == name).unwrap();
        String::from_utf8(data.clone()).unwrap()
    }

    #[test]
    fn a_table_becomes_a_worksheet() {
        let t = table(
            0,
            &[
                (0, 0, 1, 2, "Parts & list"),
                (1, 0, 1, 1, "Part"),
                (1, 1, 1, 1, "Qty"),
                (2, 0, 1, 1, "Bolt\nM6"),
                (2, 1, 1, 1, "1,234"),
                (3, 0, 1, 1, "Nut"),
                (3, 1, 1, 1, "(12.5)"),
            ],
            2,
        );
        let out = write_xlsx(std::slice::from_ref(&t), &XlsxOptions::default()).unwrap();
        let r = out.report;
        assert_eq!((r.sheets, r.tables, r.cells), (1, 1, 7));
        assert_eq!((r.merged_cells, r.header_rows), (1, 2));
        assert_eq!((r.numbers, r.ambiguous_numbers), (1, 1));
        let sheet = part(&out.bytes, "xl/worksheets/sheet1.xml");
        assert!(sheet.contains("<mergeCell ref=\"A1:B1\"/>"), "{sheet}");
        assert!(sheet.contains("<c r=\"A1\" s=\"1\" t=\"inlineStr\"><is><t xml:space=\"preserve\">Parts &amp; list</t>"), "{sheet}");
        assert!(
            sheet.contains("<c r=\"B2\" s=\"1\" t=\"inlineStr\">"),
            "{sheet}"
        );
        assert!(
            sheet.contains(
                "<c r=\"A3\" s=\"2\" t=\"inlineStr\"><is><t xml:space=\"preserve\">Bolt\nM6</t>"
            ),
            "{sheet}"
        );
        assert!(
            sheet.contains("<c r=\"B3\" t=\"inlineStr\"><is><t xml:space=\"preserve\">1,234</t>"),
            "{sheet}"
        );
        assert!(sheet.contains("<c r=\"B4\"><v>-12.5</v></c>"), "{sheet}");
        assert!(
            sheet.contains("<col min=\"1\" max=\"1\" width=\"20.00\" customWidth=\"1\"/>"),
            "{sheet}"
        );
        let wb = part(&out.bytes, "xl/workbook.xml");
        assert!(
            wb.contains("<sheet name=\"Table 1\" sheetId=\"1\" r:id=\"rId1\"/>"),
            "{wb}"
        );
        let us = write_xlsx(&[t], &XlsxOptions::default().with_numbers(NumberLocale::Us)).unwrap();
        assert_eq!((us.report.numbers, us.report.ambiguous_numbers), (2, 0));
        assert!(
            part(&us.bytes, "xl/worksheets/sheet1.xml").contains("<c r=\"B3\"><v>1234</v></c>")
        );
    }

    #[test]
    fn sheet_layouts_group_tables() {
        let a = table(0, &[(0, 0, 1, 1, "a")], 0);
        let b = table(0, &[(0, 0, 1, 1, "b")], 0);
        let c = table(2, &[(0, 0, 1, 1, "c")], 0);
        let all = [a, b, c];
        let per_table = write_xlsx(&all, &XlsxOptions::default()).unwrap();
        assert_eq!(per_table.report.sheets, 3);
        let per_page = write_xlsx(
            &all,
            &XlsxOptions::default().with_sheets(SheetLayout::PerPage),
        )
        .unwrap();
        assert_eq!(per_page.report.sheets, 2);
        let wb = part(&per_page.bytes, "xl/workbook.xml");
        assert!(
            wb.contains("name=\"Page 1\"") && wb.contains("name=\"Page 3\""),
            "{wb}"
        );
        // Stacked: one-row table, blank row, next table.
        let s1 = part(&per_page.bytes, "xl/worksheets/sheet1.xml");
        assert!(
            s1.contains("<row r=\"1\">") && s1.contains("<row r=\"3\"><c r=\"A3\""),
            "{s1}"
        );
        let single = write_xlsx(
            &all,
            &XlsxOptions::default().with_sheets(SheetLayout::Single),
        )
        .unwrap();
        assert_eq!(single.report.sheets, 1);
        assert!(part(&single.bytes, "xl/worksheets/sheet1.xml").contains("<c r=\"A5\""));
        let none = write_xlsx(&[], &XlsxOptions::default()).unwrap();
        assert_eq!((none.report.sheets, none.report.tables), (1, 0));
        assert!(part(&none.bytes, "xl/worksheets/sheet1.xml").contains("<sheetData></sheetData>"));
    }

    #[test]
    fn output_is_deterministic() {
        let t = table(0, &[(0, 0, 1, 1, "x")], 0);
        let a = write_xlsx(std::slice::from_ref(&t), &XlsxOptions::default()).unwrap();
        let b = write_xlsx(&[t], &XlsxOptions::default()).unwrap();
        assert_eq!(a.bytes, b.bytes);
    }

    #[test]
    fn cell_refs() {
        assert_eq!(cell_ref(0, 0), "A1");
        assert_eq!(cell_ref(9, 25), "Z10");
        assert_eq!(cell_ref(0, 26), "AA1");
        assert_eq!(cell_ref(0, 701), "ZZ1");
        assert_eq!(cell_ref(0, 702), "AAA1");
    }
}
