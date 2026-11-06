//! Three-component vector arithmetic shared by `pdfcer-3d` (the decoder,
//! the scene walk, the exporters, the rasterizer), the navigation crate
//! `pdfcer-3d-nav` and `pdfcer-core`'s 3D view and measurement code.
//! `pdfcer-3d` re-exports it as `pdfcer_3d::vec3`.
//!
//! Plain `[f64; 3]` arrays, componentwise IEEE 754 arithmetic. [`length`],
//! [`normalize`] and [`unit_cross`] scale before squaring, so they are
//! correct for any finite input from `±f64::MAX` down to subnormals; [`dot`]
//! and [`cross`] are not, and overflow to infinity for components beyond
//! about `1e154`.

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]

/// A point or direction in model space, `[x, y, z]`.
pub type Vec3 = [f64; 3];

/// `a + b`, componentwise; a component may overflow to infinity.
pub fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// `a - b`, componentwise; a component may overflow to infinity.
pub fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// The direction from `from` to `to`: `to - from`, halved when the full
/// difference overflows (the direction is unchanged, the length is half).
/// Finite for any finite `from` and `to`.
pub fn span(from: Vec3, to: Vec3) -> Vec3 {
    let d = sub(to, from);
    if d.iter().all(|c| c.is_finite()) {
        d
    } else {
        sub(scale(to, 0.5), scale(from, 0.5))
    }
}

/// `a` times `s`, componentwise.
pub fn scale(a: Vec3, s: f64) -> Vec3 {
    a.map(|c| c * s)
}

/// The dot product `a · b`. Unscaled: infinite when a product overflows.
pub fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The right-handed cross product `a × b`. Unscaled, like [`dot`].
pub fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Below this a plain `sqrt(a · a)` may have lost precision to underflow.
const SMALL_LENGTH: f64 = 1e-140;

/// The Euclidean length of `a`, correct for any finite `a`: components
/// whose squares would overflow or underflow are scaled first. Infinite
/// or NaN only when a component is.
pub fn length(a: Vec3) -> f64 {
    let l = dot(a, a).sqrt();
    if l.is_nan() || (l.is_finite() && l > SMALL_LENGTH) {
        return l;
    }
    match unit_scaled(a) {
        Some((m, s)) => m * dot(s, s).sqrt(),
        None => l,
    }
}

/// `a` scaled to unit length; `None` when its length is zero or any
/// component is not finite. Any finite non-zero `a` normalizes, however
/// large or small.
///
/// ```
/// use pdfcer_vec3::normalize;
/// assert_eq!(normalize([1e200, 0.0, 0.0]), Some([1.0, 0.0, 0.0]));
/// assert_eq!(normalize([0.0; 3]), None);
/// ```
pub fn normalize(a: Vec3) -> Option<Vec3> {
    let l = dot(a, a).sqrt();
    if l.is_finite() && l > SMALL_LENGTH {
        return Some(scale(a, 1.0 / l));
    }
    let (_, s) = unit_scaled(a)?;
    let l = dot(s, s).sqrt();
    Some(s.map(|c| c / l))
}

/// The unit normal of the plane through `a` and `b` (the direction of
/// `a × b`), each scaled first so edges far beyond `1e154` still give one;
/// `None` when either is zero or not finite, or they are parallel.
pub fn unit_cross(a: Vec3, b: Vec3) -> Option<Vec3> {
    let (_, a) = unit_scaled(a)?;
    let (_, b) = unit_scaled(b)?;
    normalize(cross(a, b))
}

/// `a`'s largest absolute component `m` and `a / m`; `None` when `m` is
/// zero or any component is not finite.
fn unit_scaled(a: Vec3) -> Option<(f64, Vec3)> {
    let m = a.iter().fold(0.0f64, |m, c| m.max(c.abs()));
    let ok = m > 0.0 && a.iter().all(|c| c.is_finite());
    ok.then(|| (m, a.map(|c| c / m)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cross_follows_the_right_hand_rule() {
        assert_eq!(cross([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]), [0.0, 0.0, 1.0]);
        assert_eq!(cross([0.0, 1.0, 0.0], [1.0, 0.0, 0.0]), [0.0, 0.0, -1.0]);
    }

    #[test]
    fn normalize_rejects_degenerate_vectors() {
        assert_eq!(normalize([0.0, 4.0, 0.0]), Some([0.0, 1.0, 0.0]));
        assert_eq!(normalize([0.0; 3]), None);
        assert_eq!(normalize([f64::NAN, 0.0, 0.0]), None);
        assert_eq!(normalize([f64::INFINITY, 0.0, 0.0]), None);
    }

    #[test]
    fn normalize_scales_before_squaring() {
        assert_eq!(normalize([1e200, 0.0, 0.0]), Some([1.0, 0.0, 0.0]));
        assert_eq!(normalize([0.0, -3e-320, 0.0]), Some([0.0, -1.0, 0.0]));
        assert_eq!(length([0.0, 0.0, -1e200]), 1e200);
        assert_eq!(length([0.0, -3e-320, 0.0]), 3e-320);
    }

    #[test]
    fn span_keeps_the_direction_of_a_difference_that_overflows() {
        assert_eq!(span([1.0, 2.0, 3.0], [4.0, 0.0, 3.0]), [3.0, -2.0, 0.0]);
        let wide = span([-1e308, 0.0, 0.0], [1e308, 1e308, 0.0]);
        assert_eq!(wide, [1e308, 5e307, 0.0]);
    }

    #[test]
    fn unit_cross_survives_edges_near_1e160() {
        let n = unit_cross([1e160, 0.0, 0.0], [0.0, 2e160, 0.0]);
        assert_eq!(n, Some([0.0, 0.0, 1.0]));
        assert_eq!(unit_cross([1.0, 0.0, 0.0], [2.0, 0.0, 0.0]), None);
    }
}
