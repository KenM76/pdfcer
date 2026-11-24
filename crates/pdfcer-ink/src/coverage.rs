//! Ink coverage rules: Acrobat Output Preview's *Total area coverage* and
//! *Rich black*, evaluated per pixel on ink as it lands on the sheet.

/// Which areas a coverage check flags: [`crate::InkCanvas::coverage_masks`], or `pdfcer-render`'s `coverage_warning_mask`.
///
/// Both rules read each plate's ink as it lands on the sheet: the tint
/// times the page group's alpha after compositing (ISO 32000-2 §11.4.7,
/// zero ink being the medium), with overprint, transparency and spot inks
/// already resolved exactly as the separation preview draws them. A plate
/// the caller hides counts as no ink. Every tint is compared
/// in tenths of a percent, rounded, so a 75% tint is 75.0 and not
/// 74.99998.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CoverageWarning {
    /// The sum of every plate's tint (C, M, Y, K and each spot ink) is
    /// strictly above `limit_percent` (so a 300% limit flags 300.1%, not
    /// 300%).
    TotalAreaCoverage {
        /// The limit, in percent; 0–(100 × plate count) is meaningful.
        limit_percent: u16,
    },
    /// Rich black: black at or above `black_start_percent` with any cyan,
    /// magenta or yellow present (0.1% or more). A 0% start still needs
    /// some black. Spot inks never make a black rich.
    RichBlack {
        /// The black tint a pixel must reach, in percent.
        black_start_percent: u16,
    },
}

impl CoverageWarning {
    /// Whether one pixel breaks this rule. `process` is `[C, M, Y, K]` and
    /// `spots` each spot ink, all as ink on the sheet in `0.0..=1.0`.
    #[must_use]
    pub fn flags(self, process: [f32; 4], spots: &[f32]) -> bool {
        match self {
            Self::TotalAreaCoverage { limit_percent } => {
                let total: u32 = process.iter().chain(spots).map(|&t| tenths(t)).sum();
                total > u32::from(limit_percent) * 10
            }
            Self::RichBlack {
                black_start_percent,
            } => {
                let [c, m, y, k] = process.map(tenths);
                k > 0 && k >= u32::from(black_start_percent) * 10 && c + m + y > 0
            }
        }
    }
}

/// A tint in tenths of a percent, rounded.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn tenths(tint: f32) -> u32 {
    (tint.clamp(0.0, 1.0) * 1000.0).round() as u32
}

/// The pixels a [`CoverageWarning`] flags on one page raster.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CoverageMask {
    /// Width in pixels; equal to the separation preview raster's.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// One byte per pixel, row-major from the top-left: `1` where the
    /// rule is broken, `0` elsewhere.
    pub mask: Vec<u8>,
    /// How many bytes of [`Self::mask`] are `1`.
    pub flagged: u64,
}

impl CoverageMask {
    /// Whether the pixel at `(x, y)` is flagged; `false` outside the mask.
    #[must_use]
    pub fn is_flagged(&self, x: u32, y: u32) -> bool {
        x < self.width
            && self
                .mask
                .get(y as usize * self.width as usize + x as usize)
                .is_some_and(|&b| b == 1)
    }
}

impl CoverageMask {
    /// A mask from one byte per pixel, row-major (`1` flagged, anything
    /// else not); [`Self::flagged`] is counted here.
    #[must_use]
    pub fn from_mask(width: u32, height: u32, mask: Vec<u8>) -> Self {
        let flagged = mask.iter().map(|&b| u64::from(b == 1)).sum();
        Self {
            width,
            height,
            mask,
            flagged,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CoverageWarning;

    #[test]
    fn total_area_coverage_is_strictly_above_the_limit() {
        let tac = CoverageWarning::TotalAreaCoverage { limit_percent: 300 };
        assert!(!tac.flags([0.75, 0.75, 0.75, 0.75], &[]));
        assert!(tac.flags([0.75, 0.75, 0.75, 0.76], &[]));
        assert!(tac.flags([0.75, 0.75, 0.75, 0.5], &[0.3]));
    }

    #[test]
    fn rich_black_needs_black_and_a_process_colour() {
        let rich = CoverageWarning::RichBlack {
            black_start_percent: 95,
        };
        assert!(rich.flags([0.4, 0.0, 0.0, 1.0], &[]));
        assert!(rich.flags([0.0, 0.0, 0.001, 0.95], &[]));
        assert!(!rich.flags([0.0, 0.0, 0.0, 1.0], &[0.8]));
        assert!(!rich.flags([0.4, 0.3, 0.3, 0.94], &[]));
        assert!(!rich.flags([0.0, 0.0, 0.0004, 1.0], &[]));
    }

    #[test]
    fn a_zero_start_still_needs_some_black() {
        let rich = CoverageWarning::RichBlack {
            black_start_percent: 0,
        };
        assert!(!rich.flags([0.5, 0.0, 0.0, 0.0], &[]));
        assert!(rich.flags([0.5, 0.0, 0.0, 0.001], &[]));
    }
}
