//! Page matching for the visual comparison: a coarse ink signature per page
//! and an order-preserving alignment of two signature lists, so inserted,
//! deleted and reordered pages pair up with the page they came from.

use pdfcer_core::document::Document;
use pdfcer_core::page_tree::Page;

use crate::{RenderError, RenderOptions, render_page_with};

/// Grid cells along a signature's long side.
const GRID: usize = 32;
/// Pixels along a signature render's long side.
const THUMB: f32 = 128.0;
/// Below this cell spread a page is near-flat (blank, or a scan's paper
/// tone), where correlation measures only noise.
const FLAT: f64 = 0.02;
/// Per-cell floor added to the overlap measure, so a blank page and a page
/// with one stamp still look alike.
const FLOOR: f64 = 0.02;
/// A pair must score within this of each page's best match to be paired in
/// order, so a weak in-order pair cannot beat an obvious moved page.
const MARGIN: f64 = 0.1;

/// A page's coarse appearance: ink coverage per grid cell, and its aspect.
#[derive(Debug, Clone)]
pub(crate) struct Signature {
    cells: Vec<f64>,
    cols: usize,
    rows: usize,
    aspect: f64,
}

/// Render `page` small and reduce it to a [`Signature`].
pub(crate) fn signature(
    doc: &Document,
    page: &Page,
    render: &RenderOptions,
) -> Result<Signature, RenderError> {
    let crop = page.crop_box;
    let long = (crop.urx - crop.llx).abs().max((crop.ury - crop.lly).abs());
    #[allow(clippy::cast_possible_truncation)] // a page side in points fits f32
    let scale = if long > 0.0 { THUMB / long as f32 } else { 1.0 };
    let r = render_page_with(doc, page, scale, render)?;
    let (w, h) = (r.pixmap.width() as usize, r.pixmap.height() as usize);
    let (cols, rows) = if w >= h {
        (GRID, (GRID * h / w.max(1)).max(1))
    } else {
        ((GRID * w / h.max(1)).max(1), GRID)
    };
    let mut sum = vec![0.0_f64; cols * rows];
    let mut n = vec![0_u32; cols * rows];
    for (i, px) in r.pixmap.data().as_chunks::<4>().0.iter().enumerate() {
        let (x, y) = (i % w.max(1), i / w.max(1));
        let cell = (y * rows / h.max(1)).min(rows - 1) * cols + (x * cols / w.max(1)).min(cols - 1);
        let [rr, g, b, _] = *px;
        let luma = (f64::from(rr) * 0.299 + f64::from(g) * 0.587 + f64::from(b) * 0.114) / 255.0;
        if let (Some(s), Some(c)) = (sum.get_mut(cell), n.get_mut(cell)) {
            *s += 1.0 - luma;
            *c += 1;
        }
    }
    let cells = sum
        .iter()
        .zip(&n)
        .map(|(s, c)| if *c > 0 { s / f64::from(*c) } else { 0.0 })
        .collect();
    Ok(Signature {
        cells,
        cols,
        rows,
        aspect: w as f64 / h.max(1) as f64,
    })
}

/// How alike two pages look, `0.0..=1.0`: the correlation of their ink
/// grids (so two sheets sharing a border and title block are told apart by
/// what differs), scaled down when their shapes differ. A near-flat
/// page (blank, or a scan's paper tone) is compared by ink overlap instead,
/// since its correlation is noise.
pub(crate) fn similarity(a: &Signature, b: &Signature) -> f64 {
    let shape = (a.aspect.min(b.aspect) / a.aspect.max(b.aspect)).clamp(0.0, 1.0);
    if (a.cols, a.rows) != (b.cols, b.rows) {
        return 0.0;
    }
    let n = a.cells.len().max(1) as f64;
    let (ma, mb) = (
        a.cells.iter().sum::<f64>() / n,
        b.cells.iter().sum::<f64>() / n,
    );
    let (mut cov, mut va, mut vb) = (0.0, 0.0, 0.0);
    for (x, y) in a.cells.iter().zip(&b.cells) {
        let (dx, dy) = (x - ma, y - mb);
        cov += dx * dy;
        va += dx * dx;
        vb += dy * dy;
    }
    let flat = (va / n).sqrt().min((vb / n).sqrt()) < FLAT;
    let corr = if flat {
        overlap(&a.cells, &b.cells)
    } else {
        cov / (va.sqrt() * vb.sqrt())
    };
    corr.max(0.0) * shape
}

/// Weighted overlap of two ink grids, with a per-cell floor.
fn overlap(a: &[f64], b: &[f64]) -> f64 {
    let floor = FLOOR * a.len() as f64;
    let (lo, hi) = a.iter().zip(b).fold((0.0, 0.0), |(lo, hi), (x, y)| {
        (lo + x.min(*y), hi + x.max(*y))
    });
    (lo + floor) / (hi + floor)
}

/// Whether `old[i]`/`new[j]` may pair in order: it clears `min` and is
/// within [`MARGIN`] of both pages' best score.
fn eligible(score: &[Vec<f64>], i: usize, j: usize, min: f64) -> bool {
    let s = score.get(i).and_then(|r| r.get(j)).copied().unwrap_or(0.0);
    let row = score
        .get(i)
        .map_or(0.0, |r| r.iter().copied().fold(0.0, f64::max));
    let col = score
        .iter()
        .filter_map(|r| r.get(j).copied())
        .fold(0.0, f64::max);
    s >= min && s >= row - MARGIN && s >= col - MARGIN
}

/// One pairing the alignment produced; indices are into the two signature
/// lists.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Pairing {
    Matched {
        old: usize,
        new: usize,
        score: f64,
        moved: bool,
    },
    Deleted {
        old: usize,
    },
    Inserted {
        new: usize,
    },
}

/// Pair `old` with `new`: an order-preserving alignment maximising total
/// similarity over pairs at or above `min`, then a greedy pass pairing the
/// leftovers that still clear `min` as moved pages. Output is in new-page
/// order, deleted pages after the new page they followed.
pub(crate) fn align(old: &[Signature], new: &[Signature], min: f64) -> Vec<Pairing> {
    let score: Vec<Vec<f64>> = old
        .iter()
        .map(|a| new.iter().map(|b| similarity(a, b)).collect())
        .collect();
    let at = |i: usize, j: usize| score.get(i).and_then(|r| r.get(j)).copied().unwrap_or(0.0);
    let (n, m) = (old.len(), new.len());
    // best[i][j]: best total over old[i..] and new[j..].
    let mut best = vec![vec![0.0_f64; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            let skip = cell(&best, i + 1, j).max(cell(&best, i, j + 1));
            let s = at(i, j);
            let take = if eligible(&score, i, j, min) {
                s + cell(&best, i + 1, j + 1)
            } else {
                f64::MIN
            };
            if let Some(c) = best.get_mut(i).and_then(|r| r.get_mut(j)) {
                *c = skip.max(take);
            }
        }
    }
    let (mut i, mut j, mut pairs) = (0, 0, Vec::new());
    while i < n && j < m {
        let s = at(i, j);
        let here = cell(&best, i, j);
        if eligible(&score, i, j, min) && (here - (s + cell(&best, i + 1, j + 1))).abs() < 1e-9 {
            pairs.push((i, j, s));
            i += 1;
            j += 1;
        } else if (here - cell(&best, i + 1, j)).abs() < 1e-9 {
            i += 1;
        } else {
            j += 1;
        }
    }
    let mut old_used = vec![false; n];
    let mut new_used = vec![false; m];
    for &(a, b, _) in &pairs {
        set(&mut old_used, a);
        set(&mut new_used, b);
    }
    let mut moved = Vec::new();
    let mut leftovers: Vec<(usize, usize, f64)> = (0..n)
        .filter(|a| !flag(&old_used, *a))
        .flat_map(|a| (0..m).filter(|b| !flag(&new_used, *b)).map(move |b| (a, b)))
        .map(|(a, b)| (a, b, at(a, b)))
        .filter(|t| t.2 >= min)
        .collect();
    leftovers.sort_by(|x, y| y.2.total_cmp(&x.2));
    for (a, b, s) in leftovers {
        if !flag(&old_used, a) && !flag(&new_used, b) {
            set(&mut old_used, a);
            set(&mut new_used, b);
            moved.push((a, b, s));
        }
    }
    order(&pairs, &moved, &old_used, &new_used)
}

fn cell(best: &[Vec<f64>], i: usize, j: usize) -> f64 {
    best.get(i).and_then(|r| r.get(j)).copied().unwrap_or(0.0)
}

fn flag(v: &[bool], i: usize) -> bool {
    v.get(i).copied().unwrap_or(false)
}

fn set(v: &mut [bool], i: usize) {
    if let Some(f) = v.get_mut(i) {
        *f = true;
    }
}

/// Lay pairings out in new-page order; a deleted page goes after the
/// last new page whose old partner precedes it.
fn order(
    pairs: &[(usize, usize, f64)],
    moved: &[(usize, usize, f64)],
    old_used: &[bool],
    new_used: &[bool],
) -> Vec<Pairing> {
    let mut by_new: Vec<Option<Pairing>> = vec![None; new_used.len()];
    for &(a, b, s) in pairs {
        if let Some(slot) = by_new.get_mut(b) {
            *slot = Some(Pairing::Matched {
                old: a,
                new: b,
                score: s,
                moved: false,
            });
        }
    }
    for &(a, b, s) in moved {
        if let Some(slot) = by_new.get_mut(b) {
            *slot = Some(Pairing::Matched {
                old: a,
                new: b,
                score: s,
                moved: true,
            });
        }
    }
    let mut out = Vec::new();
    let mut deleted = (0..old_used.len())
        .filter(|a| !flag(old_used, *a))
        .peekable();
    for (b, slot) in by_new.into_iter().enumerate() {
        let p = slot.unwrap_or(Pairing::Inserted { new: b });
        if let Pairing::Matched {
            old, moved: false, ..
        } = p
        {
            while let Some(&d) = deleted.peek() {
                if d >= old {
                    break;
                }
                out.push(Pairing::Deleted { old: d });
                deleted.next();
            }
        }
        out.push(p);
    }
    out.extend(deleted.map(|old| Pairing::Deleted { old }));
    out
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)] // tests index vectors they just built
mod tests {
    use super::*;

    /// A signature whose ink sits in one cell of a 4x4 grid.
    fn sig(hot: usize) -> Signature {
        let mut cells = vec![0.0; 16];
        cells[hot] = 1.0;
        Signature {
            cells,
            cols: 4,
            rows: 4,
            aspect: 1.0,
        }
    }

    fn kinds(p: &[Pairing]) -> Vec<String> {
        p.iter()
            .map(|p| match p {
                Pairing::Matched {
                    old,
                    new,
                    moved: false,
                    ..
                } => format!("{old}={new}"),
                Pairing::Matched { old, new, .. } => format!("{old}~{new}"),
                Pairing::Deleted { old } => format!("-{old}"),
                Pairing::Inserted { new } => format!("+{new}"),
            })
            .collect()
    }

    #[test]
    fn an_inserted_and_a_deleted_page_leave_the_rest_paired() {
        let old = [sig(0), sig(1), sig(2), sig(3)];
        let new = [sig(0), sig(9), sig(2), sig(3)];
        assert_eq!(
            kinds(&align(&old, &new, 0.5)),
            ["0=0", "+1", "-1", "2=2", "3=3"]
        );
    }

    #[test]
    fn a_weak_in_order_pair_loses_to_an_obvious_move() {
        // old[2] looks a little like new[1]; old[0] looks exactly like it.
        let mut weak = sig(2);
        weak.cells[0] = 0.7;
        let old = [sig(0), sig(1), weak];
        let new = [sig(1), sig(0)];
        assert_eq!(kinds(&align(&old, &new, 0.5)), ["1=0", "0~1", "-2"]);
    }

    #[test]
    fn a_page_moved_to_the_end_pairs_as_moved() {
        let old = [sig(0), sig(1), sig(2), sig(3)];
        let new = [sig(1), sig(2), sig(3), sig(0)];
        assert_eq!(kinds(&align(&old, &new, 0.5)), ["1=0", "2=1", "3=2", "0~3"]);
    }

    #[test]
    fn nothing_alike_matches_nothing() {
        let k = kinds(&align(&[sig(0)], &[sig(5)], 0.5));
        assert_eq!(k, ["+0", "-0"]);
    }
}
