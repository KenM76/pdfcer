//! Table cells as text regions: [`table_detect`](crate::table_detect)'s
//! cell rectangles, bound to the block model so a line never joins across
//! a cell boundary and each cell is one block.

use super::stages::union_bbox;
use super::{Block, BlockKind, EditableTextModel, Line};
use crate::page_tree::Rect;
use crate::table_detect::{
    BoundarySource, Table, TableError, TableOptions, detect_tables_in_pages,
};
use crate::text_extract::{ExtractOptions, ExtractedGlyph};
use crate::view::DocumentView;

/// One table cell as a region of a page, the input that makes
/// [`EditableTextModel::recognize_with_cells`] cell-aware.
///
/// A glyph belongs to the cell whose [`Self::rect`] contains the centre of
/// its [`glyph cell`](crate::text_extract::glyph_cell); when nested
/// rectangles both contain it, the smaller wins. Membership is geometric,
/// so cells from any source line up with any extraction of the same page.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct CellRegion {
    /// Which table on the page, in the order the tables were supplied.
    pub table: usize,
    /// First row, 0 at the top.
    pub row: usize,
    /// First column, 0 at the left.
    pub column: usize,
    /// Rows covered, at least 1.
    pub row_span: usize,
    /// Columns covered, at least 1.
    pub column_span: usize,
    /// The cell's rectangle in default user space.
    pub rect: Rect,
}

impl CellRegion {
    /// A one-row, one-column cell.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::page_tree::Rect;
    /// use pdfcer_core::text_edit::CellRegion;
    ///
    /// let cell = CellRegion::new(0, 1, 2, Rect::from_corners(72.0, 600.0, 200.0, 640.0))
    ///     .with_span(2, 1);
    /// assert_eq!((cell.row, cell.column, cell.row_span), (1, 2, 2));
    /// ```
    #[must_use]
    pub const fn new(table: usize, row: usize, column: usize, rect: Rect) -> Self {
        Self {
            table,
            row,
            column,
            row_span: 1,
            column_span: 1,
            rect,
        }
    }

    /// The same cell covering `row_span` rows and `column_span` columns;
    /// a zero span is taken as 1.
    #[must_use]
    pub const fn with_span(mut self, row_span: usize, column_span: usize) -> Self {
        self.row_span = if row_span == 0 { 1 } else { row_span };
        self.column_span = if column_span == 0 { 1 } else { column_span };
        self
    }

    /// The cells of every table in `tables` whose
    /// [`page_index`](Table::page_index) is `page_index`. A cell's
    /// [`Self::table`] is its table's position among that page's tables.
    #[must_use]
    pub fn from_tables(tables: &[Table], page_index: usize) -> Vec<Self> {
        tables
            .iter()
            .filter(|t| t.page_index == page_index)
            .enumerate()
            .flat_map(|(ti, t)| {
                t.cells.iter().map(move |c| {
                    Self::new(ti, c.row, c.col, c.bbox).with_span(c.row_span, c.col_span)
                })
            })
            .collect()
    }

    fn contains(&self, x: f64, y: f64) -> bool {
        let r = self.rect;
        x >= r.llx && x <= r.urx && y >= r.lly && y <= r.ury
    }

    fn covers_row(&self, row: usize) -> bool {
        self.row <= row && row < self.row + self.row_span
    }

    fn shares_columns(&self, other: &Self) -> bool {
        self.column < other.column + other.column_span
            && other.column < self.column + self.column_span
    }
}

/// The ruled-table cells on page `page_index`, ready for
/// [`EditableTextModel::recognize_with_cells`].
///
/// Runs [`detect_tables_in_pages`] with default options and keeps only
/// [`BoundarySource::Ruled`] tables: a drawn grid is evidence of a cell
/// boundary, while a whitespace-aligned "table" may be a multi-column page,
/// which the block model's own column and gutter rules already handle.
///
/// # Errors
///
/// [`TableError`] when the page's text or the page tree cannot be read,
/// including [`ExtractError::NoSuchPage`](crate::text_extract::ExtractError)
/// for an index past the last page.
///
/// # Examples
///
/// ```no_run
/// use pdfcer_core::document::Document;
/// use pdfcer_core::text_edit::{
///     EditableTextModel, detect_cell_regions, reflow_recognition_options,
/// };
/// use pdfcer_core::{page_tree, text_extract};
///
/// let doc = Document::load(std::path::Path::new("in.pdf"))?;
/// let pages = page_tree::pages(&doc)?;
/// let options = text_extract::ExtractOptions::default();
/// let page = text_extract::extract_page(&doc, &pages[0], 0, &options)?;
/// let cells = detect_cell_regions(&doc.view(), 0)?;
/// // The model reflow resolves block indices against.
/// let model = EditableTextModel::recognize_with_cells(&page, &reflow_recognition_options(), &cells);
/// println!("{} cell blocks", model.diagnostics().table_cell_blocks);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn detect_cell_regions(
    view: &DocumentView<'_>,
    page_index: usize,
) -> Result<Vec<CellRegion>, TableError> {
    let found = detect_tables_in_pages(
        view,
        &[page_index],
        &ExtractOptions::default(),
        &TableOptions::default(),
    )?;
    let ruled: Vec<Table> = found
        .tables
        .into_iter()
        .filter(|t| t.source == BoundarySource::Ruled)
        .collect();
    Ok(CellRegion::from_tables(&ruled, page_index))
}

/// The index of the smallest cell containing `g`'s centre, if any.
pub(super) fn cell_index_at(cells: &[CellRegion], g: &ExtractedGlyph) -> Option<usize> {
    if cells.is_empty() {
        return None;
    }
    let b = g.cell();
    let (x, y) = ((b.llx + b.urx) / 2.0, (b.lly + b.ury) / 2.0);
    cells
        .iter()
        .enumerate()
        .filter(|(_, c)| c.contains(x, y))
        .min_by(|a, b| area(a.1).total_cmp(&area(b.1)))
        .map(|(i, _)| i)
}

fn area(c: &CellRegion) -> f64 {
    c.rect.width() * c.rect.height()
}

/// One block per cell holding text, ordered by table, row, column; each
/// block's lines run top to bottom.
pub(super) fn cell_blocks(lines: &[Line], cells: &[CellRegion]) -> Vec<Block> {
    let mut order: Vec<usize> = (0..cells.len()).collect();
    order.sort_by_key(|&ci| cells.get(ci).map(|c| (c.table, c.row, c.column)));
    let mut blocks = Vec::new();
    for ci in order {
        let Some(cell) = cells.get(ci) else { continue };
        let mut members: Vec<usize> = lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.cell == Some(ci))
            .map(|(i, _)| i)
            .collect();
        let Some(&first) = members.first() else {
            continue;
        };
        members.sort_by(|&a, &b| top_to_bottom(lines.get(a), lines.get(b)));
        blocks.push(Block {
            kind: BlockKind::TableCell {
                table: cell.table,
                row: cell.row,
                column: cell.column,
            },
            column: lines.get(first).map_or(0, |l| l.column),
            bbox: union_bbox(lines, &members),
            line_indices: members,
            cell_rect: Some(cell.rect),
        });
    }
    blocks
}

fn top_to_bottom(a: Option<&Line>, b: Option<&Line>) -> std::cmp::Ordering {
    let key = |l: Option<&Line>| l.map_or((0.0, 0.0), |l| (-l.baseline_y, l.bbox.llx as f32));
    let (ka, kb) = (key(a), key(b));
    ka.0.total_cmp(&kb.0).then(ka.1.total_cmp(&kb.1))
}

impl EditableTextModel<'_> {
    /// The line Up/Down lands on from line `cur_idx` in cell `ci`: the
    /// nearest line of the same cell on that side, else the edge line of the
    /// adjacent cell with text in the same table, else the nearest line
    /// outside the table in the same column band.
    pub(super) fn vertical_from_cell(
        &self,
        cur_idx: usize,
        ci: usize,
        desired_x: f32,
        up: bool,
    ) -> Option<usize> {
        if let Some(li) = self.nearest_line(cur_idx, desired_x, up, |l| l.cell == Some(ci)) {
            return Some(li);
        }
        if let Some(next) = self.adjacent_cell(ci, desired_x, up) {
            return self.edge_line_of_cell(next, up);
        }
        let table = self.cells.get(ci)?.table;
        let column = self.lines.get(cur_idx)?.column;
        self.nearest_line(cur_idx, desired_x, up, |l| {
            l.column == column
                && l.cell
                    .and_then(|c| self.cells.get(c))
                    .is_none_or(|c| c.table != table)
        })
    }

    /// The nearest row above (or below) cell `ci` holding a text cell of the
    /// same table, and in it the cell under `desired_x`, else one sharing
    /// `ci`'s columns, else the nearest by x.
    fn adjacent_cell(&self, ci: usize, desired_x: f32, up: bool) -> Option<usize> {
        let cur = self.cells.get(ci)?;
        let last_row = self
            .cells
            .iter()
            .filter(|c| c.table == cur.table)
            .map(|c| c.row + c.row_span)
            .max()
            .unwrap_or(0);
        let rows: Vec<usize> = if up {
            (0..cur.row).rev().collect()
        } else {
            (cur.row + cur.row_span..last_row).collect()
        };
        let x = f64::from(desired_x);
        for row in rows {
            let best = self
                .cells
                .iter()
                .enumerate()
                .filter(|&(i, c)| {
                    i != ci && c.table == cur.table && c.covers_row(row) && self.cell_has_text(i)
                })
                .min_by(|a, b| {
                    let key = |c: &CellRegion| {
                        let dx = x_distance(c.rect, x);
                        (dx > 0.0, !c.shares_columns(cur), dx)
                    };
                    let (ka, kb) = (key(a.1), key(b.1));
                    (ka.0, ka.1).cmp(&(kb.0, kb.1)).then(ka.2.total_cmp(&kb.2))
                });
            if let Some((i, _)) = best {
                return Some(i);
            }
        }
        None
    }

    fn cell_has_text(&self, ci: usize) -> bool {
        self.lines.iter().any(|l| l.cell == Some(ci))
    }

    /// The bottom line of cell `ci` when arriving from below (`up`), else
    /// its top line.
    fn edge_line_of_cell(&self, ci: usize, up: bool) -> Option<usize> {
        let members = self
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.cell == Some(ci));
        if up {
            members.min_by(|a, b| a.1.baseline_y.total_cmp(&b.1.baseline_y))
        } else {
            members.max_by(|a, b| a.1.baseline_y.total_cmp(&b.1.baseline_y))
        }
        .map(|(i, _)| i)
    }
}

/// Horizontal distance from `x` to `rect`'s x-range; 0 inside it.
pub(super) fn x_distance(rect: Rect, x: f64) -> f64 {
    if x < rect.llx {
        rect.llx - x
    } else if x > rect.urx {
        x - rect.urx
    } else {
        0.0
    }
}
