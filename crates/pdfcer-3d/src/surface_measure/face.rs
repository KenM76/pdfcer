//! What one picked face of a tessellated model measures on its own: area,
//! perimeter, and the plane or cylinder fitted to it.

use std::collections::HashMap;

use super::fit::{across, fit_circle, least_eigenvector};
use super::{Face, PLANE_SLACK, SurfaceMeasureError, SurfacePick, area_and_centre, diagonal};
use crate::TriangleMesh;
use crate::render::corners;
use crate::vec3::{add, dot, length, normalize, scale, sub};

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

/// Which triangles a [`FaceFigures`] measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum FaceSource {
    /// The face the file gives the pick.
    File,
    /// The flat patch round the pick: the triangles of the file's face (or
    /// of the whole mesh) that lie in the picked triangle's plane. Used when
    /// the file's face is not one surface: it fits neither a plane nor a
    /// cylinder, or it is a closed shell, as when a file gives a whole part
    /// as one face. Fitted, so disclose it (rule 4).
    Grown,
    /// The whole mesh: the file separates no faces and the mesh is one
    /// surface.
    WholeMesh,
}

/// What one face measures.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct FaceFigures {
    /// The mesh the face is on.
    pub mesh: usize,
    /// The file's face, when the figures are exactly that face's
    /// ([`FaceSource::File`]); `None` otherwise.
    pub face: Option<usize>,
    /// Which triangles were measured.
    pub source: FaceSource,
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
/// The face is the file's ([`FaceSource::File`]), else the whole mesh when
/// the file separates none ([`FaceSource::WholeMesh`]). When that set is not
/// one surface (it fits neither a plane nor a cylinder, or it is closed and
/// so has no outline), the figures are the flat patch round the pick
/// instead ([`FaceSource::Grown`]), as [`super::surface_at`] gathers it.
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
    let tris = face_triangles(mesh, &face);
    let mut figures = measure(pick.mesh, &tris, flat_degrees)?;
    figures.face = face.index();
    figures.source = if figures.face.is_some() {
        FaceSource::File
    } else {
        FaceSource::WholeMesh
    };
    if !matches!(figures.shape, FaceShape::Unfitted) && figures.perimeter > 0.0 {
        return Ok(figures);
    }
    let seed = mesh
        .triangles
        .get(pick.triangle)
        .and_then(|t| corners(mesh, t))
        .ok_or(SurfaceMeasureError::Degenerate)?;
    let flat = flat_patch(&tris, &seed, flat_degrees, PLANE_SLACK * diagonal(mesh));
    let mut grown = measure(pick.mesh, &flat, flat_degrees)?;
    grown.source = FaceSource::Grown;
    Ok(grown)
}

/// The figures of `tris`, with no face named.
fn measure(
    mesh: usize,
    tris: &[[[f64; 3]; 3]],
    flat_degrees: f64,
) -> Result<FaceFigures, SurfaceMeasureError> {
    let doubled: f64 = tris.iter().map(|w| length(area_and_centre(w).0)).sum();
    if tris.is_empty() || doubled <= 0.0 {
        return Err(SurfaceMeasureError::Degenerate);
    }
    Ok(FaceFigures {
        mesh,
        face: None,
        source: FaceSource::Grown,
        triangles: tris.len(),
        area: doubled / 2.0,
        perimeter: perimeter(tris),
        shape: fit_shape(tris, flat_degrees),
    })
}

/// The triangles of `tris` facing within `flat_degrees` of `seed` and with
/// every corner within `slack` of its plane.
fn flat_patch(
    tris: &[[[f64; 3]; 3]],
    seed: &[[f64; 3]; 3],
    flat_degrees: f64,
    slack: f64,
) -> Vec<[[f64; 3]; 3]> {
    let (area, centre) = area_and_centre(seed);
    let Some(normal) = normalize(area) else {
        return Vec::new();
    };
    let cos_flat = flat_degrees.to_radians().cos();
    tris.iter()
        .filter(|w| {
            normalize(area_and_centre(w).0).is_some_and(|n| dot(n, normal) >= cos_flat)
                && w.iter()
                    .all(|&p| dot(sub(p, centre), normal).abs() <= slack)
        })
        .copied()
        .collect()
}

/// The corners of each triangle of `face` that has an area.
pub(super) fn face_triangles(mesh: &TriangleMesh, face: &Face) -> Vec<[[f64; 3]; 3]> {
    (0..mesh.triangles.len())
        .filter(|&i| face.holds(mesh, i))
        .filter_map(|i| mesh.triangles.get(i).and_then(|t| corners(mesh, t)))
        .filter(|w| normalize(area_and_centre(w).0).is_some())
        .collect()
}

/// The summed length of the edges only one triangle uses.
pub(super) fn perimeter(tris: &[[[f64; 3]; 3]]) -> f64 {
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
    // `+ 0.0`: an empty f64 sum is -0.0.
    edges
        .values()
        .filter(|(n, _)| *n == 1)
        .map(|(_, l)| l)
        .sum::<f64>()
        + 0.0
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
    let (u, v) = across(axis)?;
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
        let [c0, c1, c2] = c;
        // The arc a triangle spans is its three angles less the widest gap.
        let (start, end) = [(c1, c0), (c2, c1), (c0, c2)]
            .into_iter()
            .zip([c1 - c0, c2 - c1, c0 + TAU - c2])
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map_or((c0, c2), |(ends, _)| ends);
        let (start, end) = (index(start), index(end));
        let mut k = start;
        while k != end {
            if let Some(slot) = covered.get_mut(k) {
                *slot = true;
            }
            k = (k + 1) % g;
        }
    }
    let first = at.first().copied().unwrap_or(0.0);
    let open = at
        .iter()
        .zip(at.iter().skip(1).copied().chain([first + TAU]))
        .zip(&covered)
        .filter(|(_, covered)| !**covered)
        .map(|((here, next), _)| next - here)
        .fold(0.0, f64::max);
    TAU - open
}
