//! Enlarging a packed PDF image sample buffer (ISO 32000-1 §8.9.3) channel
//! by channel, keeping its bit depth: the kernel is [`crate::ocr::upscale`]'s
//! separable convolution, each output value rounded and clamped to the
//! sample range. At 1 bit that is a threshold at one half, so a bilevel scan
//! stays bilevel. Palette indices (`/Indexed`) are never blended: they take
//! the nearest source sample.

use std::collections::VecDeque;

use crate::deskew::Raster;
use crate::ocr::upscale::{UpscaleFilter, coeffs};

/// `samples` laid out as `raster`, resampled to `out_w x out_h` in the same
/// layout. Short input reads as zero samples.
pub(crate) fn resample_samples(
    raster: &Raster,
    samples: &[u8],
    out_w: u32,
    out_h: u32,
    filter: UpscaleFilter,
) -> Vec<u8> {
    let out = Raster {
        width: out_w,
        height: out_h,
        ..*raster
    };
    let mut dst = vec![0u8; out.row_bytes() * out_h as usize];
    if raster.indexed {
        nearest(raster, samples, &out, &mut dst);
    } else {
        convolve(raster, samples, &out, &mut dst, filter);
    }
    dst
}

/// The source index nearest output index `i` when `in_len` is stretched to
/// `out_len`.
fn nearest_index(i: u32, in_len: u32, out_len: u32) -> u32 {
    let at = (u64::from(i) * 2 + 1) * u64::from(in_len) / (u64::from(out_len) * 2);
    u32::try_from(at)
        .unwrap_or(u32::MAX)
        .min(in_len.saturating_sub(1))
}

fn nearest(src: &Raster, samples: &[u8], out: &Raster, dst: &mut [u8]) {
    for y in 0..out.height {
        let sy = nearest_index(y, src.height, out.height);
        for x in 0..out.width {
            let sx = nearest_index(x, src.width, out.width);
            for c in 0..src.components {
                out.put(dst, x, y, c, src.get(samples, sx, sy, c));
            }
        }
    }
}

/// Horizontal pass per source row (cached while output rows still read it),
/// then the vertical pass straight into the packed output; memory is the
/// kernel's height in rows, not a whole intermediate image.
fn convolve(src: &Raster, samples: &[u8], out: &Raster, dst: &mut [u8], filter: UpscaleFilter) {
    let comps = src.components as usize;
    let xs = coeffs(src.width as usize, out.width as usize, filter);
    let ys = coeffs(src.height as usize, out.height as usize, filter);
    let max = f64::from(src.max());
    let mut rows: VecDeque<(usize, Vec<f64>)> = VecDeque::new();
    let mut acc = vec![0.0f64; out.width as usize * comps];
    for (y, (lo, weights)) in ys.iter().enumerate() {
        while rows.front().is_some_and(|(r, _)| r < lo) {
            rows.pop_front();
        }
        acc.iter_mut().for_each(|v| *v = 0.0);
        for (k, w) in weights.iter().enumerate() {
            let r = lo + k;
            if rows.back().is_none_or(|(last, _)| *last < r) {
                rows.push_back((r, horizontal(src, samples, r, &xs)));
            }
            if let Some((_, row)) = rows.iter().find(|(i, _)| *i == r) {
                acc.iter_mut().zip(row).for_each(|(a, v)| *a += w * v);
            }
        }
        let y = u32::try_from(y).unwrap_or(u32::MAX);
        for (i, v) in acc.iter().enumerate() {
            let (x, c) = (i / comps, i % comps);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            // clamped to 0..=max, which fits u32
            let q = v.round().clamp(0.0, max) as u32;
            out.put(
                dst,
                u32::try_from(x).unwrap_or(u32::MAX),
                y,
                u32::try_from(c).unwrap_or(u32::MAX),
                q,
            );
        }
    }
}

/// Source row `r`, every channel, resampled to the output width.
fn horizontal(src: &Raster, samples: &[u8], r: usize, xs: &[(usize, Vec<f64>)]) -> Vec<f64> {
    let comps = src.components as usize;
    let y = u32::try_from(r).unwrap_or(u32::MAX);
    let mut row = vec![0.0f64; xs.len() * comps];
    for (x, (lo, weights)) in xs.iter().enumerate() {
        for c in 0..src.components {
            let mut sum = 0.0;
            for (k, w) in weights.iter().enumerate() {
                let sx = u32::try_from(lo + k).unwrap_or(u32::MAX);
                sum += w * f64::from(src.get(samples, sx, y, c));
            }
            if let Some(slot) = row.get_mut(x * comps + c as usize) {
                *slot = sum;
            }
        }
    }
    row
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)] // tests index fixed-size outputs they just asserted
mod tests {
    use super::*;

    fn raster(width: u32, height: u32, components: u32, bpc: u32, indexed: bool) -> Raster {
        Raster {
            width,
            height,
            components,
            bpc,
            paper_is_ones: true,
            indexed,
        }
    }

    #[test]
    fn a_flat_image_stays_flat_in_every_channel() {
        let src = raster(3, 2, 3, 8, false);
        let samples: Vec<u8> = [10u8, 128, 250].repeat(6);
        let out = resample_samples(&src, &samples, 6, 4, UpscaleFilter::Lanczos);
        assert_eq!(out, [10u8, 128, 250].repeat(24));
    }

    #[test]
    fn a_bilevel_image_stays_bilevel_and_keeps_its_shape() {
        // 8x2, 1 bpc: left half black (0), right half white (1).
        let src = raster(8, 2, 1, 1, false);
        let samples = vec![0b0000_1111u8, 0b0000_1111];
        let out = resample_samples(&src, &samples, 16, 4, UpscaleFilter::Bicubic);
        // 16 px at 1 bpc = 2 bytes a row, no padding bits to vary.
        assert_eq!(out, [0x00u8, 0xFF].repeat(4));
    }

    #[test]
    fn sixteen_bit_samples_keep_their_range() {
        let src = raster(2, 1, 1, 16, false);
        let samples = vec![0x00, 0x00, 0xFF, 0xFF];
        let out = resample_samples(&src, &samples, 4, 1, UpscaleFilter::Bicubic);
        let first = u16::from_be_bytes([out[0], out[1]]);
        let last = u16::from_be_bytes([out[6], out[7]]);
        assert_eq!((first, last), (0, u16::MAX));
        assert_eq!(out.len(), 8);
    }

    #[test]
    fn palette_indices_are_copied_never_blended() {
        let src = raster(2, 1, 1, 8, true);
        let samples = vec![3u8, 200];
        let out = resample_samples(&src, &samples, 6, 2, UpscaleFilter::Lanczos);
        assert_eq!(out, [3u8, 3, 3, 200, 200, 200].repeat(2));
    }

    #[test]
    fn an_edge_is_interpolated_between_its_sides() {
        let src = raster(2, 1, 1, 8, false);
        let out = resample_samples(&src, &[0, 200], 4, 1, UpscaleFilter::Bicubic);
        assert!(
            out[0] < out[1] && out[1] < out[2] && out[2] < out[3],
            "{out:?}"
        );
    }
}
