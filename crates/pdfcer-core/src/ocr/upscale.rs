//! Enlarging a greyscale page raster before recognition, and mapping the
//! words found on the enlarged image back onto the original.
//!
//! Pure and ungated. The resampler is PIL's separable convolution
//! (`ImagingResample`): per-axis kernel weights normalised to sum 1, the
//! horizontal pass first, a clamp to `u8` after each pass. Bicubic uses
//! a = -0.5 over a support of 2; Lanczos uses `sinc(x) * sinc(x / 3)` over
//! a support of 3. Both widen their support by the scale when shrinking.
//!
//! Upscaling helps an engine whose text-size window a small scan falls
//! below; it can hurt one that already reads the scan, because it invents
//! no detail and spreads blur over more pixels. `docs/ocr-upscale.md`
//! records the measurements behind the shells' defaults.

use super::RecognizedWord;
use super::vl_pre::Grey;

/// Largest enlargement accepted.
pub const MAX_FACTOR: f32 = 4.0;
/// Most output pixels produced; a larger result is refused, not clipped.
pub const MAX_OUTPUT_PIXELS: usize = 1 << 27;

/// The interpolation kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum UpscaleFilter {
    /// Cubic convolution, a = -0.5 (PIL `BICUBIC`).
    #[default]
    Bicubic,
    /// Three-lobe Lanczos (PIL `LANCZOS`): sharper edges, more ringing.
    Lanczos,
}

impl UpscaleFilter {
    /// The name the CLI takes and prints: `bicubic` or `lanczos`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bicubic => "bicubic",
            Self::Lanczos => "lanczos",
        }
    }
}

/// Every convolution kernel the resamplers take: the two public
/// [`UpscaleFilter`]s plus the area average PIL calls `BOX` (support one
/// half, weight 1), which shrinking widens into a mean over each output
/// pixel's footprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Kernel {
    /// PIL `BOX`.
    Average,
    /// PIL `BICUBIC`.
    Bicubic,
    /// PIL `LANCZOS`.
    Lanczos,
}

impl Kernel {
    const fn support(self) -> f64 {
        match self {
            Self::Average => 0.5,
            Self::Bicubic => 2.0,
            Self::Lanczos => 3.0,
        }
    }

    fn weight(self, x: f64) -> f64 {
        match self {
            Self::Average => {
                if x > -0.5 && x <= 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            Self::Bicubic => cubic(x),
            Self::Lanczos => lanczos3(x),
        }
    }
}

impl From<UpscaleFilter> for Kernel {
    fn from(f: UpscaleFilter) -> Self {
        match f {
            UpscaleFilter::Bicubic => Self::Bicubic,
            UpscaleFilter::Lanczos => Self::Lanczos,
        }
    }
}

impl std::fmt::Display for UpscaleFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for UpscaleFilter {
    type Err = UpscaleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "bicubic" => Ok(Self::Bicubic),
            "lanczos" => Ok(Self::Lanczos),
            _ => Err(UpscaleError::UnknownFilter(s.to_owned())),
        }
    }
}

/// How much to enlarge, and with which kernel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Upscale {
    factor: f32,
    filter: UpscaleFilter,
}

impl Upscale {
    /// Enlarge by `factor` (1.0 ..= [`MAX_FACTOR`]; 1.0 is a no-op).
    ///
    /// # Errors
    ///
    /// [`UpscaleError::Factor`] outside that range or not finite.
    pub fn new(factor: f32, filter: UpscaleFilter) -> Result<Self, UpscaleError> {
        if !factor.is_finite() || !(1.0..=MAX_FACTOR).contains(&factor) {
            return Err(UpscaleError::Factor(factor));
        }
        Ok(Self { factor, filter })
    }

    /// The enlargement factor.
    #[must_use]
    pub const fn factor(&self) -> f32 {
        self.factor
    }

    /// The kernel.
    #[must_use]
    pub const fn filter(&self) -> UpscaleFilter {
        self.filter
    }

    /// Whether this changes nothing (`factor` 1.0).
    #[must_use]
    #[allow(clippy::float_cmp)] // exactly the value `new` accepts as identity
    pub fn is_identity(&self) -> bool {
        self.factor == 1.0
    }

    /// The enlarged size of a `width x height` image: each side times the
    /// factor, rounded, at least 1.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // positive; `as` saturates
    pub fn output_size(&self, width: u32, height: u32) -> (u32, u32) {
        let f = f64::from(self.factor);
        let side = |s: u32| ((f64::from(s) * f).round() as u32).max(1);
        (side(width), side(height))
    }
}

/// Why an upscale was refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum UpscaleError {
    /// The factor is outside 1.0 ..= [`MAX_FACTOR`].
    #[error("upscale factor {0} is outside 1 to {MAX_FACTOR}")]
    Factor(f32),
    /// Not a filter name.
    #[error("unknown upscale filter `{0}` (expected bicubic or lanczos)")]
    UnknownFilter(String),
    /// The buffer is not `width * height` bytes, or a side is 0.
    #[error("a {width}x{height} image needs {} bytes, got {got}", (*width as usize) * (*height as usize))]
    Buffer {
        /// Stated width.
        width: u32,
        /// Stated height.
        height: u32,
        /// Bytes given.
        got: usize,
    },
    /// The result would exceed [`MAX_OUTPUT_PIXELS`].
    #[error("upscaling to {width}x{height} exceeds {MAX_OUTPUT_PIXELS} pixels")]
    TooLarge {
        /// Would-be width.
        width: u32,
        /// Would-be height.
        height: u32,
    },
}

/// An enlarged image and the per-axis ratios that map it back.
#[derive(Debug, Clone, PartialEq)]
pub struct Upscaled {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height` bytes, row-major, top-down.
    pub pixels: Vec<u8>,
    /// Output width over input width.
    pub scale_x: f64,
    /// Output height over input height.
    pub scale_y: f64,
}

impl Upscaled {
    /// Move `words` found on this image back into the original image's
    /// pixels (dividing by the per-axis ratios, not the nominal factor,
    /// since rounding makes them differ slightly).
    pub fn map_back(&self, words: &mut [RecognizedWord]) {
        for w in words {
            w.rect.llx /= self.scale_x;
            w.rect.urx /= self.scale_x;
            w.rect.lly /= self.scale_y;
            w.rect.ury /= self.scale_y;
        }
    }
}

/// Enlarge an 8-bit greyscale image (`width * height` bytes, row-major).
///
/// # Errors
///
/// [`UpscaleError::Buffer`] for a mis-sized buffer or a zero side;
/// [`UpscaleError::TooLarge`] past [`MAX_OUTPUT_PIXELS`].
///
/// # Examples
///
/// ```
/// use pdfcer_core::ocr::upscale::{Upscale, UpscaleFilter, upscale_grey};
///
/// let up = Upscale::new(2.0, UpscaleFilter::Bicubic)?;
/// let out = upscale_grey(3, 2, &[0, 128, 255, 255, 128, 0], up)?;
/// assert_eq!((out.width, out.height), (6, 4));
/// assert_eq!(out.pixels.len(), 24);
/// # Ok::<(), pdfcer_core::ocr::upscale::UpscaleError>(())
/// ```
pub fn upscale_grey(
    width: u32,
    height: u32,
    pixels: &[u8],
    upscale: Upscale,
) -> Result<Upscaled, UpscaleError> {
    let need = (width as usize) * (height as usize);
    if need == 0 || pixels.len() != need {
        return Err(UpscaleError::Buffer {
            width,
            height,
            got: pixels.len(),
        });
    }
    let (ow, oh) = upscale.output_size(width, height);
    if (ow as usize).saturating_mul(oh as usize) > MAX_OUTPUT_PIXELS {
        return Err(UpscaleError::TooLarge {
            width: ow,
            height: oh,
        });
    }
    let img = Grey {
        width: width as usize,
        height: height as usize,
        pixels: pixels.to_vec(),
    };
    let out = resample(&img, ow as usize, oh as usize, upscale.filter);
    Ok(Upscaled {
        width: ow,
        height: oh,
        pixels: out.pixels,
        scale_x: f64::from(ow) / f64::from(width),
        scale_y: f64::from(oh) / f64::from(height),
    })
}

/// Resize `img` to `width x height` with `filter`, PIL-exact.
#[must_use]
pub fn resample(img: &Grey, width: usize, height: usize, filter: UpscaleFilter) -> Grey {
    let mut cur = img.clone();
    if width != cur.width {
        cur = resample_pass(&cur, width, true, filter);
    }
    if height != cur.height {
        cur = resample_pass(&cur, height, false, filter);
    }
    cur
}

/// PIL's bicubic kernel (a = -0.5).
fn cubic(x: f64) -> f64 {
    let a = -0.5;
    let x = x.abs();
    if x < 1.0 {
        ((a + 2.0) * x - (a + 3.0)) * x * x + 1.0
    } else if x < 2.0 {
        (((x - 5.0) * x + 8.0) * x - 4.0) * a
    } else {
        0.0
    }
}

fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        1.0
    } else {
        let px = x * std::f64::consts::PI;
        px.sin() / px
    }
}

/// PIL's Lanczos kernel, three lobes.
fn lanczos3(x: f64) -> f64 {
    if (-3.0..3.0).contains(&x) {
        sinc(x) * sinc(x / 3.0)
    } else {
        0.0
    }
}

/// Per output index: the first input index and the normalised weights, as
/// PIL's `precompute_coeffs` builds them (support widens when shrinking).
// Casts are safe: sizes are bounded by MAX_OUTPUT_PIXELS.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]
pub(crate) fn coeffs(in_len: usize, out_len: usize, filter: Kernel) -> Vec<(usize, Vec<f64>)> {
    let scale = in_len as f64 / out_len as f64;
    let fscale = scale.max(1.0);
    let support = filter.support() * fscale;
    (0..out_len)
        .map(|i| {
            let center = (i as f64 + 0.5) * scale;
            let lo = (center - support + 0.5).floor().max(0.0) as usize;
            let hi = ((center + support + 0.5).floor() as usize).min(in_len);
            let mut w: Vec<f64> = (lo..hi)
                .map(|j| filter.weight((j as f64 - center + 0.5) / fscale))
                .collect();
            let sum: f64 = w.iter().sum();
            if sum != 0.0 {
                w.iter_mut().for_each(|v| *v /= sum);
            }
            (lo, w)
        })
        .collect()
}

/// PIL's fixed-point precision for 8-bit images (`PRECISION_BITS`).
const PRECISION_BITS: u32 = 32 - 8 - 2;

/// A weight as PIL's `normalize_coeffs_8bpc` rounds it.
#[allow(clippy::cast_possible_truncation)] // |w| <= ~1.2, so w * 2^22 fits i32
fn fixed(w: f64) -> i32 {
    let v = w * f64::from(1u32 << PRECISION_BITS);
    if v < 0.0 {
        (v - 0.5) as i32
    } else {
        (v + 0.5) as i32
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to 0..=255
fn clip8(acc: i64) -> u8 {
    (acc >> PRECISION_BITS).clamp(0, 255) as u8
}

/// One separable pass along rows (`horizontal`) or columns.
fn resample_pass(img: &Grey, out_len: usize, horizontal: bool, filter: UpscaleFilter) -> Grey {
    let (in_len, lines) = if horizontal {
        (img.width, img.height)
    } else {
        (img.height, img.width)
    };
    let cs: Vec<(usize, Vec<i32>)> = coeffs(in_len, out_len, filter.into())
        .into_iter()
        .map(|(lo, ws)| (lo, ws.into_iter().map(fixed).collect()))
        .collect();
    let (ow, oh) = if horizontal {
        (out_len, img.height)
    } else {
        (img.width, out_len)
    };
    let mut out = vec![0u8; ow * oh];
    for line in 0..lines {
        for (o, (lo, ws)) in cs.iter().enumerate() {
            let acc = ws
                .iter()
                .enumerate()
                .fold(1i64 << (PRECISION_BITS - 1), |acc, (k, &w)| {
                    let (x, y) = if horizontal {
                        (lo + k, line)
                    } else {
                        (line, lo + k)
                    };
                    let p = img.pixels.get(y * img.width + x).copied().unwrap_or(0);
                    acc + i64::from(w) * i64::from(p)
                });
            let (x, y) = if horizontal { (o, line) } else { (line, o) };
            if let Some(p) = out.get_mut(y * ow + x) {
                *p = clip8(acc);
            }
        }
    }
    Grey {
        width: ow,
        height: oh,
        pixels: out,
    }
}
