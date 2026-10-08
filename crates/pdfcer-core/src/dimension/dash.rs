//! A ce dimension's stroke dash pattern (ISO 32000-2 §8.4.3.6) — a `Copy`
//! value so it can sit in the style cascade beside the other properties.

/// The most dash/gap runs a ce dimension's pattern carries — enough for the
/// drafting linetypes (hidden `[a b]`, centre `[long gap short gap]`).
pub const MAX_DASH_RUNS: usize = 4;

/// The dash pattern a ce dimension's lines are stroked with: alternating
/// dash and gap lengths in points, phase 0. [`DimDash::SOLID`] (no runs) is
/// a solid line, and is a real value rather than "unset": a group that dashes
/// everything and one ce dimension that must stay solid is expressible.
///
/// Only the stroked lines are dashed — dimension, extension and leader lines,
/// arcs and outlines. Terminators and the label are drawn solid.
///
/// # Examples
///
/// ```
/// use pdfcer_core::dimension::DimDash;
///
/// let hidden = DimDash::new(&[3.0, 1.5]).unwrap();
/// assert_eq!(hidden.pattern(), &[3.0, 1.5]);
/// assert!(DimDash::new(&[]).unwrap().is_solid());
/// // Not dashes: negative, never-on, or too many runs.
/// assert!(DimDash::new(&[-1.0]).is_none());
/// assert!(DimDash::new(&[0.0, 0.0]).is_none());
/// assert!(DimDash::new(&[1.0; 5]).is_none());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct DimDash {
    runs: [f64; MAX_DASH_RUNS],
    len: usize,
}

impl DimDash {
    /// A solid line.
    pub const SOLID: Self = Self {
        runs: [0.0; MAX_DASH_RUNS],
        len: 0,
    };

    /// Build a pattern from its run lengths, validating §8.4.3.6: every run
    /// finite and non-negative, not all zero. An empty slice is
    /// [`Self::SOLID`]. `None` for an invalid pattern or more than
    /// [`MAX_DASH_RUNS`] runs.
    #[must_use]
    pub fn new(pattern: &[f64]) -> Option<Self> {
        if pattern.len() > MAX_DASH_RUNS
            || pattern.iter().any(|v| !v.is_finite() || *v < 0.0)
            || (!pattern.is_empty() && pattern.iter().all(|v| *v == 0.0))
        {
            return None;
        }
        let mut runs = [0.0; MAX_DASH_RUNS];
        runs.get_mut(..pattern.len())?.copy_from_slice(pattern);
        Some(Self {
            runs,
            len: pattern.len(),
        })
    }

    /// The dash and gap lengths, in points; empty for a solid line.
    #[must_use]
    pub fn pattern(&self) -> &[f64] {
        self.runs.get(..self.len).unwrap_or(&[])
    }

    /// Whether this is a solid line.
    #[must_use]
    pub const fn is_solid(&self) -> bool {
        self.len == 0
    }
}

/// Whether `opacity` is a usable ce-dimension opacity: finite, in `0..=1`.
#[must_use]
pub fn opacity_in_range(opacity: f64) -> bool {
    (0.0..=1.0).contains(&opacity)
}
