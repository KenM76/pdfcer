//! Structural similarity (SSIM) and the `ssim=` JPEG quality search.
//!
//! SSIM is Wang, Bovik, Sheikh and Simoncelli, "Image Quality Assessment:
//! From Error Visibility to Structural Similarity", IEEE TIP 13(4), 2004:
//! an 11 x 11 Gaussian window (sigma 1.5), K1 0.01, K2 0.03, L 255, the
//! mean over every window wholly inside the image. A side shorter than 11
//! uses one window over the whole image with equal weights.
//!
//! What is measured: one component directly; three as BT.601 luma; any
//! other count as the lowest per-component score, so a CMYK image passes
//! only if every ink does.

use std::fmt;

use crate::image_codec::decode_image_view;
use crate::jpeg_write::{self, JpegOptions};
use crate::object::{Dict, Name, Object};
use crate::view::DocumentView;

/// An SSIM score or target, 0 to 1, held in ten-thousandths so settings
/// keep `Eq` and `Hash`.
///
/// ```
/// use pdfcer_core::optimize::SsimScore;
///
/// let t = SsimScore::parse("0.98")?;
/// assert_eq!(t.ten_thousandths(), 9800);
/// assert_eq!(t.to_string(), "0.98");
/// # Ok::<(), String>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SsimScore(u16);

impl SsimScore {
    /// The score `n` / 10000; `None` above 10000.
    #[must_use]
    pub const fn from_ten_thousandths(n: u16) -> Option<Self> {
        if n <= 10_000 { Some(Self(n)) } else { None }
    }

    /// The score in ten-thousandths, 0 to 10000.
    #[must_use]
    pub const fn ten_thousandths(self) -> u16 {
        self.0
    }

    /// The score as a fraction, 0 to 1.
    #[must_use]
    pub fn to_f64(self) -> f64 {
        f64::from(self.0) / 10_000.0
    }

    /// `x` rounded down to a whole ten-thousandth, clamped to 0 to 1.
    #[must_use]
    pub fn from_f64_floor(x: f64) -> Self {
        let n = (x.clamp(0.0, 1.0) * 10_000.0 + 1e-9).floor();
        // In range by the clamp; a NaN clamps to NaN and casts to 0.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Self(n as u16)
    }

    /// Parse a decimal above 0 and at most 1, up to four places.
    ///
    /// # Errors
    ///
    /// A message for anything else.
    pub fn parse(text: &str) -> Result<Self, String> {
        let bad =
            || format!("bad ssim {text:?}; expected a decimal above 0 and at most 1, e.g. 0.98");
        let (whole, frac) = text.split_once('.').unwrap_or((text, ""));
        if frac.len() > 4 || !frac.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad());
        }
        let whole: u16 = whole.parse().map_err(|_| bad())?;
        let frac: u16 = format!("{frac:0<4}").parse().map_err(|_| bad())?;
        let n = whole
            .checked_mul(10_000)
            .and_then(|w| w.checked_add(frac))
            .ok_or_else(bad)?;
        match Self::from_ten_thousandths(n) {
            Some(s) if n > 0 => Ok(s),
            _ => Err(bad()),
        }
    }
}

impl fmt::Display for SsimScore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = format!("{}.{:04}", self.0 / 10_000, self.0 % 10_000);
        f.write_str(text.trim_end_matches('0').trim_end_matches('.'))
    }
}

/// What an `ssim=` search chose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct SsimPick {
    /// The target asked for.
    pub target: SsimScore,
    /// The score of the image written, rounded down.
    pub achieved: SsimScore,
    /// Whether `achieved` reaches `target`; when not, the image was
    /// written at the ceiling quality, the closest it could come.
    pub met: bool,
    /// JPEG encodes tried.
    pub trials: u8,
}

/// The lowest quality in `1..=ceiling` whose encode scores at least
/// `target`, found by bisection (SSIM rises with quality, near enough);
/// the ceiling itself when nothing reaches it. `score(q)` encodes at `q`
/// and returns the decoded image's SSIM, `None` if that failed.
pub(crate) fn search(
    ceiling: u8,
    target: SsimScore,
    mut score: impl FnMut(u8) -> Option<f64>,
) -> (u8, SsimPick) {
    let mut trials = 0u8;
    let mut trial = |q: u8| {
        trials = trials.saturating_add(1);
        score(q).map(SsimScore::from_f64_floor)
    };
    let top = trial(ceiling).unwrap_or(SsimScore(0));
    let mut best = (ceiling, top);
    if top >= target {
        let (mut lo, mut hi) = (1u8, ceiling);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match trial(mid) {
                Some(s) if s >= target => {
                    best = (mid, s);
                    hi = mid;
                }
                _ => lo = mid + 1,
            }
        }
    }
    let pick = SsimPick {
        target,
        achieved: best.1,
        met: best.1 >= target,
        trials,
    };
    (best.0, pick)
}

/// The JPEG options `ssim=` picks for `pixels` (8-bit, unpadded,
/// `size` x `components`), searching below `opts.quality`, and what it
/// chose. Each trial is decoded by pdfcer's own JPEG decoder.
pub(crate) fn jpeg_search(
    view: &DocumentView<'_>,
    pixels: &[u8],
    (size, components): ((u32, u32), u32),
    opts: JpegOptions,
    target: SsimScore,
) -> (JpegOptions, (u8, SsimPick)) {
    let n = u8::try_from(components).unwrap_or(0);
    let mut dict = Dict::new();
    let space: &[u8] = match components {
        1 => b"DeviceGray",
        4 => b"DeviceCMYK",
        _ => b"DeviceRGB",
    };
    for (k, v) in [
        (&b"Width"[..], Object::Integer(i64::from(size.0))),
        (b"Height", Object::Integer(i64::from(size.1))),
        (b"BitsPerComponent", Object::Integer(8)),
        (b"ColorSpace", Object::Name(Name::from(space))),
        (b"Filter", Object::Name(Name::from(&b"DCTDecode"[..]))),
    ] {
        dict.insert(Name::from(k), v);
    }
    let score = |quality: u8| {
        let trial = JpegOptions { quality, ..opts };
        let data = jpeg_write::encode(pixels, size.0, size.1, n, trial).ok()?;
        let back = decode_image_view(view, &dict, &data, false).ok()?;
        ssim(pixels, &back.samples, size.0, size.1, components)
    };
    let (quality, pick) = search(opts.quality, target, score);
    (JpegOptions { quality, ..opts }, (quality, pick))
}

/// SSIM of `b` against the reference `a`: 8-bit interleaved samples,
/// `width` x `height` x `components`. `None` when the lengths disagree
/// with the shape.
pub(crate) fn ssim(a: &[u8], b: &[u8], width: u32, height: u32, components: u32) -> Option<f64> {
    let (w, h, n) = (width as usize, height as usize, components as usize);
    let len = w.checked_mul(h)?.checked_mul(n)?;
    if a.len() != len || b.len() != len || len == 0 {
        return None;
    }
    if n == 3 {
        return Some(plane_ssim(&luma(a), &luma(b), w, h));
    }
    (0..n)
        .map(|c| {
            let pick = |s: &[u8]| s.iter().skip(c).step_by(n).copied().collect::<Vec<_>>();
            plane_ssim(&pick(a), &pick(b), w, h)
        })
        .reduce(f64::min)
}

/// BT.601 luma of interleaved RGB.
fn luma(rgb: &[u8]) -> Vec<u8> {
    rgb.chunks_exact(3)
        .map(|p| {
            let [r, g, b] = [0, 1, 2].map(|i| p.get(i).copied().map_or(0.0, f64::from));
            let y = 0.299 * r + 0.587 * g + 0.114 * b;
            // 0 to 255 by construction.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let y = y.round() as u8;
            y
        })
        .collect()
}

const WINDOW: usize = 11;
const C1: f64 = (0.01 * 255.0) * (0.01 * 255.0);
const C2: f64 = (0.03 * 255.0) * (0.03 * 255.0);

/// The normalised 11-tap Gaussian, sigma 1.5.
fn kernel() -> [f64; WINDOW] {
    let mut k = [0.0; WINDOW];
    let mut sum = 0.0;
    for (i, v) in k.iter_mut().enumerate() {
        #[allow(clippy::cast_precision_loss)] // i < 11
        let d = i as f64 - 5.0;
        *v = (-(d * d) / (2.0 * 1.5 * 1.5)).exp();
        sum += *v;
    }
    k.iter_mut().for_each(|v| *v /= sum);
    k
}

/// The five windowed moments: x, y, x^2, y^2, xy.
type Moments = [f64; 5];

fn index_of(m: Moments) -> f64 {
    let (mx, my) = (m[0], m[1]);
    let vx = m[2] - mx * mx;
    let vy = m[3] - my * my;
    let cxy = m[4] - mx * my;
    ((2.0 * mx * my + C1) * (2.0 * cxy + C2)) / ((mx * mx + my * my + C1) * (vx + vy + C2))
}

/// SSIM of one plane: separable Gaussian, valid windows only, streaming
/// one row of horizontal moments at a time so memory is 11 rows.
fn plane_ssim(a: &[u8], b: &[u8], w: usize, h: usize) -> f64 {
    if w < WINDOW || h < WINDOW {
        return global_ssim(a, b);
    }
    let k = kernel();
    let ow = w - WINDOW + 1;
    let horizontal = |(ra, rb): (&[u8], &[u8])| -> Vec<Moments> {
        ra.windows(WINDOW)
            .zip(rb.windows(WINDOW))
            .map(|(wa, wb)| {
                let mut m = [0.0; 5];
                for ((kt, p), q) in k.iter().zip(wa).zip(wb) {
                    let (p, q) = (f64::from(*p), f64::from(*q));
                    m[0] += kt * p;
                    m[1] += kt * q;
                    m[2] += kt * p * p;
                    m[3] += kt * q * q;
                    m[4] += kt * p * q;
                }
                m
            })
            .collect()
    };
    let mut lines = a.chunks_exact(w).zip(b.chunks_exact(w)).take(h);
    let mut rows: std::collections::VecDeque<Vec<Moments>> =
        lines.by_ref().take(WINDOW).map(horizontal).collect();
    let mut total = 0.0;
    loop {
        let mut acc = vec![[0.0; 5]; ow];
        for (row, kt) in rows.iter().zip(k.iter()) {
            for (sum, m) in acc.iter_mut().zip(row) {
                for (s, v) in sum.iter_mut().zip(m) {
                    *s += kt * v;
                }
            }
        }
        total += acc.into_iter().map(index_of).sum::<f64>();
        let Some(next) = lines.next() else { break };
        rows.pop_front();
        rows.push_back(horizontal(next));
    }
    #[allow(clippy::cast_precision_loss)] // window counts fit an f64 exactly
    let count = (ow * (h - WINDOW + 1)) as f64;
    total / count
}

/// One equal-weight window over the whole plane.
fn global_ssim(a: &[u8], b: &[u8]) -> f64 {
    let mut m = [0.0; 5];
    for (&p, &q) in a.iter().zip(b) {
        let (p, q) = (f64::from(p), f64::from(q));
        m[0] += p;
        m[1] += q;
        m[2] += p * p;
        m[3] += q * q;
        m[4] += p * q;
    }
    #[allow(clippy::cast_precision_loss)] // sample counts fit an f64 exactly
    let n = a.len() as f64;
    m.iter_mut().for_each(|v| *v /= n);
    index_of(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: u32, h: u32, n: u32) -> Vec<u8> {
        (0..w * h * n)
            .map(|i| u8::try_from((i * 7 + (i / n / w) * 13) % 256).unwrap_or(0))
            .collect()
    }

    #[test]
    fn identical_images_score_one() {
        for (w, h, n) in [(32, 24, 1), (32, 24, 3), (16, 16, 4), (5, 7, 3)] {
            let a = gradient(w, h, n);
            let s = ssim(&a, &a, w, h, n).unwrap_or(0.0);
            assert!((s - 1.0).abs() < 1e-12, "{w}x{h}x{n}: {s}");
        }
    }

    #[test]
    fn noise_lowers_the_score_and_more_noise_lowers_it_more() {
        let a = gradient(40, 30, 1);
        let noisy = |amp: u8| -> Vec<u8> {
            a.iter()
                .enumerate()
                .map(|(i, &v)| {
                    if i % 2 == 0 {
                        v.saturating_add(amp)
                    } else {
                        v.saturating_sub(amp)
                    }
                })
                .collect()
        };
        let s4 = ssim(&a, &noisy(4), 40, 30, 1).unwrap_or(1.0);
        let s40 = ssim(&a, &noisy(40), 40, 30, 1).unwrap_or(1.0);
        assert!(s4 < 1.0 && s40 < s4, "{s4} {s40}");
    }

    #[test]
    fn a_damaged_ink_fails_a_cmyk_image() {
        let a = gradient(20, 20, 4);
        let mut b = a.clone();
        b.iter_mut().skip(3).step_by(4).for_each(|v| *v = 255 - *v);
        let s = ssim(&a, &b, 20, 20, 4).unwrap_or(1.0);
        assert!(s < 0.5, "{s}");
    }

    #[test]
    fn a_shape_mismatch_has_no_score() {
        assert_eq!(ssim(&[1, 2, 3], &[1, 2], 3, 1, 1), None);
        assert_eq!(ssim(&[], &[], 0, 0, 1), None);
    }

    #[test]
    fn scores_parse_and_print() {
        let s = |t| SsimScore::parse(t).map(SsimScore::ten_thousandths);
        assert_eq!(s("0.98"), Ok(9800));
        assert_eq!(s("1"), Ok(10_000));
        assert_eq!(
            s(".5"),
            Err("bad ssim \".5\"; expected a decimal above 0 and at most 1, e.g. 0.98".to_owned())
        );
        assert!(s("0").is_err() && s("1.01").is_err() && s("0.12345").is_err() && s("x").is_err());
        assert_eq!(
            SsimScore::parse("0.9500").map(|v| v.to_string()),
            Ok("0.95".to_owned())
        );
        assert_eq!(SsimScore(10_000).to_string(), "1");
    }

    #[test]
    fn the_search_finds_the_lowest_passing_quality() {
        let (q, pick) = search(100, SsimScore(9500), |q| Some(f64::from(q) / 100.0));
        assert_eq!((q, pick.achieved, pick.met), (95, SsimScore(9500), true));
        assert!(pick.trials <= 9, "{}", pick.trials);
    }

    #[test]
    fn an_unreachable_target_keeps_the_ceiling() {
        let (q, pick) = search(60, SsimScore(9900), |q| Some(f64::from(q) / 100.0));
        assert_eq!(
            (q, pick.achieved, pick.met, pick.trials),
            (60, SsimScore(6000), false, 1)
        );
    }
}
