//! Total area coverage measured per pixel ([`total_area_coverage`]) and the
//! preflight meter built on it ([`TacMeter`]).
//!
//! The page is composited in ink under the separation preview with every
//! plate shown, exactly as [`crate::coverage_warning_masks`] composites it,
//! and each pixel's summed coverage is read from the colorant buffer before
//! it is collapsed to colour (ISO 32000-2 §10.8.3, §11.7.4). The sum is
//! [`pdfcer_ink::coverage::total_tenths`], the same number the
//! *Total area coverage* warning compares.

mod regions;

use pdfcer_core::page_tree::Page;
use pdfcer_core::preflight::{CoverageMeter, TacFailure, TacMeasurement, TacRequest};

use crate::cmyk_buffer::CmykBuffer;
use crate::{Diagnostics, DocumentView, RenderError, RenderOptions, SeparationPreview};

/// One page's total area coverage, one value per pixel.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct TacRaster {
    /// Pixels across.
    pub width: u32,
    /// Pixels down.
    pub height: u32,
    /// Each pixel's coverage in tenths of a percent (4000 is four solid
    /// plates), row by row from the top. Empty when [`Self::in_ink`] is
    /// false.
    pub tenths: Vec<u16>,
    /// False when the page could not be composited in ink (the colorant
    /// buffer was refused for memory): nothing was measured.
    pub in_ink: bool,
    /// The render's diagnostics: what was approximated or not drawn.
    pub diagnostics: Diagnostics,
}

impl TacRaster {
    /// The highest coverage on the page, in tenths of a percent.
    #[must_use]
    pub fn max_tenths(&self) -> u16 {
        self.tenths.iter().copied().max().unwrap_or(0)
    }
}

/// Composite `page` in ink at `scale` and read every pixel's total area
/// coverage. `options`' separation preview is replaced by one that shows
/// every ink, and edges are not antialiased (a pixel is inked or not).
///
/// # Errors
///
/// Whatever the render refuses with.
///
/// # Examples
///
/// ```
/// use pdfcer_core::document::Document;
/// use pdfcer_render::{RenderOptions, total_area_coverage};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::from_bytes(
///     include_bytes!("../../../fixtures/synthetic/overprint/cmyk_k_op_over_spot.pdf").to_vec(),
/// )?;
/// let pages = pdfcer_core::page_tree::pages(&doc)?;
/// let tac = total_area_coverage(&doc.view(), &pages[0], 0.5, &RenderOptions::default())?;
/// assert_eq!(tac.tenths.len(), (tac.width * tac.height) as usize);
/// # Ok(())
/// # }
/// ```
pub fn total_area_coverage(
    view: &DocumentView<'_>,
    page: &Page,
    scale: f32,
    options: &RenderOptions,
) -> Result<TacRaster, RenderError> {
    let mut options = options
        .clone()
        .with_separation_preview(Some(SeparationPreview::new()))
        .with_edge_antialiasing(crate::EdgeAntialiasing::Off);
    options.collapse = crate::font::Collapse::Readout;
    let rendered = crate::render_page_with_view(view, page, scale, &options)?;
    let in_ink = rendered.diagnostics.cmyk_buffer_engaged;
    let tenths = if in_ink {
        rendered
            .pixmap
            .pixels()
            .iter()
            .map(|p| u16::from(p.red()) << 8 | u16::from(p.green()))
            .collect()
    } else {
        Vec::new()
    };
    Ok(TacRaster {
        width: rendered.pixmap.width(),
        height: rendered.pixmap.height(),
        tenths,
        in_ink,
        diagnostics: rendered.diagnostics,
    })
}

/// The colorant buffer as an opaque raster carrying each pixel's total
/// coverage in tenths: high byte in red, low byte in green. Rows survive
/// banding and tiling the way colour pixels do.
pub(crate) fn readout_pixmap(buffer: &CmykBuffer) -> Option<tiny_skia::Pixmap> {
    let mut out = tiny_skia::Pixmap::new(buffer.width(), buffer.height())?;
    let spots = buffer.spot_plane_count();
    for (idx, slot) in out.pixels_mut().iter_mut().enumerate() {
        let (process, inks) = buffer.pixel(idx).landed();
        let inks = inks.get(..spots).unwrap_or_default();
        let total = pdfcer_ink::coverage::total_tenths(process, inks).min(u32::from(u16::MAX));
        let [_, _, hi, lo] = total.to_be_bytes();
        if let Some(c) = tiny_skia::PremultipliedColorU8::from_rgba(hi, lo, 0, 255) {
            *slot = c;
        }
    }
    Some(out)
}

/// The shipped [`CoverageMeter`]: [`total_area_coverage`] at the request's
/// resolution, its over-limit pixels grouped into regions, and the render's
/// approximations disclosed.
#[derive(Debug, Clone, Default)]
pub struct TacMeter {
    options: RenderOptions,
}

impl TacMeter {
    /// A meter rendering with the default options.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A meter rendering with `options` (fonts, annotation scope, output
    /// intent); its separation preview and antialiasing are overridden.
    #[must_use]
    pub const fn with_options(options: RenderOptions) -> Self {
        Self { options }
    }
}

impl CoverageMeter for TacMeter {
    fn measure_tac(
        &self,
        view: &DocumentView<'_>,
        page: &Page,
        request: &TacRequest,
    ) -> Result<TacMeasurement, TacFailure> {
        if !(TacRequest::MIN_PPI..=TacRequest::MAX_PPI).contains(&request.ppi) {
            return Err(TacFailure::new(format!(
                "{} ppi is outside {}-{}",
                request.ppi,
                TacRequest::MIN_PPI,
                TacRequest::MAX_PPI
            )));
        }
        #[allow(clippy::cast_possible_truncation)] // ppi is at most 600
        let scale = (request.ppi / 72.0) as f32;
        let raster = total_area_coverage(view, page, scale, &self.options)
            .map_err(|e| TacFailure::new(e.to_string()))?;
        if !raster.in_ink {
            return Err(TacFailure::new(
                "the colorant buffer was refused for memory at this resolution",
            ));
        }
        let mut m = regions::measure(&raster, page, f64::from(scale) * page.user_unit, request);
        m.ppi = request.ppi;
        disclose(&raster.diagnostics, &mut m);
        Ok(m)
    }
}

/// The render's approximations as caveats, and what it could not draw as
/// not measured.
fn disclose(d: &Diagnostics, m: &mut TacMeasurement) {
    if d.cmyk_bridged_pixels > 0 {
        m.caveats.push(format!(
            "{} pixels (images and other content decoded to sRGB) reached ink through the \
             renderer's sRGB bridge, not an ICC conversion",
            d.cmyk_bridged_pixels
        ));
    }
    if d.cmyk_groups_approximated > 0 {
        m.caveats.push(format!(
            "{} knockout or non-isolated transparency groups were composited approximately",
            d.cmyk_groups_approximated
        ));
    }
    if d.cmyk_spots_flattened > 0 {
        m.not_measured.push(format!(
            "{} spot inks past the renderer's {}-plane limit were folded into process ink, so \
             their own coverage was not counted",
            d.cmyk_spots_flattened,
            pdfcer_ink::MAX_SPOTS
        ));
    }
    let undrawn = [
        (d.images_unsupported, "images"),
        (d.fonts_unsupported, "fonts"),
        (d.contents_streams_unresolved, "content streams"),
        (d.unknown_ops, "unknown operators"),
    ];
    let undrawn: Vec<String> = undrawn
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, what)| format!("{n} {what}"))
        .collect();
    if !undrawn.is_empty() {
        m.not_measured.push(format!(
            "content the renderer could not draw ({}) painted no ink and was not counted",
            undrawn.join(", ")
        ));
    }
}
