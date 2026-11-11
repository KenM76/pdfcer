//! Octahedral maps: a unit direction to a square texel grid and back, with
//! the edge fold that makes bilinear reads seamless across the octahedron's
//! seams. Arithmetic only (no trigonometry), so every target agrees bit for
//! bit.

use crate::model::light::unit;

/// Texels along each side of a probe map.
pub(crate) const SIDE: usize = 8;
/// Texels in a probe map.
pub(crate) const TEXELS: usize = SIDE * SIDE;

/// `d` as a point of the unit square; any non-zero `d` (not just unit).
pub(crate) fn encode(d: [f64; 3]) -> [f64; 2] {
    let s = d[0].abs() + d[1].abs() + d[2].abs();
    if s.is_nan() || s <= 0.0 {
        return [0.5, 0.5];
    }
    let (mut x, mut y) = (d[0] / s, d[1] / s);
    if d[2] < 0.0 {
        (x, y) = ((1.0 - y.abs()) * sign(x), (1.0 - x.abs()) * sign(y));
    }
    [(x + 1.0) * 0.5, (y + 1.0) * 0.5]
}

/// The unit direction at point `uv` of the unit square.
pub(crate) fn decode(uv: [f64; 2]) -> [f64; 3] {
    let (x, y) = (uv[0] * 2.0 - 1.0, uv[1] * 2.0 - 1.0);
    let z = 1.0 - x.abs() - y.abs();
    let (x, y) = if z < 0.0 {
        ((1.0 - y.abs()) * sign(x), (1.0 - x.abs()) * sign(y))
    } else {
        (x, y)
    };
    unit([x, y, z]).unwrap_or([0.0, 0.0, 1.0])
}

/// The direction at the centre of texel `t`.
pub(crate) fn texel_direction(t: usize) -> [f64; 3] {
    let (i, j) = (t % SIDE, t / SIDE);
    decode([
        (i as f64 + 0.5) / SIDE as f64,
        (j as f64 + 0.5) / SIDE as f64,
    ])
}

/// The texel `d` falls in.
pub(crate) fn nearest(d: [f64; 3]) -> usize {
    let [u, v] = encode(d);
    let i = ((u * SIDE as f64) as usize).min(SIDE - 1);
    let j = ((v * SIDE as f64) as usize).min(SIDE - 1);
    j * SIDE + i
}

/// The four texels a bilinear read at `d` blends, with their weights
/// (summing to 1).
pub(crate) fn bilinear(d: [f64; 3]) -> [(usize, f64); 4] {
    let [u, v] = encode(d);
    let (fx, fy) = (u * SIDE as f64 - 0.5, v * SIDE as f64 - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (a, b) = (fx - x0, fy - y0);
    let (i, j) = (x0 as i64, y0 as i64);
    [
        (fold(i, j), (1.0 - a) * (1.0 - b)),
        (fold(i + 1, j), a * (1.0 - b)),
        (fold(i, j + 1), (1.0 - a) * b),
        (fold(i + 1, j + 1), a * b),
    ]
}

/// Texel `(i, j)`, one step outside the grid at most, folded back across the
/// octahedral seam: leaving a side mirrors the other coordinate.
fn fold(mut i: i64, mut j: i64) -> usize {
    let last = SIDE as i64 - 1;
    if i < 0 || i > last {
        i = i.clamp(0, last);
        j = last - j;
    }
    if j < 0 || j > last {
        j = j.clamp(0, last);
        i = last - i;
    }
    (j.clamp(0, last) * SIDE as i64 + i.clamp(0, last)) as usize
}

fn sign(v: f64) -> f64 {
    if v < 0.0 { -1.0 } else { 1.0 }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)] // Tests index fixed arrays.
mod tests {
    use super::*;
    use crate::vec3::dot;

    #[test]
    fn decode_inverts_encode() {
        for t in 0..TEXELS {
            let d = texel_direction(t);
            let back = decode(encode(d));
            assert!(dot(d, back) > 1.0 - 1e-12, "texel {t}");
            assert_eq!(nearest(d), t);
        }
    }

    #[test]
    fn a_seam_read_blends_texels_that_touch_across_it() {
        // Just below the equator on the +x side: the read crosses into the
        // lower hemisphere's fold, and every texel it blends lies near d.
        let d = unit([1.0, 0.3, -0.01]).unwrap_or_default();
        for (t, w) in bilinear(d) {
            assert!(w >= 0.0);
            assert!(dot(texel_direction(t), d) > 0.7, "texel {t}");
        }
        let total: f64 = bilinear(d).iter().map(|(_, w)| w).sum();
        assert!((total - 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_corner_folds_to_the_diagonally_opposite_texel() {
        assert_eq!(fold(-1, -1), TEXELS - 1);
        assert_eq!(fold(SIDE as i64, SIDE as i64), 0);
        assert_eq!(fold(-1, 2), (SIDE - 3) * SIDE);
    }
}
