//! The face a run of XFA text is set in: a shell-supplied one when the
//! [`XfaFonts`] has it, else the standard-14 best match (p.58).

use std::sync::Arc;

use crate::fontdata::{Std14, std14_descriptor, std14_styled};
use crate::vartext::{encode_winansi, text_width};
use crate::xfa::{XfaFace, XfaFonts};

/// A face text is measured and drawn in.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::xfa) enum Face {
    Std(Std14),
    Supplied(Arc<XfaFace>),
}

impl Face {
    /// The face `typeface` is drawn in, and whether that is a
    /// standard-14 substitution for a face not found.
    pub(in crate::xfa) fn resolve(
        fonts: &XfaFonts,
        typeface: &str,
        bold: bool,
        italic: bool,
    ) -> (Self, bool) {
        let t = typeface.trim().to_ascii_lowercase();
        let exact = matches!(
            t.as_str(),
            "courier" | "helvetica" | "times" | "times roman" | "times-roman"
        );
        if !exact && let Some(f) = fonts.face(typeface, bold, italic) {
            return (Self::Supplied(f), false);
        }
        let serif = [
            "times", "georgia", "garamond", "minion", "palatino", "cambria",
        ];
        let family = if t.contains("courier") || t.contains("mono") {
            Std14::Courier
        } else if serif.iter().any(|s| t.contains(s))
            || (t.contains("serif") && !t.contains("sans"))
        {
            Std14::TimesRoman
        } else {
            Std14::Helvetica
        };
        let face = Self::Std(std14_styled(family, bold, italic).unwrap_or(family));
        if !exact {
            fonts.record_fallback(typeface, || super::font_res::base_font(&face));
        }
        (face, !exact)
    }

    /// The advance of `text` at `size`.
    pub(in crate::xfa) fn width(&self, size: f64, text: &str) -> f64 {
        match self {
            Self::Std(f) => text_width(*f, size, text),
            Self::Supplied(f) => {
                let units: u32 = encode_winansi(text)
                    .0
                    .iter()
                    .map(|c| u32::from(f.width(*c).unwrap_or(0)))
                    .sum();
                f64::from(units) / 1000.0 * size
            }
        }
    }

    /// Whether some character of `text` has no glyph in this face.
    pub(in crate::xfa) fn lacks_glyph(&self, text: &str) -> bool {
        let (codes, miss) = encode_winansi(text);
        miss > 0
            || match self {
                Self::Std(_) => false,
                Self::Supplied(f) => codes.iter().any(|c| f.width(*c).is_none()),
            }
    }

    /// Ascent and descent (positive) in glyph space.
    pub(in crate::xfa) fn extents(&self) -> (f64, f64) {
        match self {
            Self::Std(f) => {
                let d = std14_descriptor(*f);
                (f64::from(d.ascender), f64::from(d.descender).abs())
            }
            Self::Supplied(f) => (f.ascent, f.descent.abs()),
        }
    }
}
