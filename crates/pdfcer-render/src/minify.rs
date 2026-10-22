//! Area-averaged image minification ([`MinifyFilter::AreaAverage`]).
//!
//! An image drawn at a k:1 reduction is box-filtered by `floor(k)` on each
//! axis, then sampled bilinearly: a one-level mipmap. Bilinear alone reads a
//! 2x2 neighbourhood at any scale, so at 4:1 it never reads 12 of every 16
//! source pixels and thin strokes break up. After the box step the source is
//! between 1x and 2x the device size, where bilinear's footprint covers it.
//!
//! The reduction is applied to the decoded image before any paint route
//! sees it, to the sRGB texels, the authored CMYK tints and every spot plane
//! alike, so all routes stay aligned. Image space is the unit square, so the
//! smaller pixmap needs no transform change; a partial last block stretches
//! the image by under one device pixel.
//!
//! [`MinifyFilter::AreaAverage`]: pdfcer_core::settings::MinifyFilter::AreaAverage

use tiny_skia::{Pixmap, PremultipliedColorU8, Transform};

use crate::image::DecodedImage;

/// The box factors for a `w` x `h` image whose unit square `ctm` maps to
/// the device: `floor(texels / device pixels)` per axis, or 1 below 2:1.
pub(crate) fn factors(w: u32, h: u32, ctm: Transform) -> (u32, u32) {
    (axis(w, ctm.sx.hypot(ctm.ky)), axis(h, ctm.kx.hypot(ctm.sy)))
}

fn axis(texels: u32, device: f32) -> u32 {
    if !device.is_finite() || device <= 0.0 {
        return 1;
    }
    let k = (texels as f32 / device).floor();
    if k < 2.0 {
        1
    } else {
        // `k` is at most `texels`, a u32, and at least 2.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let k = k as u32;
        k.min(texels)
    }
}

/// `src` averaged over `fx` x `fy` blocks; the last row and column of
/// blocks average only the pixels that exist. Premultiplied channels are
/// averaged as stored, which is the correct average for alpha.
pub(crate) fn box_reduce(src: &Pixmap, fx: u32, fy: u32) -> Option<Pixmap> {
    let (w, h) = (src.width() as usize, src.height() as usize);
    let (fx, fy) = (fx as usize, fy as usize);
    let (rw, rh) = (w.div_ceil(fx), h.div_ceil(fy));
    let mut out = Pixmap::new(u32::try_from(rw).ok()?, u32::try_from(rh).ok()?)?;
    let mut sums = vec![[0u64; 4]; rw];
    let rows = src.pixels().chunks_exact(w).collect::<Vec<_>>();
    for (band, dst_row) in rows.chunks(fy).zip(out.pixels_mut().chunks_exact_mut(rw)) {
        sums.fill([0; 4]);
        for row in band {
            for (block, sum) in row.chunks(fx).zip(sums.iter_mut()) {
                for p in block {
                    sum[0] += u64::from(p.red());
                    sum[1] += u64::from(p.green());
                    sum[2] += u64::from(p.blue());
                    sum[3] += u64::from(p.alpha());
                }
            }
        }
        let tall = band.len() as u64;
        for (x, (sum, dst)) in sums.iter().zip(dst_row.iter_mut()).enumerate() {
            let wide = (w - x * fx).min(fx) as u64;
            let n = wide * tall;
            let avg = |s: u64| u8::try_from((s + n / 2) / n).unwrap_or(u8::MAX);
            *dst =
                PremultipliedColorU8::from_rgba(avg(sum[0]), avg(sum[1]), avg(sum[2]), avg(sum[3]))
                    .unwrap_or(PremultipliedColorU8::TRANSPARENT);
        }
    }
    Some(out)
}

/// `decoded` with every pixmap box-reduced by `(fx, fy)`: all of them or,
/// if an allocation fails, none, so the routes never see mismatched grids.
pub(crate) fn reduce_decoded(mut decoded: DecodedImage, (fx, fy): (u32, u32)) -> DecodedImage {
    if (fx, fy) == (1, 1) {
        return decoded;
    }
    let mut targets: Vec<&mut Pixmap> = vec![&mut decoded.pixmap];
    if let Some(ink) = decoded.ink.as_mut() {
        targets.extend([&mut ink.cmy, &mut ink.k]);
    }
    if let Some(op) = decoded.overprint.as_mut() {
        targets.extend([&mut op.tints.cmy, &mut op.tints.k]);
        targets.extend(op.spots.iter_mut().map(|s| &mut s.tint));
    }
    let reduced: Option<Vec<Pixmap>> = targets.iter().map(|p| box_reduce(p, fx, fy)).collect();
    if let Some(reduced) = reduced {
        for (dst, px) in targets.into_iter().zip(reduced) {
            *dst = px;
        }
    }
    decoded
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn pixmap(w: u32, h: u32, f: impl Fn(u32, u32) -> u8) -> Pixmap {
        let mut p = Pixmap::new(w, h).unwrap();
        for (i, px) in p.pixels_mut().iter_mut().enumerate() {
            let (x, y) = (i as u32 % w, i as u32 / w);
            let v = f(x, y);
            *px = PremultipliedColorU8::from_rgba(v, v, v, 255).unwrap();
        }
        p
    }

    #[test]
    fn every_source_pixel_counts_toward_its_block() {
        // One-pixel vertical hairline every 4 columns: bilinear at 4:1 can
        // miss it entirely; the box keeps a quarter of it in every block.
        let src = pixmap(16, 8, |x, _| if x % 4 == 1 { 0 } else { 255 });
        let out = box_reduce(&src, 4, 4).unwrap();
        assert_eq!((out.width(), out.height()), (4, 2));
        for p in out.pixels() {
            assert_eq!(p.red(), 191);
        }
    }

    #[test]
    fn a_partial_last_block_averages_only_what_exists() {
        let src = pixmap(5, 1, |x, _| if x == 4 { 100 } else { 0 });
        let out = box_reduce(&src, 2, 1).unwrap();
        assert_eq!(out.width(), 3);
        let reds: Vec<u8> = out.pixels().iter().map(|p| p.red()).collect();
        assert_eq!(reds, [0, 0, 100]);
    }

    #[test]
    fn factors_follow_the_device_extent_on_each_axis() {
        // 1000 x 400 texels into 100 x 100 device pixels: 10:1 and 4:1.
        assert_eq!(
            factors(1000, 400, Transform::from_scale(100.0, 100.0)),
            (10, 4)
        );
        // Rotated 90 degrees: the extents swap with the axes, not vanish.
        let rot = Transform::from_row(0.0, 100.0, -50.0, 0.0, 0.0, 0.0);
        assert_eq!(factors(1000, 400, rot), (10, 8));
        // Below 2:1 nothing is reduced.
        assert_eq!(
            factors(150, 150, Transform::from_scale(100.0, 100.0)),
            (1, 1)
        );
    }
}
