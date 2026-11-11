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

use crate::banding::Band;
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

/// Largest fine tile edge: one `MAX_PIXMAP_EDGE`-safe, colorant-buffer-safe
/// square, so a subtractive page still composites in ink inside a tile.
const FINE_TILE_EDGE: u32 = 4096;

/// A supersampled sheet drawn in tiles, in output pixels.
pub(crate) struct TilePlan {
    pub(crate) k: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) tiles: Vec<Band>,
    /// The caller's probe, in output pixels.
    pub(crate) probe: Option<(u32, u32)>,
}

/// The tiles a `width × height` output raster is drawn in at `k`×, or `None`
/// when the fine raster fits [`crate::MAX_PIXMAP_EDGE`] in one pass or the
/// output itself does not (that refusal stays the render's).
pub(crate) fn tile_plan(
    k: u32,
    width: u32,
    height: u32,
    options: &crate::RenderOptions,
) -> Option<TilePlan> {
    let edge = crate::MAX_PIXMAP_EDGE;
    let fits = |n: u32| n.checked_mul(k).is_some_and(|f| f <= edge);
    if k <= 1 || (fits(width) && fits(height)) || width > edge || height > edge {
        return None;
    }
    let cap = crate::max_cmyk_composite_pixels(options.policy().max_cmyk_buffer_bytes);
    // The square root of a u64 pixel count fits a u32 edge.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let cap_edge = (cap as f64).sqrt() as u32;
    let t = (FINE_TILE_EDGE.min(cap_edge) / k).max(1);
    let tiles = (0..height)
        .step_by(t as usize)
        .flat_map(|y0| {
            (0..width).step_by(t as usize).map(move |x0| Band {
                x0,
                y0,
                cols: t.min(width - x0),
                rows: t.min(height - y0),
            })
        })
        .collect();
    Some(TilePlan {
        k,
        width,
        height,
        tiles,
        probe: options.ink_probe,
    })
}

/// Draw every tile of `plan` at `k`×, reduce each and assemble the output.
/// `fine` carries the probe in fine whole-sheet pixels; the tile holding it
/// answers (tile 0 answers one outside the sheet, as out of range).
pub(crate) fn render_tiled(
    doc: &pdfcer_core::view::DocumentView<'_>,
    page: &pdfcer_core::page_tree::Page,
    scale: f32,
    region: Option<pdfcer_core::page_tree::Rect>,
    fine: &crate::RenderOptions,
    plan: &TilePlan,
) -> Result<RenderedPage, RenderError> {
    let k = plan.k;
    let Some(mut out) = Pixmap::new(plan.width, plan.height) else {
        return Err(RenderError::BadRasterSize {
            width: plan.width,
            height: plan.height,
        });
    };
    let holder = plan.probe.map(|(x, y)| {
        plan.tiles
            .iter()
            .position(|t| (t.x0..t.x0 + t.cols).contains(&x) && (t.y0..t.y0 + t.rows).contains(&y))
            .unwrap_or(0)
    });
    let (mut merged, mut probe) = (None, None);
    for (i, t) in plan.tiles.iter().enumerate() {
        let band = Band {
            x0: t.x0 * k,
            y0: t.y0 * k,
            cols: t.cols * k,
            rows: t.rows * k,
        };
        let mut o = fine.clone();
        o.ink_probe = fine
            .ink_probe
            .filter(|_| holder == Some(i))
            .map(|(x, y)| (x.saturating_sub(band.x0), y.saturating_sub(band.y0)));
        let r = crate::render_impl_rasterize(doc, page, scale, region, Some(band), &o)?;
        blit(&mut out, &downsample(&r.pixmap, k)?, t);
        let mut d = r.diagnostics;
        if holder == Some(i) {
            probe = d.ink_probe.take();
        }
        d.ink_probe = None;
        crate::banding::absorb(&mut merged, d);
    }
    let mut diagnostics = merged.unwrap_or_default();
    diagnostics.ink_probe = probe;
    diagnostics.supersample_tiles = plan.tiles.len();
    let mut page = RenderedPage {
        pixmap: out,
        diagnostics,
    };
    to_output_pixels(&mut page, k, plan.probe);
    Ok(page)
}

/// Copy `tile` into `out` at the tile's origin.
fn blit(out: &mut Pixmap, tile: &Pixmap, at: &Band) {
    let (ow, tw) = (out.width() as usize * 4, tile.width() as usize * 4);
    let x = at.x0 as usize * 4;
    for (row, src) in tile.data().chunks_exact(tw).enumerate() {
        let start = (at.y0 as usize + row) * ow + x;
        if let Some(dst) = out.data_mut().get_mut(start..start + tw) {
            dst.copy_from_slice(src);
        }
    }
}

/// Reduce a fine render to the requested size and put the probe and the
/// pixel tallies back in output pixels.
pub(crate) fn finish(
    mut fine: RenderedPage,
    k: u32,
    probe: Option<(u32, u32)>,
) -> Result<RenderedPage, RenderError> {
    fine.pixmap = downsample(&fine.pixmap, k)?;
    to_output_pixels(&mut fine, k, probe);
    Ok(fine)
}

/// Put a reduced render's probe and pixel tallies back in output pixels.
fn to_output_pixels(fine: &mut RenderedPage, k: u32, probe: Option<(u32, u32)>) {
    let pixmap = &fine.pixmap;
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
