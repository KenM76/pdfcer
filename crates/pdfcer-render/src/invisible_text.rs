//! A render option that shows **invisible text** (ISO 32000-1 §9.3.6,
//! Table 106 modes 3 and 7) as if it were filled, in a caller-chosen colour.
//!
//! Mode 3 is how OCR text layers sit under a scanned image; a shell that
//! wants the operator to *see* where that layer lies (to check an OCR
//! alignment, say) cannot get it from a normal render, which by the spec
//! paints those glyphs not at all. This is a display aid, never a document
//! change: the file is untouched, and a render without the option is
//! byte-identical to one that never knew it existed.

/// How a render paints text whose rendering mode is invisible (3 or 7).
///
/// Set through [`crate::RenderOptions::with_invisible_text`]; `None` (the
/// default) follows the spec and paints nothing.
///
/// - The glyphs fill with [`Self::rgb`] at full opacity through the
///   ordinary glyph path: same font loading, `Tz`, `Ts`, text and current
///   matrices, the enclosing clip and any hidden optional-content section.
///   Mode 7 still adds its outlines to the clip, as the spec requires.
/// - A Type 3 glyph runs its procedure with the fill colour set to
///   [`Self::rgb`]; an uncoloured (`d1`) glyph therefore takes it, while a
///   coloured (`d0`) glyph keeps the colours its procedure states.
/// - With [`Self::only`], nothing else is painted — no paths, images,
///   shadings, visible text or annotations — and the page is returned on a
///   transparent backdrop, so a shell can lay the layer over its own render.
///   Clips are still applied, so the layer is exactly the part of the
///   invisible text a normal render would have kept.
///
/// # Example
///
/// ```
/// use pdfcer_render::{InvisibleTextPaint, RenderOptions};
///
/// let options = RenderOptions::default()
///     .with_invisible_text(Some(InvisibleTextPaint::new([255, 0, 0]).with_only(true)));
/// assert_eq!(options.invisible_text.map(|p| p.rgb), Some([255, 0, 0]));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct InvisibleTextPaint {
    /// The sRGB fill colour, 0–255 per channel.
    pub rgb: [u8; 3],
    /// Paint only the invisible text, on a transparent backdrop.
    pub only: bool,
}

impl InvisibleTextPaint {
    /// Paint invisible text in `rgb`, alongside the rest of the page.
    #[must_use]
    pub const fn new(rgb: [u8; 3]) -> Self {
        Self { rgb, only: false }
    }

    /// Set whether the render paints only the invisible text.
    #[must_use]
    pub const fn with_only(mut self, only: bool) -> Self {
        self.only = only;
        self
    }

    /// The fill colour as the interpreter's working colour.
    pub(crate) fn to_rgb(self) -> crate::gstate::Rgb {
        let [r, g, b] = self.rgb;
        crate::gstate::Rgb::from_rgb(
            f32::from(r) / 255.0,
            f32::from(g) / 255.0,
            f32::from(b) / 255.0,
        )
    }
}
