//! One fill's edges cut into the scene's tiles, so a pixel evaluates only
//! the edges crossing its tile ([`super::fill_coverage`]'s sum, split up).
//!
//! On a supersampled row an edge adds its winding to every sample column
//! at or right of its rounded `x`. A tile lists the edges whose rounded `x`
//! falls strictly inside its columns on some row it is active; an edge it
//! does not list is, on each row, wholly left of the tile's first column
//! (counted in that row's backdrop) or wholly right of its last (counted
//! nowhere). Listed edges keep build order, which the walk's tie order uses.

use super::fill_edges::{FillEdges, SHIFT};

/// Tile width, pixels.
pub(crate) const TILE_W: i32 = 16;
/// Tile height, pixels.
pub(crate) const TILE_H: i32 = 4;
/// Supersampled rows a tile spans; an aliased fill uses the first
/// `TILE_H` of a backdrop.
pub(crate) const SUB_ROWS: usize = (TILE_H << SHIFT) as usize;

/// One tile's share of a fill.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct TileEntry {
    /// Indices into [`FillEdges::edges`], ascending.
    pub edges: Vec<u32>,
    /// Per supersampled row of the tile, the winding of unlisted edges
    /// left of its first sample column.
    pub backdrop: [i32; SUB_ROWS],
}

/// A fill's tiles over the tile rectangle covering its written pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FillTiles {
    /// First tile column and row.
    pub col0: i32,
    pub row0: i32,
    /// Tile columns and rows covered.
    pub cols: i32,
    pub rows: i32,
    /// Row-major; `None` where the fill covers nothing.
    pub entries: Vec<Option<TileEntry>>,
}

/// A 16.16 value rounded to the nearest integer, halves up.
pub(crate) fn round(x: i32) -> i32 {
    x.wrapping_add(0x8000) >> 16
}

/// `fill` cut into tiles.
pub(crate) fn build(fill: &FillEdges) -> FillTiles {
    let a = &fill.area;
    let col0 = a.x().div_euclid(TILE_W);
    let row0 = a.y().div_euclid(TILE_H);
    let cols = (a.right() - 1).div_euclid(TILE_W) - col0 + 1;
    let rows = (a.bottom() - 1).div_euclid(TILE_H) - row0 + 1;
    let sub_rows = TILE_H << fill.shift;
    let mut buckets: Vec<Vec<u32>> = vec![Vec::new(); rows as usize];
    for (i, e) in fill.edges.iter().enumerate() {
        let lo = e.first_y.max(fill.start_y).div_euclid(sub_rows) - row0;
        let hi = e.last_y.min(fill.stop_y - 1).div_euclid(sub_rows) - row0;
        for r in lo.max(0)..=hi.min(rows - 1) {
            if let Some(b) = buckets.get_mut(r as usize) {
                b.push(i as u32);
            }
        }
    }
    let mut entries = Vec::with_capacity((cols * rows) as usize);
    for (r, bucket) in buckets.iter().enumerate() {
        tile_row(fill, bucket, row0 + r as i32, (col0, cols), &mut entries);
    }
    FillTiles {
        col0,
        row0,
        cols,
        rows,
        entries,
    }
}

/// Appends tile row `row`'s entries, built from `bucket`, the edges active
/// in it.
fn tile_row(
    fill: &FillEdges,
    bucket: &[u32],
    row: i32,
    (col0, cols): (i32, i32),
    out: &mut Vec<Option<TileEntry>>,
) {
    let filled = |w: i32| if fill.even_odd { w & 1 != 0 } else { w != 0 };
    let (sub_rows, sub_cols) = (TILE_H << fill.shift, TILE_W << fill.shift);
    let y0 = row * sub_rows;
    let mut listed: Vec<Vec<u32>> = vec![Vec::new(); cols as usize];
    // Per row: winding steps by tile column, then per-tile corrections.
    let width = cols as usize + 1;
    let mut steps = vec![0i32; SUB_ROWS * width];
    let mut fix = vec![0i32; SUB_ROWS * cols as usize];
    let rows_of = |e: &super::fill_edges::LineEdge| {
        let lo = e.first_y.max(fill.start_y).max(y0);
        let hi = e.last_y.min(fill.stop_y - 1).min(y0 + sub_rows - 1);
        lo..=hi
    };
    let mut tiles: Vec<i32> = Vec::new();
    for &i in bucket {
        let Some(e) = fill.edges.get(i as usize) else {
            continue;
        };
        tiles.clear();
        for y in rows_of(e) {
            let rx = round(e.x_at(y));
            let t = rx.div_euclid(sub_cols) - col0;
            if rx.rem_euclid(sub_cols) != 0 && (0..cols).contains(&t) && !tiles.contains(&t) {
                tiles.push(t);
            }
        }
        for &t in &tiles {
            if let Some(l) = listed.get_mut(t as usize) {
                l.push(i);
            }
        }
        for y in rows_of(e) {
            let s = (y - y0) as usize;
            let rx = round(e.x_at(y));
            let first = (rx + sub_cols - 1).div_euclid(sub_cols) - col0;
            if first >= cols {
                continue;
            }
            let first = first.max(0);
            if let Some(v) = steps.get_mut(s * width + first as usize) {
                *v += e.winding;
            }
            for &t in tiles.iter().filter(|t| **t >= first) {
                if let Some(v) = fix.get_mut(s * cols as usize + t as usize) {
                    *v -= e.winding;
                }
            }
        }
    }
    let mut run = [0i32; SUB_ROWS];
    for (t, edges) in listed.into_iter().enumerate() {
        let mut backdrop = [0i32; SUB_ROWS];
        let mut any = !edges.is_empty();
        for ((s, b), r) in backdrop.iter_mut().enumerate().zip(run.iter_mut()) {
            *r += steps.get(s * width + t).copied().unwrap_or(0);
            *b = *r + fix.get(s * cols as usize + t).copied().unwrap_or(0);
            let y = y0 + s as i32;
            if y >= fill.start_y && y < fill.stop_y && filled(*b) {
                any = true;
            }
        }
        out.push(any.then_some(TileEntry { edges, backdrop }));
    }
}
