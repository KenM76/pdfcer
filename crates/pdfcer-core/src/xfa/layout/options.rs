//! [`LayoutOptions`] and the gap settings it carries.

use super::PageAreaKeep;

/// What a `rotate` that is not a multiple of 90 does (gap G-L29: XFA 3.3
/// p.685 allows only multiples of 90 and says nothing of other values).
/// Either way the paint step discloses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum OddRotate {
    /// No rotation.
    #[default]
    Ignore,
    /// The nearest multiple of 90.
    Nearest,
}

/// What to do with a negative resolved coordinate, which XFA leaves
/// unspecified (gap G-L1). Reported as [`LayoutWarning::NegativeCoordinate`](super::LayoutWarning::NegativeCoordinate)
/// either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum NegativeCoordinates {
    /// Keep the template's geometry, even if it lands off the page.
    #[default]
    Keep,
    /// Clamp the coordinate to zero.
    Clamp,
}

/// Which subforms lay an overflow leader and trailer when a split cuts
/// nested subforms too. XFA 3.3 (Ch.8 pp.318-325) ties them to "the
/// subform that overflows" without saying which of the cut subforms that
/// is (gap G-L21).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum NestedOverflow {
    /// The innermost cut subform alone, spending its own leaders and
    /// trailers, then each enclosing level's in turn (p.325): one overflow,
    /// one leader and one trailer.
    #[default]
    Innermost,
    /// The innermost cut subform as above, plus every enclosing cut subform
    /// that asserts a leader (or trailer) of its own, which lays that too,
    /// trailers innermost first and leaders outermost first.
    EachAsserting,
}

/// Settings for [`layout_template`](super::layout_template).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct LayoutOptions {
    /// Gap G-L1.
    pub negative_coordinates: NegativeCoordinates,
    /// Gap G-L21.
    pub nested_overflow: NestedOverflow,
    /// Gap G-L12.
    pub page_area_keep: PageAreaKeep,
    /// Gap G-L29.
    pub odd_rotate: OddRotate,
}

impl LayoutOptions {
    /// The defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set [`Self::negative_coordinates`].
    #[must_use]
    pub const fn with_negative_coordinates(mut self, v: NegativeCoordinates) -> Self {
        self.negative_coordinates = v;
        self
    }

    /// Set [`Self::nested_overflow`].
    #[must_use]
    pub const fn with_nested_overflow(mut self, v: NestedOverflow) -> Self {
        self.nested_overflow = v;
        self
    }

    /// Set [`Self::page_area_keep`].
    #[must_use]
    pub const fn with_page_area_keep(mut self, v: PageAreaKeep) -> Self {
        self.page_area_keep = v;
        self
    }

    /// Set [`Self::odd_rotate`].
    #[must_use]
    pub const fn with_odd_rotate(mut self, v: OddRotate) -> Self {
        self.odd_rotate = v;
        self
    }
}
