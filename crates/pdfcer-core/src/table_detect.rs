//! **Table detection**: a page's tables as a grid of rows, columns and
//! cells, with each cell's glyphs (`G055`).
//!
//! Ruled tables come from the page's vector content: stroked axis-aligned
//! line segments and thin filled rectangles become rules, rules that cross
//! become corners, and the smallest rectangle closed by four connected
//! corners is a cell. Every decision is counted in [`TableDiagnostics`];
//! nothing is inferred silently (CLAUDE.md rule 4).
//!
//! Analysis runs in display space (`/Rotate` applied) so rows run the way
//! the page is read; every box returned is in default user space.
//!
//! Not governed by ISO 32000: the standard describes how lines are painted
//! (§8.5), not what they mean. A tagged file's own table structure is read
//! by [`crate::structure_tree`] instead.

use std::collections::{HashMap, HashSet};

use crate::page_tree::{self, Rect};
use crate::text_extract::{
    ExtractError, ExtractOptions, ExtractedText, extract_document_view, extract_pages_view,
};
use crate::vector::{PathObject, Segment, VectorObject, decompose_page};
use crate::view::DocumentView;

/// Tunables for [`detect_tables`]. Distances are in points.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct TableOptions {
    /// Rules this close in their fixed coordinate are one rule; corners
    /// this close are one corner.
    pub snap_tolerance: f32,
    /// Collinear pieces separated by at most this much are joined.
    pub join_tolerance: f32,
    /// A filled rectangle at most this thick is a rule, not shading.
    pub thin_fill: f32,
    /// Rules shorter than this are dropped (tick marks, dashes).
    pub min_rule_length: f32,
    /// An unruled table needs at least this many aligned rows.
    pub min_aligned_rows: usize,
    /// A whitespace gap at least this many font sizes wide separates two
    /// columns of an unruled table.
    pub min_gutter_em: f32,
    /// An unruled candidate whose cells average more characters than this
    /// is prose in columns, not a table.
    pub max_mean_cell_chars: f32,
}

impl Default for TableOptions {
    fn default() -> Self {
        Self {
            snap_tolerance: 3.0,
            join_tolerance: 3.0,
            thin_fill: 2.0,
            min_rule_length: 3.0,
            min_aligned_rows: 3,
            min_gutter_em: 1.0,
            max_mean_cell_chars: 30.0,
        }
    }
}

impl TableOptions {
    /// Sets [`Self::snap_tolerance`].
    #[must_use]
    pub const fn with_snap_tolerance(mut self, points: f32) -> Self {
        self.snap_tolerance = points;
        self
    }

    /// Sets [`Self::join_tolerance`].
    #[must_use]
    pub const fn with_join_tolerance(mut self, points: f32) -> Self {
        self.join_tolerance = points;
        self
    }

    /// Sets [`Self::min_aligned_rows`].
    #[must_use]
    pub const fn with_min_aligned_rows(mut self, rows: usize) -> Self {
        self.min_aligned_rows = rows;
        self
    }

    /// Sets [`Self::min_gutter_em`].
    #[must_use]
    pub const fn with_min_gutter_em(mut self, em: f32) -> Self {
        self.min_gutter_em = em;
        self
    }

    /// Sets [`Self::max_mean_cell_chars`].
    #[must_use]
    pub const fn with_max_mean_cell_chars(mut self, chars: f32) -> Self {
        self.max_mean_cell_chars = chars;
        self
    }
}

/// How a table's boundaries were found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BoundarySource {
    /// From drawn rules: stroked lines, rectangle edges, thin fills.
    Ruled,
    /// From whitespace alone: text rows whose gutters line up. Row edges
    /// fall midway between baselines, column edges midway across gutters.
    Aligned,
    /// From the file's own `Table`/`TR`/`TH`/`TD` structure elements
    /// ([`tables_from_structure`]); not an inference.
    Tagged,
}

/// Why the first row was taken as a header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HeaderEvidence {
    /// Its text is bold and the rows below are not.
    Bold,
    /// A non-white fill covers it and not the row below.
    Filled,
    /// The rule under it is heavier than the table's other rules.
    HeavyRule,
    /// An unruled table whose only interior rule is under its first row.
    RuleBelow,
    /// The file tags the rows `THead` or their cells `TH`; not an
    /// inference.
    Tagged,
}

/// One glyph: `text.pages[page].runs[run].glyphs[glyph]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphRef {
    /// Index into the page's runs.
    pub run: usize,
    /// Index into that run's glyphs.
    pub glyph: usize,
}

/// One cell. A merged cell covers `row..row + row_span` and
/// `col..col + col_span`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct TableCell {
    /// First row, 0 at the top.
    pub row: usize,
    /// First column, 0 at the left.
    pub col: usize,
    /// Rows covered, at least 1.
    pub row_span: usize,
    /// Columns covered, at least 1.
    pub col_span: usize,
    /// Default user space.
    pub bbox: Rect,
    /// Glyphs whose centre is inside the cell, in reading order.
    pub glyphs: Vec<GlyphRef>,
    /// Those glyphs' text: a space at a word gap, `\n` between lines.
    pub text: String,
}

/// One table.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Table {
    /// The page's index in the page tree. After [`detect_tables`] it is also
    /// the index into [`ExtractedText::pages`]; after [`detect_tables_in_pages`]
    /// find the page whose [`PageText::page_index`](crate::text_extract::PageText::page_index)
    /// equals it.
    pub page_index: usize,
    /// Default user space.
    pub bbox: Rect,
    /// How the boundaries were found.
    pub source: BoundarySource,
    /// Row bands, top to bottom as displayed, each spanning the table;
    /// default user space.
    pub rows: Vec<Rect>,
    /// Column bands, left to right as displayed; default user space.
    pub columns: Vec<Rect>,
    /// Row-major by top-left corner.
    pub cells: Vec<TableCell>,
    /// Header rows at the top: 0 or 1 when detected, any number when
    /// [`BoundarySource::Tagged`].
    pub header_rows: usize,
    /// Why [`Self::header_rows`] is 1; `None` when it is 0.
    pub header_evidence: Option<HeaderEvidence>,
}

impl Table {
    /// The cell whose top-left is at (`row`, `col`), if any.
    #[must_use]
    pub fn cell(&self, row: usize, col: usize) -> Option<&TableCell> {
        self.cells.iter().find(|c| c.row == row && c.col == col)
    }
}

/// The tables a structure tree tags, as [`Table`]s for the exporters:
/// [`BoundarySource::Tagged`], [`HeaderEvidence::Tagged`] when a header
/// row is tagged, each cell's glyphs every glyph of its runs. `text` is
/// the extraction the tagged layout indexes
/// ([`TaggedLayout::layout`](crate::tagged_layout::TaggedLayout::layout)`.text`).
///
/// # Examples
///
/// ```no_run
/// use pdfcer_core::block_layout::LayoutOptions;
/// use pdfcer_core::structure_tree::StructureTree;
/// use pdfcer_core::table_detect::tables_from_structure;
/// use pdfcer_core::tagged_layout::{TaggedLayoutOptions, layout_from_structure};
///
/// # fn demo(tree: &StructureTree) {
/// let tagged = layout_from_structure(tree, &[], &LayoutOptions::default(), &TaggedLayoutOptions::default());
/// let tables = tables_from_structure(&tagged.tables, &tagged.layout.text);
/// assert!(tables.iter().all(|t| t.source == pdfcer_core::table_detect::BoundarySource::Tagged));
/// # }
/// ```
#[must_use]
pub fn tables_from_structure(
    tagged: &[crate::tagged_layout::TaggedTable],
    text: &ExtractedText,
) -> Vec<Table> {
    tagged
        .iter()
        .map(|t| {
            let runs = text
                .pages
                .iter()
                .find(|p| p.page_index == t.page_index)
                .map_or(&[][..], |p| p.runs.as_slice());
            let cells = t
                .cells
                .iter()
                .map(|c| TableCell {
                    row: c.row,
                    col: c.col,
                    row_span: c.row_span,
                    col_span: c.col_span,
                    bbox: c.bbox,
                    glyphs: c
                        .runs
                        .iter()
                        .flat_map(|&run| {
                            let n = runs.get(run).map_or(0, |r| r.glyphs.len());
                            (0..n).map(move |glyph| GlyphRef { run, glyph })
                        })
                        .collect(),
                    text: c.text.clone(),
                })
                .collect();
            Table {
                page_index: t.page_index,
                bbox: t.bbox,
                source: BoundarySource::Tagged,
                rows: t.rows.clone(),
                columns: t.columns.clone(),
                cells,
                header_rows: t.header_rows,
                header_evidence: (t.header_rows > 0).then_some(HeaderEvidence::Tagged),
            }
        })
        .collect()
}

/// What [`detect_tables`] inferred, and what it could not use.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct TableDiagnostics {
    /// Pages examined.
    pub pages: usize,
    /// Tables found from rules.
    pub tables_ruled: usize,
    /// Tables found from whitespace alignment.
    pub tables_aligned: usize,
    /// Aligned-row blocks refused as tables: too few columns, a column
    /// used by one row, or cells long enough to be prose.
    pub aligned_blocks_rejected: usize,
    /// Cells across all tables.
    pub cells: usize,
    /// Cells with a row or column span above 1.
    pub merged_cells: usize,
    /// Header rows inferred, by any evidence.
    pub header_rows_inferred: usize,
    /// Rules taken from stroked segments.
    pub rules_from_strokes: usize,
    /// Rules taken from thin filled rectangles.
    pub rules_from_fills: usize,
    /// Closed rule rectangles dropped because they had one cell only.
    pub single_cell_frames: usize,
    /// Pages whose rules exceeded the corner ceiling and were skipped.
    pub pages_over_limit: usize,
    /// Pages whose vector content could not be read (their text still
    /// counts nowhere; no table is guessed for them).
    pages_unreadable: usize,
}

impl TableDiagnostics {
    /// Pages whose vector content could not be read.
    #[must_use]
    pub const fn pages_unreadable(&self) -> usize {
        self.pages_unreadable
    }

    /// Every table, cell-span and header decision made by heuristic.
    #[must_use]
    pub const fn inferred(&self) -> usize {
        self.tables_ruled + self.tables_aligned + self.merged_cells + self.header_rows_inferred
    }
}

/// A document's tables, with the extraction their glyph indices refer to.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct DocumentTables {
    /// The extraction [`GlyphRef`] indexes into.
    pub text: ExtractedText,
    /// Every table, by page then top to bottom.
    pub tables: Vec<Table>,
    /// Counts.
    pub diagnostics: TableDiagnostics,
}

/// Why [`detect_tables`] failed. A page whose drawing cannot be read is
/// counted, not an error.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TableError {
    /// Text extraction failed.
    #[error("text extraction: {0}")]
    Extract(#[from] ExtractError),
    /// The page tree could not be read.
    #[error("page tree: {0}")]
    PageTree(#[from] page_tree::PageTreeError),
}

/// Upper bound on corners per page; a hatched or dense CAD drawing past it
/// is skipped and counted rather than searched quadratically.
const MAX_CORNERS: usize = 20_000;
/// Upper bound on rules per page, before joining.
const MAX_RULES: usize = 50_000;

/// Finds every table in the document: ruled grids first, then
/// whitespace-aligned blocks among the text no ruled table took.
///
/// # Errors
///
/// [`TableError`] when the text or page tree cannot be read.
///
/// # Examples
///
/// ```no_run
/// use pdfcer_core::document::Document;
/// use pdfcer_core::table_detect::{TableOptions, detect_tables};
/// use pdfcer_core::text_extract::ExtractOptions;
///
/// # fn demo(doc: &Document) -> Result<(), Box<dyn std::error::Error>> {
/// let found = detect_tables(&doc.view(), &ExtractOptions::default(), &TableOptions::default())?;
/// for table in &found.tables {
///     for cell in &table.cells {
///         println!("r{} c{}: {}", cell.row, cell.col, cell.text);
///     }
/// }
/// # Ok(()) }
/// ```
pub fn detect_tables(
    view: &DocumentView<'_>,
    extract: &ExtractOptions,
    options: &TableOptions,
) -> Result<DocumentTables, TableError> {
    tables_in(view, extract_document_view(view, extract)?, options)
}

/// [`detect_tables`] over the listed pages only, in the order given.
/// `pages` are 0-based indices into the view's page list; only those pages
/// are extracted and decomposed, and [`TableDiagnostics`] counts only them.
///
/// # Errors
///
/// As [`detect_tables`], plus [`TableError::Extract`] wrapping
/// [`ExtractError::NoSuchPage`] for an index past the last page.
///
/// # Examples
///
/// ```no_run
/// use pdfcer_core::document::Document;
/// use pdfcer_core::table_detect::{TableOptions, detect_tables_in_pages};
/// use pdfcer_core::text_extract::ExtractOptions;
///
/// # fn demo(doc: &Document) -> Result<(), Box<dyn std::error::Error>> {
/// // The tables on page 3 only.
/// let found = detect_tables_in_pages(
///     &doc.view(),
///     &[2],
///     &ExtractOptions::default(),
///     &TableOptions::default(),
/// )?;
/// assert!(found.tables.iter().all(|t| t.page_index == 2));
/// # Ok(()) }
/// ```
pub fn detect_tables_in_pages(
    view: &DocumentView<'_>,
    pages: &[usize],
    extract: &ExtractOptions,
    options: &TableOptions,
) -> Result<DocumentTables, TableError> {
    tables_in(view, extract_pages_view(view, pages, extract)?, options)
}

fn tables_in(
    view: &DocumentView<'_>,
    text: ExtractedText,
    options: &TableOptions,
) -> Result<DocumentTables, TableError> {
    let pages = page_tree::pages_in(view)?;
    let mut diag = TableDiagnostics {
        pages: text.pages.len(),
        ..TableDiagnostics::default()
    };
    let mut tables = Vec::new();
    for page_text in &text.pages {
        let Some(page) = pages.get(page_text.page_index) else {
            continue;
        };
        let Ok(model) = decompose_page(view, page, crate::vector::Matrix::IDENTITY) else {
            diag.pages_unreadable += 1;
            continue;
        };
        let paths = model
            .objects
            .iter()
            .chain(model.leaves.iter().map(|l| &l.object))
            .filter_map(|o| match o {
                VectorObject::Path(p) => Some(p),
                _ => None,
            });
        let ink = collect_ink(paths, page.rotate, options, &mut diag);
        let mut found = ruled_tables(&ink, page.rotate, options, &mut diag);
        let mut taken = HashSet::new();
        for t in &mut found {
            fill_text(t, page_text, page.rotate);
            taken.extend(t.glyphs.iter().flatten().copied());
        }
        let hs = merge_rules(
            ink.horizontal.clone(),
            f64::from(options.snap_tolerance),
            f64::from(options.join_tolerance),
        );
        found.extend(aligned_tables(
            page_text,
            &taken,
            &hs,
            page.rotate,
            options,
            &mut diag,
        ));
        found.sort_by(|a, b| {
            let top = |d: &Draft| d.ys.first().copied().unwrap_or(0.0);
            top(b).total_cmp(&top(a))
        });
        for t in found {
            let table = finish(
                t,
                &ink,
                page_text,
                page_text.page_index,
                page.rotate,
                &mut diag,
            );
            tables.push(table);
        }
    }
    Ok(DocumentTables {
        text,
        tables,
        diagnostics: diag,
    })
}

// ---------------------------------------------------------------------------
// Display space
// ---------------------------------------------------------------------------

fn to_display(rotate: u16, x: f64, y: f64) -> (f64, f64) {
    match rotate {
        90 => (y, -x),
        180 => (-x, -y),
        270 => (-y, x),
        _ => (x, y),
    }
}

fn from_display(rotate: u16, x: f64, y: f64) -> (f64, f64) {
    match rotate {
        90 => (-y, x),
        180 => (-x, -y),
        270 => (y, -x),
        _ => (x, y),
    }
}

fn display_to_rect(rotate: u16, x0: f64, x1: f64, bottom: f64, top: f64) -> Rect {
    let (ax, ay) = from_display(rotate, x0, bottom);
    let (bx, by) = from_display(rotate, x1, top);
    Rect::from_corners(ax, ay, bx, by)
}

// ---------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------

/// An axis-aligned rule in display space: `at` is its fixed coordinate,
/// `lo..hi` its extent.
#[derive(Debug, Clone, Copy)]
struct Rule {
    at: f64,
    lo: f64,
    hi: f64,
    width: f64,
}

/// A filled, non-white, non-rule rectangle: header shading evidence.
#[derive(Debug, Clone, Copy)]
struct Fill {
    x0: f64,
    x1: f64,
    bottom: f64,
    top: f64,
}

#[derive(Debug, Default)]
struct Ink {
    horizontal: Vec<Rule>,
    vertical: Vec<Rule>,
    fills: Vec<Fill>,
    over_limit: bool,
}

fn is_white(c: crate::vector::Rgb) -> bool {
    c.r > 0.95 && c.g > 0.95 && c.b > 0.95
}

fn collect_ink<'a>(
    paths: impl Iterator<Item = &'a PathObject>,
    rotate: u16,
    options: &TableOptions,
    diag: &mut TableDiagnostics,
) -> Ink {
    let mut ink = Ink::default();
    let thin = f64::from(options.thin_fill);
    let min_len = f64::from(options.min_rule_length);
    for path in paths {
        let stroked = path.style.stroke && !is_white(path.stroke_color);
        let filled = path.style.fill.is_some() && !is_white(path.fill_color);
        if !stroked && !filled {
            continue;
        }
        for sub in path.page_subpaths() {
            let mut pts = vec![to_display(rotate, sub.start.x, sub.start.y)];
            for seg in &sub.segments {
                match seg {
                    Segment::Line { to } => pts.push(to_display(rotate, to.x, to.y)),
                    Segment::Cubic { .. } => {
                        pts.clear();
                        break;
                    }
                }
            }
            if pts.len() < 2 {
                continue;
            }
            if stroked {
                let mut edges: Vec<((f64, f64), (f64, f64))> = pts
                    .windows(2)
                    .filter_map(|w| Some((*w.first()?, *w.get(1)?)))
                    .collect();
                if sub.closed
                    && let (Some(&a), Some(&b)) = (pts.last(), pts.first())
                {
                    edges.push((a, b));
                }
                let width = path.line_width.abs().max(0.1);
                for ((ax, ay), (bx, by)) in edges {
                    let (dx, dy) = ((bx - ax).abs(), (by - ay).abs());
                    let len = dx.max(dy);
                    if len < min_len {
                        continue;
                    }
                    let skew = dx.min(dy);
                    if skew > (0.02 * len).max(1.0) {
                        continue;
                    }
                    let rule = if dy <= dx {
                        Rule {
                            at: (ay + by) / 2.0,
                            lo: ax.min(bx),
                            hi: ax.max(bx),
                            width,
                        }
                    } else {
                        Rule {
                            at: (ax + bx) / 2.0,
                            lo: ay.min(by),
                            hi: ay.max(by),
                            width,
                        }
                    };
                    if dy <= dx {
                        ink.horizontal.push(rule);
                    } else {
                        ink.vertical.push(rule);
                    }
                    diag.rules_from_strokes += 1;
                }
            } else if let Some((x0, x1, bottom, top)) = axis_rect(&pts) {
                let (w, h) = (x1 - x0, top - bottom);
                if h <= thin && w >= min_len {
                    ink.horizontal.push(Rule {
                        at: (bottom + top) / 2.0,
                        lo: x0,
                        hi: x1,
                        width: h.max(0.1),
                    });
                    diag.rules_from_fills += 1;
                } else if w <= thin && h >= min_len {
                    ink.vertical.push(Rule {
                        at: (x0 + x1) / 2.0,
                        lo: bottom,
                        hi: top,
                        width: w.max(0.1),
                    });
                    diag.rules_from_fills += 1;
                } else if w > thin && h > thin {
                    ink.fills.push(Fill {
                        x0,
                        x1,
                        bottom,
                        top,
                    });
                }
            }
            if ink.horizontal.len() + ink.vertical.len() > MAX_RULES {
                ink.over_limit = true;
                return ink;
            }
        }
    }
    ink
}

/// `(x0, x1, bottom, top)` when the points trace an axis-aligned
/// rectangle (four corners, optionally repeating the first).
fn axis_rect(pts: &[(f64, f64)]) -> Option<(f64, f64, f64, f64)> {
    let pts = match pts {
        [a, .., z] if pts.len() == 5 && (a.0 - z.0).abs() < 0.01 && (a.1 - z.1).abs() < 0.01 => {
            pts.get(..4)?
        }
        _ if pts.len() == 4 => pts,
        _ => return None,
    };
    let xs: Vec<f64> = pts.iter().map(|p| p.0).collect();
    let ys: Vec<f64> = pts.iter().map(|p| p.1).collect();
    let (x0, x1) = (
        xs.iter().copied().fold(f64::INFINITY, f64::min),
        xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    );
    let (y0, y1) = (
        ys.iter().copied().fold(f64::INFINITY, f64::min),
        ys.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    );
    let on_edge = |v: f64, a: f64, b: f64| (v - a).abs() < 0.01 || (v - b).abs() < 0.01;
    pts.iter()
        .all(|p| on_edge(p.0, x0, x1) && on_edge(p.1, y0, y1))
        .then_some((x0, x1, y0, y1))
}

/// Snaps rules to shared positions and joins collinear pieces.
fn merge_rules(mut rules: Vec<Rule>, snap: f64, join: f64) -> Vec<Rule> {
    rules.sort_by(|a, b| a.at.total_cmp(&b.at));
    let mut groups: Vec<Vec<Rule>> = Vec::new();
    for r in rules {
        match groups.last_mut() {
            Some(g) if g.first().is_some_and(|f| r.at - f.at <= snap) => g.push(r),
            _ => groups.push(vec![r]),
        }
    }
    let mut out = Vec::new();
    for mut g in groups {
        #[allow(clippy::cast_precision_loss)]
        let at = g.iter().map(|r| r.at).sum::<f64>() / g.len() as f64;
        g.sort_by(|a, b| a.lo.total_cmp(&b.lo));
        let mut cur: Option<Rule> = None;
        for r in g {
            cur = match cur {
                Some(c) if r.lo <= c.hi + join => Some(Rule {
                    at,
                    lo: c.lo,
                    hi: c.hi.max(r.hi),
                    width: c.width.max(r.width),
                }),
                Some(c) => {
                    out.push(c);
                    Some(Rule { at, ..r })
                }
                None => Some(Rule { at, ..r }),
            };
        }
        out.extend(cur);
    }
    out
}

// ---------------------------------------------------------------------------
// Cells and tables
// ---------------------------------------------------------------------------

/// A table in display space before its text and header are known.
#[derive(Debug)]
struct Draft {
    /// Column edges, left to right.
    xs: Vec<f64>,
    /// Row edges, top to bottom (descending y).
    ys: Vec<f64>,
    /// `(x0, x1, bottom, top)` per cell.
    cells: Vec<(f64, f64, f64, f64)>,
    glyphs: Vec<Vec<GlyphRef>>,
    /// Width of the rule under each row edge index, where one lies on it.
    edge_widths: Vec<f64>,
    source: BoundarySource,
}

fn ruled_tables(
    ink: &Ink,
    rotate: u16,
    options: &TableOptions,
    diag: &mut TableDiagnostics,
) -> Vec<Draft> {
    let _ = rotate;
    if ink.over_limit {
        diag.pages_over_limit += 1;
        return Vec::new();
    }
    let snap = f64::from(options.snap_tolerance);
    let join = f64::from(options.join_tolerance);
    let hs = merge_rules(ink.horizontal.clone(), snap, join);
    let vs = merge_rules(ink.vertical.clone(), snap, join);

    // Corners: (x, y) -> (horizontal rule, vertical rule) indices.
    let mut corners: Vec<(f64, f64, usize, usize)> = Vec::new();
    for (hi, h) in hs.iter().enumerate() {
        for (vi, v) in vs.iter().enumerate() {
            if v.at >= h.lo - snap
                && v.at <= h.hi + snap
                && h.at >= v.lo - snap
                && h.at <= v.hi + snap
            {
                corners.push((v.at, h.at, hi, vi));
                if corners.len() > MAX_CORNERS {
                    diag.pages_over_limit += 1;
                    return Vec::new();
                }
            }
        }
    }
    // Row-major, top-left first.
    corners.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.total_cmp(&b.0)));
    let mut by_h: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut by_v: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut at: HashMap<(usize, usize), usize> = HashMap::new();
    for (i, c) in corners.iter().enumerate() {
        by_h.entry(c.2).or_default().push(i);
        by_v.entry(c.3).or_default().push(i);
        at.insert((c.2, c.3), i);
    }

    // For each top-left corner, the nearest bottom-right closing a cell.
    let mut cells: Vec<(usize, usize)> = Vec::new();
    for (i, &(x, y, h, v)) in corners.iter().enumerate() {
        let rights = by_h.get(&h).map_or(&[][..], Vec::as_slice);
        let belows = by_v.get(&v).map_or(&[][..], Vec::as_slice);
        'search: for &r in rights
            .iter()
            .filter(|&&r| corners.get(r).is_some_and(|c| c.0 > x + snap))
        {
            let Some(&(_, _, _, rv)) = corners.get(r) else {
                continue;
            };
            for &b in belows
                .iter()
                .filter(|&&b| corners.get(b).is_some_and(|c| c.1 < y - snap))
            {
                let Some(&(_, _, bh, _)) = corners.get(b) else {
                    continue;
                };
                if let Some(&d) = at.get(&(bh, rv)) {
                    cells.push((i, d));
                    break 'search;
                }
            }
        }
    }
    if cells.is_empty() {
        return Vec::new();
    }

    // Tables: cells sharing a corner.
    let mut parent: Vec<usize> = (0..cells.len()).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while let Some(&q) = p.get(i) {
            if q == i {
                break;
            }
            let g = p.get(q).copied().unwrap_or(q);
            if let Some(slot) = p.get_mut(i) {
                *slot = g;
            }
            i = q;
        }
        i
    }
    let corner_xy = |i: usize| corners.get(i).map_or((0.0, 0.0), |c| (c.0, c.1));
    let mut owner: HashMap<(i64, i64), usize> = HashMap::new();
    let key = |x: f64, y: f64| {
        #[allow(clippy::cast_possible_truncation)]
        ((x * 100.0).round() as i64, (y * 100.0).round() as i64)
    };
    for (ci, &(tl, br)) in cells.iter().enumerate() {
        let (x0, y1) = corner_xy(tl);
        let (x1, y0) = corner_xy(br);
        for k in [key(x0, y1), key(x1, y1), key(x0, y0), key(x1, y0)] {
            match owner.get(&k) {
                Some(&o) => {
                    let (a, b) = (find(&mut parent, o), find(&mut parent, ci));
                    if let Some(slot) = parent.get_mut(a) {
                        *slot = b;
                    }
                }
                None => {
                    owner.insert(k, ci);
                }
            }
        }
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for ci in 0..cells.len() {
        let root = find(&mut parent, ci);
        groups.entry(root).or_default().push(ci);
    }
    let mut drafts = Vec::new();
    let mut roots: Vec<usize> = groups.keys().copied().collect();
    roots.sort_unstable();
    for root in roots {
        let Some(members) = groups.get(&root) else {
            continue;
        };
        if members.len() < 2 {
            diag.single_cell_frames += 1;
            continue;
        }
        let rects: Vec<(f64, f64, f64, f64)> = members
            .iter()
            .filter_map(|&ci| cells.get(ci))
            .map(|&(tl, br)| {
                let (x0, top) = corner_xy(tl);
                let (x1, bottom) = corner_xy(br);
                (x0, x1, bottom, top)
            })
            .collect();
        let xs = edges(rects.iter().flat_map(|r| [r.0, r.1]), snap, false);
        let ys = edges(rects.iter().flat_map(|r| [r.2, r.3]), snap, true);
        let edge_widths = ys
            .iter()
            .map(|&y| {
                hs.iter()
                    .filter(|h| (h.at - y).abs() <= snap)
                    .map(|h| h.width)
                    .fold(0.0, f64::max)
            })
            .collect();
        let n = rects.len();
        drafts.push(Draft {
            xs,
            ys,
            cells: rects,
            glyphs: vec![Vec::new(); n],
            edge_widths,
            source: BoundarySource::Ruled,
        });
    }
    // Top to bottom on the page.
    drafts.sort_by(|a, b| {
        let top = |d: &Draft| d.ys.first().copied().unwrap_or(0.0);
        top(b).total_cmp(&top(a))
    });
    drafts
}

/// Distinct positions within `snap`, ascending (or descending).
fn edges(values: impl Iterator<Item = f64>, snap: f64, descending: bool) -> Vec<f64> {
    let mut v: Vec<f64> = values.collect();
    v.sort_by(f64::total_cmp);
    let mut out: Vec<f64> = Vec::new();
    for x in v {
        if out.last().is_none_or(|&l| x - l > snap) {
            out.push(x);
        }
    }
    if descending {
        out.reverse();
    }
    out
}

fn nearest(edges: &[f64], v: f64) -> usize {
    edges
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - v).abs().total_cmp(&(b.1 - v).abs()))
        .map_or(0, |(i, _)| i)
}

// ---------------------------------------------------------------------------
// Text and header
// ---------------------------------------------------------------------------

/// Display-space centre of a glyph.
fn glyph_centre(g: &crate::text_extract::ExtractedGlyph, rotate: u16) -> (f64, f64) {
    let (dx, dy) = (f64::from(g.direction.0), f64::from(g.direction.1));
    let half = f64::from(g.advance) / 2.0;
    let rise = f64::from(g.size) * 0.3;
    let x = f64::from(g.x) + dx * half - dy * rise;
    let y = f64::from(g.y) + dy * half + dx * rise;
    to_display(rotate, x, y)
}

fn fill_text(t: &mut Draft, page: &crate::text_extract::PageText, rotate: u16) {
    for (ri, run) in page.runs.iter().enumerate() {
        for (gi, g) in run.glyphs.iter().enumerate() {
            let (x, y) = glyph_centre(g, rotate);
            let hit = t
                .cells
                .iter()
                .enumerate()
                .filter(|(_, c)| x >= c.0 && x <= c.1 && y >= c.2 && y <= c.3)
                .min_by(|a, b| {
                    let area = |c: &(f64, f64, f64, f64)| (c.1 - c.0) * (c.3 - c.2);
                    area(a.1).total_cmp(&area(b.1))
                })
                .map(|(i, _)| i);
            if let Some(slot) = hit.and_then(|i| t.glyphs.get_mut(i)) {
                slot.push(GlyphRef { run: ri, glyph: gi });
            }
        }
    }
}

/// Orders a cell's glyphs into lines and returns them with their text.
fn cell_text(
    refs: &[GlyphRef],
    page: &crate::text_extract::PageText,
    rotate: u16,
) -> (Vec<GlyphRef>, String) {
    struct G<'a> {
        r: GlyphRef,
        x0: f64,
        x1: f64,
        y: f64,
        size: f64,
        text: &'a str,
    }
    let mut gs: Vec<G<'_>> = refs
        .iter()
        .filter_map(|&r| {
            let run = page.runs.get(r.run)?;
            let g = run.glyphs.get(r.glyph)?;
            let (cx, cy) = glyph_centre(g, rotate);
            let half = f64::from(g.advance).abs() / 2.0;
            let start = usize::try_from(g.text_start).ok()?;
            let end = start + usize::try_from(g.text_len).ok()?;
            Some(G {
                r,
                x0: cx - half,
                x1: cx + half,
                y: cy,
                size: f64::from(g.size).abs().max(1.0),
                text: run.text.get(start..end).unwrap_or(""),
            })
        })
        .collect();
    gs.sort_by(|a, b| b.y.total_cmp(&a.y));
    let mut lines: Vec<Vec<G<'_>>> = Vec::new();
    for g in gs {
        match lines.last_mut() {
            Some(l) if l.first().is_some_and(|f| (f.y - g.y).abs() <= 0.5 * f.size) => l.push(g),
            _ => lines.push(vec![g]),
        }
    }
    let mut order = Vec::new();
    let mut text = String::new();
    for (li, mut line) in lines.into_iter().enumerate() {
        line.sort_by(|a, b| a.x0.total_cmp(&b.x0));
        if li > 0 {
            text.push('\n');
        }
        let mut prev_x1: Option<f64> = None;
        for g in line {
            if let Some(p) = prev_x1
                && g.x0 - p > 0.2 * g.size
                && !text.ends_with(char::is_whitespace)
                && !g.text.starts_with(char::is_whitespace)
            {
                text.push(' ');
            }
            text.push_str(g.text);
            prev_x1 = Some(g.x1);
            order.push(g.r);
        }
    }
    (order, text.trim().to_owned())
}

fn finish(
    t: Draft,
    ink: &Ink,
    page: &crate::text_extract::PageText,
    page_index: usize,
    rotate: u16,
    diag: &mut TableDiagnostics,
) -> Table {
    let (x_lo, x_hi) = (
        t.xs.first().copied().unwrap_or(0.0),
        t.xs.last().copied().unwrap_or(0.0),
    );
    let (y_top, y_bottom) = (
        t.ys.first().copied().unwrap_or(0.0),
        t.ys.last().copied().unwrap_or(0.0),
    );
    let mut cells: Vec<TableCell> = t
        .cells
        .iter()
        .zip(&t.glyphs)
        .map(|(&(x0, x1, bottom, top), refs)| {
            let col = nearest(&t.xs, x0);
            let row = nearest(&t.ys, top);
            let col_span = nearest(&t.xs, x1).saturating_sub(col).max(1);
            let row_span = nearest(&t.ys, bottom).saturating_sub(row).max(1);
            let (glyphs, text) = cell_text(refs, page, rotate);
            TableCell {
                row,
                col,
                row_span,
                col_span,
                bbox: display_to_rect(rotate, x0, x1, bottom, top),
                glyphs,
                text,
            }
        })
        .collect();
    cells.sort_by_key(|c| (c.row, c.col));
    let rows: Vec<Rect> =
        t.ys.windows(2)
            .filter_map(|w| Some(display_to_rect(rotate, x_lo, x_hi, *w.get(1)?, *w.first()?)))
            .collect();
    let columns: Vec<Rect> =
        t.xs.windows(2)
            .filter_map(|w| {
                Some(display_to_rect(
                    rotate,
                    *w.first()?,
                    *w.get(1)?,
                    y_bottom,
                    y_top,
                ))
            })
            .collect();

    let header_evidence = if rows.len() >= 2 {
        header_guess(&t, &cells, ink, page)
    } else {
        None
    };
    match t.source {
        BoundarySource::Aligned => diag.tables_aligned += 1,
        BoundarySource::Ruled => diag.tables_ruled += 1,
        // Detection never produces a tagged table.
        BoundarySource::Tagged => {}
    }
    diag.cells += cells.len();
    diag.merged_cells += cells
        .iter()
        .filter(|c| c.row_span > 1 || c.col_span > 1)
        .count();
    if header_evidence.is_some() {
        diag.header_rows_inferred += 1;
    }
    Table {
        page_index,
        bbox: display_to_rect(rotate, x_lo, x_hi, y_bottom, y_top),
        source: t.source,
        rows,
        columns,
        cells,
        header_rows: usize::from(header_evidence.is_some()),
        header_evidence,
    }
}

fn header_guess(
    t: &Draft,
    cells: &[TableCell],
    ink: &Ink,
    page: &crate::text_extract::PageText,
) -> Option<HeaderEvidence> {
    let bold_share = |pred: &dyn Fn(&TableCell) -> bool| {
        let (mut n, mut b) = (0usize, 0usize);
        for c in cells.iter().filter(|c| pred(c)) {
            for r in &c.glyphs {
                let Some(g) = page.runs.get(r.run).and_then(|run| run.glyphs.get(r.glyph)) else {
                    continue;
                };
                n += 1;
                b += usize::from(g.weight.is_bold());
            }
        }
        #[allow(clippy::cast_precision_loss)]
        (n > 0).then(|| b as f64 / n as f64)
    };
    let first = bold_share(&|c| c.row == 0);
    let rest = bold_share(&|c| c.row > 0);
    if let (Some(f), Some(r)) = (first, rest)
        && f >= 0.6
        && r < 0.3
    {
        return Some(HeaderEvidence::Bold);
    }

    let (x_lo, x_hi) = (t.xs.first()?, t.xs.last()?);
    let (y0, y1, y2) = (t.ys.first()?, t.ys.get(1)?, t.ys.get(2)?);
    let covered = |top: f64, bottom: f64| {
        let area = (x_hi - x_lo) * (top - bottom);
        let got: f64 = ink
            .fills
            .iter()
            .map(|f| {
                let w = (f.x1.min(*x_hi) - f.x0.max(*x_lo)).max(0.0);
                let h = (f.top.min(top) - f.bottom.max(bottom)).max(0.0);
                w * h
            })
            .sum();
        area > 0.0 && got / area >= 0.8
    };
    if covered(*y0, *y1) && !covered(*y1, *y2) {
        return Some(HeaderEvidence::Filled);
    }

    let under = *t.edge_widths.get(1)?;
    let last = t.edge_widths.len().saturating_sub(1);
    if t.source == BoundarySource::Aligned
        && under > 0.0
        && t.edge_widths
            .iter()
            .enumerate()
            .all(|(i, &w)| i == 0 || i == 1 || i == last || w <= 0.0)
    {
        return Some(HeaderEvidence::RuleBelow);
    }
    let mut others: Vec<f64> = t
        .edge_widths
        .iter()
        .enumerate()
        .filter(|&(i, &w)| i != 1 && w > 0.0)
        .map(|(_, &w)| w)
        .collect();
    others.sort_by(f64::total_cmp);
    let median = others.get(others.len() / 2).copied()?;
    (under >= 1.5 * median).then_some(HeaderEvidence::HeavyRule)
}

// ---------------------------------------------------------------------------
// Aligned (unruled) tables
// ---------------------------------------------------------------------------

/// A glyph placed in display space for gutter analysis.
#[derive(Debug, Clone, Copy)]
struct Placed {
    r: GlyphRef,
    x0: f64,
    x1: f64,
    y: f64,
    size: f64,
    chars: usize,
}

/// Glyphs of one row with no gap of a gutter's width between them.
#[derive(Debug)]
struct Chunk {
    x0: f64,
    x1: f64,
    glyphs: Vec<Placed>,
}

#[derive(Debug)]
struct TextRow {
    y: f64,
    size: f64,
    chunks: Vec<Chunk>,
}

fn glyph_text<'a>(
    run: &'a crate::text_extract::TextRun,
    g: &crate::text_extract::ExtractedGlyph,
) -> &'a str {
    let Ok(start) = usize::try_from(g.text_start) else {
        return "";
    };
    let Ok(len) = usize::try_from(g.text_len) else {
        return "";
    };
    run.text.get(start..start + len).unwrap_or("")
}

/// Tables laid out by whitespace alone: consecutive rows of at least two
/// chunks whose gutters line up down the whole block.
fn aligned_tables(
    page: &crate::text_extract::PageText,
    taken: &HashSet<GlyphRef>,
    hs: &[Rule],
    rotate: u16,
    options: &TableOptions,
    diag: &mut TableDiagnostics,
) -> Vec<Draft> {
    let gutter_em = f64::from(options.min_gutter_em).max(0.1);
    let mut placed = Vec::new();
    for (ri, run) in page.runs.iter().enumerate() {
        for (gi, g) in run.glyphs.iter().enumerate() {
            let r = GlyphRef { run: ri, glyph: gi };
            if taken.contains(&r) {
                continue;
            }
            let (dx, dy) = to_display(rotate, f64::from(g.direction.0), f64::from(g.direction.1));
            if dx <= 0.0 || dx < 0.9 * dx.hypot(dy) {
                continue;
            }
            let text = glyph_text(run, g);
            if text.trim().is_empty() {
                continue;
            }
            let (cx, cy) = glyph_centre(g, rotate);
            let half = f64::from(g.advance).abs() / 2.0;
            placed.push(Placed {
                r,
                x0: cx - half,
                x1: cx + half,
                y: cy,
                size: f64::from(g.size).abs().max(1.0),
                chars: text.chars().count(),
            });
        }
    }
    placed.sort_by(|a, b| b.y.total_cmp(&a.y));
    let mut lines: Vec<Vec<Placed>> = Vec::new();
    for p in placed {
        match lines.last_mut() {
            Some(l) if l.first().is_some_and(|f| (f.y - p.y).abs() <= 0.5 * f.size) => l.push(p),
            _ => lines.push(vec![p]),
        }
    }
    let rows: Vec<TextRow> = lines
        .into_iter()
        .map(|mut l| {
            l.sort_by(|a, b| a.x0.total_cmp(&b.x0));
            let size = l.iter().map(|p| p.size).fold(0.0, f64::max);
            let y = l.first().map_or(0.0, |p| p.y);
            let mut chunks: Vec<Chunk> = Vec::new();
            for p in l {
                match chunks.last_mut() {
                    Some(c) if p.x0 - c.x1 < gutter_em * p.size => {
                        c.x1 = c.x1.max(p.x1);
                        c.glyphs.push(p);
                    }
                    _ => chunks.push(Chunk {
                        x0: p.x0,
                        x1: p.x1,
                        glyphs: vec![p],
                    }),
                }
            }
            TextRow { y, size, chunks }
        })
        .collect();

    let mut drafts = Vec::new();
    let mut block: Vec<&TextRow> = Vec::new();
    let mut flush = |block: &mut Vec<&TextRow>, drafts: &mut Vec<Draft>| {
        if block.len() >= options.min_aligned_rows.max(2) {
            match aligned_block(block, hs, gutter_em, f64::from(options.max_mean_cell_chars)) {
                Some(d) => drafts.push(d),
                None => diag.aligned_blocks_rejected += 1,
            }
        }
        block.clear();
    };
    for row in &rows {
        let tabular = row.chunks.len() >= 2;
        let joins = tabular
            && block
                .last()
                .is_none_or(|p| p.y - row.y <= 2.5 * p.size.max(row.size));
        if !joins {
            flush(&mut block, &mut drafts);
        }
        if tabular {
            block.push(row);
        }
    }
    flush(&mut block, &mut drafts);
    drafts
}

/// One candidate block to a grid, or `None` when its columns do not hold
/// up: fewer than two, one used by a single row, or cells long enough to
/// be prose columns.
fn aligned_block(
    rows: &[&TextRow],
    hs: &[Rule],
    gutter_em: f64,
    max_mean_chars: f64,
) -> Option<Draft> {
    let mut sizes: Vec<f64> = rows.iter().map(|r| r.size).collect();
    sizes.sort_by(f64::total_cmp);
    let gutter = gutter_em * sizes.get(sizes.len() / 2).copied()?;
    let mut spans: Vec<(f64, f64)> = rows
        .iter()
        .flat_map(|r| r.chunks.iter().map(|c| (c.x0, c.x1)))
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut cols: Vec<(f64, f64)> = Vec::new();
    for (a, b) in spans {
        match cols.last_mut() {
            Some(c) if a < c.1 + gutter => c.1 = c.1.max(b),
            _ => cols.push((a, b)),
        }
    }
    let ncols = cols.len();
    if ncols < 2 {
        return None;
    }
    let mut grid: Vec<Vec<GlyphRef>> = vec![Vec::new(); rows.len() * ncols];
    let mut used = vec![0usize; ncols];
    let mut chars = 0usize;
    for (ri, row) in rows.iter().enumerate() {
        let mut seen = vec![false; ncols];
        for chunk in &row.chunks {
            let ci = cols
                .iter()
                .position(|c| chunk.x0 >= c.0 - 0.01 && chunk.x1 <= c.1 + 0.01)?;
            grid.get_mut(ri * ncols + ci)?
                .extend(chunk.glyphs.iter().map(|p| p.r));
            chars += chunk.glyphs.iter().map(|p| p.chars).sum::<usize>();
            *seen.get_mut(ci)? = true;
        }
        for (u, s) in used.iter_mut().zip(&seen) {
            *u += usize::from(*s);
        }
    }
    let filled: usize = used.iter().sum();
    #[allow(clippy::cast_precision_loss)]
    let mean = chars as f64 / filled.max(1) as f64;
    if used.iter().any(|&u| u < 2) || mean > max_mean_chars {
        return None;
    }

    let mut xs = vec![cols.first()?.0];
    xs.extend(
        cols.windows(2)
            .filter_map(|w| Some((w.first()?.1 + w.get(1)?.0) / 2.0)),
    );
    xs.push(cols.last()?.1);
    let (first, last) = (rows.first()?, rows.last()?);
    let mut ys = vec![first.y + 0.6 * first.size];
    ys.extend(
        rows.windows(2)
            .filter_map(|w| Some((w.first()?.y + w.get(1)?.y) / 2.0)),
    );
    ys.push(last.y - 0.6 * last.size);

    let mut cells = Vec::with_capacity(grid.len());
    for ri in 0..rows.len() {
        for ci in 0..ncols {
            cells.push((
                *xs.get(ci)?,
                *xs.get(ci + 1)?,
                *ys.get(ri + 1)?,
                *ys.get(ri)?,
            ));
        }
    }
    // A rule counts for a row edge when it lies between the two rows'
    // baselines (or within 1.5 em outside the first and last) and spans
    // most of the table.
    let (x_lo, x_hi) = (*xs.first()?, *xs.last()?);
    let edge_widths = (0..=rows.len())
        .map(|i| {
            let above = i.checked_sub(1).and_then(|p| rows.get(p));
            let (lo, hi) = match (above, rows.get(i)) {
                (None, Some(r)) => (r.y, r.y + 1.5 * r.size),
                (Some(r), None) => (r.y - 1.5 * r.size, r.y),
                (Some(a), Some(b)) => (b.y, a.y),
                (None, None) => (0.0, 0.0),
            };
            hs.iter()
                .filter(|h| h.at > lo && h.at < hi)
                .filter(|h| h.hi.min(x_hi) - h.lo.max(x_lo) >= 0.8 * (x_hi - x_lo))
                .map(|h| h.width)
                .fold(0.0, f64::max)
        })
        .collect();
    Some(Draft {
        xs,
        ys,
        cells,
        glyphs: grid,
        edge_widths,
        source: BoundarySource::Aligned,
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    fn h(at: f64, lo: f64, hi: f64) -> Rule {
        Rule {
            at,
            lo,
            hi,
            width: 1.0,
        }
    }

    #[test]
    fn collinear_pieces_join_and_near_rules_snap() {
        let merged = merge_rules(
            vec![
                h(100.0, 0.0, 50.0),
                h(101.0, 52.0, 90.0),
                h(200.0, 0.0, 10.0),
            ],
            3.0,
            3.0,
        );
        assert_eq!(merged.len(), 2);
        assert!((merged[0].at - 100.5).abs() < 1e-9);
        assert_eq!((merged[0].lo, merged[0].hi), (0.0, 90.0));
    }

    #[test]
    fn axis_rect_accepts_re_and_rejects_a_diamond() {
        let re = [(0.0, 0.0), (10.0, 0.0), (10.0, 5.0), (0.0, 5.0), (0.0, 0.0)];
        assert_eq!(axis_rect(&re), Some((0.0, 10.0, 0.0, 5.0)));
        let diamond = [(5.0, 0.0), (10.0, 5.0), (5.0, 10.0), (0.0, 5.0)];
        assert_eq!(axis_rect(&diamond), None);
    }

    #[test]
    fn a_two_by_two_grid_with_a_merged_top_row() {
        // Outer box 0..100 x 0..60, a middle horizontal at 30, a vertical at
        // 50 only in the bottom half: top row is one merged cell.
        let ink = Ink {
            horizontal: vec![h(0.0, 0.0, 100.0), h(30.0, 0.0, 100.0), h(60.0, 0.0, 100.0)],
            vertical: vec![h(0.0, 0.0, 60.0), h(100.0, 0.0, 60.0), h(50.0, 0.0, 30.0)],
            ..Ink::default()
        };
        let mut diag = TableDiagnostics::default();
        let drafts = ruled_tables(&ink, 0, &TableOptions::default(), &mut diag);
        assert_eq!(drafts.len(), 1);
        let d = &drafts[0];
        assert_eq!(d.xs, vec![0.0, 50.0, 100.0]);
        assert_eq!(d.ys, vec![60.0, 30.0, 0.0]);
        assert_eq!(d.cells.len(), 3);
        assert!(d.cells.contains(&(0.0, 100.0, 30.0, 60.0)));
    }

    #[test]
    fn a_lone_box_is_a_frame_not_a_table() {
        let ink = Ink {
            horizontal: vec![h(0.0, 0.0, 100.0), h(60.0, 0.0, 100.0)],
            vertical: vec![h(0.0, 0.0, 60.0), h(100.0, 0.0, 60.0)],
            ..Ink::default()
        };
        let mut diag = TableDiagnostics::default();
        assert!(ruled_tables(&ink, 0, &TableOptions::default(), &mut diag).is_empty());
        assert_eq!(diag.single_cell_frames, 1);
    }

    #[test]
    fn display_round_trip() {
        for rot in [0, 90, 180, 270] {
            let (x, y) = to_display(rot, 12.0, 34.0);
            assert_eq!(from_display(rot, x, y), (12.0, 34.0));
        }
    }
}
