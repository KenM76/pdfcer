//! Ink coverage warnings ([`CoverageWarning`], [`coverage_warning_mask`]):
//! Acrobat Output Preview's *Total area coverage* and *Rich black*, as a
//! per-pixel mask read from the colorant buffer the separation preview
//! composites in.
//!
//! One definition, here, for the GUI's warning layer and the CLI alike.

use pdfcer_core::page_tree::{Page, Rect};

use crate::cmyk_buffer::CmykBuffer;
use crate::{DocumentView, RenderError, RenderOptions};

/// Which areas a [`coverage_warning_mask`] flags.
///
/// Both rules read each plate's ink as it lands on the sheet: the tint
/// times the page group's alpha after compositing (ISO 32000-2 §11.4.7,
/// zero ink being the medium), with overprint, transparency and spot inks
/// already resolved exactly as the separation preview draws them. Plates
/// the [`crate::SeparationPreview`] hides count as no ink. Every tint is compared
/// in tenths of a percent, rounded, so a 75% tint is 75.0 and not
/// 74.99998.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CoverageWarning {
    /// The sum of every plate's tint (C, M, Y, K and each spot ink) is
    /// strictly above `limit_percent` (so a 300% limit flags 300.1%, not
    /// 300%).
    TotalAreaCoverage {
        /// The limit, in percent; 0–(100 × plate count) is meaningful.
        limit_percent: u16,
    },
    /// Rich black: black at or above `black_start_percent` with any cyan,
    /// magenta or yellow present (0.1% or more). Spot inks never make a
    /// black rich.
    RichBlack {
        /// The black tint a pixel must reach, in percent.
        black_start_percent: u16,
    },
}

impl CoverageWarning {
    /// Whether one pixel breaks this rule. `process` is `[C, M, Y, K]` and
    /// `spots` each spot ink, all as ink on the sheet in `0.0..=1.0`.
    #[must_use]
    pub fn flags(self, process: [f32; 4], spots: &[f32]) -> bool {
        match self {
            Self::TotalAreaCoverage { limit_percent } => {
                let total: u32 = process.iter().chain(spots).map(|&t| tenths(t)).sum();
                total > u32::from(limit_percent) * 10
            }
            Self::RichBlack {
                black_start_percent,
            } => {
                let [c, m, y, k] = process.map(tenths);
                k >= u32::from(black_start_percent) * 10 && c + m + y > 0
            }
        }
    }
}

/// A tint in tenths of a percent, rounded.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn tenths(tint: f32) -> u32 {
    (tint.clamp(0.0, 1.0) * 1000.0).round() as u32
}

/// The pixels a [`CoverageWarning`] flags on one page raster.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CoverageMask {
    /// Width in pixels; equal to the separation preview raster's.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// One byte per pixel, row-major from the top-left: `1` where the
    /// rule is broken, `0` elsewhere.
    pub mask: Vec<u8>,
    /// How many bytes of [`Self::mask`] are `1`.
    pub flagged: u64,
}

impl CoverageMask {
    /// Whether the pixel at `(x, y)` is flagged; `false` outside the mask.
    #[must_use]
    pub fn is_flagged(&self, x: u32, y: u32) -> bool {
        x < self.width
            && self
                .mask
                .get(y as usize * self.width as usize + x as usize)
                .is_some_and(|&b| b == 1)
    }
}

/// The pixels of `page` (or `region` of it, in page space as
/// [`crate::render_page_region`] takes it) at `scale` that break
/// `warning`.
///
/// Renders the page in ink under `options`' separation preview (all inks
/// shown when it has none), so the mask lines up pixel for pixel with the
/// preview raster the caller draws it over. Edges are not antialiased:
/// a pixel is flagged or not.
///
/// # Errors
///
/// Whatever the render refuses with.
///
/// # Examples
///
/// ```
/// use pdfcer_core::document::Document;
/// use pdfcer_render::{CoverageWarning, RenderOptions, coverage_warning_mask};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::from_bytes(
///     include_bytes!("../../../fixtures/synthetic/overprint/cmyk_k_op_over_spot.pdf").to_vec(),
/// )?;
/// let pages = pdfcer_core::page_tree::pages(&doc)?;
/// let tac = CoverageWarning::TotalAreaCoverage { limit_percent: 300 };
/// let mask = coverage_warning_mask(&doc.view(), &pages[0], 0.5, None, &RenderOptions::default(), tac)?;
/// assert_eq!(mask.mask.len(), (mask.width * mask.height) as usize);
/// # Ok(())
/// # }
/// ```
pub fn coverage_warning_mask(
    doc: &DocumentView<'_>,
    page: &Page,
    scale: f32,
    region: Option<Rect>,
    options: &RenderOptions,
    warning: CoverageWarning,
) -> Result<CoverageMask, RenderError> {
    let preview = options.separation_preview.clone().unwrap_or_default();
    let mut options = options
        .clone()
        .with_separation_preview(Some(preview))
        .with_edge_antialiasing(crate::EdgeAntialiasing::Off);
    options.coverage_warning = Some(warning);
    let rendered = match region {
        Some(r) => crate::render_page_region(doc, page, scale, r, &options)?,
        None => crate::render_page_with_view(doc, page, scale, &options)?,
    };
    let pixmap = &rendered.pixmap;
    let mask: Vec<u8> = pixmap
        .pixels()
        .iter()
        .map(|p| u8::from(p.red() > 127))
        .collect();
    let flagged = mask.iter().map(|&b| u64::from(b)).sum();
    Ok(CoverageMask {
        width: pixmap.width(),
        height: pixmap.height(),
        mask,
        flagged,
    })
}

/// The mask as an opaque raster in place of the page's colours: white
/// where `warning` is broken, black elsewhere. Rows survive banding and
/// tiling the way colour pixels do, which is why the mask travels as one.
pub(crate) fn mask_pixmap(
    buffer: &CmykBuffer,
    warning: CoverageWarning,
) -> Option<tiny_skia::Pixmap> {
    let mut out = tiny_skia::Pixmap::new(buffer.width(), buffer.height())?;
    let spots = buffer.spot_plane_count();
    for (idx, slot) in out.pixels_mut().iter_mut().enumerate() {
        let px = buffer.pixel(idx);
        let a = px.a.clamp(0.0, 1.0);
        let process = px.c.map(|t| t * a);
        let mut inks = [0.0; crate::compositor::MAX_SPOTS];
        for (dst, &t) in inks.iter_mut().zip(px.s.iter()).take(spots) {
            *dst = t * a;
        }
        let v = if warning.flags(process, inks.get(..spots).unwrap_or_default()) {
            255
        } else {
            0
        };
        if let Some(c) = tiny_skia::PremultipliedColorU8::from_rgba(v, v, v, 255) {
            *slot = c;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::CoverageWarning;

    #[test]
    fn total_area_coverage_is_strictly_above_the_limit() {
        let tac = CoverageWarning::TotalAreaCoverage { limit_percent: 300 };
        assert!(!tac.flags([0.75, 0.75, 0.75, 0.75], &[]));
        assert!(tac.flags([0.75, 0.75, 0.75, 0.76], &[]));
        assert!(tac.flags([0.75, 0.75, 0.75, 0.5], &[0.3]));
    }

    #[test]
    fn rich_black_needs_black_and_a_process_colour() {
        let rich = CoverageWarning::RichBlack {
            black_start_percent: 95,
        };
        assert!(rich.flags([0.4, 0.0, 0.0, 1.0], &[]));
        assert!(rich.flags([0.0, 0.0, 0.001, 0.95], &[]));
        assert!(!rich.flags([0.0, 0.0, 0.0, 1.0], &[0.8]));
        assert!(!rich.flags([0.4, 0.3, 0.3, 0.94], &[]));
        assert!(!rich.flags([0.0, 0.0, 0.0004, 1.0], &[]));
    }
}
