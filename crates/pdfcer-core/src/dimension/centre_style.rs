//! The style of centre geometry: a centre line's and a centre mark's own
//! stroke, cascaded per field like [`PartStroke`] (pdfcer-gui request G334),
//! and which style properties each kind of ce dimension reads.

use super::author::DimensionStyle;
use super::dash::DimDash;
use super::group::DimensionKind;
use super::part_stroke::{PartStroke, ResolvedStroke};

/// The style of centre lines and centre marks, carried on every tier of the
/// cascade ([`super::StyleDefaults`], [`super::GroupStyle`],
/// [`super::StyleOverrides`]) and resolved per field: the ce dimension's
/// `Some` wins, then the group's, then the factory's.
///
/// What an unset field draws with differs from [`PartStroke`] on a circle:
///
/// | part | unset `dash` | unset `width` / `color` |
/// |---|---|---|
/// | `line` (centre line) | the standard's long-short-long centre pattern ([`DimensionStyle::centre_line_dash`]) | the ce dimension's `line_width` / `color` |
/// | `mark` (centre mark, standalone or on a radius/diameter) | solid | the ce dimension's `line_width` / `color` |
///
/// The ce dimension's own `dash` is never read by centre geometry: a group
/// that dashes its measurements does not turn its centre lines into hidden
/// lines. Set `line.dash` or `mark.dash` to change their pattern.
///
/// Non-exhaustive: build with [`Self::INHERIT`] and the `with_` builders.
///
/// # Examples
///
/// ```
/// use pdfcer_core::dimension::{CentreStyle, DimDash, PartStroke};
///
/// let s = CentreStyle::INHERIT.with_line(PartStroke::FOLLOW.with_width(Some(0.35)));
/// assert_eq!(s.count(), 1);
/// assert!(s.mark.is_follow());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub struct CentreStyle {
    /// A centre line's stroke.
    pub line: PartStroke,
    /// A centre mark's stroke (the cross and its extension lines).
    pub mark: PartStroke,
}

impl CentreStyle {
    /// Inherit every field.
    pub const INHERIT: Self = Self {
        line: PartStroke::FOLLOW,
        mark: PartStroke::FOLLOW,
    };

    /// Builder: the centre line's stroke.
    #[must_use]
    pub const fn with_line(mut self, line: PartStroke) -> Self {
        self.line = line;
        self
    }

    /// Builder: the centre mark's stroke.
    #[must_use]
    pub const fn with_mark(mut self, mark: PartStroke) -> Self {
        self.mark = mark;
        self
    }

    /// Whether every field inherits.
    #[must_use]
    pub fn is_inherit(&self) -> bool {
        *self == Self::INHERIT
    }

    /// How many fields are set.
    #[must_use]
    pub fn count(&self) -> usize {
        self.line.count() + self.mark.count()
    }

    /// Per field, this tier's value, else `lower`'s.
    #[must_use]
    pub fn or(self, lower: Self) -> Self {
        Self {
            line: self.line.or(lower.line),
            mark: self.mark.or(lower.mark),
        }
    }
}

impl DimensionStyle {
    /// The stroke a centre line is drawn with, before fitting to its length:
    /// [`CentreStyle::line`] with an unset dash taken from
    /// [`Self::centre_line_dash`] at the centre line's own width, and an
    /// unset width or colour from the ce dimension's lines.
    #[must_use]
    pub fn centre_line_stroke(&self) -> ResolvedStroke {
        let width = self.centre.line.width.unwrap_or(self.line_width);
        let nominal = DimensionStyle {
            line_width: width,
            ..*self
        }
        .centre_line_dash();
        self.centre
            .line
            .resolve(ResolvedStroke::new(nominal, self.line_width, self.color))
    }

    /// The stroke a centre mark is drawn with: [`CentreStyle::mark`], an
    /// unset dash solid, an unset width or colour from the ce dimension's
    /// lines.
    #[must_use]
    pub fn centre_mark_stroke(&self) -> ResolvedStroke {
        self.centre.mark.resolve(ResolvedStroke::new(
            DimDash::SOLID,
            self.line_width,
            self.color,
        ))
    }
}

/// How a dash pattern was laid along a line it is fitted to
/// ([`DimensionKind::centre_line_dash_fit`]). The disclosure for a shell's
/// properties field; nothing is marked on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DashFit {
    /// Drawn with the pattern's own runs, or the pattern is solid.
    Nominal,
    /// Stretched or shrunk so the line starts and ends on the first run.
    Stretched,
    /// Drawn solid: the line is shorter than one cycle plus the closing run.
    SolidTooShort,
}

impl DashFit {
    /// A short stable token (`nominal` / `stretched` / `solid-too-short`).
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Nominal => "nominal",
            Self::Stretched => "stretched",
            Self::SolidTooShort => "solid-too-short",
        }
    }
}

/// `nominal` stretched so a line of `length` starts and ends on its first
/// run (drafting practice for centre lines), as the runs to stroke, plus how
/// it was fitted. An odd-length pattern is doubled first, as ISO 32000-2
/// §8.4.3.6 repeats it. Empty runs mean solid.
pub(crate) fn fit_dash(nominal: DimDash, length: f64) -> (Vec<f64>, DashFit) {
    let runs = nominal.pattern();
    let Some(&first) = runs.first() else {
        return (Vec::new(), DashFit::Nominal);
    };
    let cycle_runs: Vec<f64> = if runs.len() % 2 == 1 {
        runs.iter().chain(runs).copied().collect()
    } else {
        runs.to_vec()
    };
    let cycle: f64 = cycle_runs.iter().sum();
    let cycles = ((length - first) / cycle).round();
    if !(cycles >= 1.0 && length.is_finite()) {
        return (Vec::new(), DashFit::SolidTooShort);
    }
    let k = length / cycles.mul_add(cycle, first);
    let fit = if (k - 1.0).abs() <= 1e-9 {
        DashFit::Nominal
    } else {
        DashFit::Stretched
    };
    (cycle_runs.iter().map(|r| r * k).collect(), fit)
}

impl DimensionKind {
    /// How a centre line's pattern was fitted to its length under `style`;
    /// `None` for any other kind. [`DashFit::SolidTooShort`] is the
    /// substitution a shell must state.
    #[must_use]
    pub fn centre_line_dash_fit(&self, style: DimensionStyle) -> Option<DashFit> {
        let Self::CentreLine { a, b } = *self else {
            return None;
        };
        let length = (b.x - a.x).hypot(b.y - a.y);
        Some(fit_dash(style.centre_line_stroke().dash, length).1)
    }

    /// Whether this kind's drawing depends on the style property `name`,
    /// spelled as in [`super::StyleProvenance::each`]. A shell hides the
    /// override rows this returns `false` for, which change nothing on this
    /// ce dimension. `false` for an unknown name.
    ///
    /// ```
    /// use pdfcer_core::dimension::DimensionKind;
    /// use pdfcer_core::vector::Point;
    ///
    /// let line = DimensionKind::CentreLine { a: Point::new(0.0, 0.0), b: Point::new(90.0, 0.0) };
    /// assert!(line.reads_style_field("centre-line-dash"));
    /// assert!(!line.reads_style_field("dash"));
    /// assert!(!line.reads_style_field("unit"));
    /// ```
    #[must_use]
    pub fn reads_style_field(&self, name: &str) -> bool {
        const SHARED: &[&str] = &["line-width", "color", "opacity"];
        const MEASURED: &[&str] = &[
            "unit",
            "fraction",
            "decimal-marker",
            "standard",
            "text-height",
            "arrow-length",
            "arrow-form",
            "tolerance",
            "tolerance-places",
            "dash",
        ];
        const MARK: &[&str] = &[
            "centre-mark-size",
            "centre-mark-dash",
            "centre-mark-width",
            "centre-mark-color",
        ];
        const CIRCLE: &[&str] = &["circle-dash", "circle-width", "circle-color"];
        const LINE: &[&str] = &[
            "standard",
            "centre-line-dash",
            "centre-line-width",
            "centre-line-color",
        ];
        let lists: &[&[&str]] = match self {
            Self::CentreLine { .. } => &[SHARED, LINE],
            Self::CentreMark { .. } => &[SHARED, MARK, &["text-height"]],
            Self::Circular { centre_mark, .. } => {
                if centre_mark.is_some() {
                    &[SHARED, MEASURED, CIRCLE, MARK]
                } else {
                    &[SHARED, MEASURED, CIRCLE]
                }
            }
            Self::Linear { .. } | Self::Angular { .. } | Self::Perimeter { .. } => {
                &[SHARED, MEASURED]
            }
        };
        lists.iter().any(|l| l.contains(&name))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)] // test fixtures; a panic is the failure
mod tests {
    use super::*;

    /// Would catch: the generalised fit diverging from the four-run fit every
    /// existing centre line was baked with.
    #[test]
    fn a_four_run_fit_matches_the_original_formula() {
        let nominal = DimDash::new(&[54.0, 9.0, 9.0, 9.0]).unwrap();
        let (runs, fit) = fit_dash(nominal, 300.0);
        let cycles = ((300.0_f64 - 54.0) / 81.0).round();
        let k = 300.0 / cycles.mul_add(81.0, 54.0);
        assert_eq!(runs, vec![54.0 * k, 9.0 * k, 9.0 * k, 9.0 * k]);
        assert_eq!(fit, DashFit::Stretched);
        assert_eq!(fit_dash(nominal, 80.0).1, DashFit::SolidTooShort);
        assert_eq!(
            fit_dash(nominal, 54.0 + 81.0).1,
            DashFit::Nominal,
            "an exact cycle plus a closing dash"
        );
    }

    #[test]
    fn an_odd_pattern_is_doubled_before_fitting() {
        let (runs, _) = fit_dash(DimDash::new(&[4.0]).unwrap(), 100.0);
        assert_eq!(runs.len(), 2);
        let total = runs[0] * 13.0 + runs[1] * 12.0;
        assert!((total - 100.0).abs() < 1e-9, "both ends on a dash");
    }
}
