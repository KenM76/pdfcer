//! Whether a page's metafile can stand in for the page at a printer.
//!
//! The test is "nothing the EMF writer had to rasterise or approximate": a
//! page that needed a scratch raster for transparency, a flattened blend, a
//! raster gradient, an approximated overprint or blend, or an on-screen
//! substitute for a subtractive blending space prints differently from its
//! raster, so it prints as the raster instead.

use std::fmt;

use pdfcer_core::page_tree::Page;
use pdfcer_core::view::DocumentView;

use super::{EmfExport, EmfOptions, EmfOutcome, export_emf_view};
use crate::RenderError;
use crate::font::RenderOptions;

/// Why a page prints as an image rather than as its metafile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RasterReason {
    /// A fill or stroke with a constant alpha below 1.
    SemiTransparentPaint,
    /// A transparency group (opacity, soft mask, blend, or a non-isolated
    /// group drawn isolated).
    TransparencyGroup,
    /// A blend mode the metafile cannot express, flattened, dropped or
    /// approximated.
    BlendMode,
    /// A shading or gradient drawn as pixels.
    Shading,
    /// An approximated overprint.
    Overprint,
    /// A subtractive blending colour space shown through its on-screen
    /// substitute.
    BlendingColourSpace,
    /// A tiling pattern.
    TilingPattern,
    /// A bitmap in the metafile that is none of the page's own images.
    ExtraRasters,
}

impl RasterReason {
    /// A short lower-case English name, as `pdfcer print` reports it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::SemiTransparentPaint => "semi-transparent paint",
            Self::TransparencyGroup => "transparency group",
            Self::BlendMode => "blend mode",
            Self::Shading => "shading",
            Self::Overprint => "overprint",
            Self::BlendingColourSpace => "blending colour space",
            Self::TilingPattern => "tiling pattern",
            Self::ExtraRasters => "transparency",
        }
    }
}

impl fmt::Display for RasterReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl EmfOutcome {
    /// Why this page must print as an image, or `None` when its metafile
    /// is the whole page. The first reason found is returned. A region
    /// [`EmfOptions::patch_approximations`] wrote as the raster's own pixels
    /// is the page, so it is not a reason.
    #[must_use]
    pub fn print_fallback(&self) -> Option<RasterReason> {
        use RasterReason as R;
        let t = &self.tally;
        let reasons = [
            (self.ops_rasterised_for_alpha > 0, R::SemiTransparentPaint),
            (self.layers_rasterised > 0, R::TransparencyGroup),
            (self.blends_flattened > 0, R::BlendMode),
            (self.blend_modes_dropped > 0, R::BlendMode),
            (self.gradients_rasterised > 0, R::Shading),
            (t.overprint_approximated > 0, R::Overprint),
            (t.nonseparable_approximated > 0, R::BlendMode),
            (t.non_isolated_groups_isolated > 0, R::TransparencyGroup),
            (t.colorant_buffer_on_screen > 0, R::BlendingColourSpace),
            (t.tiling_patterns > 0, R::TilingPattern),
            (t.shadings_rasterised > 0, R::Shading),
            (
                self.rasters_embedded > self.images_embedded + self.patches.rasters,
                R::ExtraRasters,
            ),
        ];
        reasons.iter().find(|(hit, _)| *hit).map(|&(_, why)| why)
    }
}

/// Why [`print_metafile`] returned no metafile.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PrintMetafileError {
    /// The page exported, but its metafile is not the whole page.
    #[error("the page prints as an image: {0}")]
    Raster(RasterReason),
    /// The page could not be exported at all.
    #[error("vector export failed: {0}")]
    Export(#[from] RenderError),
}

/// The page as a printable metafile, with images inside it sampled at
/// `raster_dpi` (the resolution the raster route would print at), or why
/// it must print as an image instead.
///
/// # Errors
///
/// [`PrintMetafileError::Raster`] when [`EmfOutcome::print_fallback`] finds
/// a reason; [`PrintMetafileError::Export`] when the export fails.
pub fn print_metafile(
    view: &DocumentView<'_>,
    page: &Page,
    render: &RenderOptions,
    raster_dpi: f32,
) -> Result<Vec<u8>, PrintMetafileError> {
    print_metafile_with(
        view,
        page,
        render,
        &EmfOptions::default().with_raster_dpi(raster_dpi),
    )
}

/// [`print_metafile`] with every [`EmfOptions`] field chosen by the caller
/// — [`EmfOptions::with_patch_approximations`] lets a page whose overprint,
/// group or blend the metafile would approximate print as a metafile with
/// pixels only there.
///
/// # Errors
///
/// As [`print_metafile`].
pub fn print_metafile_with(
    view: &DocumentView<'_>,
    page: &Page,
    render: &RenderOptions,
    emf: &EmfOptions,
) -> Result<Vec<u8>, PrintMetafileError> {
    print_export_with(view, page, render, emf).map(|export| export.emf)
}

/// [`print_metafile_with`], keeping the metafile's [`EmfOutcome`] — the
/// print route's disclosure. Its `diagnostics` carry what the page's
/// colour fell back on, e.g. `icc_profiles_unmodelled` for an embedded
/// profile no transform could be built from.
///
/// # Errors
///
/// As [`print_metafile`].
///
/// # Examples
///
/// ```no_run
/// # fn f(view: &pdfcer_core::view::DocumentView<'_>, page: &pdfcer_core::page_tree::Page)
/// # -> Result<(), pdfcer_render::emf::PrintMetafileError> {
/// use pdfcer_render::RenderOptions;
/// use pdfcer_render::emf::{EmfOptions, print_export_with};
///
/// let export = print_export_with(view, page, &RenderOptions::default(), &EmfOptions::default())?;
/// if export.outcome.diagnostics.icc_profiles_unmodelled > 0 {
///     eprintln!("an embedded colour profile could not be used");
/// }
/// # Ok(()) }
/// ```
pub fn print_export_with(
    view: &DocumentView<'_>,
    page: &Page,
    render: &RenderOptions,
    emf: &EmfOptions,
) -> Result<EmfExport, PrintMetafileError> {
    let export = export_emf_view(view, page, render, emf)?;
    match export.outcome.print_fallback() {
        None => Ok(export),
        Some(why) => Err(PrintMetafileError::Raster(why)),
    }
}
