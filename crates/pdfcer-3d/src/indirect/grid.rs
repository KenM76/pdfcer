//! The probe lattice over a model's padded bounds, and the lookup that
//! blends the eight probes around a point.

use super::octa;
use super::{Probe, zip3};
use crate::Bounds;
use crate::model::light::unit;
use crate::vec3::{add, dot, length, scale, sub};

/// Probes at `origin + (i, j, k) * spacing`, `dims` along each axis (at
/// least 2 each).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Grid {
    pub(crate) origin: [f64; 3],
    pub(crate) spacing: [f64; 3],
    pub(crate) dims: [usize; 3],
}

impl Grid {
    /// The lattice of at most `max` probes (8 or more) around `bounds`,
    /// padded by 2% of its diagonal; `None` when the bounds are a point or
    /// not finite.
    pub(crate) fn around(bounds: &Bounds, max: usize) -> Option<Grid> {
        let diagonal = length(sub(bounds.max, bounds.min));
        if !diagonal.is_finite() || diagonal <= 0.0 {
            return None;
        }
        let pad = 0.02 * diagonal;
        let origin = bounds.min.map(|v| v - pad);
        let extent = sub(bounds.max, bounds.min).map(|e| e + 2.0 * pad);
        let mut step = (extent[0] * extent[1] * extent[2] / max as f64).cbrt();
        let mut dims = [2; 3];
        // Each pass grows the step by 10%, so the count falls below the
        // ceiling within a few dozen passes; the bound is a guard.
        for _ in 0..200 {
            dims = extent.map(|e| ((e / step) as usize).saturating_add(1).clamp(2, max));
            if dims
                .iter()
                .try_fold(1usize, |n, &d| n.checked_mul(d))
                .is_some_and(|n| n <= max)
            {
                break;
            }
            dims = [2; 3];
            step *= 1.1;
        }
        let spacing = zip3(extent, dims, |e, d| e / (d - 1) as f64);
        Some(Grid {
            origin,
            spacing,
            dims,
        })
    }

    /// The number of probes.
    pub(crate) fn len(&self) -> usize {
        self.dims.iter().product()
    }

    /// The smallest spacing between neighbouring probes.
    pub(crate) fn min_spacing(&self) -> f64 {
        self.spacing.iter().copied().fold(f64::INFINITY, f64::min)
    }

    /// The length of the lattice's diagonal.
    pub(crate) fn diagonal(&self) -> f64 {
        length(zip3(self.spacing, self.dims, |s, d| s * (d - 1) as f64))
    }

    /// Probe `i`'s position.
    pub(crate) fn position(&self, i: usize) -> [f64; 3] {
        let [dx, dy, _] = self.dims.map(|d| d.max(1));
        let (x, rest) = (i % dx, i / dx);
        let cell = [x, rest % dy, rest / dy];
        add(self.origin, zip3(cell, self.spacing, |c, s| c as f64 * s))
    }

    /// The lowest corner of the cell holding `p` (clamped into the lattice)
    /// and `p`'s position across it, each 0 to 1.
    fn cell(&self, p: [f64; 3]) -> ([usize; 3], [f64; 3]) {
        let axis = |offset: f64, (spacing, dims): (f64, usize)| {
            let f = (offset / spacing).clamp(0.0, (dims - 1) as f64);
            let f = if f.is_nan() { 0.0 } else { f };
            let b = (f.floor() as usize).min(dims - 2);
            (b, f - b as f64)
        };
        let per = zip3(
            sub(p, self.origin),
            zip3(self.spacing, self.dims, |s, d| (s, d)),
            axis,
        );
        (per.map(|(b, _)| b), per.map(|(_, f)| f))
    }

    fn index(&self, c: [usize; 3]) -> usize {
        let ([x, y, z], [dx, dy, _]) = (c, self.dims);
        x + dx * (y + dy * z)
    }
}

/// The irradiance at `p` on the side its unit normal `n` faces, blended
/// from the eight probes around `p`: each weighted by its trilinear share,
/// by how far it sits in front of the surface, and by whether its depth
/// moments say it can see `p`. Inactive probes weigh nothing; with none
/// left the result is zero.
pub(crate) fn gather(
    grid: &Grid,
    probes: &[Probe],
    active: &[bool],
    p: [f64; 3],
    n: [f64; 3],
) -> [f64; 3] {
    let (base, frac) = grid.cell(p);
    let biased = add(p, scale(n, 0.25 * grid.min_spacing()));
    let mut sum = [0.0; 3];
    let mut total = 0.0;
    for corner in 0..8usize {
        let bits = [0, 1, 2].map(|k| (corner >> k) & 1);
        let c = zip3(base, bits, |b, o| b + o);
        let i = grid.index(c);
        let (Some(probe), Some(true)) = (probes.get(i), active.get(i).copied()) else {
            continue;
        };
        let q = grid.position(i);
        let trilinear: f64 = zip3(frac, bits, |f, b| if b == 1 { f } else { 1.0 - f })
            .iter()
            .product();
        let towards = unit(sub(q, p)).unwrap_or(n);
        let wrap = (dot(towards, n) + 1.0) * 0.5;
        let mut w = wrap * wrap + 0.2;
        w *= visibility(probe, sub(biased, q));
        if w < 0.2 {
            w *= w * w / 0.04;
        }
        w *= trilinear;
        if w <= 0.0 {
            continue;
        }
        let e = probe.irradiance_at(n);
        sum = add(sum, scale(e, w));
        total += w;
    }
    if total > 1e-12 {
        sum.map(|s| s / total)
    } else {
        [0.0; 3]
    }
}

/// The share of the probe's view that reaches the point `offset` away: 1
/// when nearer than the mean depth it recorded that way, else Chebyshev's
/// bound from the depth variance, cubed to sharpen it.
fn visibility(probe: &Probe, offset: [f64; 3]) -> f64 {
    let d = length(offset);
    let [mean, square] = probe.depth_at(offset).map(f64::from);
    if d <= mean {
        return 1.0;
    }
    let variance = (square - mean * mean).abs().max(1e-12 * (1.0 + square));
    let gap = d - mean;
    (variance / (variance + gap * gap)).powi(3)
}

/// Bilinear irradiance at `d` from a probe map.
pub(crate) fn blend(map: &[[f32; 3]; octa::TEXELS], d: [f64; 3]) -> [f64; 3] {
    let mut out = [0.0; 3];
    for (t, w) in octa::bilinear(d) {
        if let Some(e) = map.get(t) {
            out = add(out, scale(e.map(f64::from), w));
        }
    }
    out
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use crate::indirect::MAX_PROBES;

    #[test]
    fn a_flat_model_still_gets_two_layers_within_the_ceiling() {
        let flat = Bounds {
            min: [0.0; 3],
            max: [10.0, 10.0, 0.0],
        };
        let g = Grid::around(&flat, MAX_PROBES).unwrap();
        assert!(g.len() <= MAX_PROBES);
        assert_eq!(g.dims[2], 2);
        assert!(g.dims[0] > 30 && g.dims[1] > 30, "{:?}", g.dims);
    }

    #[test]
    fn a_point_has_no_grid() {
        assert!(
            Grid::around(
                &Bounds {
                    min: [1.0; 3],
                    max: [1.0; 3]
                },
                MAX_PROBES
            )
            .is_none()
        );
    }

    #[test]
    fn an_inner_probe_sits_at_the_corner_of_its_own_cell() {
        let g = Grid::around(
            &Bounds {
                min: [0.0; 3],
                max: [3.0, 5.0, 7.0],
            },
            MAX_PROBES,
        )
        .unwrap();
        let i = g.index([1, 2, 1]);
        let (base, frac) = g.cell(g.position(i));
        assert_eq!(base, [1, 2, 1]);
        assert!(frac.iter().all(|f| f.abs() < 1e-9), "{frac:?}");
    }
}
