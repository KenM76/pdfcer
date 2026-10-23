//! A table's cell boundaries measured from the page image.
//!
//! Each axis is measured on its own, first from drawn ruling lines, then
//! from the blank gutters between cells, and is used only when the image
//! gives exactly one boundary per cell edge. Any other count leaves that
//! axis to the even grid, so a missing, broken or extra line or gap can
//! never shift a cell onto the wrong text.
//!
//! - A **rule** is a run of at most [`MAX_RULE`] pixel rows (or columns)
//!   whose ink covers at least [`RULE_COVER`] of the crop.
//! - A **gutter** is an interior run of at least [`MIN_GUTTER`] rows (or
//!   columns) with no ink once every rule is masked out, so a horizontal
//!   rule does not close a column gutter and a border rule does not turn
//!   the cell padding inside it into one. With more gutters than cell
//!   edges, the widest are used only when each is at least
//!   [`GUTTER_MARGIN`] times as wide as every one left over.

use super::vl_page::{EdgeSource, TableEdges};
use super::vl_pre::{Grey, INK_BELOW};

/// Ink must cover at least this share of a pixel row/column to be a rule.
/// Row/column spans break a rule for the spanned cells, so it is not 1.0;
/// a text line through a table leaves cell padding blank, so it rarely
/// reaches it.
const RULE_COVER: f64 = 0.6;
/// The thickest run still counted as one rule; thicker is a filled band.
const MAX_RULE: usize = 8;
/// Rule runs this many pixels apart or closer are one rule (double lines,
/// antialiasing).
const MERGE_GAP: usize = 2;
/// The narrowest blank run counted as a gutter.
const MIN_GUTTER: usize = 2;
/// How many times wider than every unused gap a chosen gutter must be.
const GUTTER_MARGIN: usize = 2;

/// The axis being measured.
#[derive(Clone, Copy)]
pub(super) enum Axis {
    /// Column boundaries, from vertical rules or gutters.
    Columns,
    /// Row boundaries, from horizontal rules or gutters.
    Rows,
}

impl Axis {
    fn other(self) -> Self {
        match self {
            Self::Columns => Self::Rows,
            Self::Rows => Self::Columns,
        }
    }

    /// Lines along this axis, and pixels across each.
    fn extent(self, img: &Grey) -> (usize, usize) {
        match self {
            Self::Columns => (img.width, img.height),
            Self::Rows => (img.height, img.width),
        }
    }
}

/// A run of pixel lines `start..end`.
type Run = (usize, usize);

/// The `n + 1` boundaries of `n` cells along `axis`, in `img` pixels,
/// ascending, and what they were measured from; `None` when neither the
/// rules nor the gutters give exactly that many.
pub(super) fn edges(img: &Grey, axis: Axis, n: usize) -> Option<TableEdges> {
    let (len, _) = axis.extent(img);
    if n == 0 || len == 0 {
        return None;
    }
    let rules = rule_runs(img, axis);
    if let Some(e) = from_rules(&rules, len, n) {
        return Some(TableEdges::new(e, EdgeSource::Rules));
    }
    let mut counts = ink_counts(img, axis, &rule_runs(img, axis.other()));
    for &(a, b) in &rules {
        counts
            .get_mut(a..b)
            .into_iter()
            .flatten()
            .for_each(|c| *c = 0);
    }
    from_gutters(&counts, n).map(|e| TableEdges::new(e, EdgeSource::Gutters))
}

/// Ink per line along `axis`, skipping pixels on the `masked` lines of the
/// other axis.
fn ink_counts(img: &Grey, axis: Axis, masked: &[Run]) -> Vec<usize> {
    let (w, h) = (img.width, img.height);
    let mut counts = vec![0usize; axis.extent(img).0];
    let hidden = |k: usize| masked.iter().any(|&(a, b)| (a..b).contains(&k));
    for (i, &p) in img.pixels.iter().enumerate().take(w * h) {
        let (along, across) = match axis {
            Axis::Columns => (i % w, i / w),
            Axis::Rows => (i / w, i % w),
        };
        if p < INK_BELOW
            && !hidden(across)
            && let Some(c) = counts.get_mut(along)
        {
            *c += 1;
        }
    }
    counts
}

/// Thin runs of covered lines along `axis`, nearby runs merged.
fn rule_runs(img: &Grey, axis: Axis) -> Vec<Run> {
    let (_, across) = axis.extent(img);
    let mut runs: Vec<Run> = Vec::new();
    for (i, c) in ink_counts(img, axis, &[]).into_iter().enumerate() {
        #[allow(clippy::cast_precision_loss)] // pixel counts are capped by vl_pre::MAX_INPUT_PIXELS
        if (c as f64) < RULE_COVER * across as f64 {
            continue;
        }
        match runs.last_mut() {
            Some(r) if i <= r.1 + MERGE_GAP => r.1 = i + 1,
            _ => runs.push((i, i + 1)),
        }
    }
    runs.retain(|(a, b)| b - a <= MAX_RULE);
    runs
}

#[allow(clippy::cast_precision_loss)] // pixel indices are capped by vl_pre::MAX_INPUT_PIXELS
fn centre((a, b): Run) -> f64 {
    (a + b) as f64 / 2.0
}

/// Edges at the rule centres; a missing outer border is supplied by the
/// crop side, so a table with interior rules only still measures.
fn from_rules(rules: &[Run], len: usize, n: usize) -> Option<Vec<f64>> {
    let (&first, &last) = (rules.first()?, rules.last()?);
    #[allow(clippy::cast_precision_loss)] // pixel counts are capped by vl_pre::MAX_INPUT_PIXELS
    let (len, tol) = (len as f64, (len / (4 * n)).max(1) as f64);
    let mut out = Vec::with_capacity(n + 1);
    if centre(first) > tol {
        out.push(0.0);
    }
    out.extend(rules.iter().copied().map(centre));
    if centre(last) < len - tol {
        out.push(len);
    }
    (out.len() == n + 1).then_some(out)
}

/// Edges at the centres of the `n - 1` gutters, the crop sides outside.
fn from_gutters(counts: &[usize], n: usize) -> Option<Vec<f64>> {
    if n < 2 {
        return None;
    }
    let mut gaps: Vec<Run> = Vec::new();
    let mut start = None;
    for (i, &c) in counts.iter().enumerate() {
        match (c == 0, start) {
            (true, None) => start = Some(i),
            // A blank run from the crop side is its margin, not a gutter;
            // one still open at the end is the far margin and never closes.
            (false, Some(a)) => {
                if a > 0 && i - a >= MIN_GUTTER {
                    gaps.push((a, i));
                }
                start = None;
            }
            _ => {}
        }
    }
    let width = |r: &Run| r.1 - r.0;
    let mut by_width = gaps.clone();
    by_width.sort_by_key(|r| std::cmp::Reverse(width(r)));
    let chosen = by_width.get(..n - 1)?;
    if let (Some(narrowest), Some(left)) = (chosen.last(), by_width.get(n - 1))
        && width(narrowest) < GUTTER_MARGIN * width(left)
    {
        return None;
    }
    gaps.retain(|g| chosen.contains(g));
    #[allow(clippy::cast_precision_loss)] // pixel counts are capped by vl_pre::MAX_INPUT_PIXELS
    let len = counts.len() as f64;
    let mut out = vec![0.0];
    out.extend(gaps.into_iter().map(centre));
    out.push(len);
    Some(out)
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)] // fixed-size synthetic images
mod tests {
    use super::*;

    /// A white `w`×`h` image with the given rules drawn 2 px thick, plus a
    /// short "text" stroke in every cell.
    fn ruled(w: usize, h: usize, xs: &[usize], ys: &[usize]) -> Grey {
        let mut pixels = vec![255u8; w * h];
        for &x in xs {
            for y in 0..h {
                for dx in 0..2 {
                    pixels[y * w + (x + dx).min(w - 1)] = 0;
                }
            }
        }
        for &y in ys {
            for x in 0..w {
                for dy in 0..2 {
                    pixels[(y + dy).min(h - 1) * w + x] = 0;
                }
            }
        }
        for y in (10..h).step_by(30) {
            for x in (5..w).step_by(50) {
                for k in 0..20 {
                    pixels[y * w + (x + k).min(w - 1)] = 0;
                }
            }
        }
        Grey {
            width: w,
            height: h,
            pixels,
        }
    }

    /// A white `w`×`h` image with a solid "text" block `x0..x1`×`y0..y1`
    /// per entry.
    fn blocks(w: usize, h: usize, cells: &[(usize, usize, usize, usize)]) -> Grey {
        let mut pixels = vec![255u8; w * h];
        for &(x0, y0, x1, y1) in cells {
            for y in y0..y1 {
                pixels[y * w + x0..y * w + x1].fill(0);
            }
        }
        Grey {
            width: w,
            height: h,
            pixels,
        }
    }

    fn measured(img: &Grey, axis: Axis, n: usize) -> Option<(Vec<f64>, EdgeSource)> {
        edges(img, axis, n).map(|e| (e.edges, e.source))
    }

    fn rules_at(e: &[f64]) -> Option<(Vec<f64>, EdgeSource)> {
        Some((e.to_vec(), EdgeSource::Rules))
    }

    fn gutters_at(e: &[f64]) -> Option<(Vec<f64>, EdgeSource)> {
        Some((e.to_vec(), EdgeSource::Gutters))
    }

    /// Two columns (x 10..60, 120..190) by three rows (y 5..15, 25..35,
    /// 45..55) of text blocks.
    fn unruled() -> Grey {
        let mut cells = Vec::new();
        for y in [5, 25, 45] {
            cells.push((10, y, 60, y + 10));
            cells.push((120, y, 190, y + 10));
        }
        blocks(200, 60, &cells)
    }

    #[test]
    fn a_fully_ruled_table_measures_both_axes() {
        let img = ruled(300, 90, &[0, 80, 298], &[0, 30, 88]);
        assert_eq!(
            measured(&img, Axis::Columns, 2),
            rules_at(&[1.0, 81.0, 299.0])
        );
        assert_eq!(measured(&img, Axis::Rows, 2), rules_at(&[1.0, 31.0, 89.0]));
    }

    #[test]
    fn interior_rules_take_the_crop_sides_as_borders() {
        let img = ruled(300, 90, &[100, 200], &[]);
        assert_eq!(
            measured(&img, Axis::Columns, 3),
            rules_at(&[0.0, 101.0, 201.0, 300.0])
        );
    }

    #[test]
    fn a_count_that_disagrees_with_the_structure_is_refused() {
        let img = ruled(300, 90, &[0, 100, 298], &[]);
        assert_eq!(measured(&img, Axis::Columns, 3), None);
        assert_eq!(measured(&img, Axis::Columns, 1), None);
    }

    #[test]
    fn evenly_pitched_text_with_no_rules_measures_nothing() {
        // The strokes leave 30 px gutters every 50 px: eight gaps of one
        // width, so no three of them stand out.
        let img = ruled(400, 90, &[], &[]);
        assert_eq!(measured(&img, Axis::Columns, 3), None);
    }

    #[test]
    fn a_thick_band_is_not_a_rule_and_a_double_line_is_one() {
        let mut img = ruled(300, 90, &[], &[40, 43]);
        assert_eq!(measured(&img, Axis::Rows, 2), rules_at(&[0.0, 42.5, 90.0]));
        for y in 40..60 {
            for x in 0..300 {
                img.pixels[y * 300 + x] = 0;
            }
        }
        assert_eq!(
            measured(&img, Axis::Rows, 2).map(|m| m.1),
            Some(EdgeSource::Gutters)
        );
    }

    #[test]
    fn an_unruled_table_measures_from_its_gutters() {
        let img = unruled();
        assert_eq!(
            measured(&img, Axis::Columns, 2),
            gutters_at(&[0.0, 90.0, 200.0])
        );
        assert_eq!(
            measured(&img, Axis::Rows, 3),
            gutters_at(&[0.0, 20.0, 40.0, 60.0])
        );
    }

    #[test]
    fn rules_on_the_other_axis_do_not_close_a_gutter() {
        // Booktabs: full-width horizontal rules top, under the header and
        // bottom cross the column gutter.
        let mut img = unruled();
        for y in [0, 20, 58] {
            img.pixels[y * 200..(y + 2) * 200].fill(0);
        }
        assert_eq!(
            measured(&img, Axis::Columns, 2),
            gutters_at(&[0.0, 90.0, 200.0])
        );
    }

    #[test]
    fn a_border_rule_does_not_make_the_padding_a_gutter() {
        // A box around the table, no interior rules: two vertical rules
        // where three edges are needed, so the gutter decides.
        let mut img = unruled();
        for y in 0..60 {
            img.pixels[y * 200] = 0;
            img.pixels[y * 200 + 199] = 0;
        }
        assert_eq!(
            measured(&img, Axis::Columns, 2),
            gutters_at(&[0.0, 90.0, 200.0])
        );
    }

    #[test]
    fn gutters_that_do_not_stand_out_are_refused() {
        // Two equal gaps where one column edge is asked for: neither
        // stands out. Asked for both, both are used.
        let img = blocks(
            200,
            20,
            &[(10, 5, 60, 15), (70, 5, 90, 15), (100, 5, 190, 15)],
        );
        assert_eq!(measured(&img, Axis::Columns, 2), None);
        assert_eq!(
            measured(&img, Axis::Columns, 3),
            gutters_at(&[0.0, 65.0, 95.0, 200.0])
        );
        assert_eq!(measured(&img, Axis::Columns, 4), None);
    }
}
