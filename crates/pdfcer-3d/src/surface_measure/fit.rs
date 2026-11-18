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
pub(super) fn least_eigenvector(mut m: [[f64; 3]; 3]) -> [f64; 3] {
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..50 {
        let off = m[0][1].abs() + m[0][2].abs() + m[1][2].abs();
        if off < 1e-15 {
            break;
        }
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            let (mpp, mqq, mpq) = (at(&m, p, p), at(&m, q, q), at(&m, p, q));
            if mpq.abs() < 1e-300 {
                continue;
            }
            let theta = (mqq - mpp) / (2.0 * mpq);
            let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            rotate(&mut m, &mut v, p, q, c, s);
        }
    }
    let diagonal = [m[0][0], m[1][1], m[2][2]];
    let k = diagonal
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.total_cmp(b.1))
        .map_or(0, |(k, _)| k);
    normalize(v.map(|row| row.get(k).copied().unwrap_or(0.0))).unwrap_or([0.0, 0.0, 1.0])
}

/// `m[i][j]`, 0 off the matrix.
fn at(m: &[[f64; 3]; 3], i: usize, j: usize) -> f64 {
    m.get(i).and_then(|r| r.get(j)).copied().unwrap_or(0.0)
}

/// Replaces entries `p` and `q` of `pair` by `mix` of them.
fn turn<T: Copy>(pair: &mut [T; 3], p: usize, q: usize, mix: impl Fn(T, T) -> (T, T)) {
    if let (Some(&a), Some(&b)) = (pair.get(p), pair.get(q)) {
        let (x, y) = mix(a, b);
        if let Some(e) = pair.get_mut(p) {
            *e = x;
        }
        if let Some(e) = pair.get_mut(q) {
            *e = y;
        }
    }
}

/// One Jacobi rotation in the `p`,`q` plane: `m = Jᵀ m J`, `v = v J`.
fn rotate(m: &mut [[f64; 3]; 3], v: &mut [[f64; 3]; 3], p: usize, q: usize, c: f64, s: f64) {
    let mix = |a: f64, b: f64| (c * a - s * b, s * a + c * b);
    for row in m.iter_mut().chain(v.iter_mut()) {
        turn(row, p, q, mix);
    }
    turn(m, p, q, |mut rp: [f64; 3], mut rq: [f64; 3]| {
        for (a, b) in rp.iter_mut().zip(rq.iter_mut()) {
            (*a, *b) = mix(*a, *b);
        }
        (rp, rq)
    });
}
