//! Sauvola adaptive binarization over an integral image.
//!
//! # Contract
//!
//! [`binarize`] takes a grayscale page and returns an ink mask the same size:
//! `1` where there is ink, `0` where there is not. Deterministic — integer
//! and `f64` arithmetic only, no transcendental function, so the same page
//! gives the same mask on x86 and wasm32.
//!
//! Sauvola rather than Otsu because Otsu is a single global threshold and
//! fails on the uneven illumination of a real scan; Sauvola is local and
//! costs one integral-image pass (`ARCHITECTURE.md` section 6).

use crate::image::Gray;

/// Window side in pixels, from `ARCHITECTURE.md` section 6.
///
/// Wide enough to contain a stroke and its local background at ordinary
/// document resolutions, narrow enough to track illumination that changes
/// across a page. Odd, so the window is centred on its pixel.
pub const WINDOW: u32 = 25;

/// Sauvola's `k`, from `ARCHITECTURE.md` section 6. Higher pulls the
/// threshold further below the local mean, keeping more faint ink and more
/// noise; lower does the reverse.
pub const K: f32 = 0.34;

/// Sauvola's `R`, the assumed dynamic range of the local standard
/// deviation. Half of the 8-bit range, which is Sauvola's own value and the
/// one every implementation uses for 8-bit input.
pub const R: f32 = 128.0;

/// Binarization parameters.
///
/// A struct rather than three constants at the call site because chunk 8
/// tunes these against a corpus and a tuned value needs somewhere to live
/// that is not a literal in a kernel.
#[derive(Debug, Clone, Copy)]
pub struct Params {
    pub window: u32,
    pub k: f32,
    pub r: f32,
    /// Whether to apply the minority-ink rule below. Off when the caller
    /// already knows the polarity.
    pub auto_polarity: bool,
}

impl Default for Params {
    fn default() -> Self {
        Params { window: WINDOW, k: K, r: R, auto_polarity: true }
    }
}

/// Binarizes with the default parameters.
pub fn binarize(img: &Gray<'_>) -> Vec<u8> {
    binarize_with(img, &Params::default())
}

/// Binarizes a grayscale page into an ink mask.
///
/// # Polarity
///
/// Sauvola marks pixels *below* the local threshold, which is ink on a
/// dark-on-light page and background on a light-on-dark one. With
/// `auto_polarity` set, a mask covering more than half the page is inverted:
/// **ink is the minority class on any page of text.** A drawing plotted
/// white-on-black is the case this exists for; a page so dense that ink is
/// genuinely the majority is not a page of text.
///
/// # Panics
/// Panics if `img.data.len() != img.width * img.height`.
pub fn binarize_with(img: &Gray<'_>, p: &Params) -> Vec<u8> {
    let w = img.width as usize;
    let h = img.height as usize;
    assert_eq!(img.data.len(), w * h, "image data length must equal width*height");
    if w == 0 || h == 0 {
        return Vec::new();
    }

    let (sum, sq) = integrals(img.data, w, h);
    let half = (p.window.max(1) / 2) as i64;
    let mut mask = vec![0u8; w * h];
    let mut ink = 0usize;

    for y in 0..h {
        let y0 = (y as i64 - half).max(0) as usize;
        let y1 = ((y as i64 + half) as usize).min(h - 1);
        for x in 0..w {
            let x0 = (x as i64 - half).max(0) as usize;
            let x1 = ((x as i64 + half) as usize).min(w - 1);
            let n = ((x1 - x0 + 1) * (y1 - y0 + 1)) as f64;

            let s = box_sum(&sum, w, x0, y0, x1, y1) as f64;
            let s2 = box_sum(&sq, w, x0, y0, x1, y1) as f64;
            let mean = s / n;
            // Clamped at zero: the two sums are exact in u64 and their
            // difference cannot be negative mathematically, but the f64
            // subtraction of two large values can round below it.
            let var = (s2 / n - mean * mean).max(0.0);
            let sd = var.sqrt();

            let t = mean * (1.0 + f64::from(p.k) * (sd / f64::from(p.r) - 1.0));
            if f64::from(img.data[y * w + x]) < t {
                mask[y * w + x] = 1;
                ink += 1;
            }
        }
    }

    if p.auto_polarity && ink * 2 > w * h {
        for m in &mut mask {
            *m ^= 1;
        }
    }
    mask
}

/// Integral images of the values and of their squares, `(w+1) * (h+1)`.
///
/// `u64` throughout: a 10,000 x 10,000 page sums to 2.6e10 in values and
/// 6.5e12 in squares, both of which overflow `u32` and neither of which
/// comes near `u64`.
fn integrals(data: &[u8], w: usize, h: usize) -> (Vec<u64>, Vec<u64>) {
    let stride = w + 1;
    let mut sum = vec![0u64; stride * (h + 1)];
    let mut sq = vec![0u64; stride * (h + 1)];
    for y in 0..h {
        let mut row = 0u64;
        let mut row_sq = 0u64;
        for x in 0..w {
            let v = u64::from(data[y * w + x]);
            row += v;
            row_sq += v * v;
            sum[(y + 1) * stride + x + 1] = sum[y * stride + x + 1] + row;
            sq[(y + 1) * stride + x + 1] = sq[y * stride + x + 1] + row_sq;
        }
    }
    (sum, sq)
}

/// The inclusive box `(x0,y0)..=(x1,y1)` summed from an integral image.
fn box_sum(integral: &[u64], w: usize, x0: usize, y0: usize, x1: usize, y1: usize) -> u64 {
    let stride = w + 1;
    let a = integral[y0 * stride + x0];
    let b = integral[y0 * stride + x1 + 1];
    let c = integral[(y1 + 1) * stride + x0];
    let d = integral[(y1 + 1) * stride + x1 + 1];
    d + a - b - c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(w: u32, h: u32, f: impl Fn(u32, u32) -> u8) -> Vec<u8> {
        let mut out = Vec::with_capacity((w * h) as usize);
        for y in 0..h {
            for x in 0..w {
                out.push(f(x, y));
            }
        }
        out
    }

    #[test]
    fn a_blank_page_has_no_ink() {
        let data = page(40, 40, |_, _| 240);
        let m = binarize(&Gray { width: 40, height: 40, data: &data });
        assert!(m.iter().all(|&v| v == 0), "a flat page must produce no ink");
    }

    /// The case Sauvola exists for: a dark bar of text over a background
    /// that ramps from bright to dim across the page. A global threshold
    /// either loses the text on the dim side or floods the dim side with
    /// ink; a local one gets both.
    #[test]
    fn a_dark_mark_survives_an_illumination_ramp_that_defeats_a_global_threshold() {
        let (w, h) = (120u32, 40u32);
        let data = page(w, h, |x, y| {
            let bg = 250 - (x * 140 / w) as u8; // 250 down to 110
            if (14..26).contains(&y) && (x % 20) < 6 {
                bg.saturating_sub(150)
            } else {
                bg
            }
        });
        let m = binarize(&Gray { width: w, height: h, data: &data });
        // Every mark is found, on the bright side and the dim side alike.
        for x0 in (0..w).step_by(20) {
            let hit = (x0..x0 + 6).any(|x| m[20 * w as usize + x as usize] == 1);
            assert!(hit, "mark at x={x0} was lost");
        }
        // And the dim background is not itself ink.
        assert_eq!(m[2 * w as usize + (w - 3) as usize], 0);
    }

    /// Ink is the minority class on a page of text, so a mask that covers
    /// most of the page means the page was plotted light-on-dark.
    #[test]
    fn a_light_on_dark_page_comes_back_the_same_way_up() {
        let (w, h) = (80u32, 40u32);
        let dark = page(w, h, |x, y| if (14..26).contains(&y) && (x % 16) < 5 { 230 } else { 20 });
        let light = page(w, h, |x, y| if (14..26).contains(&y) && (x % 16) < 5 { 20 } else { 230 });
        let a = binarize(&Gray { width: w, height: h, data: &dark });
        let b = binarize(&Gray { width: w, height: h, data: &light });
        let ink_a = a.iter().filter(|&&v| v == 1).count();
        let ink_b = b.iter().filter(|&&v| v == 1).count();
        assert!(ink_a * 2 < (w * h) as usize, "inverted page left as majority ink");
        assert!(ink_b * 2 < (w * h) as usize);
        // Both find ink where the marks are, not where the field is.
        assert_eq!(a[20 * w as usize + 2], 1);
        assert_eq!(b[20 * w as usize + 2], 1);
    }

    #[test]
    fn the_same_page_binarizes_the_same_way_twice() {
        let data = page(64, 64, |x, y| ((x * 7 + y * 13) % 256) as u8);
        let g = Gray { width: 64, height: 64, data: &data };
        assert_eq!(binarize(&g), binarize(&g));
    }

    #[test]
    fn a_zero_sized_page_is_not_an_error() {
        assert!(binarize(&Gray { width: 0, height: 0, data: &[] }).is_empty());
        assert!(binarize(&Gray { width: 10, height: 0, data: &[] }).is_empty());
    }

    /// The integral image is the whole speed argument; if it disagrees with
    /// the direct sum the thresholds are wrong everywhere at once.
    #[test]
    fn box_sums_match_direct_summation() {
        let (w, h) = (23usize, 17usize);
        let data: Vec<u8> = (0..w * h).map(|i| ((i * 31) % 251) as u8).collect();
        let (sum, sq) = integrals(&data, w, h);
        for (x0, y0, x1, y1) in [(0, 0, 0, 0), (0, 0, 22, 16), (3, 4, 9, 11), (20, 15, 22, 16)] {
            let mut s = 0u64;
            let mut s2 = 0u64;
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let v = u64::from(data[y * w + x]);
                    s += v;
                    s2 += v * v;
                }
            }
            assert_eq!(box_sum(&sum, w, x0, y0, x1, y1), s);
            assert_eq!(box_sum(&sq, w, x0, y0, x1, y1), s2);
        }
    }
}
