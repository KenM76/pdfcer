//! Skew detection and correction for scanned images (pdfcer-gui request
//! G165).
//!
//! **Detection** is a projection profile: ink pixels are projected onto the
//! axis perpendicular to a candidate text-line direction, and the angle whose
//! profile is most sharply peaked (largest sum of squared bin counts) wins.
//! Lines of text, table rules and title blocks all produce that peak; it
//! needs no OCR and no language. Precision is about one pixel over the
//! length of the lines: 0.05° for lines 1000 pixels long.
//!
//! **Angles** are degrees, positive counter-clockwise **as displayed**: a
//! skew of `+1.0` means the content's horizontal lines rise to the right by
//! one degree, and correcting it rotates the content one degree clockwise.
//!
//! **Correction** rotates the samples about the image centre into an image of
//! the same size, so the placement on the page is unchanged; corners the
//! rotation uncovers take the image's background — the pixel value most
//! common along its border — so a scan on tinted paper is not given white or
//! black corners. Samples are laid out as ISO 32000-1 §8.9.3 describes:
//! components interleaved, each row padded to a byte boundary, most
//! significant bit first. 8- and 16-bit images are resampled bilinearly;
//! 1-, 2- and 4-bit images and `/Indexed` images by nearest neighbour,
//! because a blend of two bilevel samples or two palette indices is not a
//! sample of the same kind.

pub use crate::edit::deskew::{detect_image_skew, page_scan_image};

/// The largest skew searched for, either way. Scans are rarely off by more
/// than a few degrees; past this a page is more likely rotated by 90°, which
/// is a page-rotation question, not a skew.
pub const MAX_SKEW_DEGREES: f64 = 15.0;

/// The fewest ink pixels (after subsampling) a measurement needs. Below it
/// the page is blank or nearly so and any angle is noise.
const MIN_INK_POINTS: usize = 64;

/// The longest side, in pixels, the detector samples at. Larger images are
/// subsampled; text lines survive it and the search stays fast.
const DETECT_SIDE: u32 = 1200;

const COARSE_STEP: f64 = 0.5;
const FINE_STEP: f64 = 0.05;

/// A measured skew.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct SkewEstimate {
    /// Degrees, positive counter-clockwise as displayed (module docs),
    /// within ±[`MAX_SKEW_DEGREES`].
    pub angle_degrees: f64,
    /// How sharply the best angle stands out, from 0 (no angle is better
    /// than any other) towards 1. A page of text lines typically measures
    /// above 0.5; below about 0.2 the angle should not be trusted.
    pub confidence: f64,
}

/// Measure the skew of an 8-bit greyscale raster — `width × height` bytes,
/// row-major, top row first, `0` black. Pixels darker than mid-grey are ink.
///
/// This is the entry point for a shell holding a rendered page; for an image
/// in the document use [`detect_image_skew`].
///
/// Returns `None` when the raster is empty, `grey` is shorter than
/// `width × height`, or there is too little ink to measure.
///
/// # Examples
///
/// ```
/// use pdfcer_core::deskew::detect_skew;
///
/// // Horizontal bars: no skew.
/// let (w, h) = (200u32, 200u32);
/// let mut grey = vec![255u8; (w * h) as usize];
/// for y in (20..180).step_by(20) {
///     for x in 10..190 {
///         grey[(y * w + x) as usize] = 0;
///     }
/// }
/// let skew = detect_skew(w, h, &grey).unwrap();
/// assert!(skew.angle_degrees.abs() < 0.1);
/// ```
#[must_use]
pub fn detect_skew(width: u32, height: u32, grey: &[u8]) -> Option<SkewEstimate> {
    let len = (width as usize).checked_mul(height as usize)?;
    if len == 0 || grey.len() < len {
        return None;
    }
    let points = ink_points(width, height, |x, y| {
        grey.get(y as usize * width as usize + x as usize)
            .is_some_and(|&g| g < 128)
    });
    estimate(&points)
}

/// The layout of a sample buffer (§8.9.3).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Raster {
    pub width: u32,
    pub height: u32,
    pub components: u32,
    pub bpc: u32,
    /// Paper is the all-ones sample (additive and grey spaces, image masks)
    /// rather than all-zeros (subtractive spaces).
    pub paper_is_ones: bool,
    /// Samples are palette indices (§8.6.6.3): never blended, and ink is
    /// any pixel that differs from the background.
    pub indexed: bool,
}

impl Raster {
    fn row_bytes(&self) -> usize {
        (self.width as usize * self.components as usize * self.bpc as usize).div_ceil(8)
    }

    fn max(&self) -> u32 {
        (1u32 << self.bpc) - 1
    }

    /// Sample `c` of pixel `(x, y)`; 0 when out of the buffer.
    fn get(&self, samples: &[u8], x: u32, y: u32, c: u32) -> u32 {
        let row = y as usize * self.row_bytes();
        let index = (x as usize * self.components as usize + c as usize) * self.bpc as usize;
        match self.bpc {
            16 => {
                let at = row + index / 8;
                let hi = samples.get(at).copied().unwrap_or(0);
                let lo = samples.get(at + 1).copied().unwrap_or(0);
                u32::from(u16::from_be_bytes([hi, lo]))
            }
            8 => u32::from(samples.get(row + index / 8).copied().unwrap_or(0)),
            bpc => {
                let byte = u32::from(samples.get(row + index / 8).copied().unwrap_or(0));
                let shift = 8 - bpc - (index % 8) as u32;
                (byte >> shift) & self.max()
            }
        }
    }

    fn put(&self, samples: &mut [u8], x: u32, y: u32, c: u32, value: u32) {
        let row = y as usize * self.row_bytes();
        let index = (x as usize * self.components as usize + c as usize) * self.bpc as usize;
        let at = row + index / 8;
        match self.bpc {
            16 => {
                let [hi, lo] = u16::try_from(value).unwrap_or(u16::MAX).to_be_bytes();
                if let Some(b) = samples.get_mut(at) {
                    *b = hi;
                }
                if let Some(b) = samples.get_mut(at + 1) {
                    *b = lo;
                }
            }
            8 => {
                if let Some(b) = samples.get_mut(at) {
                    *b = u8::try_from(value).unwrap_or(u8::MAX);
                }
            }
            bpc => {
                let shift = 8 - bpc - (index % 8) as u32;
                let mask = self.max() << shift;
                if let Some(b) = samples.get_mut(at) {
                    let cleared = u32::from(*b) & !mask;
                    *b = u8::try_from(cleared | ((value << shift) & mask)).unwrap_or(0);
                }
            }
        }
    }

    /// Whether pixel `(x, y)` is ink: its mean sample is nearer full ink than
    /// paper, or for an indexed image, it is not the background.
    fn is_ink(&self, samples: &[u8], x: u32, y: u32, background: &[u32]) -> bool {
        if self.indexed {
            return (0..self.components)
                .zip(background)
                .any(|(c, &b)| self.get(samples, x, y, c) != b);
        }
        let sum: u64 = (0..self.components)
            .map(|c| u64::from(self.get(samples, x, y, c)))
            .sum();
        let full = u64::from(self.max()) * u64::from(self.components);
        let level = if self.paper_is_ones { full - sum } else { sum };
        level * 2 > full
    }
}

/// Measure the skew of a decoded image's samples.
pub(crate) fn detect_in_samples(raster: &Raster, samples: &[u8]) -> Option<SkewEstimate> {
    let needed = raster.row_bytes().checked_mul(raster.height as usize)?;
    if raster.width == 0 || raster.height == 0 || samples.len() < needed {
        return None;
    }
    let background = background(raster, samples);
    let points = ink_points(raster.width, raster.height, |x, y| {
        raster.is_ink(samples, x, y, &background)
    });
    estimate(&points)
}

/// Rotate the content `angle_degrees` clockwise as displayed — the
/// correction for a skew of `angle_degrees` — about the image centre, into a
/// buffer of the same layout. Returns `None` when `samples` is shorter than
/// the layout requires.
pub(crate) fn rotate_samples(
    raster: &Raster,
    samples: &[u8],
    angle_degrees: f64,
) -> Option<Vec<u8>> {
    let needed = raster.row_bytes().checked_mul(raster.height as usize)?;
    if samples.len() < needed {
        return None;
    }
    let fill = background(raster, samples);
    let mut out = vec![0u8; needed];
    let (sin, cos) = angle_degrees.to_radians().sin_cos();
    let (cx, cy) = (
        f64::from(raster.width) / 2.0,
        f64::from(raster.height) / 2.0,
    );
    for y in 0..raster.height {
        for x in 0..raster.width {
            // Pixel centres. The source of a corrected pixel is the content
            // rotated back by the skew (module docs on the sign).
            let u = f64::from(x) + 0.5 - cx;
            let v = f64::from(y) + 0.5 - cy;
            let sx = u * cos + v * sin + cx - 0.5;
            let sy = -u * sin + v * cos + cy - 0.5;
            for (c, &paper) in (0..raster.components).zip(&fill) {
                let value = sample_at(raster, samples, sx, sy, c, paper);
                raster.put(&mut out, x, y, c, value);
            }
        }
    }
    Some(out)
}

/// The value of component `c` at source position `(sx, sy)` in pixel
/// units, `paper` outside the image.
fn sample_at(raster: &Raster, samples: &[u8], sx: f64, sy: f64, c: u32, paper: u32) -> u32 {
    let (w, h) = (f64::from(raster.width), f64::from(raster.height));
    let at = |x: i64, y: i64| -> f64 {
        if x < 0 || y < 0 || x as f64 >= w || y as f64 >= h {
            f64::from(paper)
        } else {
            f64::from(raster.get(samples, x as u32, y as u32, c))
        }
    };
    if raster.bpc < 8 || raster.indexed {
        let (x, y) = (sx.round() as i64, sy.round() as i64);
        return at(x, y) as u32;
    }
    let (x0, y0) = (sx.floor(), sy.floor());
    let (fx, fy) = (sx - x0, sy - y0);
    let (x0, y0) = (x0 as i64, y0 as i64);
    let top = at(x0, y0) * (1.0 - fx) + at(x0 + 1, y0) * fx;
    let bottom = at(x0, y0 + 1) * (1.0 - fx) + at(x0 + 1, y0 + 1) * fx;
    let value = top * (1.0 - fy) + bottom * fy;
    value.round().clamp(0.0, f64::from(raster.max())) as u32
}

/// The pixel value (all components) most common along the image's border;
/// ties go to the smallest value.
fn background(raster: &Raster, samples: &[u8]) -> Vec<u32> {
    let (w, h) = (raster.width, raster.height);
    let mut counts: std::collections::BTreeMap<Vec<u32>, usize> = Default::default();
    let border = (0..w)
        .flat_map(|x| [(x, 0), (x, h.saturating_sub(1))])
        .chain((0..h).flat_map(|y| [(0, y), (w.saturating_sub(1), y)]));
    for (x, y) in border {
        let pixel = (0..raster.components)
            .map(|c| raster.get(samples, x, y, c))
            .collect();
        *counts.entry(pixel).or_default() += 1;
    }
    counts
        .into_iter()
        .fold(
            (Vec::new(), 0),
            |best, (pixel, n)| {
                if n > best.1 { (pixel, n) } else { best }
            },
        )
        .0
}

/// Ink pixel positions on a grid that keeps the longer side near
/// [`DETECT_SIDE`] samples.
fn ink_points(width: u32, height: u32, is_ink: impl Fn(u32, u32) -> bool) -> Vec<(f64, f64)> {
    let step = (width.max(height) / DETECT_SIDE).max(1);
    let mut points = Vec::new();
    for y in (0..height).step_by(step as usize) {
        for x in (0..width).step_by(step as usize) {
            if is_ink(x, y) {
                points.push((f64::from(x / step), f64::from(y / step)));
            }
        }
    }
    points
}

/// The projection-profile search over `points` (module docs).
fn estimate(points: &[(f64, f64)]) -> Option<SkewEstimate> {
    if points.len() < MIN_INK_POINTS {
        return None;
    }
    let steps = (MAX_SKEW_DEGREES / COARSE_STEP).round() as i32;
    let coarse: Vec<(f64, f64)> = (-steps..=steps)
        .map(|i| {
            let a = f64::from(i) * COARSE_STEP;
            (a, profile_score(points, a))
        })
        .collect();
    let (best_coarse, _) = coarse.iter().copied().max_by(|a, b| a.1.total_cmp(&b.1))?;
    let fine_steps = (COARSE_STEP / FINE_STEP).round() as i32;
    let (angle, best) = (-fine_steps..=fine_steps)
        .map(|i| {
            let a =
                (best_coarse + f64::from(i) * FINE_STEP).clamp(-MAX_SKEW_DEGREES, MAX_SKEW_DEGREES);
            (a, profile_score(points, a))
        })
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    let mut scores: Vec<f64> = coarse.iter().map(|&(_, s)| s).collect();
    scores.sort_by(f64::total_cmp);
    let median = scores.get(scores.len() / 2).copied().unwrap_or(0.0);
    let confidence = if best > 0.0 {
        (1.0 - median / best).clamp(0.0, 1.0)
    } else {
        0.0
    };
    Some(SkewEstimate {
        angle_degrees: (angle * 100.0).round() / 100.0,
        confidence,
    })
}

/// Sum of squared bin counts of the points projected perpendicular to a
/// line at `angle_degrees` (in image coordinates, y down, a line tilted
/// counter-clockwise as displayed keeps `y·cos + x·sin` constant). Each
/// point's weight is split linearly between its two nearest bins: rounding
/// to one bin beats against the pixel grid and biases the angle by about a
/// pixel over the line length.
fn profile_score(points: &[(f64, f64)], angle_degrees: f64) -> f64 {
    let (sin, cos) = angle_degrees.to_radians().sin_cos();
    let keys: Vec<f64> = points.iter().map(|&(x, y)| y * cos + x * sin).collect();
    let lo = keys.iter().copied().fold(f64::INFINITY, f64::min).floor();
    let hi = keys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if !lo.is_finite() || !hi.is_finite() || hi < lo {
        return 0.0;
    }
    let mut bins = vec![0.0f64; (hi - lo) as usize + 2];
    for k in keys {
        let at = k - lo;
        let i = at as usize;
        let w = at - at.floor();
        if let Some(b) = bins.get_mut(i) {
            *b += 1.0 - w;
        }
        if let Some(b) = bins.get_mut(i + 1) {
            *b += w;
        }
    }
    bins.iter().map(|n| n * n).sum()
}
