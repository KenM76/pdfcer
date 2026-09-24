//! Skew estimate and correction.
//!
//! # Contract
//!
//! [`estimate`] reads an ink mask and returns the page's skew as a **slope**
//! — pixels of vertical drift per pixel of horizontal travel — searched
//! coarse-to-fine by maximising the sharpness of the horizontal projection
//! profile. Text lines produce sharp profile peaks only when horizontal
//! (`ARCHITECTURE.md` section 6). [`correct`] applies it to the grayscale
//! page.
//!
//! **Slope, not degrees, and a shear, not a rotation.** Both for
//! determinism: converting between an angle and a displacement needs `sin`,
//! `cos` or `tan`, none of which IEEE 754 requires to be correctly rounded,
//! so the same page could deskew differently on x86 and wasm32 and every
//! golden fixture downstream would be platform-dependent. A vertical shear
//! needs multiplication and rounding only. Across the ±5° this stage
//! searches, a shear and a rotation differ by under 0.4% in horizontal
//! scale, which is smaller than the scale spread the prototype bank already
//! spans.

use crate::image::Gray;

/// The widest skew searched, as a slope. `tan 5°`, rounded down, from
/// section 6's ±5 degrees. A page more skewed than this was fed in sideways
/// and is not a deskew problem.
pub const MAX_SLOPE: f64 = 0.0874;

/// Below this the page is left alone: a line drifts under one pixel across a
/// 600-pixel column, which is inside the line grouper's own tolerance, so
/// resampling would cost sharpness and buy nothing.
pub const MIN_CORRECTED_SLOPE: f64 = 0.0015;

/// A deskewed page, owning its pixels.
pub struct Deskewed {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    /// The slope that was removed. Zero when the page was left alone.
    pub slope: f64,
}

impl Deskewed {
    pub fn gray(&self) -> Gray<'_> {
        Gray { width: self.width, height: self.height, data: &self.data }
    }
}

/// Estimates the page's skew from its ink mask.
///
/// Returns a slope in `-MAX_SLOPE..=MAX_SLOPE`. A page with no ink returns
/// `0.0`.
///
/// # Panics
/// Panics if `mask.len() != width * height`.
pub fn estimate(mask: &[u8], width: u32, height: u32) -> f64 {
    estimate_with(mask, width, height, MAX_SLOPE)
}

/// [`estimate`] with the search limit from a parameter block.
///
/// The limit is a parameter rather than a constant because it is the one
/// number here a page can argue with: a scan that is genuinely more skewed
/// than the default allows is clamped to the default and stays crooked, and
/// the operator wants to be able to widen it without a rebuild.
pub fn estimate_with(mask: &[u8], width: u32, height: u32, max_slope: f64) -> f64 {
    let max_slope = if max_slope > 0.0 { max_slope } else { MAX_SLOPE };
    let w = width as usize;
    let h = height as usize;
    assert_eq!(mask.len(), w * h, "mask length must equal width*height");
    if w == 0 || h == 0 {
        return 0.0;
    }

    // One pass to collect ink coordinates; every candidate slope after this
    // is a pass over ink rather than over the page.
    let mut ink: Vec<(f64, f64)> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            if mask[y * w + x] != 0 {
                ink.push((x as f64, y as f64));
            }
        }
    }
    if ink.is_empty() {
        return 0.0;
    }

    // Coarse to fine. The steps are exact binary fractions so the candidate
    // set is reproducible bit for bit on any target.
    let mut best = 0.0f64;
    let mut span = max_slope;
    for step in [1.0 / 128.0, 1.0 / 1024.0, 1.0 / 8192.0] {
        best = search(&ink, h, w, best, span, step, max_slope);
        span = step * 2.0;
    }
    best.clamp(-max_slope, max_slope)
}

/// Scans slopes in `centre ± span` at `step`, returning the sharpest.
fn search(
    ink: &[(f64, f64)],
    h: usize,
    w: usize,
    centre: f64,
    span: f64,
    step: f64,
    max_slope: f64,
) -> f64 {
    let n = (span / step).round() as i64;
    let mut best = centre;
    let mut best_score = f64::NEG_INFINITY;
    for i in -n..=n {
        let slope = centre + step * i as f64;
        if slope.abs() > max_slope {
            continue;
        }
        let score = sharpness(ink, h, w, slope);
        // Strictly greater, and candidates are visited from the most
        // negative slope upward, so a tie needs the explicit rule: a page is
        // presumed straight unless the evidence says otherwise, so the
        // slope nearer zero wins.
        if score > best_score || (score == best_score && slope.abs() < best.abs()) {
            best_score = score;
            best = slope;
        }
    }
    best
}

/// How sharply the ink stacks into rows after shearing by `slope`.
///
/// The sum of squared row counts. Total ink is invariant under the shear, so
/// this is the row-count variance up to a constant, and it is maximal when
/// text lines are horizontal — which is what section 6's projection-profile
/// criterion says.
fn sharpness(ink: &[(f64, f64)], h: usize, w: usize, slope: f64) -> f64 {
    let pad = (slope.abs() * w as f64).ceil() as usize + 1;
    let mut rows = vec![0u32; h + 2 * pad];
    for &(x, y) in ink {
        let r = (y - slope * x).round() + pad as f64;
        if r >= 0.0 {
            let r = r as usize;
            if r < rows.len() {
                rows[r] += 1;
            }
        }
    }
    rows.iter().map(|&c| f64::from(c) * f64::from(c)).sum()
}

/// Shears a grayscale page to remove `slope`.
///
/// The canvas grows vertically by however much the shear displaces the
/// corners, so no ink is pushed off the page. New pixels are filled with the
/// page's border value rather than with black, which would read as ink.
/// Vertical interpolation is linear; nothing moves horizontally, so there is
/// no horizontal resampling at all.
pub fn correct(img: &Gray<'_>, slope: f64) -> Deskewed {
    correct_with(img, slope, MIN_CORRECTED_SLOPE)
}

/// [`correct`] with the do-nothing threshold from a parameter block.
///
/// Below `min_slope` the page is returned untouched. Resampling a page that
/// is already straight costs a generation of interpolation for nothing, and
/// the threshold is where "already straight" is defined.
pub fn correct_with(img: &Gray<'_>, slope: f64, min_slope: f64) -> Deskewed {
    let w = img.width as usize;
    let h = img.height as usize;
    assert_eq!(img.data.len(), w * h, "image data length must equal width*height");
    if w == 0 || h == 0 || slope.abs() < min_slope {
        return Deskewed {
            width: img.width,
            height: img.height,
            data: img.data.to_vec(),
            slope: 0.0,
        };
    }

    let pad = (slope.abs() * (w as f64 - 1.0)).ceil() as usize;
    let out_h = h + pad;
    let fill = border_value(img.data, w, h);
    let mut out = vec![fill; w * out_h];

    // Output row r at column x samples source row r - pad_top + slope * x,
    // where pad_top places the displaced corner back on the canvas.
    let pad_top = if slope > 0.0 { 0.0 } else { pad as f64 };
    for x in 0..w {
        let shift = slope * x as f64;
        for r in 0..out_h {
            let sy = r as f64 - pad_top + shift;
            let y0 = sy.floor();
            let t = sy - y0;
            let y0 = y0 as i64;
            let a = sample(img.data, w, h, x, y0, fill);
            let b = sample(img.data, w, h, x, y0 + 1, fill);
            let v = f64::from(a) * (1.0 - t) + f64::from(b) * t;
            out[r * w + x] = v.round().clamp(0.0, 255.0) as u8;
        }
    }

    Deskewed { width: img.width, height: out_h as u32, data: out, slope }
}

fn sample(data: &[u8], w: usize, h: usize, x: usize, y: i64, fill: u8) -> u8 {
    if y < 0 || y as usize >= h {
        fill
    } else {
        data[y as usize * w + x]
    }
}

/// The page's border value: the median of its four edges.
///
/// Median rather than mean because a page edge often carries a scanner's
/// dark band along one side, and a mean would drag the fill toward it. What
/// the fill must not be is a value that reads as ink.
fn border_value(data: &[u8], w: usize, h: usize) -> u8 {
    let mut edge: Vec<u8> = Vec::with_capacity(2 * (w + h));
    for x in 0..w {
        edge.push(data[x]);
        edge.push(data[(h - 1) * w + x]);
    }
    for y in 0..h {
        edge.push(data[y * w]);
        edge.push(data[y * w + w - 1]);
    }
    edge.sort_unstable();
    edge[edge.len() / 2]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Draws horizontal bars sheared by `slope`, as a grayscale page.
    fn skewed_page(w: u32, h: u32, slope: f64) -> Vec<u8> {
        let mut data = vec![245u8; (w * h) as usize];
        for x in 0..w {
            let shift = slope * f64::from(x);
            for band in [10.0, 26.0, 42.0] {
                let y0 = (band + shift).round() as i64;
                for dy in 0..5i64 {
                    let y = y0 + dy;
                    if y >= 0 && (y as u32) < h {
                        data[(y as u32 * w + x) as usize] = 20;
                    }
                }
            }
        }
        data
    }

    fn mask_of(data: &[u8]) -> Vec<u8> {
        data.iter().map(|&v| u8::from(v < 128)).collect()
    }

    #[test]
    fn a_straight_page_reads_as_straight() {
        let (w, h) = (200u32, 60u32);
        let data = skewed_page(w, h, 0.0);
        let m = mask_of(&data);
        assert_eq!(estimate(&m, w, h), 0.0);
    }

    #[test]
    fn a_known_skew_is_recovered_to_within_a_fine_step() {
        let (w, h) = (300u32, 90u32);
        for truth in [0.02, -0.02, 0.05, -0.07] {
            let data = skewed_page(w, h, truth);
            let m = mask_of(&data);
            let got = estimate(&m, w, h);
            assert!(
                (got - truth).abs() < 0.002,
                "slope {truth}: estimated {got}, off by {}",
                (got - truth).abs()
            );
        }
    }

    /// Correcting the estimate must leave a page the estimator then calls
    /// straight. This is the property that matters downstream; the estimate
    /// agreeing with the number a test generated is only evidence for it.
    #[test]
    fn correcting_a_skewed_page_leaves_it_straight() {
        let (w, h) = (300u32, 90u32);
        let data = skewed_page(w, h, 0.045);
        let m = mask_of(&data);
        let s = estimate(&m, w, h);
        let d = correct(&Gray { width: w, height: h, data: &data }, s);
        let m2 = crate::image::binarize::binarize(&d.gray());
        let residual = estimate(&m2, d.width, d.height);
        assert!(residual.abs() < 0.003, "residual slope {residual} after correction");
    }

    #[test]
    fn a_page_inside_the_dead_band_is_returned_untouched() {
        let (w, h) = (100u32, 40u32);
        let data = skewed_page(w, h, 0.0);
        let d = correct(&Gray { width: w, height: h, data: &data }, 0.0005);
        assert_eq!(d.slope, 0.0);
        assert_eq!(d.data, data);
        assert_eq!((d.width, d.height), (w, h));
    }

    /// The fill must not read as ink, or deskewing a page would invent a
    /// black wedge along one edge and the component finder would chase it.
    #[test]
    fn the_new_corner_is_filled_with_background_not_ink() {
        let (w, h) = (200u32, 60u32);
        let data = skewed_page(w, h, 0.06);
        let d = correct(&Gray { width: w, height: h, data: &data }, 0.06);
        assert!(d.height > h);
        let m = crate::image::binarize::binarize(&d.gray());
        // Top-left and bottom-right are the corners the shear vacates.
        assert_eq!(m[0], 0);
        assert_eq!(m[(d.height * d.width - 1) as usize], 0);
    }

    #[test]
    fn a_blank_or_empty_page_is_not_an_error() {
        assert_eq!(estimate(&[], 0, 0), 0.0);
        assert_eq!(estimate(&vec![0u8; 400], 20, 20), 0.0);
    }
}
