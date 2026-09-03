//! # Best-fit circle — the Taubin algebraic fit (decision 011 §2.3)
//!
//! The headline geometry of Pass 12.M2's radius/diameter capability: given
//! a set of points sampled from a circle *or from a short arc approximated
//! by small line segments*, recover the circle (centre + radius) they best
//! lie on, plus a **fit residual** (RMS point-to-circle distance) the
//! operator sees so a loose fit is never presented as a clean one
//! (fuzzy-never-sneaky, rule 4).
//!
//! ## Why Taubin and not Kåsa (the deliberate algorithm choice)
//!
//! The operator's stated case (decision 011 §2.3) is *"multiple selected
//! nodes … that make up a circular area (might be small line segments)"* —
//! **partial-arc / short-arc** data. That is the exact regime where the
//! simplest algebraic fit, **Kåsa** (minimise `Σ(x²+y²+Dx+Ey+F)²`, a plain
//! linear least-squares), is **strongly biased**: on a short arc with
//! measurement noise Kåsa systematically **under**-estimates the radius,
//! because minimising the *algebraic* distance weights points by their
//! distance from the centre and a short arc has no counter-balancing points
//! on the far side. **Taubin** minimises the same algebraic distance but
//! **normalised by the gradient of the constraint** — turning the problem
//! into a generalised eigenproblem whose closed-form solution is
//! near-unbiased for partial arcs at nearly the same cost. The
//! [`tests::taubin_beats_kasa_on_short_arcs`] Monte-Carlo test *proves* this
//! bias difference on synthetic short-arc data (decision 011 Appendix A
//! Pass 12.M2 acceptance: "Taubin beats Kåsa — proven by test").
//!
//! ## The algorithm (Chernov's `CircleFitByTaubin`, closed form)
//!
//! Working in **centroid-centred** coordinates `(u,v) = (x−x̄, y−ȳ)` with
//! `z = u²+v²`, form the scaled moment matrix and solve the characteristic
//! cubic for its smallest non-negative root by a few Newton iterations
//! (bounded, [`NEWTON_ITERS`]) — no external linear-algebra dependency
//! (rule 13, ~80 lines of arithmetic). The root gives the centre offset and
//! radius directly. A single optional Gauss-Newton geometric refinement
//! step ([`refine_geometric`]) tightens the fit when the caller asks.
//!
//! ## Panic-free (ARCHITECTURE.md §10)
//!
//! Fewer than three finite points, all-collinear points (a singular fit),
//! or non-finite input yields [`None`], never a panic. Every divisor is
//! guarded; every coordinate is finiteness-checked.

use crate::vector::Point;

/// A fitted circle: centre, radius, and the RMS fit residual — the reviewable
/// hint the operator accepts or rejects (decision 011 §2.3, ui-spec §3.4).
///
/// All three are in the same (page-space, 1/72") units as the input points.
/// The displayed radius is `radius × group.scale`; the displayed diameter is
/// `2 × radius × group.scale` (the *display*-only distinction — radius and
/// diameter are one geometry, decision 011 §2.3 / ui-spec §1.1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FitCircle {
    /// Circle centre, page space.
    pub center: Point,
    /// Circle radius, page-space units (always ≥ 0 and finite for a `Some`).
    pub radius: f64,
    /// **Fit residual** — the RMS of each input point's distance to the
    /// fitted circle (`|distance(point, centre) − radius|`), page-space
    /// units. `0.0` for points exactly on a circle; grows with scatter. The
    /// GUI surfaces this always (never on request) so the operator sees fit
    /// quality (decision 011 §2.3: "the fit residual … is reported").
    pub residual: f64,
}

/// Newton-iteration cap for the characteristic-cubic root solve — the
/// closed-form Taubin fit converges in a handful of steps; this bounds a
/// pathological input to a fixed, tiny cost (panic-free posture).
const NEWTON_ITERS: usize = 40;

/// Gauss-Newton refinement-step cap (used by [`fit_circle_taubin_refined`]).
const REFINE_ITERS: usize = 20;

/// Minimum usable point count — a circle has three degrees of freedom, so a
/// fit needs at least three non-collinear points.
const MIN_POINTS: usize = 3;

/// Best-fit a circle to `points` by the **Taubin algebraic method**
/// (decision 011 §2.3), returning the circle and its RMS residual.
///
/// `None` when the input is degenerate: fewer than [`MIN_POINTS`] finite
/// points, all points effectively collinear (a numerically singular fit), or
/// any intermediate value non-finite. This is the exact call shape the GUI's
/// `MeasureCircular` tool needs (ui-spec §3.3): it flattens each picked
/// object's Béziers to samples in PDF space and passes the concatenated
/// point set here.
///
/// # Examples
///
/// ```
/// use pdfcer_core::vector::Point;
/// use pdfcer_core::dimension::fit_circle_taubin;
///
/// // Twelve points on the circle centred at (5, 5), radius 3.
/// let pts: Vec<Point> = (0..12)
///     .map(|i| {
///         let t = std::f64::consts::TAU * f64::from(i) / 12.0;
///         Point::new(5.0 + 3.0 * t.cos(), 5.0 + 3.0 * t.sin())
///     })
///     .collect();
/// let fit = fit_circle_taubin(&pts).unwrap();
/// assert!((fit.center.x - 5.0).abs() < 1e-9);
/// assert!((fit.center.y - 5.0).abs() < 1e-9);
/// assert!((fit.radius - 3.0).abs() < 1e-9);
/// assert!(fit.residual < 1e-9);
/// ```
#[must_use]
pub fn fit_circle_taubin(points: &[Point]) -> Option<FitCircle> {
    let (center, radius) = taubin_center_radius(points)?;
    let residual = rms_residual(points, center, radius)?;
    Some(FitCircle {
        center,
        radius,
        residual,
    })
}

/// Best-fit by Taubin, then tighten with up to [`REFINE_ITERS`] Gauss-Newton
/// **geometric** refinement steps (minimising the true orthogonal
/// point-to-circle distances rather than the algebraic ones).
///
/// Decision 011 §2.3 makes this refinement *optional* ("an optional single
/// Gauss-Newton geometric-refinement step if residual matters"); pdfcer runs
/// a small bounded number of steps and keeps whichever fit has the smaller
/// residual, so refinement can only ever improve or match the algebraic fit,
/// never worsen it. Falls back to the plain Taubin fit if refinement
/// diverges. `None` under the same degenerate conditions as
/// [`fit_circle_taubin`].
#[must_use]
pub fn fit_circle_taubin_refined(points: &[Point]) -> Option<FitCircle> {
    let base = fit_circle_taubin(points)?;
    match refine_geometric(points, base.center, base.radius) {
        Some((center, radius)) => {
            let residual = rms_residual(points, center, radius)?;
            // Keep the refinement only if it did not make the fit worse.
            if residual <= base.residual {
                Some(FitCircle {
                    center,
                    radius,
                    residual,
                })
            } else {
                Some(base)
            }
        }
        None => Some(base),
    }
}

/// The moment accumulator over the centred coordinates `(u, v)` and
/// `z = u²+v²`. All sums are means (already divided by `n`). Kept separate so
/// both the Taubin and Kåsa fits share one pass.
#[derive(Default, Clone, Copy)]
struct Moments {
    mean_x: f64,
    mean_y: f64,
    muu: f64,
    mvv: f64,
    muv: f64,
    muz: f64,
    mvz: f64,
    mzz: f64,
}

/// Accumulate the centred second/third/fourth moments of the finite points,
/// or `None` if fewer than [`MIN_POINTS`] are finite. The means `muu…mzz` are
/// already divided by `n`.
fn moments(points: &[Point]) -> Option<Moments> {
    // First pass: centroid over finite points.
    let mut n = 0.0f64;
    let mut sx = 0.0f64;
    let mut sy = 0.0f64;
    for p in points.iter().filter(|p| p.is_finite()) {
        n += 1.0;
        sx += p.x;
        sy += p.y;
    }
    if n < MIN_POINTS as f64 {
        return None;
    }
    let mean_x = sx / n;
    let mean_y = sy / n;

    // Second pass: centred moments.
    let mut muu = 0.0;
    let mut mvv = 0.0;
    let mut muv = 0.0;
    let mut muz = 0.0;
    let mut mvz = 0.0;
    let mut mzz = 0.0;
    for p in points.iter().filter(|p| p.is_finite()) {
        let u = p.x - mean_x;
        let v = p.y - mean_y;
        let z = u * u + v * v;
        muu += u * u;
        mvv += v * v;
        muv += u * v;
        muz += u * z;
        mvz += v * z;
        mzz += z * z;
    }
    let m = Moments {
        mean_x,
        mean_y,
        muu: muu / n,
        mvv: mvv / n,
        muv: muv / n,
        muz: muz / n,
        mvz: mvz / n,
        mzz: mzz / n,
    };
    m.all_finite().then_some(m)
}

impl Moments {
    fn all_finite(self) -> bool {
        [
            self.mean_x,
            self.mean_y,
            self.muu,
            self.mvv,
            self.muv,
            self.muz,
            self.mvz,
            self.mzz,
        ]
        .iter()
        .all(|v| v.is_finite())
    }
}

/// The core Taubin solve: centre (absolute page space) and radius, or `None`
/// on a singular/degenerate fit. Separated from residual computation so both
/// `fit_circle_taubin` and the refinement seed reuse it.
fn taubin_center_radius(points: &[Point]) -> Option<(Point, f64)> {
    let m = moments(points)?;

    let mz = m.muu + m.mvv;
    let cov_uv = m.muu * m.mvv - m.muv * m.muv;
    let var_z = m.mzz - mz * mz;

    // Characteristic cubic  a3·x³ + a2·x² + a1·x + a0  (Chernov, Taubin).
    let a3 = 4.0 * mz;
    let a2 = -3.0 * mz * mz - m.mzz;
    let a1 = var_z * mz + 4.0 * cov_uv * mz - m.muz * m.muz - m.mvz * m.mvz;
    let a0 = m.muz * (m.muz * m.mvv - m.mvz * m.muv) + m.mvz * (m.mvz * m.muu - m.muz * m.muv)
        - var_z * cov_uv;
    let a22 = a2 + a2;
    let a33 = a3 + a3 + a3;

    // Newton's method for the smallest non-negative root, seeded at 0.
    let mut x = 0.0f64;
    let mut y = a0;
    for _ in 0..NEWTON_ITERS {
        let dy = a1 + x * (a22 + x * a33);
        if dy == 0.0 || !dy.is_finite() {
            break;
        }
        let x_new = x - y / dy;
        if !x_new.is_finite() {
            return None;
        }
        if (x_new - x).abs() < f64::EPSILON * (1.0 + x_new.abs()) {
            x = x_new;
            break;
        }
        let y_new = a0 + x_new * (a1 + x_new * (a2 + x_new * a3));
        // If the residual polynomial value grew, we are past the root; stop.
        if y_new.abs() > y.abs() {
            break;
        }
        x = x_new;
        y = y_new;
    }

    let det = x * x - x * mz + cov_uv;
    if det.abs() <= f64::EPSILON || !det.is_finite() {
        return None; // singular — collinear points, no unique circle.
    }
    let uc = (m.muz * (m.mvv - x) - m.mvz * m.muv) / det / 2.0;
    let vc = (m.mvz * (m.muu - x) - m.muz * m.muv) / det / 2.0;
    let radius = (uc * uc + vc * vc + mz).sqrt();
    let center = Point::new(uc + m.mean_x, vc + m.mean_y);
    if center.is_finite() && radius.is_finite() && radius >= 0.0 {
        Some((center, radius))
    } else {
        None
    }
}

/// Kåsa (algebraic linear least-squares) circle fit — **the biased baseline**
/// the Taubin choice is measured against, kept `pub(crate)` **only so the
/// test can prove Taubin beats it** on short arcs (decision 011 §2.3). It is
/// never used to author a dimension. In centred coordinates the Kåsa normal
/// equations reduce to a 2×2 solve.
#[cfg(test)]
#[must_use]
fn fit_circle_kasa(points: &[Point]) -> Option<FitCircle> {
    let m = moments(points)?;
    let det = m.muu * m.mvv - m.muv * m.muv;
    if det.abs() <= f64::EPSILON || !det.is_finite() {
        return None;
    }
    let bx = m.muz / 2.0;
    let by = m.mvz / 2.0;
    let uc = (bx * m.mvv - by * m.muv) / det;
    let vc = (m.muu * by - m.muv * bx) / det;
    let radius = (uc * uc + vc * vc + m.muu + m.mvv).sqrt();
    let center = Point::new(uc + m.mean_x, vc + m.mean_y);
    if !center.is_finite() || !radius.is_finite() {
        return None;
    }
    let residual = rms_residual(points, center, radius)?;
    Some(FitCircle {
        center,
        radius,
        residual,
    })
}

/// One-or-more Gauss-Newton geometric refinement steps minimising the true
/// orthogonal residuals, seeded at `(center, radius)`. Returns the refined
/// `(center, radius)`, or `None` if a step becomes non-finite (the caller
/// then keeps the algebraic fit).
///
/// Indexing is over compile-time-fixed 3-element normal-equation arrays with
/// constant indices `0..=2` — provably in bounds (see [`solve3`]).
#[allow(clippy::indexing_slicing)]
fn refine_geometric(points: &[Point], seed: Point, seed_r: f64) -> Option<(Point, f64)> {
    let mut cx = seed.x;
    let mut cy = seed.y;
    let mut r = seed_r;
    for _ in 0..REFINE_ITERS {
        // Normal equations for the linearised distance residual
        // f_i = sqrt((x-cx)²+(y-cy)²) − r, unknowns (dcx, dcy, dr).
        let mut jtj = [[0.0f64; 3]; 3];
        let mut jtf = [0.0f64; 3];
        let mut count = 0.0f64;
        for p in points.iter().filter(|p| p.is_finite()) {
            let dx = p.x - cx;
            let dy = p.y - cy;
            let di = (dx * dx + dy * dy).sqrt();
            if di <= f64::EPSILON {
                continue;
            }
            // Jacobian row of f = di - r is [-dx/di, -dy/di, -1].
            let j = [-dx / di, -dy / di, -1.0];
            let f = di - r;
            for a in 0..3 {
                jtf[a] += j[a] * f;
                for b in 0..3 {
                    jtj[a][b] += j[a] * j[b];
                }
            }
            count += 1.0;
        }
        if count < MIN_POINTS as f64 {
            return None;
        }
        let delta = solve3(jtj, jtf)?;
        cx -= delta[0];
        cy -= delta[1];
        r -= delta[2];
        if !(cx.is_finite() && cy.is_finite() && r.is_finite()) {
            return None;
        }
        if delta.iter().all(|d| d.abs() < 1e-12) {
            break;
        }
    }
    (r >= 0.0).then_some((Point::new(cx, cy), r))
}

/// Solve the 3×3 system `a · x = b` by Cramer's rule; `None` if singular.
///
/// Indexing is over compile-time-fixed `[[f64; 3]; 3]` / `[f64; 3]` arrays with
/// constant indices `0..=2`, so every access is provably in bounds — the
/// `indexing_slicing` restriction is allowed here for readable linear algebra.
#[allow(clippy::indexing_slicing)]
fn solve3(a: [[f64; 3]; 3], b: [f64; 3]) -> Option<[f64; 3]> {
    let det = a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
        - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]);
    if det.abs() <= f64::EPSILON || !det.is_finite() {
        return None;
    }
    // Replace each column with b and take the ratio of determinants.
    let col = |c: usize| -> f64 {
        let mut m = a;
        for r in 0..3 {
            m[r][c] = b[r];
        }
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let x = [col(0) / det, col(1) / det, col(2) / det];
    x.iter().all(|v| v.is_finite()).then_some(x)
}

/// The RMS point-to-circle residual — `sqrt(mean((|p−c| − r)²))` over the
/// finite points. `None` if fewer than [`MIN_POINTS`] are finite.
fn rms_residual(points: &[Point], center: Point, radius: f64) -> Option<f64> {
    let mut sum = 0.0f64;
    let mut n = 0.0f64;
    for p in points.iter().filter(|p| p.is_finite()) {
        let d = p.distance(center) - radius;
        sum += d * d;
        n += 1.0;
    }
    if n < MIN_POINTS as f64 {
        return None;
    }
    let rms = (sum / n).sqrt();
    rms.is_finite().then_some(rms)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    /// Points exactly on a circle, over the arc `[0, sweep)` radians.
    fn arc_points(center: Point, radius: f64, sweep: f64, n: usize) -> Vec<Point> {
        (0..n)
            .map(|i| {
                let t = sweep * (i as f64) / ((n.max(2) - 1) as f64);
                Point::new(center.x + radius * t.cos(), center.y + radius * t.sin())
            })
            .collect()
    }

    #[test]
    fn full_circle_recovered_exactly() {
        let c = Point::new(12.0, -7.0);
        let pts = arc_points(c, 20.0, TAU * 0.999, 60);
        let fit = fit_circle_taubin(&pts).unwrap();
        assert!((fit.center.x - c.x).abs() < 1e-6, "{fit:?}");
        assert!((fit.center.y - c.y).abs() < 1e-6);
        assert!((fit.radius - 20.0).abs() < 1e-6);
        assert!(fit.residual < 1e-6);
    }

    #[test]
    fn short_arc_noiseless_recovered_exactly() {
        // Even a 30° noiseless arc pins the circle exactly for Taubin.
        let c = Point::new(0.0, 0.0);
        let pts = arc_points(c, 100.0, TAU * 30.0 / 360.0, 8);
        let fit = fit_circle_taubin(&pts).unwrap();
        assert!((fit.radius - 100.0).abs() < 1e-6, "r={}", fit.radius);
        assert!(fit.residual < 1e-6);
    }

    #[test]
    fn residual_grows_with_scatter() {
        // A circle with one point pushed off it has a non-zero residual.
        let c = Point::new(0.0, 0.0);
        let mut pts = arc_points(c, 50.0, TAU * 0.9, 20);
        pts.push(Point::new(5.0, 0.0)); // way inside
        let fit = fit_circle_taubin(&pts).unwrap();
        assert!(fit.residual > 0.5, "residual should reflect the outlier");
    }

    #[test]
    fn degenerate_inputs_return_none_not_panic() {
        assert!(fit_circle_taubin(&[]).is_none());
        assert!(fit_circle_taubin(&[Point::new(0.0, 0.0)]).is_none());
        assert!(fit_circle_taubin(&[Point::new(0.0, 0.0), Point::new(1.0, 1.0)]).is_none());
        // Three collinear points — singular, no unique circle.
        let collinear = [
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.0),
            Point::new(2.0, 0.0),
        ];
        assert!(fit_circle_taubin(&collinear).is_none());
        // Non-finite points are skipped; too few finite → None.
        let bad = [
            Point::new(f64::NAN, 0.0),
            Point::new(f64::INFINITY, 0.0),
            Point::new(1.0, 1.0),
        ];
        assert!(fit_circle_taubin(&bad).is_none());
    }

    #[test]
    fn refinement_never_worsens_the_residual() {
        let c = Point::new(3.0, 4.0);
        let mut pts = arc_points(c, 15.0, TAU * 0.5, 25);
        // Perturb deterministically so there is something to refine.
        for (i, p) in pts.iter_mut().enumerate() {
            let s = if i % 2 == 0 { 0.2 } else { -0.2 };
            p.x += s;
            p.y -= s;
        }
        let base = fit_circle_taubin(&pts).unwrap();
        let refined = fit_circle_taubin_refined(&pts).unwrap();
        assert!(refined.residual <= base.residual + 1e-9);
    }

    /// A tiny deterministic LCG + Box-Muller Gaussian, so the Monte-Carlo
    /// bias test is reproducible without a dependency.
    struct Lcg(u64);
    impl Lcg {
        fn next_u(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
        }
        fn gauss(&mut self) -> f64 {
            let u1 = self.next_u().max(1e-12);
            let u2 = self.next_u();
            (-2.0 * u1.ln()).sqrt() * (TAU * u2).cos()
        }
    }

    #[test]
    fn taubin_beats_kasa_on_short_arcs() {
        // Decision 011 §2.3 / Appendix A Pass 12.M2 acceptance:
        // "Best-fit circle near-unbiased on a synthetic short-arc fixture
        //  (Taubin beats Kåsa — proven by test)."
        //
        // Method: many trials of a SHORT arc (40°) of a known circle
        // (radius 100) with radial Gaussian noise. Kåsa biases the radius LOW
        // on short arcs; Taubin is near-unbiased. Assert the MEAN recovered
        // radius over the trials.
        let truth_r = 100.0;
        let center = Point::new(40.0, 55.0);
        let sweep = TAU * 90.0 / 360.0; // 90° arc (a partial arc — Kåsa still biases here)
        let n_per = 24;
        let trials = 1200;
        let sigma = 1.5;

        let mut rng = Lcg(0x1234_5678_9abc_def0);
        let mut sum_taubin = 0.0;
        let mut sum_kasa = 0.0;
        for _ in 0..trials {
            let mut pts = Vec::with_capacity(n_per);
            for i in 0..n_per {
                let t = sweep * (i as f64) / ((n_per - 1) as f64);
                let rn = truth_r + sigma * rng.gauss();
                pts.push(Point::new(center.x + rn * t.cos(), center.y + rn * t.sin()));
            }
            sum_taubin += fit_circle_taubin(&pts).unwrap().radius;
            sum_kasa += fit_circle_kasa(&pts).unwrap().radius;
        }
        let mean_taubin = sum_taubin / f64::from(trials);
        let mean_kasa = sum_kasa / f64::from(trials);
        let bias_taubin = (mean_taubin - truth_r).abs();
        let bias_kasa = (mean_kasa - truth_r).abs();

        // Kåsa underestimates on a short arc.
        assert!(
            mean_kasa < truth_r,
            "Kåsa should underestimate on a short arc: mean_kasa={mean_kasa}"
        );
        // Taubin is near-unbiased (mean within ~1.5% of truth).
        assert!(
            bias_taubin < truth_r * 0.015,
            "Taubin mean radius {mean_taubin} should be near {truth_r} (bias {bias_taubin})"
        );
        // And Taubin's bias is materially smaller than Kåsa's.
        assert!(
            bias_taubin < bias_kasa,
            "Taubin bias {bias_taubin} must beat Kåsa bias {bias_kasa} \
             (mean_taubin={mean_taubin}, mean_kasa={mean_kasa})"
        );
    }
}
