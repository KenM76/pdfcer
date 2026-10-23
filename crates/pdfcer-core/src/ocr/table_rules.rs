//! A table's cell boundaries measured from its drawn ruling lines.
//!
//! A rule is a thin run of pixel rows (or columns) whose ink covers most of
//! the crop's width (or height). Each axis is measured on its own and is
//! only used when the rules give exactly one boundary per cell edge; any
//! other count leaves that axis to the even grid, so a missing, broken or
//! extra line can never shift a cell onto the wrong text.

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

/// The axis being measured.
#[derive(Clone, Copy)]
pub(super) enum Axis {
    /// Column boundaries, from vertical rules.
    Columns,
    /// Row boundaries, from horizontal rules.
    Rows,
}

/// The `n + 1` boundaries of `n` cells along `axis`, in `img` pixels,
/// ascending; `None` when the drawn rules do not give exactly that many.
///
/// A missing outer border is supplied by the crop's side, so a table with
/// interior rules only still measures.
pub(super) fn edges(img: &Grey, axis: Axis, n: usize) -> Option<Vec<f64>> {
    let len = match axis {
        Axis::Columns => img.width,
        Axis::Rows => img.height,
    };
    if n == 0 || len == 0 {
        return None;
    }
    let rules = rules(&coverage(img, axis));
    let (&first, &last) = (rules.first()?, rules.last()?);
    #[allow(clippy::cast_precision_loss)] // pixel counts are capped by vl_pre::MAX_INPUT_PIXELS
    let (len, tol) = (len as f64, (len / (4 * n)).max(1) as f64);
    let mut out = Vec::with_capacity(n + 1);
    if first > tol {
        out.push(0.0);
    }
    out.extend_from_slice(&rules);
    if last < len - tol {
        out.push(len);
    }
    (out.len() == n + 1).then_some(out)
}

/// Share of each pixel row (`Rows`) or column (`Columns`) that is ink.
fn coverage(img: &Grey, axis: Axis) -> Vec<f64> {
    let (w, h) = (img.width, img.height);
    let mut ink = vec![0usize; if matches!(axis, Axis::Rows) { h } else { w }];
    for (i, &p) in img.pixels.iter().enumerate().take(w * h) {
        if p < INK_BELOW {
            let k = if matches!(axis, Axis::Rows) {
                i / w
            } else {
                i % w
            };
            if let Some(c) = ink.get_mut(k) {
                *c += 1;
            }
        }
    }
    let across = if matches!(axis, Axis::Rows) { w } else { h };
    #[allow(clippy::cast_precision_loss)] // pixel counts are capped by vl_pre::MAX_INPUT_PIXELS
    ink.into_iter().map(|c| c as f64 / across as f64).collect()
}

/// The centre of every rule: a thin run of covered lines, nearby runs merged.
fn rules(cover: &[f64]) -> Vec<f64> {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    for (i, &c) in cover.iter().enumerate() {
        if c < RULE_COVER {
            continue;
        }
        match runs.last_mut() {
            Some(r) if i <= r.1 + MERGE_GAP => r.1 = i + 1,
            _ => runs.push((i, i + 1)),
        }
    }
    #[allow(clippy::cast_precision_loss)] // pixel indices are capped by vl_pre::MAX_INPUT_PIXELS
    runs.into_iter()
        .filter(|(a, b)| b - a <= MAX_RULE)
        .map(|(a, b)| (a + b) as f64 / 2.0)
        .collect()
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

    #[test]
    fn a_fully_ruled_table_measures_both_axes() {
        let img = ruled(300, 90, &[0, 80, 298], &[0, 30, 88]);
        assert_eq!(edges(&img, Axis::Columns, 2), Some(vec![1.0, 81.0, 299.0]));
        assert_eq!(edges(&img, Axis::Rows, 2), Some(vec![1.0, 31.0, 89.0]));
    }

    #[test]
    fn interior_rules_take_the_crop_sides_as_borders() {
        let img = ruled(300, 90, &[100, 200], &[]);
        assert_eq!(
            edges(&img, Axis::Columns, 3),
            Some(vec![0.0, 101.0, 201.0, 300.0])
        );
    }

    #[test]
    fn a_count_that_disagrees_with_the_structure_is_refused() {
        let img = ruled(300, 90, &[0, 100, 298], &[]);
        assert_eq!(edges(&img, Axis::Columns, 3), None);
        assert_eq!(edges(&img, Axis::Columns, 1), None);
    }

    #[test]
    fn an_unruled_table_and_text_rows_measure_nothing() {
        let img = ruled(300, 90, &[], &[]);
        assert_eq!(edges(&img, Axis::Rows, 3), None);
        assert_eq!(edges(&img, Axis::Columns, 3), None);
    }

    #[test]
    fn a_thick_band_is_not_a_rule_and_a_double_line_is_one() {
        let mut img = ruled(300, 90, &[], &[40, 43]);
        assert_eq!(edges(&img, Axis::Rows, 2), Some(vec![0.0, 42.5, 90.0]));
        for y in 40..60 {
            for x in 0..300 {
                img.pixels[y * 300 + x] = 0;
            }
        }
        assert_eq!(edges(&img, Axis::Rows, 2), None);
    }
}
