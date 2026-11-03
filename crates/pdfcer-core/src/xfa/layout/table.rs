//! Table and row layout, `layout="table"` / `"row"` / `"rl-row"` (XFA 3.3
//! Ch.8 pp.327-332; spec RAG slice `adobe_ext__xfa_layout_tables`).
//!
//! A row is laid at its cells' natural sizes, remembering the cells
//! ([`RowCells`]); its table then fixes the column widths over every row
//! and re-lays each one: cells side by side in their columns, each expanded
//! to its columns' width and the row's height (p.329, gap G-L44).

use super::geometry::Placed;
use crate::xfa::{LayoutStrategy, Measurement, XfaNode};

/// What a row needs to be re-laid by its table.
#[derive(Clone)]
pub(super) struct RowCells {
    /// The cells as laid at their natural sizes, at the origin.
    pub(super) cells: Vec<Placed>,
    /// Margin insets: left, top, right, bottom.
    pub(super) inset: [f64; 4],
    /// `rl-row`: column 1 is rightmost (p.327).
    pub(super) rtl: bool,
    /// The row's own `w` and `h`, when it declares them.
    pub(super) given: (Option<f64>, Option<f64>),
}

/// Whether `s` lays cells (`row`, `rl-row`).
pub(super) const fn is_row(s: LayoutStrategy) -> bool {
    matches!(s, LayoutStrategy::Row | LayoutStrategy::RlRow)
}

/// Whether children of a parent laid by `s` ignore their `x`, `y` and
/// `anchorType`: a flowing parent (p.279) or a table or row (p.329).
pub(super) const fn places_children(s: LayoutStrategy) -> bool {
    super::flow::is_flow(s) || is_row(s) || matches!(s, LayoutStrategy::Table)
}

/// A cell's `colSpan`: `None` for `-1`, the rest of the row (p.330). Zero,
/// below `-1` or unreadable count as 1 (gap G-L42, derived default).
pub(super) fn col_span(node: &XfaNode) -> Option<u16> {
    match node.attr("colSpan").map(str::trim).map(str::parse::<i64>) {
        Some(Ok(-1)) => None,
        Some(Ok(n)) if n >= 1 => Some(u16::try_from(n).unwrap_or(u16::MAX)),
        _ => Some(1),
    }
}

/// `columnWidths` (Ch.17 p.844): a fixed width per column, `None` for
/// `-1` (fit the widest cell) or an unreadable token.
fn column_widths(node: &XfaNode) -> Vec<Option<f64>> {
    node.attr("columnWidths")
        .unwrap_or("")
        .split_whitespace()
        .map(|t| match Measurement::parse(t) {
            Some(Measurement::Points(v)) if v >= 0.0 => Some(v),
            _ => None,
        })
        .collect()
}

/// The column (start, count) of each cell of a `columns`-wide row; `None`
/// for a cell after a `-1` span, which is not displayed (p.330).
fn spans(cells: &[Placed], columns: usize) -> Vec<Option<(usize, usize)>> {
    let mut at = 0;
    let mut rest = false;
    cells
        .iter()
        .map(|c| {
            if rest {
                return None;
            }
            let n = c.col_span.map_or_else(
                || {
                    rest = true;
                    columns.saturating_sub(at).max(1)
                },
                usize::from,
            );
            let span = (at, n);
            at += n;
            Some(span)
        })
        .collect()
}

/// Fix the columns of the table `node` over its row children and re-lay
/// each row (p.329). Column `c` is its `columnWidths` token, else as wide
/// as its widest one-column cell; a spanning cell does not widen a column
/// (gap G-L43). Columns run past the token list as `-1` (p.327); there are
/// as many as the longest row or the list needs (gap G-L45). Children that
/// are not rows are left as laid (p.332, gap G-L38).
pub(super) fn align(node: &XfaNode, mut kids: Vec<&mut Placed>) {
    let fixed = column_widths(node);
    let columns = kids
        .iter()
        .filter_map(|k| k.row.as_ref())
        .map(|r| {
            r.cells
                .iter()
                .map(|c| usize::from(c.col_span.unwrap_or(1)))
                .sum()
        })
        .fold(fixed.len(), usize::max);
    let mut widths: Vec<f64> = vec![0.0; columns];
    for row in kids.iter().filter_map(|k| k.row.as_ref()) {
        for (cell, span) in row.cells.iter().zip(spans(&row.cells, columns)) {
            if let Some((c, 1)) = span
                && let Some(w) = widths.get_mut(c)
            {
                *w = w.max(cell.rect.w);
            }
        }
    }
    for (w, f) in widths.iter_mut().zip(&fixed) {
        if let Some(f) = f {
            *w = *f;
        }
    }
    for k in &mut kids {
        relay(k, &widths);
    }
}

/// Re-lay row `p` on `widths`: each displayed cell at its columns' left
/// edge, at least as wide as them and exactly as tall as the tallest cell.
fn relay(p: &mut Placed, widths: &[f64]) {
    let Some(row) = p.row.take() else { return };
    let [left, top, right, bottom] = row.inset;
    let total: f64 = widths.iter().sum();
    let sp = spans(&row.cells, widths.len());
    let shown: Vec<_> = row
        .cells
        .into_iter()
        .zip(sp)
        .filter_map(|(c, s)| s.map(|s| (c, s)))
        .collect();
    let row_h = shown.iter().fold(0.0_f64, |h, (c, _)| h.max(c.rect.h));
    let (mut items, mut flags) = (p.items.drain(..1).collect::<Vec<_>>(), Vec::new());
    flags.extend(p.flags.drain(..1));
    for (cell, (c, n)) in shown {
        let x0: f64 = widths.iter().take(c).sum();
        let w: f64 = widths.iter().skip(c).take(n).sum();
        let x = if row.rtl { total - x0 - w } else { x0 };
        let mut cell_items = cell.items;
        if let Some(first) = cell_items.first_mut() {
            first.rect.w = first.rect.w.max(w);
            first.rect.h = row_h;
        }
        items.extend(super::geometry::shifted(cell_items, left + x, top));
        flags.extend(cell.flags);
    }
    let w = row.given.0.unwrap_or(left + total + right);
    let h = row.given.1.unwrap_or(top + row_h + bottom);
    if let Some(own) = items.first_mut() {
        own.rect.w = w;
        own.rect.h = h;
    }
    (p.rect.w, p.rect.h) = (w, h);
    (p.items, p.flags) = (items, flags);
}
