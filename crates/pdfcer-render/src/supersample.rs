//! [`EdgeAntialiasing::Supersampled`](crate::EdgeAntialiasing::Supersampled):
//! draw with hard edges at `k`× the resolution, then box-filter down.
//!
//! Per-edge antialiasing composites each shape's partial coverage one after
//! another, so two shapes abutting along a pixel each cover half of it and
//! `½ over ½` leaves a quarter of the backdrop showing — a seam the page
//! does not contain. Hard edges tile exactly (§10.6.4's pixel-centre rule),
//! and averaging `k × k` exactly-tiled samples gives each output pixel the
//! coverage-weighted mix of what is really there: smooth edges, no seam.
//!
//! The fine raster is exactly `k`× the coarse one on both axes (the base CTM
//! is post-scaled after the region geometry is resolved), so output pixel
//! `(x, y)` is the mean of fine pixels `kx..kx+k × ky..ky+k`.

use crate::{InkProbeSource, RenderError, RenderedPage, gstate};
use tiny_skia::{Pixmap, Transform};

/// The coarse raster's geometry scaled by `k`, refusing one past
/// [`crate::MAX_PIXMAP_EDGE`] with the fine size in the error.
pub(crate) fn fine_geometry(
    k: u32,
    width: u32,
    height: u32,
    base: gstate::Mat64,
) -> Result<(u32, u32, Transform, gstate::Mat64), RenderError> {
    if k <= 1 {
        return Ok((width, height, base.to_f32(), base));
    }
    let (Some(w), Some(h)) = (width.checked_mul(k), height.checked_mul(k)) else {
        return Err(RenderError::BadRasterSize {
            width: u32::MAX,
            height: u32::MAX,
        });
    };
    let f = f64::from(k);
    let wide = base.post_concat(gstate::Mat64::from_row(f, 0.0, 0.0, f, 0.0, 0.0));
    Ok((w, h, wide.to_f32(), wide))
}

/// A probe coordinate on the fine raster: the centre sample of its block.
pub(crate) const fn fine_probe(k: u32, (x, y): (u32, u32)) -> (u32, u32) {
    let c = k / 2;
    (
        x.saturating_mul(k).saturating_add(c),
        y.saturating_mul(k).saturating_add(c),
    )
}

/// Reduce a fine render to the requested size and put the probe and the
/// pixel tallies back in output pixels.
pub(crate) fn finish(
    mut fine: RenderedPage,
    k: u32,
    probe: Option<(u32, u32)>,
) -> Result<RenderedPage, RenderError> {
    let pixmap = downsample(&fine.pixmap, k)?;
    let d = &mut fine.diagnostics;
    let area = u64::from(k) * u64::from(k);
    for n in [
        &mut d.nonseparable_pixels,
        &mut d.cmyk_bridged_pixels,
        &mut d.cmyk_native_image_pixels,
        &mut d.overprint_pixels,
    ] {
        *n = n.div_ceil(area);
    }
    if let (Some(p), Some((x, y))) = (d.ink_probe.as_mut(), probe) {
        p.x = x;
        p.y = y;
        let idx = (y as usize) * (pixmap.width() as usize) + (x as usize);
        match pixmap.pixels().get(idx).filter(|_| x < pixmap.width()) {
            Some(px) if p.source != InkProbeSource::OutOfRange => {
                let c = px.demultiply();
                p.srgb = Some([c.red(), c.green(), c.blue()]);
            }
            _ => {
                p.source = InkProbeSource::OutOfRange;
                p.cmyk = None;
                p.spots.clear();
                p.alpha = None;
                p.srgb = None;
            }
        }
    }
    fine.pixmap = pixmap;
    Ok(fine)
}

/// The `k × k` box mean of a premultiplied raster whose sides are multiples
/// of `k`. Averaging premultiplied values is the correct coverage mix: a
/// half-covered block of red over transparency is half-transparent red.
pub(crate) fn downsample(fine: &Pixmap, k: u32) -> Result<Pixmap, RenderError> {
    let (w, h) = (fine.width() / k, fine.height() / k);
    let Some(mut out) = Pixmap::new(w, h) else {
        return Err(RenderError::BadRasterSize {
            width: w,
            height: h,
        });
    };
    let (k, fw) = (k as usize, fine.width() as usize);
    let half = k * k / 2;
    let src = fine.data();
    for (oy, row) in out.data_mut().chunks_exact_mut(w as usize * 4).enumerate() {
        for (ox, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let mut sum = [0usize; 4];
            for fy in oy * k..oy * k + k {
                let start = (fy * fw + ox * k) * 4;
                let block = src.get(start..start + k * 4).unwrap_or_default();
                for s in block.as_chunks::<4>().0 {
                    for (acc, v) in sum.iter_mut().zip(s) {
                        *acc += usize::from(*v);
                    }
                }
            }
            for (dst, acc) in px.iter_mut().zip(sum) {
                // At most 255 by construction: a mean of bytes.
                #[allow(clippy::cast_possible_truncation)]
                let v = ((acc + half) / (k * k)) as u8;
                *dst = v;
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn downsample_is_the_box_mean() {
        let mut fine = Pixmap::new(4, 2).unwrap();
        // Left block: two opaque white samples of four; right: all red.
        let d = fine.data_mut();
        for i in [0, 1] {
            d[i * 4..i * 4 + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
        for i in [2, 3, 6, 7] {
            d[i * 4..i * 4 + 4].copy_from_slice(&[255, 0, 0, 255]);
        }
        let out = downsample(&fine, 2).unwrap();
        assert_eq!(out.data(), &[128, 128, 128, 128, 255, 0, 0, 255]);
    }

    #[test]
    fn probe_lands_on_its_block_centre() {
        assert_eq!(fine_probe(4, (3, 0)), (14, 2));
    }

    #[test]
    fn oversized_fine_raster_is_refused() {
        let m = gstate::Mat64::from_row(1.0, 0.0, 0.0, 1.0, 0.0, 0.0);
        assert!(fine_geometry(4, u32::MAX / 2, 10, m).is_err());
        let (w, h, t, _) = fine_geometry(4, 10, 5, m).unwrap();
        assert_eq!((w, h, t.sx), (40, 20, 4.0));
    }
}
