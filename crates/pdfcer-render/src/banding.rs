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

/// Rows `y0..y0 + rows` of the whole-page raster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Band {
    pub(crate) y0: u32,
    pub(crate) rows: u32,
}

impl Band {
    /// The raster's base CTM moved so this band's first row lands on device
    /// row 0, as the narrowed and the wide copy the interpreter needs.
    pub(crate) fn ctm(self, base: gstate::Mat64) -> (Transform, gstate::Mat64) {
        let dy = f64::from(self.y0);
        let wide = base.post_concat(gstate::Mat64::from_row(1.0, 0.0, 0.0, 1.0, 0.0, -dy));
        (wide.to_f32(), wide)
    }
}

/// The bands a `width × height` whole-page raster splits into, or `None`
/// when banding cannot help: the whole raster already fits (the buffer was
/// refused for another reason) or not even one row does.
pub(crate) fn plan(width: u32, height: u32, options: &RenderOptions) -> Option<Vec<Band>> {
    let cap = crate::max_cmyk_composite_pixels(options.policy().max_cmyk_buffer_bytes);
    let rows = u32::try_from(cap / u64::from(width.max(1))).unwrap_or(u32::MAX);
    if rows == 0 || rows >= height {
        return None;
    }
    Some(
        (0..height)
            .step_by(rows as usize)
            .map(|y0| Band {
                y0,
                rows: rows.min(height - y0),
            })
            .collect(),
    )
}

/// Render every band of `bands` into `full` (the whole raster: the page, or
/// `region` of it) and return it with one set of diagnostics.
///
/// Diagnostics come from the first band, because every band interprets the
/// same operators; the pixel tallies are summed, a refusal in any band is
/// kept, and `cmyk_bands` records the split. An ink probe is answered by the
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
        match merged.as_mut() {
            None => merged = Some(d),
            Some(m) => {
                m.nonseparable_pixels += d.nonseparable_pixels;
                m.cmyk_bridged_pixels += d.cmyk_bridged_pixels;
                m.cmyk_native_image_pixels += d.cmyk_native_image_pixels;
                m.overprint_pixels += d.overprint_pixels;
                m.cmyk_buffer_refused = m.cmyk_buffer_refused.max(d.cmyk_buffer_refused);
                m.cmyk_buffer_engaged &= d.cmyk_buffer_engaged;
            }
        }
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
        let bands = plan(1, 5, &options).unwrap();
        assert_eq!(
            bands,
            [
                Band { y0: 0, rows: 2 },
                Band { y0: 2, rows: 2 },
                Band { y0: 4, rows: 1 }
            ]
        );
    }

    #[test]
    fn plan_declines_when_banding_cannot_help() {
        let options = RenderOptions::default().with_max_cmyk_buffer_bytes(Some(40));
        assert_eq!(plan(1, 2, &options), None, "already fits");
        assert_eq!(plan(3, 5, &options), None, "not even one row fits");
    }
}
