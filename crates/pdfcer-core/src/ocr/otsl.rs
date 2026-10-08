//! OTSL, the table language PaddleOCR-VL answers "Table Recognition:" in
//! (Lysak et al., "Optimized Table Tokenization for Table Structure
//! Recognition", 2023): a row-major grid of cell tokens, `<nl>` ending each
//! row.
//!
//! | Token | Cell |
//! |---|---|
//! | `<fcel>` | starts a cell; its text follows, up to the next token |
//! | `<ecel>` | starts an empty cell |
//! | `<lcel>` | continues the cell to its left (column span) |
//! | `<ucel>` | continues the cell above (row span) |
//! | `<xcel>` | continues both (a 2-D span) |
//! | `<nl>` | ends the row |
//!
//! Model output is not trusted to be well formed: a continuation with no
//! cell to continue starts an empty one, short rows are padded, and the
//! grid is capped at [`MAX_CELLS`].

/// Most cells one table may produce; the rest are dropped.
pub const MAX_CELLS: usize = 10_000;

/// One table cell, in grid units.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct TableCell {
    /// First row, from 0.
    pub row: usize,
    /// First column, from 0.
    pub col: usize,
    /// Rows spanned, at least 1.
    pub row_span: usize,
    /// Columns spanned, at least 1.
    pub col_span: usize,
    /// The cell's text, trimmed; empty for `<ecel>`.
    pub text: String,
}

/// A parsed table.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Table {
    /// Rows in the grid.
    pub rows: usize,
    /// Columns in the widest row.
    pub cols: usize,
    /// Cells in row-major order of their first grid square.
    pub cells: Vec<TableCell>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tok {
    F,
    E,
    L,
    U,
    X,
    Nl,
}

const TOKENS: [(&str, Tok); 6] = [
    ("<fcel>", Tok::F),
    ("<ecel>", Tok::E),
    ("<lcel>", Tok::L),
    ("<ucel>", Tok::U),
    ("<xcel>", Tok::X),
    ("<nl>", Tok::Nl),
];

/// Split into `(token, text after it)`; text before the first token is
/// dropped, and a `<` that starts no token is cell text.
fn tokens(s: &str) -> Vec<(Tok, &str)> {
    let mut out = Vec::new();
    let mut pending: Option<(Tok, usize)> = None;
    let mut i = 0;
    while let Some(off) = s.get(i..).and_then(|t| t.find('<')) {
        let at = i + off;
        let tail = s.get(at..).unwrap_or("");
        if let Some(&(t, k)) = TOKENS.iter().find(|(t, _)| tail.starts_with(t)) {
            if let Some((pk, start)) = pending.take() {
                out.push((pk, s.get(start..at).unwrap_or("")));
            }
            pending = Some((k, at + t.len()));
            i = at + t.len();
        } else {
            i = at + 1;
        }
    }
    if let Some((pk, start)) = pending {
        out.push((pk, s.get(start..).unwrap_or("")));
    }
    out
}

/// Parse OTSL into a grid of cells.
#[must_use]
pub fn parse(s: &str) -> Table {
    let mut grid: Vec<Vec<Option<usize>>> = vec![Vec::new()];
    let mut cells: Vec<TableCell> = Vec::new();
    for (tok, text) in tokens(s) {
        let r = grid.len() - 1;
        let c = grid.last().map_or(0, Vec::len);
        let owner = match tok {
            Tok::Nl => {
                grid.push(Vec::new());
                continue;
            }
            Tok::F | Tok::E => None,
            Tok::L => left(&grid, r, c),
            Tok::U => up(&grid, r, c),
            Tok::X => up(&grid, r, c).or_else(|| left(&grid, r, c)),
        };
        let id = match owner {
            Some(id) => id,
            None if cells.len() >= MAX_CELLS => break,
            None => {
                let text = if tok == Tok::F { text.trim() } else { "" };
                cells.push(TableCell {
                    row: r,
                    col: c,
                    row_span: 1,
                    col_span: 1,
                    text: text.to_owned(),
                });
                cells.len() - 1
            }
        };
        if let Some(cell) = cells.get_mut(id) {
            cell.row_span = cell.row_span.max(r + 1 - cell.row);
            cell.col_span = cell.col_span.max(c + 1 - cell.col);
        }
        if let Some(row) = grid.last_mut() {
            row.push(Some(id));
        }
    }
    while grid.last().is_some_and(Vec::is_empty) {
        grid.pop();
    }
    Table {
        rows: grid.len(),
        cols: grid.iter().map(Vec::len).max().unwrap_or(0),
        cells,
    }
}

fn left(grid: &[Vec<Option<usize>>], r: usize, c: usize) -> Option<usize> {
    let c = c.checked_sub(1)?;
    *grid.get(r)?.get(c)?
}

fn up(grid: &[Vec<Option<usize>>], r: usize, c: usize) -> Option<usize> {
    let r = r.checked_sub(1)?;
    *grid.get(r)?.get(c)?
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn cell(t: &Table, row: usize, col: usize) -> &TableCell {
        t.cells
            .iter()
            .find(|c| c.row == row && c.col == col)
            .expect("cell")
    }

    #[test]
    fn a_plain_grid() {
        let t = parse("<fcel>Month<fcel>Units<nl><fcel>Jan<fcel>100<nl><fcel>Feb<ecel><nl>");
        assert_eq!((t.rows, t.cols, t.cells.len()), (3, 2, 6));
        assert_eq!(cell(&t, 0, 1).text, "Units");
        assert_eq!(cell(&t, 2, 1).text, "");
        assert_eq!((cell(&t, 1, 1).row_span, cell(&t, 1, 1).col_span), (1, 1));
    }

    #[test]
    fn spans_across_and_down() {
        // A header over two columns; a row label over two rows; a 2x2 block.
        let t = parse(
            "<fcel>H<lcel><fcel>A<lcel><nl>\
             <fcel>R<fcel>1<fcel>B<lcel><nl>\
             <ucel><fcel>2<ucel><xcel><nl>",
        );
        assert_eq!((t.rows, t.cols), (3, 4));
        let h = cell(&t, 0, 0);
        assert_eq!((h.text.as_str(), h.row_span, h.col_span), ("H", 1, 2));
        let r = cell(&t, 1, 0);
        assert_eq!((r.row_span, r.col_span), (2, 1));
        let b = cell(&t, 1, 2);
        assert_eq!((b.text.as_str(), b.row_span, b.col_span), ("B", 2, 2));
        assert_eq!(t.cells.len(), 6);
    }

    #[test]
    fn malformed_output_degrades_to_cells() {
        // Leading prose, an orphan continuation, a stray `<`, no final <nl>.
        let t = parse("table: <lcel><fcel>a < b<ucel>");
        assert_eq!(t.rows, 1);
        assert_eq!(t.cells[0].text, "");
        assert_eq!(t.cells[1].text, "a < b");
        assert_eq!(t.cells.len(), 3);
        assert_eq!(parse(""), Table::default());
        assert_eq!(parse("no table here").cells.len(), 0);
    }

    #[test]
    fn the_cell_count_is_capped() {
        let t = parse(&"<ecel>".repeat(MAX_CELLS + 5));
        assert_eq!(t.cells.len(), MAX_CELLS);
    }
}
