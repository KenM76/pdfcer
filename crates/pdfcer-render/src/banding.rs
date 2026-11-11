//! Rendering a raster in row bands, so a subtractive page larger than one
//! colorant buffer still composites in ink.
//!
//! A page whose group declares a subtractive blending space (ISO 32000-1
//! §11.4.7, §11.3.4) needs a [`crate::cmyk_buffer::CmykBuffer`], and that
//! buffer is capped (`max_cmyk_buffer_bytes`, `ARCHITECTURE.md` §10). Past
//! the cap the page used to composite in sRGB, which is visibly wrong for
//! overprint and the non-`Normal` blend modes — at print resolution a whole
//! conformance page failed this way. Instead the page is split into
//! horizontal bands that each fit the cap; every band is the same page (or
//! region) interpreted under its base CTM translated up by the band's first
//! row, so its pixels are the corresponding rows of the full raster.
//!
//! The cost is time, not memory: interpretation is repaid once per band.

use crate::{RenderError, RenderOptions, RenderedPage, gstate, render_impl_rasterize};
use pdfcer_core::page_tree::{Page, Rect};
use pdfcer_core::view::DocumentView;
use tiny_skia::{Pixmap, Transform};

/// Rows `y0..y0 + rows` and columns `x0..x0 + cols` of the whole-page
/// raster. A row band spans every column; a supersampled tile does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Band {
    pub(crate) x0: u32,
    pub(crate) y0: u32,
    pub(crate) cols: u32,
    pub(crate) rows: u32,
}

impl Band {
    /// The raster's base CTM moved so this band's first pixel lands on
    /// device `(0, 0)`, as the narrowed and the wide copy the interpreter
    /// needs.
    pub(crate) fn ctm(self, base: gstate::Mat64) -> (Transform, gstate::Mat64) {
        let (dx, dy) = (f64::from(self.x0), f64::from(self.y0));
        let wide = base.post_concat(gstate::Mat64::from_row(1.0, 0.0, 0.0, 1.0, -dx, -dy));
        (wide.to_f32(), wide)
    }
}

/// Fold one band's diagnostics into the running total: the first band's
/// counters stand for all (each band interprets the same operators), pixel
/// tallies are summed and a refusal in any band is kept.
///
/// Spot-plane counters take the band that needed most: a band skips the
/// paints outside it, so different bands name different colorants. That is
/// the page's own count whenever the bands refuse the same inks, and the
/// budget [`replan_for_spots`] needs either way.
pub(crate) fn absorb(merged: &mut Option<crate::Diagnostics>, d: crate::Diagnostics) {
    match merged.as_mut() {
        None => *merged = Some(d),
        Some(m) => {
            m.cmyk_spot_planes = m.cmyk_spot_planes.max(d.cmyk_spot_planes);
            m.cmyk_spots_flattened_roster = m
                .cmyk_spots_flattened_roster
                .max(d.cmyk_spots_flattened_roster);
            m.cmyk_spots_flattened_ceiling = m
                .cmyk_spots_flattened_ceiling
                .max(d.cmyk_spots_flattened_ceiling);
            m.cmyk_spots_flattened_allocation = m
                .cmyk_spots_flattened_allocation
                .max(d.cmyk_spots_flattened_allocation);
            m.cmyk_spots_flattened = m.cmyk_spots_flattened_roster
                + m.cmyk_spots_flattened_ceiling
                + m.cmyk_spots_flattened_allocation;
            m.nonseparable_pixels += d.nonseparable_pixels;
            m.cmyk_bridged_pixels += d.cmyk_bridged_pixels;
            m.cmyk_native_image_pixels += d.cmyk_native_image_pixels;
            m.overprint_pixels += d.overprint_pixels;
            m.cmyk_buffer_refused = m.cmyk_buffer_refused.max(d.cmyk_buffer_refused);
            m.cmyk_buffer_engaged &= d.cmyk_buffer_engaged;
        }
    }
}

/// The bands a `width × height` whole-page raster splits into when each
/// band's buffer must hold the five base planes plus `spot_planes` spot
/// planes, or `None` when banding cannot help: the whole raster already
/// fits (the buffer was refused for another reason) or not even one row
/// does.
pub(crate) fn plan(
    width: u32,
    height: u32,
    options: &RenderOptions,
    spot_planes: usize,
) -> Option<Vec<Band>> {
    let bytes = crate::cmyk_buffer::resolve_max_bytes(options.policy().max_cmyk_buffer_bytes);
    let per_px = crate::CMYK_BYTES_PER_PIXEL + spot_planes * (crate::CMYK_BYTES_PER_PIXEL / 5);
    let cap = (bytes / per_px) as u64;
    let rows = u32::try_from(cap / u64::from(width.max(1))).unwrap_or(u32::MAX);
    if rows == 0 || rows >= height {
        return None;
    }
    Some(
        (0..height)
            .step_by(rows as usize)
            .map(|y0| Band {
                x0: 0,
                y0,
                cols: width,
                rows: rows.min(height - y0),
            })
            .collect(),
    )
}

/// The bands to re-render a `width × height` raster in when `first` (its
/// render) refused spot planes for the byte ceiling, sized so every band
/// holds every spot plane the page named; `None` when nothing was refused
/// for the ceiling or not even one row with those planes fits.
///
/// Without this a banded render refuses every spot plane, because the
/// five-plane plan fills each band's buffer exactly. One re-render is
/// enough: `first` named every colorant the page paints.
pub(crate) fn replan_for_spots(
    width: u32,
    height: u32,
    options: &RenderOptions,
    first: &crate::Diagnostics,
) -> Option<Vec<Band>> {
    if first.cmyk_spots_flattened_ceiling == 0 {
        return None;
    }
    let wanted = first.cmyk_spot_planes + first.cmyk_spots_flattened_ceiling;
    let wanted = usize::try_from(wanted)
        .unwrap_or(usize::MAX)
        .min(crate::compositor::MAX_SPOTS);
    let bands = plan(width, height, options, wanted)?;
    (bands.len() > first.cmyk_bands).then_some(bands)
}

/// Render every band of `bands` into `full`(the whole raster: the page, or
/// `region` of it) and return it with one set of diagnostics.
///
/// Diagnostics come from the first band, because every band interprets the
/// same operators; the pixel tallies are summed, a refusal in any band is
/// kept, the spot-plane counters take the most any one band reported (a
/// spot band 0 never paints gets no plane there), and `cmyk_bands` records
/// the split. An ink probe is answered by the
/// band holding its row, in whole-page coordinates.
pub(crate) fn render_bands(
    doc: &DocumentView<'_>,
    page: &Page,
    scale: f32,
    region: Option<Rect>,
    options: &RenderOptions,
    bands: &[Band],
    mut full: Pixmap,
) -> Result<RenderedPage, RenderError> {
    let stride = full.width() as usize * 4;
    let mut merged: Option<crate::Diagnostics> = None;
    let mut probe = None;
    for &band in bands {
        let mut band_options = options.clone();
        band_options.ink_probe = options.ink_probe.and_then(|(x, y)| {
            let holds = (band.y0..band.y0 + band.rows).contains(&y);
            let past_page = y >= full.height() && band.y0 == 0;
            (holds || past_page).then(|| (x, if holds { y - band.y0 } else { y }))
        });
        let rendered = render_impl_rasterize(doc, page, scale, region, Some(band), &band_options)?;
        let start = band.y0 as usize * stride;
        let len = band.rows as usize * stride;
        if let (Some(dst), Some(src)) = (
            full.data_mut().get_mut(start..start + len),
            rendered.pixmap.data().get(..len),
        ) {
            dst.copy_from_slice(src);
        }
        let mut d = rendered.diagnostics;
        if let Some(mut p) = d.ink_probe.take() {
            if options.ink_probe.is_some_and(|(_, y)| y < full.height()) {
                p.y += band.y0;
            }
            probe = Some(p);
        }
        absorb(&mut merged, d);
    }
    let mut diagnostics = merged.unwrap_or_default();
    diagnostics.ink_probe = probe;
    diagnostics.cmyk_bands = bands.len();
    Ok(RenderedPage {
        pixmap: full,
        diagnostics,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn plan_covers_every_row_once_within_the_cap() {
        // 40 bytes is 2 pixels: a 1-pixel-wide, 5-row raster bands 2+2+1.
        let options = RenderOptions::default().with_max_cmyk_buffer_bytes(Some(40));
        let bands = plan(1, 5, &options, 0).unwrap();
        assert_eq!(
            bands,
            [
                Band {
                    x0: 0,
                    y0: 0,
                    cols: 1,
                    rows: 2
                },
                Band {
                    x0: 0,
                    y0: 2,
                    cols: 1,
                    rows: 2
                },
                Band {
                    x0: 0,
                    y0: 4,
                    cols: 1,
                    rows: 1
                }
            ]
        );
    }

    #[test]
    fn plan_declines_when_banding_cannot_help() {
        let options = RenderOptions::default().with_max_cmyk_buffer_bytes(Some(40));
        assert_eq!(plan(1, 2, &options, 0), None, "already fits");
        assert_eq!(plan(3, 5, &options, 0), None, "not even one row fits");
    }

    #[test]
    fn plan_budgets_for_spot_planes() {
        // 40 bytes holds two 20-byte pixels but one 24-byte (one-spot)
        // pixel, and no 44-byte (six-spot) pixel.
        let options = RenderOptions::default().with_max_cmyk_buffer_bytes(Some(40));
        assert_eq!(plan(1, 4, &options, 0).unwrap().len(), 2);
        assert_eq!(plan(1, 4, &options, 1).unwrap().len(), 4);
        assert_eq!(plan(1, 4, &options, 6), None, "not one row with 6 spots");
    }

    #[test]
    fn replan_only_for_ceiling_refusals() {
        let options = RenderOptions::default().with_max_cmyk_buffer_bytes(Some(40));
        let mut d = crate::Diagnostics {
            cmyk_bands: 2,
            ..Default::default()
        };
        assert_eq!(
            replan_for_spots(1, 4, &options, &d),
            None,
            "nothing refused"
        );
        d.cmyk_spots_flattened_roster = 1;
        assert_eq!(
            replan_for_spots(1, 4, &options, &d),
            None,
            "roster, not ceiling"
        );
        d.cmyk_spots_flattened_ceiling = 1;
        assert_eq!(replan_for_spots(1, 4, &options, &d).unwrap().len(), 4);
    }
}
