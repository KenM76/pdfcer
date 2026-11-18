//! What one picked face of a tessellated model measures on its own: area,
//! perimeter, and the plane or cylinder fitted to it.

use std::collections::HashMap;

use super::{Face, SurfaceMeasureError, SurfacePick, area_and_centre};
use crate::TriangleMesh;
use crate::render::corners;
use crate::vec3::{add, cross, dot, length, normalize, scale, sub};

/// The shape fitted to a whole face.
///
/// Fitted from the triangles, not read from the file (rule 4): a figure
/// that rests on it is shown beside its residual.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum FaceShape {
    /// Every triangle faces within the flat tolerance of one direction.
    Plane {
        /// The fitted unit normal, the way the triangles wind.
        normal: [f64; 3],
        /// The widest angle, in degrees, between a triangle and the plane.
        flatness_degrees: f64,
    },
    /// Every triangle's normal lies within the flat tolerance of square to
    /// one axis, and the corners fit a circle round it.
    Cylinder {
        /// The axis point midway along the face.
        centre: [f64; 3],
        /// The axis, a unit vector (its sign is arbitrary).
        axis: [f64; 3],
        /// The fitted radius, model units.
        radius: f64,
        /// The farthest a corner lies off the fitted cylinder, model units.
        residual: f64,
        /// The face's extent along the axis, model units.
        length: f64,
        /// How far round the axis the face reaches, degrees: 360 for a
        /// whole cylinder, less for an arc-shaped face.
        sweep_degrees: f64,
    },
    /// Neither a plane nor a cylinder within the tolerance.
    Unfitted,
}

/// What one face measures.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct FaceFigures {
    /// The mesh the face is on.
    pub mesh: usize,
    /// The face; `None` when the file separates no faces, and the figures
    /// are then the whole mesh's.
    pub face: Option<usize>,
    /// Triangles with an area that were measured.
    pub triangles: usize,
    /// Their area, model units squared.
    pub area: f64,
    /// The length of the face's edges no second triangle of the face
    /// shares, model units: its outline and the rims of its holes. Corners
    /// at the same position count as one, so a face stored as separate
    /// triangles still has its true outline.
    pub perimeter: f64,
    /// The plane or cylinder fitted.
    pub shape: FaceShape,
}

/// Measures the whole face `pick` lies on.
///
/// `flat_degrees` (0 to 90) is how far a triangle may turn and still count
/// as on the fitted plane, or square to the fitted axis.
///
/// # Errors
///
/// [`SurfaceMeasureError::NoSuchTriangle`] when `pick` names a mesh or
/// triangle `meshes` does not have, [`SurfaceMeasureError::Degenerate`]
/// when the face has no area, [`SurfaceMeasureError::BadTolerance`] when
/// `flat_degrees` is not within 0 to 90.
pub fn face_figures(
    meshes: &[TriangleMesh],
    pick: &SurfacePick,
    flat_degrees: f64,
) -> Result<FaceFigures, SurfaceMeasureError> {
    if !(0.0..=90.0).contains(&flat_degrees) {
        return Err(SurfaceMeasureError::BadTolerance("flat tolerance"));
    }
    let missing = SurfaceMeasureError::NoSuchTriangle {
        mesh: pick.mesh,
        triangle: pick.triangle,
    };
    let mesh = meshes.get(pick.mesh).ok_or(missing.clone())?;
    if pick.triangle >= mesh.triangles.len() {
        return Err(missing);
    }
    let face = Face::of(mesh, pick.triangle);
    let tris: Vec<[[f64; 3]; 3]> = (0..mesh.triangles.len())
        .filter(|&i| face.holds(mesh, i))
        .filter_map(|i| mesh.triangles.get(i).and_then(|t| corners(mesh, t)))
        .filter(|w| normalize(area_and_centre(w).0).is_some())
        .collect();
    let doubled: f64 = tris.iter().map(|w| length(area_and_centre(w).0)).sum();
    if tris.is_empty() || doubled <= 0.0 {
        return Err(SurfaceMeasureError::Degenerate);
    }
    Ok(FaceFigures {
        mesh: pick.mesh,
        face: face.index(),
        triangles: tris.len(),
        area: doubled / 2.0,
        perimeter: perimeter(&tris),
        shape: fit_shape(&tris, flat_degrees),
    })
}

/// The summed length of the edges only one triangle uses.
fn perimeter(tris: &[[[f64; 3]; 3]]) -> f64 {
    let mut ids: HashMap<[u64; 3], usize> = HashMap::new();
    let mut id = |p: [f64; 3]| {
        let next = ids.len();
        *ids.entry(p.map(|c| (c + 0.0).to_bits())).or_insert(next)
    };
    let mut edges: HashMap<(usize, usize), (usize, f64)> = HashMap::new();
    for w in tris {
        let [a, b, c] = w.map(&mut id);
        let [pa, pb, pc] = *w;
        for ((i, p), (j, q)) in [((a, pa), (b, pb)), ((b, pb), (c, pc)), ((c, pc), (a, pa))] {
            let entry = edges.entry((i.min(j), i.max(j)));
            entry.or_insert((0, length(sub(q, p)))).0 += 1;
        }
    }
    edges
        .values()
        .filter(|(n, _)| *n == 1)
        .map(|(_, l)| l)
        .sum()
}

/// Each triangle's unit normal and its area weight.
fn normals(tris: &[[[f64; 3]; 3]]) -> Vec<([f64; 3], f64)> {
    tris.iter()
        .filter_map(|w| {
            let a = area_and_centre(w).0;
            normalize(a).map(|n| (n, length(a)))
        })
        .collect()
}

fn fit_shape(tris: &[[[f64; 3]; 3]], flat_degrees: f64) -> FaceShape {
    let ns = normals(tris);
    let sum = ns.iter().fold([0.0; 3], |s, &(n, w)| add(s, scale(n, w)));
    if let Some(normal) = normalize(sum) {
        let flatness = ns
            .iter()
            .map(|&(n, _)| dot(n, normal).clamp(-1.0, 1.0).acos().to_degrees())
            .fold(0.0, f64::max);
        if flatness <= flat_degrees {
            return FaceShape::Plane {
                normal,
                flatness_degrees: flatness,
            };
        }
    }
    let mut m = [[0.0; 3]; 3];
    for &(n, w) in &ns {
        for (r, &nr) in m.iter_mut().zip(&n) {
            for (c, &nc) in r.iter_mut().zip(&n) {
                *c += w * nr * nc;
            }
        }
    }
    let axis = least_eigenvector(m);
    let square = flat_degrees.to_radians().sin();
    if ns.iter().any(|&(n, _)| dot(n, axis).abs() > square) {
        return FaceShape::Unfitted;
    }
    cylinder(tris, axis).unwrap_or(FaceShape::Unfitted)
}

/// The corners' circle round `axis`, as a [`FaceShape::Cylinder`].
fn cylinder(tris: &[[[f64; 3]; 3]], axis: [f64; 3]) -> Option<FaceShape> {
    let seed = if axis.first().is_some_and(|x| x.abs() < 0.9) {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let u = normalize(cross(axis, seed))?;
    let v = cross(axis, u);
    let points: Vec<[f64; 3]> = tris.iter().flatten().copied().collect();
    let flat: Vec<(f64, f64)> = points.iter().map(|&p| (dot(p, u), dot(p, v))).collect();
    let (cu, cv, radius) = fit_circle(&flat)?;
    let residual = flat
        .iter()
        .map(|&(x, y)| ((x - cu).hypot(y - cv) - radius).abs())
        .fold(0.0, f64::max);
    let along: Vec<f64> = points.iter().map(|&p| dot(p, axis)).collect();
    let lo = along.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = along.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let centre = add(
        add(scale(u, cu), scale(v, cv)),
        scale(axis, (lo + hi) / 2.0),
    );
    let angles: Vec<[f64; 3]> = tris
        .iter()
        .map(|w| w.map(|p| (dot(p, v) - cv).atan2(dot(p, u) - cu)))
        .collect();
    Some(FaceShape::Cylinder {
        centre,
        axis,
        radius,
        residual,
        length: hi - lo,
        sweep_degrees: sweep(&angles).to_degrees(),
    })
}

/// How far round the axis the triangles reach, from their corners' angles
/// `tri_angles`: a full turn less the widest gap between neighbouring
/// corners that no triangle spans.
#[allow(clippy::indexing_slicing)] // Indices are below 3 or reduced mod `g`.
fn sweep(tri_angles: &[[f64; 3]]) -> f64 {
    use std::f64::consts::TAU;
    let mut at: Vec<f64> = tri_angles.iter().flatten().copied().collect();
    at.sort_by(f64::total_cmp);
    at.dedup();
    let g = at.len();
    if g < 2 {
        return 0.0;
    }
    let mut covered = vec![false; g];
    let index = |a: f64| at.partition_point(|&x| x < a) % g;
    for t in tri_angles {
        let mut c = *t;
        c.sort_by(f64::total_cmp);
        let gaps = [c[1] - c[0], c[2] - c[1], c[0] + TAU - c[2]];
        let widest = (0..3)
            .max_by(|&a, &b| gaps[a].total_cmp(&gaps[b]))
            .unwrap_or(2);
        let (start, end) = (index(c[(widest + 1) % 3]), index(c[widest]));
        let mut k = start;
        while k != end {
            covered[k] = true;
            k = (k + 1) % g;
        }
    }
    let gap = |k: usize| {
        let next = at.get(k + 1).copied().unwrap_or(at[0] + TAU);
        next - at[k]
    };
    let open = (0..g).filter(|&k| !covered[k]).map(gap).fold(0.0, f64::max);
    TAU - open
}

/// The algebraic least-squares circle through `points` (Kåsa): centre and
/// radius, `None` when they are collinear or too few.
fn fit_circle(points: &[(f64, f64)]) -> Option<(f64, f64, f64)> {
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
fn least_eigenvector(mut m: [[f64; 3]; 3]) -> [f64; 3] {
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
