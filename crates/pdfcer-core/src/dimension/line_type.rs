//! Named drafting line types for a ce dimension's lines and parts
//! (pdfcer-gui request G331): the pattern is derived from the name, the
//! drafting standard, the line width and a line-type scale, so a change of
//! standard or width re-derives it.

use super::author::DimensionStyle;
use super::centre_style::CentreStyle;
use super::dash::DimDash;
use super::group::DimStandard;
use super::part_stroke::PartStroke;

/// A named drafting line type, in the CAD package's `swLineStyles_e` order.
///
/// The thin/thick chain (a chain line stroked heavier at its ends) is not a
/// dash pattern and is not offered; its absence from [`Self::ALL`] is how a
/// shell knows.
///
/// # Pattern source
///
/// [`DimStandard::Ansi`] runs are in points, from the AutoCAD `acad.lin`
/// half-scale family the centre line already follows (`CENTER2`): hidden
/// `HIDDEN2` `[9 4.5]`, phantom `PHANTOM2` `[45 9 9 9 9 9]`, chain
/// `DASHDOT2` `[18 9 dot 9]`, centre `CENTER2` `[54 9 9 9]`.
///
/// [`DimStandard::Iso`] runs are multiples of the line width `w`, after the
/// line elements of ISO 128-2 (formerly ISO 128-20: long dash 24w, dash 12w,
/// gap 3w, dot 0.5w): hidden type 02 `[12w 3w]`, chain and centre type 04
/// `[24w 3w dot 3w]`, phantom type 05 `[24w 3w dot 3w dot 3w]`. ISO draws a
/// centre line as the long-dash dotted line, so chain and centre coincide
/// under ISO.
///
/// Stitch has neither an `acad.lin` nor an ISO 128 counterpart; pdfcer draws
/// it as equal short dashes and gaps, `[4.5 4.5]` (ANSI) or `[3w 3w]` (ISO).
///
/// Every run except a dot is multiplied by the line-type scale. A dot is
/// drawn `w` long whatever the scale, because the cap is butt and a
/// zero-length dash would draw nothing.
///
/// # Examples
///
/// ```
/// use pdfcer_core::dimension::{DimStandard, LineType};
///
/// let centre = LineType::Center.runs(DimStandard::Ansi, 0.75, 1.0);
/// assert_eq!(centre.pattern(), &[54.0, 9.0, 9.0, 9.0]);
/// let chain = LineType::Chain.runs(DimStandard::Iso, 0.5, 2.0);
/// assert_eq!(chain.pattern(), &[24.0, 3.0, 0.5, 3.0]);
/// assert_eq!(LineType::parse("phantom"), Some(LineType::Phantom));
/// assert!(LineType::Continuous.runs(DimStandard::Iso, 1.0, 1.0).is_solid());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LineType {
    /// Solid.
    Continuous,
    /// Dashed: dash, gap.
    Hidden,
    /// Long dash double-dotted.
    Phantom,
    /// Long dash dotted.
    Chain,
    /// Long dash short dash (ANSI); long dash dotted (ISO).
    Center,
    /// Short dashes, evenly and finely spaced.
    Stitch,
}

/// A run drawn as a dot.
const DOT: f64 = 0.0;

impl LineType {
    /// Every line type pdfcer draws, in the reference order.
    pub const ALL: [Self; 6] = [
        Self::Continuous,
        Self::Hidden,
        Self::Phantom,
        Self::Chain,
        Self::Center,
        Self::Stitch,
    ];

    /// The CLI and sidecar spelling.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Continuous => "continuous",
            Self::Hidden => "hidden",
            Self::Phantom => "phantom",
            Self::Chain => "chain",
            Self::Center => "center",
            Self::Stitch => "stitch",
        }
    }

    /// Parse [`Self::token`], ignoring ASCII case; `centre` is accepted for
    /// `center`.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if s.eq_ignore_ascii_case("centre") {
            return Some(Self::Center);
        }
        Self::ALL
            .into_iter()
            .find(|t| t.token().eq_ignore_ascii_case(s))
    }

    /// The table row: runs, and whether they are multiples of the width.
    const fn table(self, standard: DimStandard) -> (&'static [f64], bool) {
        match (self, standard) {
            (Self::Continuous, _) => (&[], false),
            (Self::Hidden, DimStandard::Ansi) => (&[9.0, 4.5], false),
            (Self::Phantom, DimStandard::Ansi) => (&[45.0, 9.0, 9.0, 9.0, 9.0, 9.0], false),
            (Self::Chain, DimStandard::Ansi) => (&[18.0, 9.0, DOT, 9.0], false),
            (Self::Center, DimStandard::Ansi) => (&[54.0, 9.0, 9.0, 9.0], false),
            (Self::Stitch, DimStandard::Ansi) => (&[4.5, 4.5], false),
            (Self::Hidden, DimStandard::Iso) => (&[12.0, 3.0], true),
            (Self::Phantom, DimStandard::Iso) => (&[24.0, 3.0, DOT, 3.0, DOT, 3.0], true),
            (Self::Chain | Self::Center, DimStandard::Iso) => (&[24.0, 3.0, DOT, 3.0], true),
            (Self::Stitch, DimStandard::Iso) => (&[3.0, 3.0], true),
        }
    }

    /// The dash pattern for a line `width` points wide at line-type `scale`
    /// under `standard`. Solid for [`Self::Continuous`], and for a width or
    /// scale that is not finite and positive.
    #[must_use]
    pub fn runs(self, standard: DimStandard, width: f64, scale: f64) -> DimDash {
        if !(width.is_finite() && width > 0.0 && scale.is_finite() && scale > 0.0) {
            return DimDash::SOLID;
        }
        let (runs, per_width) = self.table(standard);
        let unit = if per_width { width } else { 1.0 };
        let drawn: Vec<f64> = runs
            .iter()
            .map(|&r| if r > DOT { r * unit * scale } else { width })
            .collect();
        DimDash::new(&drawn).unwrap_or(DimDash::SOLID)
    }
}

/// A stroke's dash as chosen: a named [`LineType`], derived when drawn, or a
/// custom run list in points (what a document written before named line
/// types carries).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LineTypeRef {
    /// A named type, re-derived from the standard, width and scale.
    Named(LineType),
    /// Fixed runs in points; the width and line-type scale do not apply.
    Custom(DimDash),
}

impl LineTypeRef {
    /// The runs drawn for a line `width` points wide; see [`LineType::runs`].
    #[must_use]
    pub fn runs(self, standard: DimStandard, width: f64, scale: f64) -> DimDash {
        match self {
            Self::Named(t) => t.runs(standard, width, scale),
            Self::Custom(d) => d,
        }
    }

    /// The name a chooser shows: the named type, [`LineType::Continuous`]
    /// for a solid custom pattern, `None` for any other custom pattern.
    #[must_use]
    pub const fn line_type(self) -> Option<LineType> {
        match self {
            Self::Named(t) => Some(t),
            Self::Custom(d) if d.is_solid() => Some(LineType::Continuous),
            Self::Custom(_) => None,
        }
    }

    /// Whether this is [`Self::Named`], the value sidecar version 10 added.
    #[must_use]
    pub const fn is_named(self) -> bool {
        matches!(self, Self::Named(_))
    }
}

impl Default for LineTypeRef {
    fn default() -> Self {
        Self::Custom(DimDash::SOLID)
    }
}

impl From<LineType> for LineTypeRef {
    fn from(t: LineType) -> Self {
        Self::Named(t)
    }
}

impl From<DimDash> for LineTypeRef {
    fn from(d: DimDash) -> Self {
        Self::Custom(d)
    }
}

/// Whether a tier uses something only sidecar version 10 can store: a named
/// line type on its lines or any part, or a line-type scale.
pub(crate) fn tier_uses_line_types(
    dash: Option<LineTypeRef>,
    scale: Option<f64>,
    circle: PartStroke,
    centre: CentreStyle,
) -> bool {
    let named = |d: Option<LineTypeRef>| d.is_some_and(LineTypeRef::is_named);
    scale.is_some()
        || named(dash)
        || named(circle.dash)
        || named(centre.line.dash)
        || named(centre.mark.dash)
}

/// Whether `scale` is a usable line-type scale: finite and positive.
#[must_use]
pub fn line_type_scale_in_range(scale: f64) -> bool {
    scale.is_finite() && scale > 0.0
}

impl DimensionStyle {
    /// The circle's dash as chosen: its own, else the lines'.
    #[must_use]
    pub fn circle_line_type(&self) -> LineTypeRef {
        self.circle.dash.unwrap_or(self.line_type)
    }

    /// A centre line's dash as chosen: its own, else [`LineType::Center`].
    #[must_use]
    pub fn centre_line_type(&self) -> LineTypeRef {
        self.centre
            .line
            .dash
            .unwrap_or(LineTypeRef::Named(LineType::Center))
    }

    /// A centre mark's dash as chosen: its own, else solid.
    #[must_use]
    pub fn centre_mark_line_type(&self) -> LineTypeRef {
        self.centre.mark.dash.unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Would catch: a pattern longer than the run limit, or a zero-length
    /// dot reaching the content stream.
    #[test]
    fn every_type_builds_under_both_standards_with_visible_dots() {
        for t in LineType::ALL {
            for std in [DimStandard::Ansi, DimStandard::Iso] {
                let d = t.runs(std, 0.5, 3.0);
                assert_eq!(d.is_solid(), t == LineType::Continuous, "{t:?} {std:?}");
                assert!(d.pattern().iter().all(|r| *r > 0.0), "{t:?} {std:?}");
            }
            assert_eq!(LineType::parse(t.token()), Some(t));
        }
        assert_eq!(
            LineType::Phantom
                .runs(DimStandard::Iso, 1.0, 1.0)
                .pattern()
                .len(),
            6
        );
    }

    #[test]
    fn a_bad_scale_or_width_draws_solid() {
        for (w, s) in [(0.0, 1.0), (1.0, 0.0), (1.0, f64::NAN), (-1.0, 1.0)] {
            assert!(LineType::Hidden.runs(DimStandard::Ansi, w, s).is_solid());
        }
    }
}
