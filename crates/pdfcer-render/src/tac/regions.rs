//! Over-limit pixels grouped into regions: 8-connected components of
//! eighth-inch cells, each bounded by its exact over-limit pixels.

use pdfcer_core::page_tree::{Page, Rect};
use pdfcer_core::preflight::{TacMeasurement, TacRegion, TacRequest};

use super::TacRaster;

/// Per-cell tallies of over-limit pixels.
#[derive(Debug, Clone, Copy, Default)]
struct Cell {
    over: u64,
    max: u16,
    bounds: Option<[u32; 4]>,
}

impl Cell {
    fn add(&mut self, x: u32, y: u32, t: u16) {
        self.over += 1;
        self.max = self.max.max(t);
        self.bounds = Some(match self.bounds {
            Some([x0, y0, x1, y1]) => [x0.min(x), y0.min(y), x1.max(x), y1.max(y)],
            None => [x, y, x, y],
        });
    }

    fn merge(&mut self, other: &Self) {
        self.over += other.over;
        self.max = self.max.max(other.max);
        self.bounds = match (self.bounds, other.bounds) {
            (Some([a, b, c, d]), Some([e, f, g, h])) => {
                Some([a.min(e), b.min(f), c.max(g), d.max(h)])
            }
            (a, b) => a.or(b),
        };
    }
}

/// `raster` measured against `request`; `s` is device pixels per default
/// user-space unit (scale × `UserUnit`).
pub(super) fn measure(
    raster: &TacRaster,
    page: &Page,
    s: f64,
    request: &TacRequest,
) -> TacMeasurement {
    let limit = request.limit_percent * 10.0;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // ppi ≤ 600
    let side = ((request.ppi / 8.0).round() as u32).max(1);
    let (cw, ch) = (raster.width.div_ceil(side), raster.height.div_ceil(side));
    let mut cells = vec![Cell::default(); (cw as usize) * (ch as usize)];
    let mut over = 0_u64;
    for (i, &t) in raster.tenths.iter().enumerate() {
        if f64::from(t) <= limit {
            continue;
        }
        over += 1;
        #[allow(clippy::cast_possible_truncation)] // i < width × height
        let (x, y) = ((i as u32) % raster.width, (i as u32) / raster.width);
        if let Some(cell) = cells.get_mut(((y / side) * cw + x / side) as usize) {
            cell.add(x, y, t);
        }
    }
    let mut found = components(&cells, cw, ch);
    found.sort_by_key(|c| std::cmp::Reverse(c.over));
    let regions = fold(found, request.max_regions.max(1))
        .into_iter()
        .filter_map(|(cell, merged)| {
            let rect = to_page(page, s, cell.bounds?);
            Some(TacRegion::new(rect, percent(cell.max), cell.over).with_merged(merged))
        })
        .collect();
    let mut m = TacMeasurement::default();
    m.max_percent = percent(raster.max_tenths());
    m.pixels = u64::from(raster.width) * u64::from(raster.height);
    m.over_pixels = over;
    m.regions = regions;
    m
}

fn percent(tenths: u16) -> f64 {
    f64::from(tenths) / 10.0
}

/// The 8-connected components of cells holding over-limit pixels.
fn components(cells: &[Cell], cw: u32, ch: u32) -> Vec<Cell> {
    let mut seen = vec![false; cells.len()];
    // Marks `i` seen and says whether it starts or joins a component.
    let mut claim = |i: usize| {
        let inked = cells.get(i).is_some_and(|c| c.over > 0);
        match seen.get_mut(i) {
            Some(s) if inked && !*s => {
                *s = true;
                true
            }
            _ => false,
        }
    };
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for start in 0..cells.len() {
        if !claim(start) {
            continue;
        }
        stack.push(start);
        let mut total = Cell::default();
        while let Some(i) = stack.pop() {
            if let Some(c) = cells.get(i) {
                total.merge(c);
            }
            #[allow(clippy::cast_possible_truncation)] // i < cw × ch
            let (x, y) = ((i as u32) % cw, (i as u32) / cw);
            for (nx, ny) in neighbours(x, y, cw, ch) {
                let j = (ny * cw + nx) as usize;
                if claim(j) {
                    stack.push(j);
                }
            }
        }
        out.push(total);
    }
    out
}

fn neighbours(x: u32, y: u32, cw: u32, ch: u32) -> impl Iterator<Item = (u32, u32)> {
    let xs = x.saturating_sub(1)..=(x + 1).min(cw - 1);
    xs.flat_map(move |nx| (y.saturating_sub(1)..=(y + 1).min(ch - 1)).map(move |ny| (nx, ny)))
        .filter(move |&p| p != (x, y))
}

/// At most `max` regions: the largest `max - 1` kept, the rest bounded
/// together in one, each with how many areas it stands for.
fn fold(found: Vec<Cell>, max: usize) -> Vec<(Cell, usize)> {
    if found.len() <= max {
        return found.into_iter().map(|c| (c, 1)).collect();
    }
    let mut out: Vec<(Cell, usize)> = found.iter().take(max - 1).map(|&c| (c, 1)).collect();
    let mut rest = Cell::default();
    for c in found.iter().skip(max - 1) {
        rest.merge(c);
    }
    out.push((rest, found.len() - (max - 1)));
    out
}

/// Pixel bounds `[x0, y0, x1, y1]` (inclusive) as a page-space rectangle:
/// the inverse of the render's crop-box and `/Rotate` mapping.
fn to_page(page: &Page, s: f64, [x0, y0, x1, y1]: [u32; 4]) -> Rect {
    let c = page.crop_box;
    let at = |px: u32, py: u32| {
        let (u, v) = (f64::from(px) / s, f64::from(py) / s);
        match page.rotate {
            90 => (c.llx + v, c.lly + u),
            180 => (c.urx - u, c.lly + v),
            270 => (c.urx - v, c.ury - u),
            _ => (c.llx + u, c.ury - v),
        }
    };
    let (ax, ay) = at(x0, y0);
    let (bx, by) = at(x1 + 1, y1 + 1);
    Rect::from_corners(ax, ay, bx, by)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::indexing_slicing)]
    use super::*;

    fn cell(over: u64, b: [u32; 4]) -> Cell {
        Cell {
            over,
            max: 4000,
            bounds: Some(b),
        }
    }

    #[test]
    fn folding_keeps_the_largest_and_bounds_the_rest() {
        let found = vec![
            cell(9, [0, 0, 2, 2]),
            cell(4, [10, 10, 11, 11]),
            cell(1, [20, 0, 20, 0]),
        ];
        let out = fold(found, 2);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].1, 1);
        assert_eq!(out[1].1, 2, "two areas folded into one");
        assert_eq!(out[1].0.over, 5);
        assert_eq!(out[1].0.bounds, Some([10, 0, 20, 11]));
    }

    #[test]
    fn diagonal_cells_join_and_separate_cells_do_not() {
        let mut cells = vec![Cell::default(); 9];
        cells[0] = cell(1, [0, 0, 0, 0]);
        cells[4] = cell(1, [1, 1, 1, 1]);
        cells[2] = Cell::default();
        let joined = components(&cells, 3, 3);
        assert_eq!(joined.len(), 1, "8-connected");
        cells[4] = Cell::default();
        cells[8] = cell(1, [2, 2, 2, 2]);
        assert_eq!(components(&cells, 3, 3).len(), 2);
    }
}
