//! Ink coverage warnings ([`CoverageWarning`], [`coverage_warning_mask`]):
//! Acrobat Output Preview's *Total area coverage* and *Rich black*, as a
//! per-pixel mask read from the colorant buffer the separation preview
//! composites in.
//!
//! One definition, here, for the GUI's warning layer and the CLI alike.

use pdfcer_core::page_tree::{Page, Rect};

use crate::cmyk_buffer::CmykBuffer;
use crate::{DocumentView, RenderError, RenderOptions};

pub use pdfcer_ink::coverage::{CoverageMask, CoverageWarning};

/// The pixels of `page` (or `region` of it, in page space as
/// [`crate::render_page_region`] takes it) at `scale` that break
/// `warning`. One rule of [`coverage_warning_masks`].
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
    let pixmap = render_rules(doc, page, scale, region, options, &[warning])?;
    Ok(channel_mask(&pixmap, 0))
}

/// One [`CoverageMask`] per entry of `warnings`, in the same order, from
/// one render per three rules.
///
/// Renders the page in ink under `options`' separation preview (all inks
/// shown when it has none), so each mask lines up pixel for pixel with the
/// preview raster the caller draws it over. Edges are not antialiased:
/// a pixel is flagged or not. An empty `warnings` renders nothing.
///
/// # Errors
///
/// Whatever the render refuses with.
///
/// # Examples
///
/// ```
/// use pdfcer_core::document::Document;
/// use pdfcer_render::{CoverageWarning, RenderOptions, coverage_warning_masks};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::from_bytes(
///     include_bytes!("../../../fixtures/synthetic/overprint/cmyk_k_op_over_spot.pdf").to_vec(),
/// )?;
/// let pages = pdfcer_core::page_tree::pages(&doc)?;
/// let rules = [
///     CoverageWarning::TotalAreaCoverage { limit_percent: 300 },
///     CoverageWarning::RichBlack { black_start_percent: 95 },
/// ];
/// let masks = coverage_warning_masks(&doc.view(), &pages[0], 0.5, None, &RenderOptions::default(), &rules)?;
/// assert_eq!(masks.len(), 2);
/// # Ok(())
/// # }
/// ```
pub fn coverage_warning_masks(
    doc: &DocumentView<'_>,
    page: &Page,
    scale: f32,
    region: Option<Rect>,
    options: &RenderOptions,
    warnings: &[CoverageWarning],
) -> Result<Vec<CoverageMask>, RenderError> {
    let mut out = Vec::with_capacity(warnings.len());
    for chunk in warnings.chunks(3) {
        let pixmap = render_rules(doc, page, scale, region, options, chunk)?;
        out.extend((0..chunk.len()).map(|ch| channel_mask(&pixmap, ch)));
    }
    Ok(out)
}

/// One mask render of up to three `rules` (extra rules are ignored).
fn render_rules(
    doc: &DocumentView<'_>,
    page: &Page,
    scale: f32,
    region: Option<Rect>,
    options: &RenderOptions,
    rules: &[CoverageWarning],
) -> Result<tiny_skia::Pixmap, RenderError> {
    let preview = options.separation_preview.clone().unwrap_or_default();
    let mut options = options
        .clone()
        .with_separation_preview(Some(preview))
        .with_edge_antialiasing(crate::EdgeAntialiasing::Off);
    let mut slots: RuleSlots = [None; 3];
    for (slot, &w) in slots.iter_mut().zip(rules) {
        *slot = Some(w);
    }
    options.collapse = crate::font::Collapse::Warnings(slots);
    let rendered = match region {
        Some(r) => crate::render_page_region(doc, page, scale, r, &options)?,
        None => crate::render_page_with_view(doc, page, scale, &options)?,
    };
    Ok(rendered.pixmap)
}

/// The mask carried in RGB channel `channel` (0..3) of a mask render.
fn channel_mask(pixmap: &tiny_skia::Pixmap, channel: usize) -> CoverageMask {
    let mask: Vec<u8> = pixmap
        .pixels()
        .iter()
        .map(|p| {
            let v = match channel {
                0 => p.red(),
                1 => p.green(),
                _ => p.blue(),
            };
            u8::from(v > 127)
        })
        .collect();
    CoverageMask::from_mask(pixmap.width(), pixmap.height(), mask)
}

/// Up to three rules collapsed in one render, one per RGB channel.
pub(crate) type RuleSlots = [Option<CoverageWarning>; 3];

/// The masks as an opaque raster in place of the page's colours: channel
/// `i` is 255 where `rules[i]` is broken, 0 elsewhere. Rows survive banding
/// and tiling the way colour pixels do, which is why the masks travel as
/// one.
pub(crate) fn mask_pixmap(buffer: &CmykBuffer, rules: &RuleSlots) -> Option<tiny_skia::Pixmap> {
    let mut out = tiny_skia::Pixmap::new(buffer.width(), buffer.height())?;
    let spots = buffer.spot_plane_count();
    for (idx, slot) in out.pixels_mut().iter_mut().enumerate() {
        let (process, inks) = buffer.pixel(idx).landed();
        let inks = inks.get(..spots).unwrap_or_default();
        let [r, g, b] = rules.map(|w| {
            if w.is_some_and(|w| w.flags(process, inks)) {
                255
            } else {
                0
            }
        });
        if let Some(c) = tiny_skia::PremultipliedColorU8::from_rgba(r, g, b, 255) {
            *slot = c;
        }
    }
    Some(out)
}
