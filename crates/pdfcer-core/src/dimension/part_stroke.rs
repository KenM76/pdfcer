//! The stroke of one drawn part of a ce dimension, set apart from the ce
//! dimension's own lines and cascaded field by field (pdfcer-gui request
//! G329; the shape centre lines and centre marks reuse).

use super::dash::DimDash;
use crate::annot_author::Color;

/// Overrides for the stroke of one drawn part of a ce dimension (today the
/// circle of a radius/diameter ce dimension). Every `None` follows the ce
/// dimension's own resolved `dash`, `line_width` or `color`, so
/// [`Self::FOLLOW`] draws the part exactly as the rest of the ce dimension.
///
/// Carried on every tier of the style cascade ([`super::StyleDefaults`],
/// [`super::GroupStyle`], [`super::StyleOverrides`]) and resolved per field:
/// the ce dimension's `Some` wins, then the group's, then the factory's, then
/// the ce dimension's main stroke.
///
/// **Opacity is not a part property.** It stays annotation-wide (`/CA`,
/// ISO 32000-1 §12.5.2 Table 166), as in CAD drafting packages, which have no
/// per-line opacity; a per-part `ExtGState` path is deliberately absent.
///
/// Non-exhaustive so a named line type can join `dash`; build one with
/// [`Self::FOLLOW`] and the `with_` builders.
///
/// # Examples
///
/// ```
/// use pdfcer_core::dimension::{DimDash, PartStroke};
///
/// let phantom = PartStroke::FOLLOW
///     .with_dash(DimDash::new(&[6.0, 2.0, 1.0, 2.0]))
///     .with_width(Some(0.35));
/// assert!(!phantom.is_follow());
/// assert_eq!(phantom.color, None); // the colour still follows
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub struct PartStroke {
    /// Dash pattern; `None` follows the ce dimension's `dash`.
    /// `Some(DimDash::SOLID)` keeps the part solid under dashed lines.
    pub dash: Option<DimDash>,
    /// Stroke width in points; `None` follows the ce dimension's
    /// `line_width`.
    pub width: Option<f64>,
    /// Stroke colour; `None` follows the ce dimension's `color`.
    pub color: Option<Color>,
}

impl PartStroke {
    /// Follow the ce dimension in every field.
    pub const FOLLOW: Self = Self {
        dash: None,
        width: None,
        color: None,
    };

    /// Builder: set or clear the dash override.
    #[must_use]
    pub const fn with_dash(mut self, dash: Option<DimDash>) -> Self {
        self.dash = dash;
        self
    }

    /// Builder: set or clear the width override, in points.
    #[must_use]
    pub const fn with_width(mut self, width: Option<f64>) -> Self {
        self.width = width;
        self
    }

    /// Builder: set or clear the colour override.
    #[must_use]
    pub const fn with_color(mut self, color: Option<Color>) -> Self {
        self.color = color;
        self
    }

    /// Whether every field follows the ce dimension.
    #[must_use]
    pub fn is_follow(&self) -> bool {
        *self == Self::FOLLOW
    }

    /// How many fields are set.
    #[must_use]
    pub fn count(&self) -> usize {
        usize::from(self.dash.is_some())
            + usize::from(self.width.is_some())
            + usize::from(self.color.is_some())
    }

    /// Per field, this tier's value, else `lower`'s.
    #[must_use]
    pub fn or(self, lower: Self) -> Self {
        Self {
            dash: self.dash.or(lower.dash),
            width: self.width.or(lower.width),
            color: self.color.or(lower.color),
        }
    }

    /// The stroke this part draws with, each unset field taken from `main`,
    /// the ce dimension's own stroke.
    #[must_use]
    pub fn resolve(self, main: ResolvedStroke) -> ResolvedStroke {
        ResolvedStroke {
            dash: self.dash.unwrap_or(main.dash),
            width: self.width.unwrap_or(main.width),
            color: self.color.unwrap_or(main.color),
        }
    }
}

/// A fully specified stroke: what one part of a ce dimension is drawn with.
/// Read it from [`super::DimensionStyle::line_stroke`] or
/// [`super::DimensionStyle::circle_stroke`].
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct ResolvedStroke {
    /// Dash pattern.
    pub dash: DimDash,
    /// Stroke width in points.
    pub width: f64,
    /// Stroke colour.
    pub color: Color,
}

impl ResolvedStroke {
    /// A stroke from its three fields.
    #[must_use]
    pub const fn new(dash: DimDash, width: f64, color: Color) -> Self {
        Self { dash, width, color }
    }
}
