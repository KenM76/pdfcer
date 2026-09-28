//! # DOCX export — the document's text as a Word file
//!
//! Writes a WordprocessingML package (ECMA-376 Part 1 / ISO/IEC 29500-1
//! §17) from a [`DocumentLayout`] and, optionally, detected [`Table`]s.
//! The text flows: one Word paragraph per layout block in reading order,
//! columns read one after another, a page break between PDF pages. It
//! does not reproduce the page's positions.
//!
//! | Layout block | Word |
//! |---|---|
//! | [`BlockKind::Heading`] level n | style `Heading n` |
//! | [`BlockKind::Paragraph`] | `Normal`, with the block's alignment and first-line or hanging indent |
//! | [`BlockKind::ListItem`] | `List Paragraph`, marker kept as text, hanging indent |
//! | [`BlockKind::Caption`] | `Caption` |
//! | [`BlockKind::RunningHeader`] | the page header, written once |
//! | [`BlockKind::RunningFooter`] | the page footer, written once |
//! | [`BlockKind::PageNumber`] | a `PAGE` field in the header or footer |
//! | a block whose centre is inside a table | replaced by that table |
//!
//! Running text goes into Word's header and footer parts rather than the
//! body, so it repeats on Word's pages instead of appearing mid-text at
//! each old page boundary. A document whose running text changes (odd and
//! even pages, a chapter title) keeps the first variant and counts the
//! rest in [`DocxReport::running_variants_dropped`].
//!
//! Every block kind is an inference unless a tag decided it; the report
//! counts what was carried so a caller can disclose it (`CLAUDE.md`
//! rule 4).

use std::collections::BTreeSet;
use std::fmt::Write as _;

use super::ooxml_zip::{PackageError, ZipWriter, escape_into};
use crate::block_layout::{
    Alignment, Block, BlockKind, BlockSource, DocumentLayout, PageGeometry, PageLayout,
};
use crate::page_tree::Rect;
use crate::table_detect::Table;

/// Options for [`write_docx`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct DocxOptions {
    /// A page break between PDF pages. Default `true`.
    pub page_breaks: bool,
    /// Write detected tables as Word tables in place of their text.
    /// Default `true`.
    pub tables: bool,
}

impl Default for DocxOptions {
    fn default() -> Self {
        Self {
            page_breaks: true,
            tables: true,
        }
    }
}

impl DocxOptions {
    /// Sets [`Self::page_breaks`].
    #[must_use]
    pub const fn with_page_breaks(mut self, page_breaks: bool) -> Self {
        self.page_breaks = page_breaks;
        self
    }

    /// Sets [`Self::tables`].
    #[must_use]
    pub const fn with_tables(mut self, tables: bool) -> Self {
        self.tables = tables;
        self
    }
}

/// What [`write_docx`] wrote, and what it moved or left out.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct DocxReport {
    /// PDF pages written.
    pub pages: usize,
    /// Heading paragraphs.
    pub headings: usize,
    /// Body paragraphs.
    pub paragraphs: usize,
    /// List-item paragraphs.
    pub list_items: usize,
    /// Caption paragraphs.
    pub captions: usize,
    /// Blocks written (body, header and footer) whose kind was inferred
    /// rather than tagged.
    pub inferred_blocks: usize,
    /// Tables written.
    pub tables: usize,
    /// Table cells written.
    pub table_cells: usize,
    /// Table cells written with a row or column span.
    pub merged_cells: usize,
    /// Blocks left out of the body because a table carries their text.
    pub blocks_in_tables: usize,
    /// Tables wider than Word's 63 columns, left as text.
    pub tables_too_wide: usize,
    /// Whether a page header was written.
    pub header: bool,
    /// Whether a page footer was written.
    pub footer: bool,
    /// Whether the page number became a `PAGE` field.
    pub page_number_field: bool,
    /// Running header, footer and page-number blocks, across all pages,
    /// represented by the header and footer rather than the body.
    pub running_blocks: usize,
    /// Running texts different from the one written (digits aside), left
    /// out.
    pub running_variants_dropped: usize,
    /// Characters XML cannot carry (control characters), dropped.
    pub characters_dropped: usize,
}

/// A written Word document.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DocxOutput {
    /// The `.docx` file.
    pub bytes: Vec<u8>,
    /// What was written.
    pub report: DocxReport,
}

/// Word's column limit per table.
const MAX_TABLE_COLUMNS: usize = 63;
/// Page margin, twips (1 inch).
const MARGIN: i64 = 1440;
/// Indent of a list paragraph, twips (0.5 inch).
const LIST_INDENT: i64 = 720;

const NS_W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const XML_DECL: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";

/// Writes `layout` as a `.docx` document.
///
/// `geometry[i]` belongs to `layout.pages[i]`, as for
/// [`crate::block_layout::layout_text`]; the Word page size is the first
/// page's crop box as displayed, US Letter when `geometry` is empty.
/// `tables` are the document's detected tables; only those on a laid-out
/// page are used, and none when [`DocxOptions::tables`] is off.
///
/// # Errors
///
/// [`PackageError`] when the package exceeds what a zip without ZIP64 can
/// hold.
///
/// # Examples
///
/// ```no_run
/// use pdfcer_core::block_layout::{LayoutOptions, PageGeometry, analyze_layout};
/// use pdfcer_core::document::Document;
/// use pdfcer_core::export::docx::{DocxOptions, write_docx};
/// use pdfcer_core::page_tree::pages_in;
/// use pdfcer_core::table_detect::{TableOptions, detect_tables};
/// use pdfcer_core::text_extract::ExtractOptions;
///
/// # fn demo(doc: &Document) -> Result<(), Box<dyn std::error::Error>> {
/// let view = doc.view();
/// let layout = analyze_layout(&view, &ExtractOptions::default(), &LayoutOptions::default())?;
/// let pages = pages_in(&view)?;
/// let geometry: Vec<PageGeometry> = layout
///     .pages
///     .iter()
///     .filter_map(|p| pages.get(p.page_index))
///     .map(|p| PageGeometry::new(p.crop_box, p.rotate))
///     .collect();
/// let tables = detect_tables(&view, &ExtractOptions::default(), &TableOptions::default())?;
/// let out = write_docx(&layout, &geometry, &tables.tables, &DocxOptions::default())?;
/// std::fs::write("out.docx", &out.bytes)?;
/// # Ok(()) }
/// ```
pub fn write_docx(
    layout: &DocumentLayout,
    geometry: &[PageGeometry],
    tables: &[Table],
    options: &DocxOptions,
) -> Result<DocxOutput, PackageError> {
    let mut report = DocxReport {
        pages: layout.pages.len(),
        ..DocxReport::default()
    };
    let body_size = layout.diagnostics.body_font_size;
    let running = collect_running(layout, geometry, &mut report);

    let mut body = String::new();
    for (i, page) in layout.pages.iter().enumerate() {
        if i > 0 && options.page_breaks {
            body.push_str("<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>");
        }
        let geom = geometry.get(i).copied();
        let page_tables: Vec<&Table> = if options.tables {
            tables
                .iter()
                .filter(|t| t.page_index == page.page_index)
                .collect()
        } else {
            Vec::new()
        };
        write_page(&mut body, page, geom, &page_tables, body_size, &mut report);
    }

    let (width, height) = page_size(geometry.first().copied());
    let mut document = format!(
        "{XML_DECL}<w:document xmlns:w=\"{NS_W}\" xmlns:r=\"{NS_R}\"><w:body>{body}<w:sectPr>"
    );
    if running.header.is_some() {
        document.push_str("<w:headerReference w:type=\"default\" r:id=\"rIdHeader\"/>");
    }
    if running.footer.is_some() {
        document.push_str("<w:footerReference w:type=\"default\" r:id=\"rIdFooter\"/>");
    }
    let orient = if width > height {
        " w:orient=\"landscape\""
    } else {
        ""
    };
    let _ = write!(
        document,
        "<w:pgSz w:w=\"{width}\" w:h=\"{height}\"{orient}/>\
         <w:pgMar w:top=\"{MARGIN}\" w:right=\"{MARGIN}\" w:bottom=\"{MARGIN}\" \
         w:left=\"{MARGIN}\" w:header=\"720\" w:footer=\"720\" w:gutter=\"0\"/>\
         </w:sectPr></w:body></w:document>"
    );

    let mut zip = ZipWriter::new();
    zip.add(
        "[Content_Types].xml",
        content_types(running.header.is_some(), running.footer.is_some()).as_bytes(),
    )?;
    zip.add("_rels/.rels", ROOT_RELS.as_bytes())?;
    zip.add("word/document.xml", document.as_bytes())?;
    zip.add(
        "word/_rels/document.xml.rels",
        document_rels(running.header.is_some(), running.footer.is_some()).as_bytes(),
    )?;
    zip.add("word/styles.xml", styles(body_size).as_bytes())?;
    if let Some(xml) = &running.header {
        zip.add("word/header1.xml", part("hdr", xml).as_bytes())?;
    }
    if let Some(xml) = &running.footer {
        zip.add("word/footer1.xml", part("ftr", xml).as_bytes())?;
    }
    Ok(DocxOutput {
        bytes: zip.finish()?,
        report,
    })
}

/// The header and footer parts' paragraphs.
struct Running {
    header: Option<String>,
    footer: Option<String>,
}

/// Picks the first running header, footer and page number, and counts
/// every running block and every variant left out.
fn collect_running(
    layout: &DocumentLayout,
    geometry: &[PageGeometry],
    report: &mut DocxReport,
) -> Running {
    let mut header: Option<(String, Alignment)> = None;
    let mut footer: Option<(String, Alignment)> = None;
    let mut number: Option<(String, Alignment, bool)> = None;
    let mut variants: [BTreeSet<String>; 3] = Default::default();
    for (i, page) in layout.pages.iter().enumerate() {
        for block in &page.blocks {
            let slot = match block.kind {
                BlockKind::RunningHeader => 0,
                BlockKind::RunningFooter => 1,
                BlockKind::PageNumber => 2,
                _ => continue,
            };
            report.running_blocks += 1;
            if block.source == BlockSource::Inferred {
                report.inferred_blocks += 1;
            }
            let text = block.text(page);
            if let Some(set) = variants.get_mut(slot) {
                set.insert(digits_normalised(&text));
            }
            match slot {
                0 if header.is_none() => header = Some((text, block.alignment)),
                1 if footer.is_none() => footer = Some((text, block.alignment)),
                2 if number.is_none() => {
                    let top = display_top(block.bbox, geometry.get(i).copied())
                        > display_middle(geometry.get(i).copied());
                    number = Some((text, block.alignment, top));
                }
                _ => {}
            }
        }
    }
    report.running_variants_dropped = variants.iter().map(|set| set.len().saturating_sub(1)).sum();

    let mut header_xml = String::new();
    let mut footer_xml = String::new();
    if let Some((text, align)) = &header {
        plain_paragraph(&mut header_xml, text, *align, report);
    }
    if let Some((text, align)) = &footer {
        plain_paragraph(&mut footer_xml, text, *align, report);
    }
    if let Some((text, align, top)) = &number {
        let out = if *top {
            &mut header_xml
        } else {
            &mut footer_xml
        };
        report.page_number_field = page_number_paragraph(out, text, *align, report);
    }
    report.header = !header_xml.is_empty();
    report.footer = !footer_xml.is_empty();
    Running {
        header: (!header_xml.is_empty()).then_some(header_xml),
        footer: (!footer_xml.is_empty()).then_some(footer_xml),
    }
}

/// `text` with each run of digits replaced by `#`.
fn digits_normalised(text: &str) -> String {
    let mut out = String::new();
    let mut in_digits = false;
    for c in text.chars() {
        if c.is_ascii_digit() {
            if !in_digits {
                out.push('#');
            }
            in_digits = true;
        } else {
            out.push(c);
            in_digits = false;
        }
    }
    out
}

/// A coordinate that grows toward the top of the displayed page.
fn display_top(r: Rect, geom: Option<PageGeometry>) -> f64 {
    match geom.map_or(0, |g| g.rotate) {
        90 => -r.llx,
        180 => -r.lly,
        270 => r.urx,
        _ => r.ury,
    }
}

/// [`display_top`]'s value at the displayed page's vertical middle.
fn display_middle(geom: Option<PageGeometry>) -> f64 {
    let Some(g) = geom else {
        return 396.0;
    };
    let b = g.crop_box;
    let (cx, cy) = (f64::midpoint(b.llx, b.urx), f64::midpoint(b.lly, b.ury));
    match g.rotate {
        90 => -cx,
        180 => -cy,
        270 => cx,
        _ => cy,
    }
}

/// The Word page size in twips, as displayed.
fn page_size(geom: Option<PageGeometry>) -> (i64, i64) {
    let (w, h) = geom.map_or((612.0, 792.0), |g| {
        let (w, h) = (g.crop_box.width(), g.crop_box.height());
        if g.rotate % 180 == 90 { (h, w) } else { (w, h) }
    });
    // Word's page size range: 0.1 in to 22 in.
    (twips(w).clamp(144, 31_680), twips(h).clamp(144, 31_680))
}

fn twips(points: f64) -> i64 {
    (points * 20.0).round() as i64
}

fn write_page(
    out: &mut String,
    page: &PageLayout,
    geom: Option<PageGeometry>,
    tables: &[&Table],
    body_size: Option<f32>,
    report: &mut DocxReport,
) {
    // For each table: where it goes in the block sequence, and which
    // blocks it replaces.
    let mut replaced = vec![false; page.blocks.len()];
    let mut placed: Vec<(usize, &Table)> = Vec::new();
    for &table in tables {
        if table.columns.len() > MAX_TABLE_COLUMNS {
            report.tables_too_wide += 1;
            continue;
        }
        let mut first = None;
        for (i, block) in page.blocks.iter().enumerate() {
            if is_running(&block.kind) || !centre_inside(block.bbox, table.bbox) {
                continue;
            }
            if let Some(slot) = replaced.get_mut(i)
                && !*slot
            {
                *slot = true;
                report.blocks_in_tables += 1;
            }
            first.get_or_insert(i);
        }
        let at = first.unwrap_or_else(|| {
            let top = display_top(table.bbox, geom);
            page.blocks
                .iter()
                .position(|b| !is_running(&b.kind) && display_top(b.bbox, geom) < top)
                .unwrap_or(page.blocks.len())
        });
        placed.push((at, table));
    }
    placed.sort_by_key(|&(at, _)| at);

    let mut next = placed.iter().peekable();
    for (i, block) in page.blocks.iter().enumerate() {
        while let Some(&(_, table)) = next.next_if(|&&(at, _)| at <= i) {
            table_xml(out, table, report);
        }
        if is_running(&block.kind) || replaced.get(i).copied().unwrap_or(false) {
            continue;
        }
        body_paragraph(out, page, block, body_size, report);
    }
    for &(_, table) in next {
        table_xml(out, table, report);
    }
}

fn is_running(kind: &BlockKind) -> bool {
    matches!(
        kind,
        BlockKind::RunningHeader | BlockKind::RunningFooter | BlockKind::PageNumber
    )
}

fn centre_inside(r: Rect, outer: Rect) -> bool {
    let (x, y) = (f64::midpoint(r.llx, r.urx), f64::midpoint(r.lly, r.ury));
    x >= outer.llx && x <= outer.urx && y >= outer.lly && y <= outer.ury
}

fn jc(align: Alignment) -> Option<&'static str> {
    match align {
        Alignment::Center => Some("center"),
        Alignment::Right => Some("right"),
        Alignment::Justified => Some("both"),
        _ => None,
    }
}

/// Half-points, Word's font-size unit.
fn half_points(size: f32) -> i64 {
    (f64::from(size) * 2.0).round().clamp(2.0, 3276.0) as i64
}

fn body_paragraph(
    out: &mut String,
    page: &PageLayout,
    block: &Block,
    body_size: Option<f32>,
    report: &mut DocxReport,
) {
    if block.source == BlockSource::Inferred {
        report.inferred_blocks += 1;
    }
    let fli = f64::from(block.first_line_indent);
    let mut ppr = String::new();
    let heading = match &block.kind {
        BlockKind::Heading { level } => {
            report.headings += 1;
            let _ = write!(ppr, "<w:pStyle w:val=\"Heading{}\"/>", (*level).clamp(1, 6));
            true
        }
        BlockKind::ListItem { .. } => {
            report.list_items += 1;
            ppr.push_str("<w:pStyle w:val=\"ListParagraph\"/>");
            false
        }
        BlockKind::Caption => {
            report.captions += 1;
            ppr.push_str("<w:pStyle w:val=\"Caption\"/>");
            false
        }
        _ => {
            report.paragraphs += 1;
            false
        }
    };
    if let Some(v) = jc(block.alignment) {
        let _ = write!(ppr, "<w:jc w:val=\"{v}\"/>");
    }
    if matches!(block.kind, BlockKind::ListItem { .. }) {
        if fli < -0.5 {
            let _ = write!(
                ppr,
                "<w:ind w:left=\"{LIST_INDENT}\" w:hanging=\"{}\"/>",
                twips(-fli)
            );
        }
    } else if fli > 0.5 {
        let _ = write!(ppr, "<w:ind w:firstLine=\"{}\"/>", twips(fli));
    } else if fli < -0.5 {
        let t = twips(-fli);
        let _ = write!(ppr, "<w:ind w:left=\"{t}\" w:hanging=\"{t}\"/>");
    }

    let mut rpr = String::new();
    if block.bold && !heading {
        rpr.push_str("<w:b/>");
    }
    let differs = body_size.is_none_or(|b| (block.font_size - b).abs() > 0.5);
    if heading || differs {
        let _ = write!(rpr, "<w:sz w:val=\"{}\"/>", half_points(block.font_size));
    }
    out.push_str("<w:p>");
    if !ppr.is_empty() {
        let _ = write!(out, "<w:pPr>{ppr}</w:pPr>");
    }
    run(out, &block.text(page), &rpr, report);
    out.push_str("</w:p>");
}

/// A text run; nothing for empty text.
fn run(out: &mut String, text: &str, rpr: &str, report: &mut DocxReport) {
    if text.is_empty() {
        return;
    }
    out.push_str("<w:r>");
    if !rpr.is_empty() {
        let _ = write!(out, "<w:rPr>{rpr}</w:rPr>");
    }
    out.push_str("<w:t xml:space=\"preserve\">");
    report.characters_dropped += escape_into(out, text);
    out.push_str("</w:t></w:r>");
}

fn plain_paragraph(out: &mut String, text: &str, align: Alignment, report: &mut DocxReport) {
    out.push_str("<w:p>");
    if let Some(v) = jc(align) {
        let _ = write!(out, "<w:pPr><w:jc w:val=\"{v}\"/></w:pPr>");
    }
    run(out, text, "", report);
    out.push_str("</w:p>");
}

/// The page-number paragraph: the text with its number replaced by a
/// `PAGE` field. Returns whether a field was written.
fn page_number_paragraph(
    out: &mut String,
    text: &str,
    align: Alignment,
    report: &mut DocxReport,
) -> bool {
    let Some((prefix, number, suffix, format)) = split_page_number(text) else {
        plain_paragraph(out, text, align, report);
        return false;
    };
    out.push_str("<w:p>");
    if let Some(v) = jc(align) {
        let _ = write!(out, "<w:pPr><w:jc w:val=\"{v}\"/></w:pPr>");
    }
    run(out, prefix, "", report);
    let _ = write!(out, "<w:fldSimple w:instr=\" PAGE {format}\">");
    run(out, number, "", report);
    out.push_str("</w:fldSimple>");
    run(out, suffix, "", report);
    out.push_str("</w:p>");
    true
}

/// Splits page-number text around its number: the first run of digits,
/// or a text made only of one roman numeral. Returns (prefix, number,
/// suffix, field format switch).
fn split_page_number(text: &str) -> Option<(&str, &str, &str, &'static str)> {
    if let Some(start) = text.find(|c: char| c.is_ascii_digit()) {
        let rest = text.get(start..)?;
        let len = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        return Some((text.get(..start)?, rest.get(..len)?, rest.get(len..)?, ""));
    }
    let t = text.trim();
    if t.is_empty() {
        return None;
    }
    let format = if t.chars().all(|c| "ivxlcdm".contains(c)) {
        "\\* roman "
    } else if t.chars().all(|c| "IVXLCDM".contains(c)) {
        "\\* ROMAN "
    } else {
        return None;
    };
    let start = text.find(t)?;
    Some((text.get(..start)?, t, text.get(start + t.len()..)?, format))
}

fn table_xml(out: &mut String, table: &Table, report: &mut DocxReport) {
    let (rows, cols) = (table.rows.len(), table.columns.len());
    if rows == 0 || cols == 0 {
        return;
    }
    report.tables += 1;
    // Which cell covers each grid position.
    let mut grid: Vec<Option<usize>> = vec![None; rows * cols];
    for (ci, cell) in table.cells.iter().enumerate() {
        if cell.row >= rows || cell.col >= cols {
            continue;
        }
        report.table_cells += 1;
        if cell.row_span > 1 || cell.col_span > 1 {
            report.merged_cells += 1;
        }
        let row_end = (cell.row + cell.row_span.max(1)).min(rows);
        let col_end = (cell.col + cell.col_span.max(1)).min(cols);
        for r in cell.row..row_end {
            for c in cell.col..col_end {
                if let Some(slot) = grid.get_mut(r * cols + c) {
                    *slot = Some(ci);
                }
            }
        }
    }
    let widths: Vec<i64> = table
        .columns
        .iter()
        .map(|c| twips(c.width()).max(1))
        .collect();

    out.push_str(
        "<w:tbl><w:tblPr><w:tblStyle w:val=\"TableGrid\"/><w:tblW w:w=\"0\" w:type=\"auto\"/>\
         </w:tblPr><w:tblGrid>",
    );
    for w in &widths {
        let _ = write!(out, "<w:gridCol w:w=\"{w}\"/>");
    }
    out.push_str("</w:tblGrid>");
    for r in 0..rows {
        let header = r < table.header_rows;
        out.push_str("<w:tr>");
        if header {
            out.push_str("<w:trPr><w:tblHeader/></w:trPr>");
        }
        let mut c = 0;
        while c < cols {
            let covering = grid
                .get(r * cols + c)
                .copied()
                .flatten()
                .and_then(|ci| table.cells.get(ci))
                .filter(|cell| cell.col == c);
            let Some(cell) = covering else {
                let w = widths.get(c).copied().unwrap_or(1);
                let _ = write!(
                    out,
                    "<w:tc><w:tcPr><w:tcW w:w=\"{w}\" w:type=\"dxa\"/></w:tcPr><w:p/></w:tc>"
                );
                c += 1;
                continue;
            };
            let span = cell.col_span.max(1).min(cols - c);
            let w: i64 = widths.iter().skip(c).take(span).sum();
            let _ = write!(out, "<w:tc><w:tcPr><w:tcW w:w=\"{w}\" w:type=\"dxa\"/>");
            if span > 1 {
                let _ = write!(out, "<w:gridSpan w:val=\"{span}\"/>");
            }
            let origin = cell.row == r;
            if !origin {
                out.push_str("<w:vMerge/>");
            } else if cell.row_span > 1 {
                out.push_str("<w:vMerge w:val=\"restart\"/>");
            }
            out.push_str("</w:tcPr>");
            if origin && !cell.text.is_empty() {
                let rpr = if header { "<w:b/>" } else { "" };
                for line in cell.text.split('\n') {
                    out.push_str("<w:p>");
                    run(out, line, rpr, report);
                    out.push_str("</w:p>");
                }
            } else {
                out.push_str("<w:p/>");
            }
            out.push_str("</w:tc>");
            c += span;
        }
        out.push_str("</w:tr>");
    }
    // Two adjacent tables would merge into one; a paragraph keeps them
    // apart and gives the cursor somewhere to land after the table.
    out.push_str("</w:tbl><w:p/>");
}

fn part(root: &str, paragraphs: &str) -> String {
    format!("{XML_DECL}<w:{root} xmlns:w=\"{NS_W}\" xmlns:r=\"{NS_R}\">{paragraphs}</w:{root}>")
}

fn content_types(header: bool, footer: bool) -> String {
    let mut s = format!(
        "{XML_DECL}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
         <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
         <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
         <Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>\
         <Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/>"
    );
    if header {
        s.push_str(
            "<Override PartName=\"/word/header1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml\"/>",
        );
    }
    if footer {
        s.push_str(
            "<Override PartName=\"/word/footer1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml\"/>",
        );
    }
    s.push_str("</Types>");
    s
}

const ROOT_RELS: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
<Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>\
</Relationships>";

fn document_rels(header: bool, footer: bool) -> String {
    const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let mut s = format!(
        "{XML_DECL}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
         <Relationship Id=\"rIdStyles\" Type=\"{REL}/styles\" Target=\"styles.xml\"/>"
    );
    if header {
        let _ = write!(
            s,
            "<Relationship Id=\"rIdHeader\" Type=\"{REL}/header\" Target=\"header1.xml\"/>"
        );
    }
    if footer {
        let _ = write!(
            s,
            "<Relationship Id=\"rIdFooter\" Type=\"{REL}/footer\" Target=\"footer1.xml\"/>"
        );
    }
    s.push_str("</Relationships>");
    s
}

/// Styles: `Normal` at the body size, `Heading 1`–`6`, `List Paragraph`,
/// `Caption` and a bordered `Table Grid`. The built-in lower-case names
/// (`heading 1`) are what make Word treat them as its own headings, in
/// the navigation pane and a table of contents.
fn styles(body_size: Option<f32>) -> String {
    let sz = half_points(body_size.unwrap_or(11.0));
    let mut s = format!(
        "{XML_DECL}<w:styles xmlns:w=\"{NS_W}\">\
         <w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val=\"{sz}\"/><w:szCs w:val=\"{sz}\"/>\
         </w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after=\"120\"/></w:pPr>\
         </w:pPrDefault></w:docDefaults>\
         <w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/>\
         <w:qFormat/></w:style>"
    );
    for level in 1..=6 {
        let _ = write!(
            s,
            "<w:style w:type=\"paragraph\" w:styleId=\"Heading{level}\">\
             <w:name w:val=\"heading {level}\"/><w:basedOn w:val=\"Normal\"/>\
             <w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:keepNext/>\
             <w:spacing w:before=\"240\"/><w:outlineLvl w:val=\"{}\"/></w:pPr>\
             <w:rPr><w:b/></w:rPr></w:style>",
            level - 1
        );
    }
    let _ = write!(
        s,
        "<w:style w:type=\"paragraph\" w:styleId=\"ListParagraph\">\
         <w:name w:val=\"List Paragraph\"/><w:basedOn w:val=\"Normal\"/><w:qFormat/>\
         <w:pPr><w:ind w:left=\"{LIST_INDENT}\"/></w:pPr></w:style>\
         <w:style w:type=\"paragraph\" w:styleId=\"Caption\"><w:name w:val=\"caption\"/>\
         <w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/>\
         <w:rPr><w:i/></w:rPr></w:style>\
         <w:style w:type=\"table\" w:styleId=\"TableGrid\"><w:name w:val=\"Table Grid\"/>\
         <w:tblPr><w:tblBorders>\
         <w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>\
         <w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>\
         <w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>\
         <w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>\
         <w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>\
         <w:insideV w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/>\
         </w:tblBorders></w:tblPr></w:style></w:styles>"
    );
    s
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::block_layout::{LayoutOptions, analyze_layout};
    use crate::document::Document;
    use crate::export::ooxml_zip::tests::read_zip;
    use crate::table_detect::{TableOptions, detect_tables};
    use crate::text_extract::ExtractOptions;

    fn build_pdf(contents: &[String]) -> Document {
        let n = contents.len();
        let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 5 + 2 * i)).collect();
        let mut bodies = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
                .to_owned(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>"
                .to_owned(),
        ];
        for (i, c) in contents.iter().enumerate() {
            bodies.push(format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {} 0 R \
                 /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> >>",
                6 + 2 * i
            ));
            bodies.push(format!("<< /Length {} >>\nstream\n{c}\nendstream", c.len()));
        }
        let mut buf = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, body) in bodies.iter().enumerate() {
            offsets.push(buf.len());
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
        Document::from_bytes(buf).unwrap()
    }

    fn line(font: &str, size: u32, x: u32, y: u32, text: &str) -> String {
        format!("BT /{font} {size} Tf {x} {y} Td ({text}) Tj ET\n")
    }

    /// Two pages, each with a running header and a page number.
    fn text_doc() -> Document {
        let run = |n: u32| {
            line("F1", 9, 72, 760, "Annual Report 2024")
                + &line("F1", 9, 290, 30, &format!("Page {n}"))
        };
        let p1 = [
            run(1),
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
                90,
                638,
                "A second paragraph starts indented by eighteen points,",
            ),
            line("F1", 10, 72, 626, "then returns to the margin."),
            "BT /F1 10 Tf 72 600 Td <95> Tj ( First item) Tj ET\n".to_owned(),
            line("F1", 10, 200, 570, "Figure 1: Growth & <decline>"),
        ]
        .concat();
        let p2 = run(2) + &line("F1", 10, 72, 700, "Second page body text.");
        build_pdf(&[p1, p2])
    }

    fn export(doc: &Document, options: &DocxOptions) -> (DocxReport, Vec<(String, String)>) {
        let view = doc.view();
        let layout =
            analyze_layout(&view, &ExtractOptions::default(), &LayoutOptions::default()).unwrap();
        let geometry = vec![
            PageGeometry::new(Rect::from_corners(0.0, 0.0, 612.0, 792.0), 0);
            layout.pages.len()
        ];
        let tables =
            detect_tables(&view, &ExtractOptions::default(), &TableOptions::default()).unwrap();
        let out = write_docx(&layout, &geometry, &tables.tables, options).unwrap();
        let parts = read_zip(&out.bytes)
            .into_iter()
            .map(|(n, d)| (n, String::from_utf8(d).unwrap()))
            .collect();
        (out.report, parts)
    }

    fn part_text<'a>(parts: &'a [(String, String)], name: &str) -> &'a str {
        &parts.iter().find(|(n, _)| n == name).unwrap().1
    }

    #[test]
    fn blocks_become_styled_paragraphs() {
        let (report, parts) = export(&text_doc(), &DocxOptions::default());
        let doc = part_text(&parts, "word/document.xml");
        assert!(doc.contains("<w:pStyle w:val=\"Heading1\"/>"), "{doc}");
        assert!(doc.contains(">Introduction</w:t>"));
        // 18 pt first-line indent = 360 twips.
        assert!(doc.contains("<w:ind w:firstLine=\"360\"/>"), "{doc}");
        assert!(doc.contains("<w:pStyle w:val=\"ListParagraph\"/>"));
        assert!(doc.contains("\u{2022} First item"));
        assert!(doc.contains("<w:pStyle w:val=\"Caption\"/>"));
        assert!(doc.contains("Figure 1: Growth &amp; &lt;decline&gt;"));
        assert!(doc.contains("<w:br w:type=\"page\"/>"));
        assert!(doc.contains("<w:pgSz w:w=\"12240\" w:h=\"15840\"/>"));
        assert_eq!(
            (
                report.pages,
                report.headings,
                report.paragraphs,
                report.list_items,
                report.captions
            ),
            (2, 1, 3, 1, 1),
            "{report:?}"
        );
        assert_eq!(report.inferred_blocks, 10, "{report:?}");
    }

    #[test]
    fn running_text_goes_to_the_header_and_footer_once() {
        let (report, parts) = export(&text_doc(), &DocxOptions::default());
        let doc = part_text(&parts, "word/document.xml");
        assert!(!doc.contains("Annual Report"), "{doc}");
        assert!(!doc.contains(">Page "), "{doc}");
        assert!(doc.contains("<w:headerReference w:type=\"default\" r:id=\"rIdHeader\"/>"));
        assert!(doc.contains("<w:footerReference w:type=\"default\" r:id=\"rIdFooter\"/>"));
        let header = part_text(&parts, "word/header1.xml");
        assert_eq!(header.matches("Annual Report 2024").count(), 1);
        let footer = part_text(&parts, "word/footer1.xml");
        assert!(
            footer.contains(">Page </w:t></w:r><w:fldSimple w:instr=\" PAGE \">"),
            "{footer}"
        );
        assert!(report.header && report.footer && report.page_number_field);
        assert_eq!(
            (report.running_blocks, report.running_variants_dropped),
            (4, 0)
        );
        let types = part_text(&parts, "[Content_Types].xml");
        assert!(types.contains("/word/header1.xml") && types.contains("/word/footer1.xml"));
    }

    #[test]
    fn alternating_headers_keep_the_first_and_count_the_other() {
        let pages: Vec<String> = (0..4)
            .map(|i| {
                let header = if i % 2 == 0 {
                    "Odd Header"
                } else {
                    "Even Header"
                };
                line("F1", 9, 72, 760, header) + &line("F1", 10, 72, 500, &format!("Body {i}."))
            })
            .collect();
        let (report, parts) = export(&build_pdf(&pages), &DocxOptions::default());
        let header = part_text(&parts, "word/header1.xml");
        assert!(header.contains("Odd Header") && !header.contains("Even Header"));
        assert_eq!(
            (report.running_blocks, report.running_variants_dropped),
            (4, 1),
            "{report:?}"
        );
    }

    #[test]
    fn no_page_breaks_on_request() {
        let (_, parts) = export(&text_doc(), &DocxOptions::default().with_page_breaks(false));
        assert!(!part_text(&parts, "word/document.xml").contains("w:type=\"page\""));
    }

    /// A ruled 3x2 grid (a bold merged title row, two plain rows)
    /// between two paragraphs.
    fn table_doc() -> Document {
        let content = [
            line("F1", 10, 72, 730, "Before the table."),
            "0 G 0.5 w\n\
             72 700 m 328 700 l S 72 680 m 328 680 l S 72 660 m 328 660 l S 72 640 m 328 640 l S\n\
             72 640 m 72 700 l S 328 640 m 328 700 l S 200 640 m 200 680 l S\n"
                .to_owned(),
            line("F2", 10, 76, 686, "Parts list"),
            line("F1", 10, 76, 666, "Part"),
            line("F1", 10, 204, 666, "Qty"),
            line("F1", 10, 76, 646, "Bolt M6"),
            line("F1", 10, 204, 646, "12"),
            line("F1", 10, 72, 600, "After the table."),
        ]
        .concat();
        build_pdf(&[content])
    }

    #[test]
    fn a_table_replaces_the_blocks_inside_it() {
        let (report, parts) = export(&table_doc(), &DocxOptions::default());
        let doc = part_text(&parts, "word/document.xml");
        assert_eq!(doc.matches("Bolt M6").count(), 1, "{doc}");
        let before = doc.find("Before the table.").unwrap();
        let table = doc.find("<w:tbl>").unwrap();
        let after = doc.find("After the table.").unwrap();
        assert!(before < table && table < after, "{doc}");
        assert!(doc.contains("<w:gridSpan w:val=\"2\"/>"));
        assert!(doc.contains("<w:trPr><w:tblHeader/></w:trPr>"));
        assert!(doc.contains("<w:rPr><w:b/></w:rPr><w:t xml:space=\"preserve\">Parts list"));
        assert_eq!(
            (report.tables, report.table_cells, report.merged_cells),
            (1, 5, 1),
            "{report:?}"
        );
        assert!(report.blocks_in_tables >= 3, "{report:?}");

        let (report, parts) = export(&table_doc(), &DocxOptions::default().with_tables(false));
        let doc = part_text(&parts, "word/document.xml");
        assert!(!doc.contains("<w:tbl>"));
        assert!(doc.contains("Bolt M6"));
        assert_eq!((report.tables, report.blocks_in_tables), (0, 0));
    }

    #[test]
    fn page_numbers_split_around_the_number() {
        assert_eq!(
            split_page_number("Page 12 of"),
            Some(("Page ", "12", " of", ""))
        );
        assert_eq!(split_page_number("- iv -"), None);
        assert_eq!(
            split_page_number(" iv "),
            Some((" ", "iv", " ", "\\* roman "))
        );
        assert_eq!(
            split_page_number("XII"),
            Some(("", "XII", "", "\\* ROMAN "))
        );
        assert_eq!(split_page_number("Contents"), None);
        assert_eq!(digits_normalised("Page 12 of 30"), "Page # of #");
    }

    #[test]
    fn output_is_deterministic() {
        let doc = text_doc();
        let view = doc.view();
        let layout =
            analyze_layout(&view, &ExtractOptions::default(), &LayoutOptions::default()).unwrap();
        let a = write_docx(&layout, &[], &[], &DocxOptions::default()).unwrap();
        let b = write_docx(&layout, &[], &[], &DocxOptions::default()).unwrap();
        assert_eq!(a.bytes, b.bytes);
    }
}
