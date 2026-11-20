//! A pixel's anti-aliased fill coverage from [`FillEdges`], evaluated per
//! pixel as the fragment shader does, equal to what `tiny-skia` 0.11's
//! scanline walk accumulates for it.
//!
//! The walk blits, on each of a pixel's four supersampled rows, the spans
//! where the winding of the edges at or left of a sample column satisfies
//! the fill rule; a span covering all four columns adds the row's full
//! value (64, or 63 on the last row), a partial one 16 a column, and the
//! sum is held to 255. A full row is two spans, so adds 64 even on the
//! last row, where edges meeting inside the pixel bring the winding to
//! zero in the walk's order and back: the order of edges at one `x` is
//! their order the row before, so equal edges sort by their previous `x`,
//! then the one active longer first, then build order.

use super::bins::round;
use super::fill_edges::{FillEdges, LineEdge, SHIFT};

/// The coverage `tiny-skia` writes at pixel (`px`, `py`), 0 outside
/// `edges.area`.
pub(crate) fn pixel(edges: &FillEdges, px: i32, py: i32) -> u8 {
    let a = &edges.area;
    if px < a.x() || px >= a.right() || py < a.y() || py >= a.bottom() {
        return 0;
    }
    let mut sum = 0u32;
    for k in 0..4 {
        let y = (py << SHIFT) + k;
        if y < edges.start_y || y >= edges.stop_y {
            continue;
        }
        sum += row(edges, px << SHIFT, y, k == 3);
    }
    sum.min(255) as u8
}

/// What supersampled row `y` adds to the pixel whose first sample column
/// is `c0`.
fn row(edges: &FillEdges, c0: i32, y: i32, last: bool) -> u32 {
    let filled = |w: i32| if edges.even_odd { w & 1 != 0 } else { w != 0 };
    let mut winding = [0i32; 4];
    for (_, e) in active(edges, y) {
        let x = round(e.x_at(y));
        for (c, w) in (c0..).zip(winding.iter_mut()) {
            if x <= c {
                *w += e.winding;
            }
        }
    }
    let covered = winding.iter().filter(|w| filled(**w)).count() as u32;
    if covered < 4 {
        return 16 * covered;
    }
    if last && !split(edges, c0, y) { 63 } else { 64 }
}

fn active(edges: &FillEdges, y: i32) -> impl Iterator<Item = (usize, &LineEdge)> + '_ {
    edges
        .edges
        .iter()
        .enumerate()
        .filter(move |(_, e)| e.first_y <= y && y <= e.last_y)
}

/// Whether the walk ends a span and starts another at a sample column
/// inside the pixel whose first is `c0`, on a row it covers fully: the
/// winding is unfilled after an edge followed by another at the same `x`.
fn split(edges: &FillEdges, c0: i32, y: i32) -> bool {
    let filled = |w: i32| if edges.even_odd { w & 1 != 0 } else { w != 0 };
    let mut list: Vec<(usize, &LineEdge)> = active(edges, y).collect();
    list.sort_by(|(i, a), (j, b)| order(y, (*i, a), (*j, b)));
    let xs: Vec<i32> = list.iter().map(|(_, e)| round(e.x_at(y))).collect();
    let mut w = 0;
    for (n, ((_, e), x)) in list.iter().zip(&xs).enumerate() {
        w += e.winding;
        if *x > c0 && *x < c0 + 4 && xs.get(n + 1) == Some(x) && !filled(w) {
            return true;
        }
    }
    false
}

/// The walk's order of two edges on row `y`.
fn order(y: i32, (i, a): (usize, &LineEdge), (j, b): (usize, &LineEdge)) -> core::cmp::Ordering {
    let (xa, xb) = (a.x_at(y), b.x_at(y));
    if xa != xb {
        return xa.cmp(&xb);
    }
    let (new_a, new_b) = (a.first_y == y, b.first_y == y);
    if !new_a && !new_b && a.dx != b.dx {
        return xa.wrapping_sub(a.dx).cmp(&xb.wrapping_sub(b.dx));
    }
    a.first_y.cmp(&b.first_y).then(i.cmp(&j))
}

#[cfg(test)]
#[path = "fill_coverage_tests.rs"]
mod tests;
