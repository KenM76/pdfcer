//! Print-production questions about a whole document, answered before any
//! page is drawn: does anything it draws ask for overprint, and which
//! PDF/X level does it claim.

mod pdfx;

pub use pdfx::{PdfxClaim, pdfx_claim, pdfx_output_intents};

use crate::object::ObjId;
use crate::view::DocumentView;

/// Whether anything a document draws asks for overprint. From
/// [`overprint_use`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OverprintUse {
    /// The page, or the stream (form XObject, annotation appearance, tiling
    /// pattern, soft-mask group or Type 3 glyph procedure), whose graphics
    /// state first turns overprint on. `None` when nothing walked does.
    pub first: Option<ObjId>,
    /// What stopped the walk reaching every drawn stream: a stream that
    /// could not be read, nesting or an operator budget exceeded. Empty
    /// when the walk was complete. When [`Self::first`] is `None` and this
    /// is not empty, "no overprint" is unproven.
    pub unwalked: Vec<String>,
}

impl OverprintUse {
    /// Something drawn asks for overprint.
    #[must_use]
    pub fn asks(&self) -> bool {
        self.first.is_some()
    }

    /// The answer covers everything: a request was found, or the walk
    /// reached every drawn stream and found none.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.first.is_some() || self.unwalked.is_empty()
    }
}

/// Whether any page content, annotation appearance (`/AP` N, R, D and
/// their states), form XObject, tiling pattern, shading pattern, soft-mask
/// group or Type 3 glyph procedure selects a graphics state with `/OP` or
/// `/op` true (ISO 32000-2 §8.6.7, Table 57; §8.7.4.1, Table 75). Only an
/// `ExtGState` a `gs` actually invokes counts; one merely listed in a
/// resource dictionary does not.
///
/// Stops at the first request. A "no" parses every content stream, so a
/// shell should ask off its UI thread for a large document. Unreadable
/// streams and limits are reported in [`OverprintUse::unwalked`], never
/// guessed past.
///
/// # Examples
///
/// ```
/// use pdfcer_core::document::Document;
/// use pdfcer_core::prepress::overprint_use;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::from_bytes(
///     include_bytes!("../../../../fixtures/synthetic/overprint/cmyk_k_op_over_spot.pdf").to_vec(),
/// )?;
/// let found = overprint_use(&doc.view());
/// assert!(found.asks() && found.is_complete());
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn overprint_use(view: &DocumentView<'_>) -> OverprintUse {
    let walk = crate::optimize::glyph_usage::find_overprint(view);
    OverprintUse {
        first: walk.overprint,
        unwalked: walk.unwalked,
    }
}

#[cfg(test)]
mod tests;
