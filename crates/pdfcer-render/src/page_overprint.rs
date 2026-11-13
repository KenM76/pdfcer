//! Whether a page uses overprint ([`PageOverprint`], [`page_overprint`]):
//! Acrobat Output Preview's "Page has overprint", answered from the
//! renderer's own reading of the graphics state rather than a second scan.

use pdfcer_core::page_tree::Page;

use crate::{Diagnostics, DocumentView, RenderError, RenderOptions, SeparationPreview};

/// The scale [`page_overprint`] draws at. The answer does not depend on
/// pixels (both counters are decided from the graphics and colour state at
/// each paint), so the cheapest raster that still runs every operator.
const PRESENCE_SCALE: f32 = 0.125;

/// Whether a page's drawing uses overprint, two ways (ISO 32000-2 §8.6.7,
/// §11.7.4).
///
/// [`Self::set`] is what Acrobat's "Page has overprint: Yes" means: a
/// graphics state with `/OP` or `/op` true was selected while the page
/// (its content, the forms and patterns it paints, and the annotation
/// appearances the render draws) was interpreted. It says nothing about
/// whether anything shows.
///
/// [`Self::takes_effect`] is the stricter answer: at least one fill,
/// stroke, glyph, image or shading was painted with overprint on and
/// leaves some colorant to the backdrop that a knockout would erase. That
/// is a source in `Separation` or `DeviceN` not naming every process ink,
/// `DeviceCMYK` under `/OPM 1` with a zero component, or any process
/// source over a page with a spot plane (Table 149, "any process colour
/// space × spot colorant"). `DeviceCMYK` at `/OPM 0` over process ink
/// alone is set but has no effect. `takes_effect` implies `set`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct PageOverprint {
    /// A selected graphics state turned overprint on.
    pub set: bool,
    /// A paint under overprint left a colorant to the backdrop.
    pub takes_effect: bool,
}

impl PageOverprint {
    /// The answer from a render's diagnostics, so a shell that already
    /// rendered the page (the separation preview) need not render again.
    ///
    /// `takes_effect` is `false` for a render made under
    /// [`crate::OverprintPreview::Off`]: with simulation off nothing
    /// overprints, so read it from a render that simulates (the default,
    /// and always the case under a separation preview).
    #[must_use]
    pub fn from_diagnostics(diagnostics: &Diagnostics) -> Self {
        Self {
            set: diagnostics.overprint_requested > 0,
            takes_effect: diagnostics.overprint_effective > 0,
        }
    }
}

/// Whether `page` uses overprint, for a caller with no render of it to
/// read [`PageOverprint::from_diagnostics`] from.
///
/// Draws the page once at a small scale under a [`SeparationPreview`] with
/// every ink shown, keeping `options`' annotation scope, layer visibility
/// and other content choices, so the answer covers exactly what that
/// render would paint.
///
/// # Errors
///
/// Whatever [`crate::render_page_with_view`] refuses with; the answer is
/// then unknown, never "no".
///
/// # Examples
///
/// ```
/// use pdfcer_core::document::Document;
/// use pdfcer_render::{RenderOptions, page_overprint};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::from_bytes(
///     include_bytes!("../../../fixtures/synthetic/overprint/cmyk_k_op_over_spot.pdf").to_vec(),
/// )?;
/// let pages = pdfcer_core::page_tree::pages(&doc)?;
/// let found = page_overprint(&doc.view(), &pages[0], &RenderOptions::default())?;
/// assert!(found.set && found.takes_effect);
/// # Ok(())
/// # }
/// ```
pub fn page_overprint(
    doc: &DocumentView<'_>,
    page: &Page,
    options: &RenderOptions,
) -> Result<PageOverprint, RenderError> {
    let options = options
        .clone()
        .with_separation_preview(Some(SeparationPreview::new()))
        .with_overprint_preview(crate::OverprintPreview::Simulate)
        .with_edge_antialiasing(crate::EdgeAntialiasing::Off);
    let rendered = crate::render_page_with_view(doc, page, PRESENCE_SCALE, &options)?;
    Ok(PageOverprint::from_diagnostics(&rendered.diagnostics))
}
