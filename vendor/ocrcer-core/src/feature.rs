//! Glyph normalisation and the 107-dimensional feature extractor. Called
//! from both `ocrcer-build` (prototype bank) and this crate's own
//! recognition path, so it exists exactly once (see `CLAUDE.md` rule 4).
//!
//! Implements `docs/ARCHITECTURE.md` section 3.1 to the arithmetic: fixed
//! resampling, fixed gradient binning, fixed cut positions, fixed clamps,
//! `f64` accumulation narrowed to `f32` only on output, no transcendental
//! function anywhere (`sqrt` is the sole exception, since IEEE 754 requires
//! it to be correctly rounded on every target, wasm32 included).

use std::ops::Range;

use crate::image::components::{self, Connectivity};

/// One glyph hypothesis, normalised bitmap plus the line context needed for
/// baseline-relative geometry.
///
/// `ink` is row-major, `width * height` bytes; `0` is background, any
/// nonzero byte is ink. `baseline_dy` is the line's baseline in pixels
/// measured downward from this bitmap's top; it may be negative or exceed
/// `height`. `x_height` is the line's x-height in pixels and is expected to
/// be strictly positive — see [`extract`]'s degenerate-input policy for what
/// happens when a caller violates that.
#[derive(Debug, Clone, Copy)]
pub struct GlyphInput<'a> {
    pub ink: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub baseline_dy: f32,
    pub x_height: f32,
}

/// The hole count a feature vector carries, as the integer it was before the
/// extractor widened it to `f32`.
///
/// Lives here rather than beside either caller because the bank builder and
/// the matcher both have to read this dimension the same way forever
/// (`CLAUDE.md` rule 4): the gate in `ARCHITECTURE.md` section 4.1 step 1 is
/// an *exact* integer match, so a half-pixel disagreement between two copies
/// of this would silently gate away the right answer.
pub fn holes_of(raw: &[f32; FEATURE_DIMS]) -> u8 {
    raw[HOLE_COUNT].round().clamp(0.0, MAX_HOLES as f32) as u8
}

/// Most holes the extractor distinguishes; three or more is reported as two.
pub const MAX_HOLES: u8 = 2;

/// Total feature vector length. Fixed; see the dimension layout table in
/// ARCHITECTURE.md section 3.1.
pub const FEATURE_DIMS: usize = 107;

/// Identifies the feature-vector *definition* — what each dimension measures,
/// in what order, from what.
///
/// Every prototype ever computed is only meaningful against one of these.
/// Reordering a block, changing what a dimension measures, or changing the
/// source a dimension is taken from (the normalised grid versus the original
/// bitmap) makes every stored vector silently wrong: it still loads, it still
/// compares, and it matches the wrong things. Nothing in the numbers
/// themselves would say so.
///
/// So this is bumped on any such change, `ARCHITECTURE.md` section 11 records
/// why, and a model file whose `meta` names a different value is **refused**
/// at load rather than read. A runtime that reads a mismatched file is the
/// failure this constant exists to make impossible.
///
/// It is not a release number and does not move with the crate's version.
pub const FEATURE_VERSION: u32 = 1;

/// 4x4 zone ink density: mean of the normalised grid over each 8x8 zone.
pub const ZONE_DENSITY: Range<usize> = 0..16;
/// 4x4 zones x 4 Sobel gradient orientation bins.
pub const GRADIENT: Range<usize> = 16..80;
/// Horizontal projection profile, 8 row bins, top to bottom.
pub const H_PROJECTION: Range<usize> = 80..88;
/// Vertical projection profile, 8 column bins, left to right.
pub const V_PROJECTION: Range<usize> = 88..96;
/// Hole count (Euler number), clamped to `0..=2`. A single dimension.
pub const HOLE_COUNT: usize = 96;
/// Crossing counts: 3 horizontal cuts then 3 vertical cuts.
pub const CROSSINGS: Range<usize> = 97..103;
/// Aspect ratio, ink fraction, height above baseline, depth below.
pub const GEOMETRY: Range<usize> = 103..107;

const GRID: usize = 32;
const ZONES_PER_AXIS: usize = 4;
const ZONE_SIZE: usize = GRID / ZONES_PER_AXIS;

/// Extracts the 107-dimensional feature vector for one glyph hypothesis.
///
/// # Degenerate input
///
/// This function never panics on a merely unusual (as opposed to
/// contract-violating) glyph, and never returns a silent, undocumented
/// zero vector. Specifically:
///
/// - `width == 0` or `height == 0`: every dimension that depends on pixel
///   content degrades to `0.0` through ordinary arithmetic (there is no
///   ink to sample), except [`GEOMETRY`]`[0]` (aspect ratio), which would
///   divide `0/0`; that dimension is defined as `0.0` in this case.
/// - An all-background bitmap (`width` and `height` nonzero, no ink pixel
///   set): the same dimensions fall out to `0.0` naturally, since there is
///   no ink to sample; aspect ratio and the crossing/projection groups are
///   still computed normally from the bitmap's shape.
/// - `x_height <= 0` (violating the documented precondition that it be
///   strictly positive): [`GEOMETRY`]`[2]` and `[3]` (the baseline-relative
///   dimensions) are defined as `-1.0` rather than propagating NaN or an
///   infinity. This value is not meant to be distinguishable from a
///   legitimate extreme reading; it exists only so the output stays finite
///   and deterministic.
///
/// # Panics
/// Panics if `input.ink.len() != input.width as usize * input.height as usize`.
/// This is a contract violation, not a degenerate-but-valid input, so it is
/// not defined away.
pub fn extract(input: &GlyphInput<'_>) -> [f32; FEATURE_DIMS] {
    assert_eq!(
        input.ink.len(),
        input.width as usize * input.height as usize,
        "ink length must equal width*height"
    );

    let width = input.width;
    let height = input.height;
    let wf = width as f64;
    let hf = height as f64;

    let (cx, cy, ink_pixels) = centroid(input.ink, width, height);

    let grid = build_grid(input.ink, width, height, cx, cy);

    let mut out = [0.0f32; FEATURE_DIMS];

    fill_zone_density(&grid, &mut out);
    fill_gradient(&grid, &mut out);
    fill_projections(&grid, &mut out);

    out[HOLE_COUNT] = hole_count(input.ink, width, height) as f32;

    fill_crossings(&grid, &mut out);

    let aspect = if wf + hf > 0.0 {
        (wf - hf) / (wf + hf)
    } else {
        0.0
    };
    let ink_fraction = if wf * hf > 0.0 {
        ink_pixels as f64 / (wf * hf)
    } else {
        0.0
    };
    let x_height = input.x_height as f64;
    let baseline_dy = input.baseline_dy as f64;
    let (above, below) = if x_height > 0.0 {
        (
            (baseline_dy / x_height).clamp(-1.0, 4.0),
            ((hf - baseline_dy) / x_height).clamp(-1.0, 4.0),
        )
    } else {
        (-1.0, -1.0)
    };
    out[GEOMETRY.start] = aspect as f32;
    out[GEOMETRY.start + 1] = ink_fraction as f32;
    out[GEOMETRY.start + 2] = above as f32;
    out[GEOMETRY.start + 3] = below as f32;

    out
}

/// Ink centroid in source bitmap coordinates, and the ink pixel count.
/// Falls back to the bitmap's geometric center when there is no ink; that
/// choice is unobservable in the output, since the normalised grid is then
/// all-background regardless of where the (unused) centroid sits.
fn centroid(ink: &[u8], width: u32, height: u32) -> (f64, f64, u64) {
    let mut sum_x = 0.0f64;
    let mut sum_y = 0.0f64;
    let mut count = 0u64;
    for y in 0..height as usize {
        for x in 0..width as usize {
            if ink[y * width as usize + x] != 0 {
                sum_x += x as f64;
                sum_y += y as f64;
                count += 1;
            }
        }
    }
    if count > 0 {
        (sum_x / count as f64, sum_y / count as f64, count)
    } else {
        (width as f64 / 2.0, height as f64 / 2.0, 0)
    }
}

/// Builds the 32x32 normalised grid by area-averaging, per ARCHITECTURE.md
/// section 3.1: each destination cell is the ink fraction of its source
/// preimage rectangle under the inverse of `dst = (src - centroid) * s +
/// 15.5`, with out-of-bitmap preimage area counted as background.
// jy/jx feed the inverse-affine arithmetic below, not just the index.
#[allow(clippy::needless_range_loop)]
fn build_grid(ink: &[u8], width: u32, height: u32, cx: f64, cy: f64) -> [[f64; GRID]; GRID] {
    let mut grid = [[0.0f64; GRID]; GRID];
    let wf = width as f64;
    let hf = height as f64;
    let s = GRID as f64 / wf.max(hf);
    if s <= 0.0 || !s.is_finite() {
        return grid;
    }

    for jy in 0..GRID {
        let y0 = (jy as f64 - 15.5) / s + cy;
        let y1 = (jy as f64 + 1.0 - 15.5) / s + cy;
        for jx in 0..GRID {
            let x0 = (jx as f64 - 15.5) / s + cx;
            let x1 = (jx as f64 + 1.0 - 15.5) / s + cx;
            let area = (x1 - x0) * (y1 - y0);
            if area <= 0.0 {
                continue;
            }
            grid[jy][jx] = preimage_ink_fraction(ink, width, height, x0, x1, y0, y1) / area;
        }
    }
    grid
}

/// Sum of ink * overlap-area over the intersection of the source bitmap
/// with rectangle `[x0,x1) x [y0,y1)`, in source pixel coordinates.
fn preimage_ink_fraction(ink: &[u8], width: u32, height: u32, x0: f64, x1: f64, y0: f64, y1: f64) -> f64 {
    let w = width as usize;
    let h = height as usize;
    let iy_lo = y0.floor().max(0.0) as usize;
    let iy_hi = (y1.ceil().min(h as f64)) as usize;
    let ix_lo = x0.floor().max(0.0) as usize;
    let ix_hi = (x1.ceil().min(w as f64)) as usize;
    let mut acc = 0.0f64;
    for iy in iy_lo..iy_hi {
        let oy = (y1.min(iy as f64 + 1.0) - y0.max(iy as f64)).max(0.0);
        if oy <= 0.0 {
            continue;
        }
        for ix in ix_lo..ix_hi {
            let ox = (x1.min(ix as f64 + 1.0) - x0.max(ix as f64)).max(0.0);
            if ox <= 0.0 {
                continue;
            }
            if ink[iy * w + ix] != 0 {
                acc += ox * oy;
            }
        }
    }
    acc
}

fn zone_of(row: usize, col: usize) -> usize {
    (row / ZONE_SIZE) * ZONES_PER_AXIS + col / ZONE_SIZE
}

fn fill_zone_density(grid: &[[f64; GRID]; GRID], out: &mut [f32; FEATURE_DIMS]) {
    let mut sums = [0.0f64; 16];
    for row in 0..GRID {
        for col in 0..GRID {
            sums[zone_of(row, col)] += grid[row][col];
        }
    }
    let cells_per_zone = (ZONE_SIZE * ZONE_SIZE) as f64;
    for (i, s) in sums.iter().enumerate() {
        out[ZONE_DENSITY.start + i] = (s / cells_per_zone) as f32;
    }
}

/// Sobel gradient, zero-padded at the border, folded so `gy >= 0` (edges
/// are undirected), binned by comparison alone per ARCHITECTURE.md section
/// 3.1, magnitude-accumulated per zone and normalised so the 64-dim group
/// sums to 1 (left at all-zero if the grid carries no gradient energy at
/// all, e.g. an all-background glyph).
fn fill_gradient(grid: &[[f64; GRID]; GRID], out: &mut [f32; FEATURE_DIMS]) {
    let g = |row: isize, col: isize| -> f64 {
        if row < 0 || col < 0 || row >= GRID as isize || col >= GRID as isize {
            0.0
        } else {
            grid[row as usize][col as usize]
        }
    };

    let mut bins = [0.0f64; 64];
    let mut total = 0.0f64;

    for row in 0..GRID {
        for col in 0..GRID {
            let r = row as isize;
            let c = col as isize;
            let gx = (g(r - 1, c + 1) + 2.0 * g(r, c + 1) + g(r + 1, c + 1))
                - (g(r - 1, c - 1) + 2.0 * g(r, c - 1) + g(r + 1, c - 1));
            let gy = (g(r + 1, c - 1) + 2.0 * g(r + 1, c) + g(r + 1, c + 1))
                - (g(r - 1, c - 1) + 2.0 * g(r - 1, c) + g(r - 1, c + 1));
            let magnitude = (gx * gx + gy * gy).sqrt();
            if magnitude == 0.0 {
                continue;
            }
            let (fgx, fgy) = if gy < 0.0 { (-gx, -gy) } else { (gx, gy) };
            let bin = if fgx > 0.0 && fgy < fgx {
                0
            } else if fgx > 0.0 && fgy >= fgx {
                1
            } else if fgx <= 0.0 && fgy >= -fgx {
                2
            } else {
                3
            };
            bins[zone_of(row, col) * 4 + bin] += magnitude;
            total += magnitude;
        }
    }

    if total > 0.0 {
        for (i, b) in bins.iter().enumerate() {
            out[GRADIENT.start + i] = (b / total) as f32;
        }
    }
}

fn fill_projections(grid: &[[f64; GRID]; GRID], out: &mut [f32; FEATURE_DIMS]) {
    let mut row_sum = [0.0f64; GRID];
    let mut col_sum = [0.0f64; GRID];
    let mut total = 0.0f64;
    for row in 0..GRID {
        for col in 0..GRID {
            let v = grid[row][col];
            row_sum[row] += v;
            col_sum[col] += v;
            total += v;
        }
    }
    if total <= 0.0 {
        return;
    }
    for bin in 0..8 {
        let h: f64 = row_sum[bin * 4..bin * 4 + 4].iter().sum();
        out[H_PROJECTION.start + bin] = (h / total) as f32;
        let v: f64 = col_sum[bin * 4..bin * 4 + 4].iter().sum();
        out[V_PROJECTION.start + bin] = (v / total) as f32;
    }
}

fn fill_crossings(grid: &[[f64; GRID]; GRID], out: &mut [f32; FEATURE_DIMS]) {
    const CUTS: [usize; 3] = [8, 16, 24];
    let thresholded = |v: f64| v >= 0.5;

    for (i, &r) in CUTS.iter().enumerate() {
        let mut crossings = 0u32;
        let mut prev = false;
        for &cell in &grid[r] {
            let cur = thresholded(cell);
            if !prev && cur {
                crossings += 1;
            }
            prev = cur;
        }
        out[CROSSINGS.start + i] = (crossings.min(6) as f64 / 6.0) as f32;
    }
    for (i, &c) in CUTS.iter().enumerate() {
        let mut crossings = 0u32;
        let mut prev = false;
        for row in grid {
            let cur = thresholded(row[c]);
            if !prev && cur {
                crossings += 1;
            }
            prev = cur;
        }
        out[CROSSINGS.start + 3 + i] = (crossings.min(6) as f64 / 6.0) as f32;
    }
}

/// Hole count on the *original* bitmap (not the normalised grid, per
/// ARCHITECTURE.md section 3.1): background components not touching the
/// bitmap border, background labelled 4-connected, clamped to `0..=2`.
fn hole_count(ink: &[u8], width: u32, height: u32) -> u32 {
    let w = width as usize;
    let h = height as usize;
    if w == 0 || h == 0 {
        return 0;
    }
    let bg: Vec<u8> = ink.iter().map(|&v| if v == 0 { 1 } else { 0 }).collect();
    let (labels, count) = components::label(&bg, width, height, Connectivity::Four);
    if count == 0 {
        return 0;
    }
    let mut touches_border = vec![false; count as usize + 1];
    for x in 0..w {
        mark(&labels, &mut touches_border, x);
        mark(&labels, &mut touches_border, (h - 1) * w + x);
    }
    for y in 0..h {
        mark(&labels, &mut touches_border, y * w);
        mark(&labels, &mut touches_border, y * w + w - 1);
    }
    let holes = (1..=count).filter(|&l| !touches_border[l as usize]).count() as u32;
    holes.min(2)
}

fn mark(labels: &[u32], touches_border: &mut [bool], idx: usize) {
    let l = labels[idx];
    if l != 0 {
        touches_border[l as usize] = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyph(ink: &[u8], width: u32, height: u32, baseline_dy: f32, x_height: f32) -> [f32; FEATURE_DIMS] {
        extract(&GlyphInput {
            ink,
            width,
            height,
            baseline_dy,
            x_height,
        })
    }

    fn solid(width: u32, height: u32) -> Vec<u8> {
        vec![1u8; (width * height) as usize]
    }

    // ---- hole count ----

    #[test]
    fn hole_count_solid_rectangle_is_zero() {
        let ink = solid(10, 10);
        let out = glyph(&ink, 10, 10, 8.0, 6.0);
        assert_eq!(out[HOLE_COUNT], 0.0);
    }

    #[test]
    fn hole_count_ring_is_one() {
        let w = 10usize;
        let h = 10usize;
        let mut ink = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                if y == 0 || y == h - 1 || x == 0 || x == w - 1 {
                    ink[y * w + x] = 1;
                }
            }
        }
        let out = glyph(&ink, w as u32, h as u32, 8.0, 6.0);
        assert_eq!(out[HOLE_COUNT], 1.0);
    }

    #[test]
    fn hole_count_figure_eight_is_two() {
        // Two 5x5 rings sharing no pixels, separated by a solid column so
        // each interior stays enclosed, joined by a solid bridge so the
        // ink is one shape.
        let w = 11usize;
        let h = 5usize;
        let mut ink = vec![0u8; w * h];
        let ring = |ink: &mut Vec<u8>, x_off: usize| {
            for y in 0..h {
                for x in 0..5 {
                    if y == 0 || y == h - 1 || x == 0 || x == 4 {
                        ink[y * w + x + x_off] = 1;
                    }
                }
            }
        };
        ring(&mut ink, 0);
        ring(&mut ink, 6);
        // bridge the two rings along the middle row so it is one component
        for x in 4..7 {
            ink[2 * w + x] = 1;
        }
        let out = glyph(&ink, w as u32, h as u32, 4.0, 3.0);
        assert_eq!(out[HOLE_COUNT], 2.0);
    }

    #[test]
    fn hole_count_ring_with_wall_gap_is_zero() {
        let w = 10usize;
        let h = 10usize;
        let mut ink = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                if y == 0 || y == h - 1 || x == 0 || x == w - 1 {
                    ink[y * w + x] = 1;
                }
            }
        }
        // punch a one-pixel gap in the top wall
        ink[5] = 0;
        let out = glyph(&ink, w as u32, h as u32, 8.0, 6.0);
        assert_eq!(out[HOLE_COUNT], 0.0);
    }

    #[test]
    fn hole_count_three_holes_clamps_to_two() {
        // Three separate rings bridged into one shape, each enclosing its
        // own hole: the count of *distinct* holes is 3, clamped to 2.
        let w = 17usize;
        let h = 5usize;
        let mut ink = vec![0u8; w * h];
        let ring = |ink: &mut Vec<u8>, x_off: usize| {
            for y in 0..h {
                for x in 0..5 {
                    if y == 0 || y == h - 1 || x == 0 || x == 4 {
                        ink[y * w + x + x_off] = 1;
                    }
                }
            }
        };
        ring(&mut ink, 0);
        ring(&mut ink, 6);
        ring(&mut ink, 12);
        for x in 4..7 {
            ink[2 * w + x] = 1;
        }
        for x in 10..13 {
            ink[2 * w + x] = 1;
        }
        let out = glyph(&ink, w as u32, h as u32, 4.0, 3.0);
        assert_eq!(out[HOLE_COUNT], 2.0);
    }

    // ---- shape and range invariants ----

    #[test]
    fn output_has_107_dimensions() {
        let ink = solid(12, 12);
        let out = glyph(&ink, 12, 12, 9.0, 7.0);
        assert_eq!(out.len(), FEATURE_DIMS);
    }

    #[test]
    fn gradient_group_sums_to_one() {
        let ink = solid(16, 20);
        let out = glyph(&ink, 16, 20, 15.0, 10.0);
        let sum: f32 = out[GRADIENT].iter().sum();
        assert!((sum - 1.0).abs() < 1e-4, "gradient sum = {sum}");
    }

    #[test]
    fn projection_groups_sum_to_one() {
        let ink = solid(16, 20);
        let out = glyph(&ink, 16, 20, 15.0, 10.0);
        let h_sum: f32 = out[H_PROJECTION].iter().sum();
        let v_sum: f32 = out[V_PROJECTION].iter().sum();
        assert!((h_sum - 1.0).abs() < 1e-4, "h sum = {h_sum}");
        assert!((v_sum - 1.0).abs() < 1e-4, "v sum = {v_sum}");
    }

    #[test]
    fn zone_densities_are_in_unit_range() {
        let ink = solid(16, 20);
        let out = glyph(&ink, 16, 20, 15.0, 10.0);
        for &v in &out[ZONE_DENSITY] {
            assert!((0.0..=1.0).contains(&v), "zone density {v} out of range");
        }
    }

    // ---- geometry, and the o/O case discriminator ----

    #[test]
    fn geometry_matches_hand_computation() {
        let ink = solid(10, 20);
        let out = glyph(&ink, 10, 20, 16.0, 8.0);
        let expect_aspect = (10.0 - 20.0) / (10.0 + 20.0);
        assert!((out[GEOMETRY.start] - expect_aspect as f32).abs() < 1e-6);
        assert!((out[GEOMETRY.start + 1] - 1.0).abs() < 1e-6); // fully ink
        let expect_above = (16.0f32 / 8.0).clamp(-1.0, 4.0);
        let expect_below = ((20.0f32 - 16.0) / 8.0).clamp(-1.0, 4.0);
        assert!((out[GEOMETRY.start + 2] - expect_above).abs() < 1e-6);
        assert!((out[GEOMETRY.start + 3] - expect_below).abs() < 1e-6);
    }

    #[test]
    fn same_shape_at_different_heights_separates_only_on_geometry() {
        // The apostrophe / comma case, and the reason the baseline-relative
        // group exists. Both are classes in the charset, both are the same
        // mark, and they are distinguished by nothing except where the mark
        // sits relative to the baseline of the line it is on. So: one bitmap,
        // one x-height, two heights above the baseline. Everything that
        // describes shape must be bit-identical, and only 105 and 106 may move.
        let w = 4usize;
        let h = 6usize;
        let mut ink = vec![0u8; w * h];
        for y in 0..h {
            for x in 1..w - 1 {
                ink[y * w + x] = 1;
            }
        }
        let x_height = 10.0;

        // Apostrophe: hangs at x-height, its foot well clear of the baseline.
        let high = glyph(&ink, w as u32, h as u32, x_height, x_height);
        // Comma: its top at the baseline, its tail descending below it.
        let low = glyph(&ink, w as u32, h as u32, 0.0, x_height);

        for i in 0..105 {
            assert_eq!(high[i], low[i], "dim {i} describes shape and must not move");
        }
        assert_ne!(high[105], low[105], "height above baseline must separate them");
        assert_ne!(high[106], low[106], "depth below baseline must separate them");

        // And the sign of each is the thing a decoder can actually reason
        // about, not merely that two floats differ.
        assert!(high[105] > low[105]);
        assert!(low[106] > high[106]);
    }

    #[test]
    fn case_pair_separates_when_only_its_size_relative_to_the_line_changes() {
        // 'o' and 'O' are the same shape at different sizes on one line. The
        // honest assertion is narrower than it looks: the two are NOT identical
        // outside the geometry group, because a stroke one pixel wide is a
        // larger fraction of a small glyph than of a big one, so ink fraction
        // moves too. What must hold is that the two baseline-relative
        // dimensions separate them decisively, since that is all the decoder
        // has to go on for case.
        let ring = |n: usize| {
            let mut ink = vec![0u8; n * n];
            for y in 0..n {
                for x in 0..n {
                    if y == 0 || y == n - 1 || x == 0 || x == n - 1 {
                        ink[y * n + x] = 1;
                    }
                }
            }
            ink
        };
        let x_height = 10.0;
        let lower = ring(10);
        let upper = ring(14);
        // Both sit on the same baseline; each glyph's top is its own height
        // above it.
        let o = glyph(&lower, 10, 10, 10.0, x_height);
        let big_o = glyph(&upper, 14, 14, 14.0, x_height);

        assert_eq!(o[103], big_o[103], "both are square, so aspect must agree");
        assert!(
            big_o[105] > o[105] + 0.3,
            "cap height must read as clearly taller than x-height: {} vs {}",
            big_o[105],
            o[105]
        );
    }

    // ---- determinism ----

    #[test]
    fn extraction_is_deterministic() {
        let w = 13usize;
        let h = 17usize;
        let mut ink = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                if (x + y) % 3 == 0 {
                    ink[y * w + x] = 1;
                }
            }
        }
        let a = glyph(&ink, w as u32, h as u32, 12.0, 9.0);
        let b = glyph(&ink, w as u32, h as u32, 12.0, 9.0);
        assert_eq!(a, b);
    }

    // ---- degenerate input ----

    #[test]
    fn zero_width_does_not_panic_and_zeroes_pixel_dimensions() {
        let out = glyph(&[], 0, 5, 3.0, 2.0);
        assert_eq!(out.len(), FEATURE_DIMS);
        for &v in &out[ZONE_DENSITY] {
            assert_eq!(v, 0.0);
        }
        assert_eq!(out[GEOMETRY.start], -1.0); // (0-5)/(0+5)
    }

    #[test]
    fn zero_width_and_height_does_not_produce_nan() {
        let out = glyph(&[], 0, 0, 0.0, 0.0);
        assert!(out.iter().all(|v| v.is_finite()));
        assert_eq!(out[GEOMETRY.start], 0.0);
        assert_eq!(out[GEOMETRY.start + 1], 0.0);
    }

    #[test]
    fn all_background_bitmap_has_no_nan_and_zero_ink_fraction() {
        let ink = vec![0u8; 9 * 9];
        let out = glyph(&ink, 9, 9, 5.0, 4.0);
        assert!(out.iter().all(|v| v.is_finite()));
        assert_eq!(out[GEOMETRY.start + 1], 0.0);
        assert_eq!(out[HOLE_COUNT], 0.0);
    }

    #[test]
    fn non_positive_x_height_does_not_produce_nan() {
        let ink = solid(6, 6);
        let out = glyph(&ink, 6, 6, 3.0, 0.0);
        assert!(out.iter().all(|v| v.is_finite()));
        assert_eq!(out[GEOMETRY.start + 2], -1.0);
        assert_eq!(out[GEOMETRY.start + 3], -1.0);
    }

    #[test]
    #[should_panic(expected = "ink length must equal width*height")]
    fn mismatched_ink_length_panics() {
        let _ = glyph(&[1, 0, 1], 2, 2, 1.0, 1.0);
    }
}
