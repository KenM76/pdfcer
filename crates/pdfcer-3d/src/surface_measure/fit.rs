//! The least-squares fits the measures share.

use crate::vec3::{cross, normalize};

/// Two unit vectors square to `axis` and to each other, `None` when `axis`
/// is not a unit direction.
pub(super) fn across(axis: [f64; 3]) -> Option<([f64; 3], [f64; 3])> {
    let seed = if axis.first().is_some_and(|x| x.abs() < 0.9) {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let u = normalize(cross(axis, seed))?;
    Some((u, cross(axis, u)))
}

/// The algebraic least-squares circle through `points` (Kåsa): centre and
/// radius, `None` when they are collinear or too few.
pub(super) fn fit_circle(points: &[(f64, f64)]) -> Option<(f64, f64, f64)> {
    let n = points.len() as f64;
    if points.len() < 3 {
        return None;
    }
    let (mx, my) = points
        .iter()
        .fold((0.0, 0.0), |(a, b), &(x, y)| (a + x / n, b + y / n));
    let (mut suu, mut suv, mut svv, mut suz, mut svz) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for &(x, y) in points {
        let (a, b) = (x - mx, y - my);
        let z = a * a + b * b;
        suu += a * a;
        suv += a * b;
        svv += b * b;
        suz += a * z;
        svz += b * z;
    }
    let det = suu * svv - suv * suv;
    if det.abs() <= f64::EPSILON * (suu * svv).max(f64::MIN_POSITIVE) {
        return None;
    }
    let a = (suz * svv - svz * suv) / (2.0 * det);
    let b = (svz * suu - suz * suv) / (2.0 * det);
    let r2 = a * a + b * b + (suu + svv) / n;
    (r2 > 0.0).then(|| (a + mx, b + my, r2.sqrt()))
}

/// The unit eigenvector of the symmetric `m`'s smallest eigenvalue, by
/// cyclic Jacobi rotations.
#[allow(clippy::indexing_slicing)] // Every index is below 3.
pub(super) fn least_eigenvector(mut m: [[f64; 3]; 3]) -> [f64; 3] {
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..50 {
        let off = m[0][1].abs() + m[0][2].abs() + m[1][2].abs();
        if off < 1e-15 {
            break;
        }
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            if m[p][q].abs() < 1e-300 {
                continue;
            }
            let theta = (m[q][q] - m[p][p]) / (2.0 * m[p][q]);
            let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            rotate(&mut m, &mut v, p, q, c, s);
        }
    }
    let k = (0..3)
        .min_by(|&a, &b| m[a][a].total_cmp(&m[b][b]))
        .unwrap_or(0);
    normalize([v[0][k], v[1][k], v[2][k]]).unwrap_or([0.0, 0.0, 1.0])
}

/// One Jacobi rotation in the `p`,`q` plane: `m = Jᵀ m J`, `v = v J`.
#[allow(clippy::indexing_slicing)] // p, q < 3 by the caller's loop.
fn rotate(m: &mut [[f64; 3]; 3], v: &mut [[f64; 3]; 3], p: usize, q: usize, c: f64, s: f64) {
    for row in m.iter_mut() {
        let (a, b) = (row[p], row[q]);
        row[p] = c * a - s * b;
        row[q] = s * a + c * b;
    }
    let (rp, rq) = (m[p], m[q]);
    m[p] = std::array::from_fn(|k| c * rp[k] - s * rq[k]);
    m[q] = std::array::from_fn(|k| s * rp[k] + c * rq[k]);
    for row in v.iter_mut() {
        let (a, b) = (row[p], row[q]);
        row[p] = c * a - s * b;
        row[q] = s * a + c * b;
    }
}
