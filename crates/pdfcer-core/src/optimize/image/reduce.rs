//! Reducing 8-bit samples to fewer values: grey to 1 bit (Otsu, a fixed
//! threshold, or Floyd-Steinberg dither) and colour to a median-cut palette;
//! and packing the result into PDF sample rows (ISO 32000-2 §8.9.3: high
//! bits first, each row padded to a byte).

use std::collections::HashMap;

/// Otsu's threshold: the grey level `t` that maximises the between-class
/// variance of `{v <= t}` and `{v > t}`.
#[allow(clippy::cast_precision_loss)] // pixel counts well below 2^52
pub(crate) fn otsu(gray: &[u8]) -> u8 {
    let mut hist = [0u64; 256];
    for &v in gray {
        if let Some(h) = hist.get_mut(usize::from(v)) {
            *h += 1;
        }
    }
    let total = gray.len() as f64;
    let sum: f64 = hist
        .iter()
        .enumerate()
        .map(|(i, h)| i as f64 * *h as f64)
        .sum();
    let (mut weight, mut below, mut best, mut at) = (0.0, 0.0, -1.0, 127u8);
    for (t, h) in hist.iter().enumerate() {
        weight += *h as f64;
        below += t as f64 * *h as f64;
        let above = total - weight;
        if weight == 0.0 || above == 0.0 {
            continue;
        }
        let diff = below / weight - (sum - below) / above;
        let between = weight * above * diff * diff;
        if between > best {
            best = between;
            at = u8::try_from(t).unwrap_or(u8::MAX);
        }
    }
    at
}

/// 1-bit rows: grey above `threshold` is white (1, as `/DeviceGray`).
pub(crate) fn threshold(gray: &[u8], width: u32, threshold: u8) -> Vec<u8> {
    let bits: Vec<u8> = gray.iter().map(|&v| u8::from(v > threshold)).collect();
    pack(&bits, width, 1)
}

/// 1-bit rows by Floyd-Steinberg error diffusion at the midpoint.
pub(crate) fn dither(gray: &[u8], width: u32) -> Vec<u8> {
    let w = width as usize;
    if w == 0 {
        return Vec::new();
    }
    let mut err: Vec<i32> = gray.iter().map(|&v| i32::from(v)).collect();
    let mut bits = vec![0u8; err.len()];
    for i in 0..err.len() {
        let old = err.get(i).copied().unwrap_or(0);
        let white = old > 127;
        if let Some(b) = bits.get_mut(i) {
            *b = u8::from(white);
        }
        let e = old - if white { 255 } else { 0 };
        let x = i % w;
        let mut spread = |j: usize, part: i32| {
            if let Some(v) = err.get_mut(j) {
                *v += e * part / 16;
            }
        };
        if x + 1 < w {
            spread(i + 1, 7);
            spread(i + w + 1, 1);
        }
        if x > 0 {
            spread(i + w - 1, 3);
        }
        spread(i + w, 5);
    }
    pack(&bits, width, 1)
}

/// Pack one value per sample (each below `2^bpc`) into rows `width`
/// samples long.
pub(crate) fn pack(values: &[u8], width: u32, bpc: u32) -> Vec<u8> {
    let w = width as usize;
    if w == 0 || bpc == 8 {
        return values.to_vec();
    }
    let stride = (w * bpc as usize).div_ceil(8);
    let rows = values.len() / w;
    let mut out = vec![0u8; stride * rows];
    for (y, row) in values.chunks_exact(w).enumerate() {
        for (x, &v) in row.iter().enumerate() {
            let bit = x * bpc as usize;
            let shift = 8 - bpc as usize - bit % 8;
            if let Some(b) = out.get_mut(y * stride + bit / 8) {
                *b |= v << shift;
            }
        }
    }
    out
}

/// The smallest PDF bit depth holding `entries` palette indices.
pub(crate) const fn index_bits(entries: usize) -> u32 {
    match entries {
        0..=2 => 1,
        3..=4 => 2,
        5..=16 => 4,
        _ => 8,
    }
}

/// A palette of at most `max` entries for `values` (`n` bytes per pixel),
/// and one index per pixel. `exact` when the image had no more colours
/// than `max`.
pub(crate) struct Quantised {
    pub(crate) palette: Vec<u8>,
    pub(crate) indices: Vec<u8>,
    pub(crate) exact: bool,
}

/// Distinct colours above this are histogrammed at 5 bits a component.
const FULL_HISTOGRAM: usize = 1 << 16;

/// Median cut (Heckbert 1982): split the box with the widest component
/// range at its population median until there are `max` boxes; each
/// entry is its box's population-weighted mean.
pub(crate) fn quantise(values: &[u8], n: usize, max: usize) -> Quantised {
    let n = n.clamp(1, 32);
    let max = max.clamp(1, 256);
    let mut hist = histogram(values, n, 0xFF);
    let coarse = hist.len() > FULL_HISTOGRAM;
    if coarse {
        hist = histogram(values, n, 0xF8);
    }
    if !coarse && hist.len() <= max {
        let mut colours: Vec<Vec<u8>> = hist.into_keys().collect();
        colours.sort_unstable();
        let at: HashMap<&[u8], u8> = colours
            .iter()
            .enumerate()
            .map(|(i, c)| (c.as_slice(), u8::try_from(i).unwrap_or(u8::MAX)))
            .collect();
        let indices = values
            .chunks_exact(n)
            .map(|p| at.get(p).copied().unwrap_or(0))
            .collect();
        return Quantised {
            palette: colours.concat(),
            indices,
            exact: true,
        };
    }
    let entries: Vec<(Vec<u8>, u64)> = hist.into_iter().collect();
    let palette = median_cut(entries, n, max);
    let mask = if coarse { 0xF8 } else { 0xFF };
    let indices = nearest_indices(values, n, &palette, mask);
    Quantised {
        palette: palette.concat(),
        indices,
        exact: false,
    }
}

fn histogram(values: &[u8], n: usize, mask: u8) -> HashMap<Vec<u8>, u64> {
    let mut hist: HashMap<Vec<u8>, u64> = HashMap::new();
    for p in values.chunks_exact(n) {
        let key: Vec<u8> = p.iter().map(|v| v & mask).collect();
        *hist.entry(key).or_insert(0) += 1;
    }
    hist
}

/// The widest component of `entries` and its range.
fn widest(entries: &[(Vec<u8>, u64)], n: usize) -> (usize, u8) {
    (0..n)
        .map(|c| {
            let comp = entries.iter().filter_map(|(k, _)| k.get(c).copied());
            let (lo, hi) = comp.fold((u8::MAX, 0), |(lo, hi), v| (lo.min(v), hi.max(v)));
            (c, hi.saturating_sub(lo))
        })
        .max_by_key(|(_, r)| *r)
        .unwrap_or((0, 0))
}

fn median_cut(entries: Vec<(Vec<u8>, u64)>, n: usize, max: usize) -> Vec<Vec<u8>> {
    let mut boxes = vec![entries];
    while boxes.len() < max {
        let Some((at, (axis, _))) = boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.len() > 1)
            .map(|(i, b)| (i, widest(b, n)))
            .max_by_key(|(_, (_, r))| *r)
        else {
            break;
        };
        let mut b = boxes.swap_remove(at);
        b.sort_unstable_by_key(|(k, _)| k.get(axis).copied().unwrap_or(0));
        let half: u64 = b.iter().map(|(_, c)| c).sum::<u64>() / 2;
        let mut run = 0;
        let split = b
            .iter()
            .position(|(_, c)| {
                run += c;
                run > half
            })
            .unwrap_or(0)
            .clamp(1, b.len() - 1);
        let upper = b.split_off(split);
        boxes.push(b);
        boxes.push(upper);
    }
    boxes.iter().map(|b| mean(b, n)).collect()
}

fn mean(entries: &[(Vec<u8>, u64)], n: usize) -> Vec<u8> {
    let total: u64 = entries.iter().map(|(_, c)| c).sum::<u64>().max(1);
    (0..n)
        .map(|c| {
            let s: u64 = entries
                .iter()
                .map(|(k, cnt)| u64::from(k.get(c).copied().unwrap_or(0)) * cnt)
                .sum();
            u8::try_from((s + total / 2) / total).unwrap_or(u8::MAX)
        })
        .collect()
}

/// The nearest palette entry for each pixel, cached per (masked) colour.
fn nearest_indices(values: &[u8], n: usize, palette: &[Vec<u8>], mask: u8) -> Vec<u8> {
    let mut cache: HashMap<Vec<u8>, u8> = HashMap::new();
    values
        .chunks_exact(n)
        .map(|p| {
            let key: Vec<u8> = p.iter().map(|v| v & mask).collect();
            *cache.entry(key).or_insert_with(|| nearest(p, palette))
        })
        .collect()
}

fn nearest(p: &[u8], palette: &[Vec<u8>]) -> u8 {
    let distance = |e: &Vec<u8>| -> u32 {
        p.iter()
            .zip(e)
            .map(|(a, b)| u32::from(a.abs_diff(*b)).pow(2))
            .sum()
    };
    let best = palette
        .iter()
        .enumerate()
        .min_by_key(|(_, e)| distance(e))
        .map_or(0, |(i, _)| i);
    u8::try_from(best).unwrap_or(u8::MAX)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn otsu_splits_two_clusters_between_them() {
        let mut g = vec![30u8; 500];
        g.extend(vec![200u8; 500]);
        let t = otsu(&g);
        assert!((30..200).contains(&t), "{t}");
    }

    #[test]
    fn pack_puts_the_first_sample_in_the_high_bits_and_pads_rows() {
        assert_eq!(
            pack(&[1, 0, 1, 1, 0, 1, 1, 1, 1, 0], 10, 1),
            vec![0b1011_0111, 0b1000_0000]
        );
        assert_eq!(pack(&[3, 1, 2], 3, 2), vec![0b1101_1000]);
    }

    #[test]
    fn dither_keeps_mean_grey() {
        let g = vec![64u8; 64 * 64];
        let bits = dither(&g, 64);
        let white: u32 = bits.iter().map(|b| b.count_ones()).sum();
        let frac = f64::from(white) / 4096.0;
        assert!((frac - 0.25).abs() < 0.02, "{frac}");
    }

    #[test]
    fn few_colours_give_an_exact_palette() {
        let v = [10, 20, 30, 40, 50, 60, 10, 20, 30];
        let q = quantise(&v, 3, 256);
        assert!(q.exact);
        assert_eq!(q.palette.len(), 6);
        assert_eq!(q.indices[0], q.indices[2]);
        let back: Vec<u8> = q
            .indices
            .iter()
            .flat_map(|i| q.palette[*i as usize * 3..*i as usize * 3 + 3].to_vec())
            .collect();
        assert_eq!(back, v);
    }

    #[test]
    fn many_colours_cut_to_the_cap_with_small_error() {
        let v: Vec<u8> = (0..=255u8).flat_map(|i| [i, 255 - i, i / 2]).collect();
        let q = quantise(&v, 3, 16);
        assert!(!q.exact);
        assert_eq!(q.palette.len(), 16 * 3);
        for (p, i) in v.chunks(3).zip(&q.indices) {
            let e = &q.palette[*i as usize * 3..*i as usize * 3 + 3];
            let d: u32 = p
                .iter()
                .zip(e)
                .map(|(a, b)| u32::from(a.abs_diff(*b)))
                .sum();
            assert!(d <= 40, "{p:?} -> {e:?}");
        }
    }
}
