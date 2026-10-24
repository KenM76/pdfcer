//! Changed-pixel grouping for the visual comparison: changed pixels within
//! a merge gap of each other become one tight box, so one moved symbol is
//! one box and not the whole sheet.

use tiny_skia::Pixmap;

use crate::ChangedRegion;

/// A cell's changed-pixel bounds `(x0, y0, x1, y1)` and count.
type Cell = Option<(u32, u32, u32, u32, u64)>;

/// One group of changed pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Group {
    pub(crate) region: ChangedRegion,
    pub(crate) pixels: u64,
}

/// Group the pixels where `a` and `b` differ by more than `threshold` on
/// any channel. Both are read on a canvas the size of the larger, with
/// white wherever a pixmap does not reach (pages anchored top-left).
/// Pixels within `gap` of each other share a group; groups with fewer than
/// `min_pixels` changed pixels are dropped. Largest group first.
pub(crate) fn changed_groups(
    a: &Pixmap,
    b: &Pixmap,
    threshold: u8,
    gap: u32,
    min_pixels: u64,
) -> Vec<Group> {
    let (w, h) = (a.width().max(b.width()), a.height().max(b.height()));
    let c = gap.max(1);
    let (cw, ch) = (w.div_ceil(c) as usize, h.div_ceil(c) as usize);
    let mut cells: Vec<Cell> = vec![None; cw * ch];
    for y in 0..h {
        for x in 0..w {
            if delta(rgb_at(a, x, y), rgb_at(b, x, y)) <= threshold {
                continue;
            }
            let i = (y / c) as usize * cw + (x / c) as usize;
            if let Some(slot) = cells.get_mut(i) {
                *slot = Some(slot.map_or((x, y, x, y, 1), |(x0, y0, x1, y1, n)| {
                    (x0.min(x), y0.min(y), x1.max(x), y1.max(y), n + 1)
                }));
            }
        }
    }
    let mut seen = vec![false; cells.len()];
    let mut out = Vec::new();
    for start in 0..cells.len() {
        if seen.get(start).copied().unwrap_or(true) || cells.get(start).is_none_or(Option::is_none)
        {
            continue;
        }
        if let Some(g) = flood(&cells, &mut seen, start, cw, ch)
            && g.pixels >= min_pixels
        {
            out.push(g);
        }
    }
    out.sort_by_key(|g| std::cmp::Reverse(g.pixels));
    out
}

/// Union every changed cell 8-connected to `start`.
fn flood(cells: &[Cell], seen: &mut [bool], start: usize, cw: usize, ch: usize) -> Option<Group> {
    let mut stack = vec![start];
    let mut acc: Option<(u32, u32, u32, u32, u64)> = None;
    if let Some(s) = seen.get_mut(start) {
        *s = true;
    }
    while let Some(i) = stack.pop() {
        let Some(Some((x0, y0, x1, y1, n))) = cells.get(i).copied() else {
            continue;
        };
        acc = Some(acc.map_or((x0, y0, x1, y1, n), |(a0, b0, a1, b1, m)| {
            (a0.min(x0), b0.min(y0), a1.max(x1), b1.max(y1), m + n)
        }));
        let (cx, cy) = (i % cw, i / cw);
        for ny in cy.saturating_sub(1)..=(cy + 1).min(ch - 1) {
            for nx in cx.saturating_sub(1)..=(cx + 1).min(cw - 1) {
                let j = ny * cw + nx;
                let changed = cells.get(j).is_some_and(Option::is_some);
                if let Some(s) = seen.get_mut(j)
                    && changed
                    && !*s
                {
                    *s = true;
                    stack.push(j);
                }
            }
        }
    }
    acc.map(|(x0, y0, x1, y1, pixels)| Group {
        region: ChangedRegion {
            x: x0,
            y: y0,
            width: x1 - x0 + 1,
            height: y1 - y0 + 1,
        },
        pixels,
    })
}

/// The colour at `(x, y)`, white outside the pixmap.
fn rgb_at(p: &Pixmap, x: u32, y: u32) -> [u8; 3] {
    if x >= p.width() || y >= p.height() {
        return [255; 3];
    }
    let i = (y as usize * p.width() as usize + x as usize) * 4;
    match p.data().get(i..i + 3) {
        Some(&[r, g, b]) => [r, g, b],
        _ => [255; 3],
    }
}

fn delta(a: [u8; 3], b: [u8; 3]) -> u8 {
    a[0].abs_diff(b[0])
        .max(a[1].abs_diff(b[1]))
        .max(a[2].abs_diff(b[2]))
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)] // tests build their own pixmaps
mod tests {
    use super::*;
    use tiny_skia::{Color, Paint, Rect, Transform};

    fn sheet(w: u32, h: u32, boxes: &[(f32, f32)]) -> Pixmap {
        let mut p = Pixmap::new(w, h).unwrap();
        p.fill(Color::WHITE);
        let mut paint = Paint::default();
        paint.set_color(Color::BLACK);
        for &(x, y) in boxes {
            p.fill_rect(
                Rect::from_xywh(x, y, 4.0, 4.0).unwrap(),
                &paint,
                Transform::identity(),
                None,
            );
        }
        p
    }

    #[test]
    fn a_moved_box_is_two_tight_groups_not_the_sheet() {
        let a = sheet(200, 200, &[(10.0, 10.0), (100.0, 100.0)]);
        let b = sheet(200, 200, &[(10.0, 10.0), (160.0, 20.0)]);
        let g = changed_groups(&a, &b, 8, 6, 1);
        assert_eq!(g.len(), 2, "{g:?}");
        assert!(
            g.iter()
                .all(|g| g.region.width == 4 && g.region.height == 4),
            "{g:?}"
        );
    }

    #[test]
    fn near_neighbours_merge_and_specks_drop() {
        let a = sheet(100, 100, &[]);
        let b = sheet(100, 100, &[(10.0, 10.0), (16.0, 10.0)]);
        let g = changed_groups(&a, &b, 8, 6, 1);
        assert_eq!(g.len(), 1, "{g:?}");
        assert_eq!(g[0].region.width, 10);
        assert!(changed_groups(&a, &b, 8, 6, 100).is_empty());
    }

    #[test]
    fn the_smaller_page_reads_white_beyond_its_edge() {
        let a = sheet(50, 50, &[]);
        let b = sheet(60, 50, &[(54.0, 10.0)]);
        let g = changed_groups(&a, &b, 8, 6, 1);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].region.x, 54);
    }
}
