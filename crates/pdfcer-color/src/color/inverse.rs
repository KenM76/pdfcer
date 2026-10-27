//! sRGB → `DeviceCMYK` as the inverse of [`super::cmyk_to_srgb_with`]: the
//! CMYK whose rendering through pdfcer's own table is the asked-for colour.
//!
//! RGB has three degrees of freedom and CMYK four, so every reachable colour
//! has a range of black amounts that draw it. [`BlackGeneration`] picks the
//! end of that range; the colourants are then solved for that black by a
//! bounded least-squares fit over the table. ISO 32000-2 §10.4.2's formula
//! (`k = min(1 − r, 1 − g, 1 − b)`) is not used: it is the inverse of the
//! naive forward formula, not of the calibrated one pdfcer renders with.

use super::{CmykIntent, cmyk_to_srgb_with};

/// A colour within this of the target in every channel (0..1 scale) is
/// reached: half an 8-bit count, so an 8-bit target rounds back to itself.
/// It is half a count and not less because an 8-bit colour on the edge of
/// the gamut can itself sit up to half a count outside it.
pub const CMYK_FIT_TOLERANCE: f32 = 0.5 / 255.0;

/// Which end of the range of black amounts that draw a colour to take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BlackGeneration {
    /// The most black ink that still draws the colour (full grey-component
    /// replacement): neutrals and dark colours lean on K, the convention of
    /// the §10.4.2 formula and of line art. The default.
    #[default]
    Most,
    /// The least black ink that draws the colour: rich colours built from
    /// C, M and Y, K only where they cannot reach the darkness.
    Least,
    /// [`MEDIUM_BLACK_FRACTION`] of the way from [`Self::Least`]'s black to
    /// [`Self::Most`]'s: neutrals carry C, M and Y with a little K, which is
    /// how a press profile separates them. Fitted to Acrobat's rendering of
    /// RGB paint in a `DeviceCMYK` blending group (decision 202).
    Medium,
}

/// The share of the reachable black range [`BlackGeneration::Medium`] takes.
pub const MEDIUM_BLACK_FRACTION: f32 = 0.15;

/// The result of [`srgb_to_cmyk`]: the CMYK, and how close its rendering is.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct CmykFit {
    /// C, M, Y, K, each in 0..1.
    pub cmyk: [f32; 4],
    /// The largest per-channel difference (0..1) between the CMYK's
    /// rendering and the asked-for colour.
    pub residual: f32,
    /// `residual <= CMYK_FIT_TOLERANCE`. When `false` the colour is outside
    /// what CMYK can draw, and `cmyk` is the reachable colour nearest to it
    /// (least squares in sRGB): a bright saturated blue comes back duller.
    pub in_gamut: bool,
}

/// sRGB (0..1 each) → the `DeviceCMYK` that [`super::cmyk_to_srgb`] draws as
/// that colour, with [`BlackGeneration::Most`].
///
/// # Examples
///
/// ```
/// use pdfcer_color::color::{cmyk_to_srgb, srgb_to_cmyk};
///
/// let fit = srgb_to_cmyk(0.2, 0.5, 0.3);
/// assert!(fit.in_gamut);
/// let [c, m, y, k] = fit.cmyk;
/// let back = cmyk_to_srgb(c, m, y, k);
/// assert!((back[1] - 0.5).abs() < 1.0 / 255.0);
/// ```
#[must_use]
pub fn srgb_to_cmyk(r: f32, g: f32, b: f32) -> CmykFit {
    srgb_to_cmyk_with(CmykIntent::Calibrated, BlackGeneration::Most, r, g, b)
}

/// [`srgb_to_cmyk`] inverting [`super::cmyk_to_srgb_with`] at `intent`, with
/// the black amount chosen by `black`. A NaN component reads as 0.
///
/// # Examples
///
/// ```
/// use pdfcer_color::color::{BlackGeneration, CmykIntent, srgb_to_cmyk_with};
///
/// // Under NeutralBlack a grey is drawn by black ink alone.
/// let fit = srgb_to_cmyk_with(CmykIntent::NeutralBlack, BlackGeneration::Most, 0.4, 0.4, 0.4);
/// assert_eq!(fit.cmyk, [0.0, 0.0, 0.0, 0.6]);
/// ```
#[must_use]
pub fn srgb_to_cmyk_with(
    intent: CmykIntent,
    black: BlackGeneration,
    r: f32,
    g: f32,
    b: f32,
) -> CmykFit {
    let target = [r, g, b].map(|v| if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) });
    let [tr, tg, tb] = target;
    if intent == CmykIntent::NeutralBlack && black == BlackGeneration::Most && tr == tg && tg == tb
    {
        return CmykFit {
            cmyk: [0.0, 0.0, 0.0, 1.0 - tr],
            residual: 0.0,
            in_gamut: true,
        };
    }
    let solver = Solver { intent, target };
    let k = solver.choose_black(black);
    let (cmy, residual) = solver.fit_cmy(k);
    let [c, m, y] = cmy;
    CmykFit {
        cmyk: [c, m, y, k],
        residual,
        in_gamut: residual <= CMYK_FIT_TOLERANCE,
    }
}

/// Black amounts tried before the boundary is refined.
const K_STEPS: u16 = 32;
/// Bisection rounds on the black boundary: 2⁻¹² of a step.
const K_REFINE: u32 = 12;
/// Damped Gauss-Newton rounds per fit.
const ROUNDS: u32 = 40;

struct Solver {
    intent: CmykIntent,
    target: [f32; 3],
}

impl Solver {
    fn draw(&self, [c, m, y]: [f32; 3], k: f32) -> [f32; 3] {
        cmyk_to_srgb_with(self.intent, c, m, y, k)
    }

    /// Whether black `k` draws the target, to a tenth of
    /// [`CMYK_FIT_TOLERANCE`]: the end of the black range is taken where the
    /// fit is still exact, not where its error has grown to the tolerance.
    fn reached(&self, k: f32) -> bool {
        self.fit_cmy(k).1 <= CMYK_FIT_TOLERANCE / 10.0
    }

    /// The black at the chosen end of the reachable range, or the black of
    /// the closest fit when no black reaches the colour.
    fn choose_black(&self, black: BlackGeneration) -> f32 {
        let end = match black {
            BlackGeneration::Medium => {
                let least = self.choose_black(BlackGeneration::Least);
                let most = self.choose_black(BlackGeneration::Most);
                return least + MEDIUM_BLACK_FRACTION * (most - least);
            }
            end => end,
        };
        self.range_end(end)
    }

    /// [`Self::choose_black`] for [`BlackGeneration::Most`] or
    /// [`BlackGeneration::Least`].
    fn range_end(&self, black: BlackGeneration) -> f32 {
        let grid: Vec<f32> = (0..=K_STEPS)
            .map(|i| f32::from(i) / f32::from(K_STEPS))
            .collect();
        let reach: Vec<bool> = grid.iter().map(|&k| self.reached(k)).collect();
        let pick = match black {
            BlackGeneration::Most => reach.iter().rposition(|&r| r),
            BlackGeneration::Least | BlackGeneration::Medium => reach.iter().position(|&r| r),
        };
        let Some(at) = pick else {
            return self.closest_black(&grid);
        };
        let step = 1.0 / f32::from(K_STEPS);
        let inside = grid.get(at).copied().unwrap_or(0.0);
        let outside = match black {
            BlackGeneration::Most => inside + step,
            BlackGeneration::Least | BlackGeneration::Medium => inside - step,
        };
        if !(0.0..=1.0).contains(&outside) {
            return inside;
        }
        self.refine(inside, outside)
    }

    /// Bisect between a reached black and an unreached one.
    fn refine(&self, mut inside: f32, mut outside: f32) -> f32 {
        for _ in 0..K_REFINE {
            let mid = (inside + outside) / 2.0;
            if self.reached(mid) {
                inside = mid;
            } else {
                outside = mid;
            }
        }
        inside
    }

    /// The grid black whose fit has the smallest residual, refined by a
    /// golden-section search over its neighbouring steps.
    fn closest_black(&self, grid: &[f32]) -> f32 {
        let err = |k: f32| self.fit_cmy(k).1;
        let best = grid
            .iter()
            .copied()
            .min_by(|a, b| err(*a).total_cmp(&err(*b)))
            .unwrap_or(0.0);
        let step = 1.0 / f32::from(K_STEPS);
        let (mut lo, mut hi) = ((best - step).max(0.0), (best + step).min(1.0));
        let phi = 0.618_034_f32;
        for _ in 0..K_REFINE {
            let a = hi - phi * (hi - lo);
            let b = lo + phi * (hi - lo);
            if err(a) <= err(b) {
                hi = b;
            } else {
                lo = a;
            }
        }
        let mid = (lo + hi) / 2.0;
        if err(mid) <= err(best) { mid } else { best }
    }

    /// C, M and Y in 0..1 drawing the target at black `k` as closely as the
    /// table allows, and the largest channel error left.
    fn fit_cmy(&self, k: f32) -> ([f32; 3], f32) {
        let [tr, tg, tb] = self.target;
        let lift = |t: f32| {
            if k >= 1.0 {
                0.0
            } else {
                ((1.0 - t - k) / (1.0 - k)).clamp(0.0, 1.0)
            }
        };
        let starts = [[lift(tr), lift(tg), lift(tb)], [0.5; 3]];
        starts
            .into_iter()
            .map(|s| self.descend(s, k))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap_or(([0.0; 3], f32::INFINITY))
    }

    /// Damped Gauss-Newton from `x`, each step clamped into the unit cube.
    fn descend(&self, mut x: [f32; 3], k: f32) -> ([f32; 3], f32) {
        let mut err = sq_error(self.draw(x, k), self.target);
        let mut damping = 1e-3_f32;
        for _ in 0..ROUNDS {
            let jac = self.jacobian(x, k);
            let resid = sub(self.target, self.draw(x, k));
            let Some(step) = damped_step(&jac, resid, damping) else {
                break;
            };
            let next = add(x, step).map(|v| v.clamp(0.0, 1.0));
            let next_err = sq_error(self.draw(next, k), self.target);
            if next_err < err {
                x = next;
                err = next_err;
                damping = (damping / 3.0).max(1e-7);
            } else {
                damping *= 4.0;
            }
            if err < 1e-12 || damping > 1e6 {
                break;
            }
        }
        (x, max_error(self.draw(x, k), self.target))
    }

    /// Columns are ∂rgb/∂c, ∂rgb/∂m, ∂rgb/∂y by a one-sided difference that
    /// stays inside the unit cube.
    fn jacobian(&self, x: [f32; 3], k: f32) -> [[f32; 3]; 3] {
        let h = 1e-3_f32;
        let base = self.draw(x, k);
        let column = |axis: usize| {
            let mut moved = x;
            let mut dir = h;
            if let Some(v) = moved.get_mut(axis) {
                if *v + h > 1.0 {
                    dir = -h;
                }
                *v += dir;
            }
            sub(self.draw(moved, k), base).map(|d| d / dir)
        };
        [column(0), column(1), column(2)]
    }
}

/// `(JᵀJ + λI)⁻¹ Jᵀ r` for the 3×3 case, `None` when singular.
fn damped_step(jac: &[[f32; 3]; 3], resid: [f32; 3], damping: f32) -> Option<[f32; 3]> {
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let [j0, j1, j2] = *jac;
    let cols = [j0, j1, j2];
    let mut a = [[0.0_f32; 3]; 3];
    for (i, row) in a.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            let (ci, cj) = (cols.get(i)?, cols.get(j)?);
            *cell = dot(*ci, *cj) + if i == j { damping } else { 0.0 };
        }
    }
    let rhs = [dot(j0, resid), dot(j1, resid), dot(j2, resid)];
    solve3(a, rhs)
}

/// Cramer's rule.
fn solve3(a: [[f32; 3]; 3], b: [f32; 3]) -> Option<[f32; 3]> {
    let det = |m: [[f32; 3]; 3]| {
        let [[a, b, c], [d, e, f], [g, h, i]] = m;
        a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
    };
    let d = det(a);
    if d.abs() < 1e-20 || !d.is_finite() {
        return None;
    }
    let with = |col: usize| {
        let mut m = a;
        for (row, &v) in m.iter_mut().zip(b.iter()) {
            if let Some(cell) = row.get_mut(col) {
                *cell = v;
            }
        }
        det(m) / d
    };
    Some([with(0), with(1), with(2)])
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn sq_error(a: [f32; 3], b: [f32; 3]) -> f32 {
    sub(a, b).iter().map(|d| d * d).sum()
}

fn max_error(a: [f32; 3], b: [f32; 3]) -> f32 {
    sub(a, b).iter().fold(0.0, |m, d| m.max(d.abs()))
}

#[cfg(test)]
#[path = "inverse_tests.rs"]
mod tests;
