//! Dashed and dotted edge strokes (XFA 3.3 edge `stroke` p.688; gap G-D5:
//! the shapes are named, the lengths are not).

use crate::writer::content::LineCap;

use super::Painter;

/// How long the dashes and gaps of a dashed or dotted edge are (gap G-D5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum DashLengths {
    /// In multiples of the edge's thickness `t`: dashed `[4t 2t]`, dotted
    /// `[t 2t]`, `dashDot` `[3t 2t t 2t]`, `dashDotDot` `[3t 2t t 2t t 2t]`
    /// (equal, at 3pt, to the Acrobat output below).
    #[default]
    Scaled,
    /// The arrays an Acrobat-flattened form drew for 3pt edges, at every
    /// thickness: dashed `[12 6]`, dotted `[3 6]`, `dashDot` `[9 6 3 6]`,
    /// `dashDotDot` `[9 6 3 6 3 6]`.
    Fixed,
}

/// The shape of a dashed edge's dots and dash ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum DashCaps {
    /// Square, projecting by half the thickness (`2 J`), as the Acrobat
    /// output draws them.
    #[default]
    Projecting,
    /// Round dots and dash ends, the shape p.688 describes for `dotted`;
    /// each period keeps its length.
    Round,
}

/// A dashed edge `stroke` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Pattern {
    Dashed,
    Dotted,
    DashDot,
    DashDotDot,
}

impl Pattern {
    /// The pattern an edge `stroke` value names, or `None` for solid and
    /// unknown values.
    pub(super) fn from_stroke(s: &str) -> Option<Self> {
        Some(match s {
            "dashed" => Self::Dashed,
            "dotted" => Self::Dotted,
            "dashDot" => Self::DashDot,
            "dashDotDot" => Self::DashDotDot,
            _ => return None,
        })
    }

    /// The dash array, in thickness units: dash, gap, dash, gap...
    fn units(self) -> &'static [f64] {
        match self {
            Self::Dashed => &[4.0, 2.0],
            Self::Dotted => &[1.0, 2.0],
            Self::DashDot => &[3.0, 2.0, 1.0, 2.0],
            Self::DashDotDot => &[3.0, 2.0, 1.0, 2.0, 1.0, 2.0],
        }
    }

    /// The array for a stroke `t` thick, and the cap it is drawn with.
    pub(super) fn pattern(
        self,
        t: f64,
        lengths: DashLengths,
        caps: DashCaps,
    ) -> (Vec<f64>, LineCap) {
        let unit = match lengths {
            DashLengths::Scaled => t,
            DashLengths::Fixed => 3.0,
        };
        let raw = self.units().iter().map(|u| u * unit);
        match caps {
            DashCaps::Projecting => (raw.collect(), LineCap::ProjectingSquare),
            // A round cap adds t/2 at each end: shorten the dash, widen the gap.
            DashCaps::Round => {
                let adjusted = raw
                    .enumerate()
                    .map(|(i, v)| if i % 2 == 0 { (v - t).max(0.0) } else { v + t })
                    .collect();
                (adjusted, LineCap::Round)
            }
        }
    }
}

impl Painter<'_> {
    /// Set the dash for the next stroke: `dash`'s pattern, or solid. The
    /// solid reset is written only after a dashed stroke.
    pub(super) fn set_dash(&mut self, dash: Option<Pattern>, t: f64, cap: LineCap) -> LineCap {
        match dash {
            Some(d) => {
                let (pattern, cap) = d.pattern(t, self.opts.dash_lengths, self.opts.dash_caps);
                self.b.set_dash(&pattern, 0.0);
                self.dashed = true;
                cap
            }
            None => {
                if self.dashed {
                    self.b.set_dash(&[], 0.0);
                    self.dashed = false;
                }
                cap
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scaled_lengths_match_the_fixed_arrays_at_three_points() {
        for d in [
            Pattern::Dashed,
            Pattern::Dotted,
            Pattern::DashDot,
            Pattern::DashDotDot,
        ] {
            let caps = DashCaps::Projecting;
            let scaled = d.pattern(3.0, DashLengths::Scaled, caps);
            assert_eq!(scaled, d.pattern(3.0, DashLengths::Fixed, caps));
        }
        let (p, _) = Pattern::DashDot.pattern(3.0, DashLengths::Fixed, DashCaps::Projecting);
        assert_eq!(p, [9.0, 6.0, 3.0, 6.0]);
    }

    #[test]
    fn round_dots_keep_their_period() {
        let (p, cap) = Pattern::Dotted.pattern(2.0, DashLengths::Scaled, DashCaps::Round);
        assert_eq!(cap, LineCap::Round);
        assert_eq!(p, [0.0, 6.0]);
        assert_eq!(p.iter().sum::<f64>(), 6.0);
    }
}
