//! Measuring between two picked surfaces of a tessellated model.
//!
//! A model keeps triangles, not the planes they were cut from, so a
//! surface is the flat patch round a picked triangle: every triangle of the
//! same face (the whole mesh when the file separates no faces) that lies in
//! the picked triangle's plane. The patch's plane is fitted, and
//! [`SurfacePatch`] says how much was fitted from what.

use crate::render::corners;
use crate::vec3::{add, cross, dot, length, normalize, scale, sub};
use crate::{Hit, TriangleMesh};

mod types;

pub use types::{
    DEFAULT_FLAT_DEGREES, DEFAULT_PARALLEL_DEGREES, SurfaceMeasure, SurfaceMeasureError,
    SurfacePatch, SurfacePick, SurfaceRelation, SurfaceTolerance,
};

/// How far off the picked triangle's plane a corner may lie, as a share of
/// the mesh's diagonal: room for coordinates stored as 32-bit floats.
const PLANE_SLACK: f64 = 1e-5;

impl From<&Hit> for SurfacePick {
    fn from(hit: &Hit) -> Self {
        SurfacePick {
            mesh: hit.mesh,
            triangle: hit.triangle,
            point: hit.point,
        }
    }
}

/// The triangles a patch may draw on: the picked triangle's face.
enum Face {
    /// Face `0` is this range of triangles.
    Range(usize, std::ops::Range<usize>),
    /// The compressed form's per-triangle face index.
    Stored(u32),
    /// The file separates no faces.
    None,
}

impl Face {
    fn of(mesh: &TriangleMesh, triangle: usize) -> Face {
        if let Some(i) = mesh.faces.iter().position(|r| r.contains(&triangle)) {
            let range = mesh.faces.get(i).cloned().unwrap_or(0..0);
            return Face::Range(i, range);
        }
        if mesh.triangle_faces.len() == mesh.triangles.len()
            && let Some(&f) = mesh.triangle_faces.get(triangle)
        {
            return Face::Stored(f);
        }
        Face::None
    }

    fn holds(&self, mesh: &TriangleMesh, triangle: usize) -> bool {
        match self {
            Face::Range(_, range) => range.contains(&triangle),
            Face::Stored(f) => mesh.triangle_faces.get(triangle) == Some(f),
            Face::None => true,
        }
    }

    fn index(&self) -> Option<usize> {
        match self {
            Face::Range(i, _) => Some(*i),
            Face::Stored(f) => Some(*f as usize),
            Face::None => None,
        }
    }
}

/// A triangle's area vector (twice its area long, along its winding normal)
/// and its centre.
fn area_and_centre(world: &[[f64; 3]; 3]) -> ([f64; 3], [f64; 3]) {
    let [a, b, c] = *world;
    (
        cross(sub(b, a), sub(c, a)),
        scale(add(add(a, b), c), 1.0 / 3.0),
    )
}

/// The length of the diagonal of the box round `mesh`'s finite points.
fn diagonal(mesh: &TriangleMesh) -> f64 {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in mesh
        .positions
        .iter()
        .filter(|p| p.iter().all(|c| c.is_finite()))
    {
        for ((l, h), &c) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
            *l = l.min(c);
            *h = h.max(c);
        }
    }
    let d = length(sub(hi, lo));
    if d.is_finite() { d } else { 0.0 }
}

/// The patch being gathered round a picked triangle.
struct Gather {
    seed_normal: [f64; 3],
    seed_point: [f64; 3],
    cos_flat: f64,
    slack: f64,
    sum: [f64; 3],
    weighted_centre: [f64; 3],
    normals: Vec<[f64; 3]>,
    /// Triangles of the face that have an area, kept or not.
    sound: usize,
}

impl Gather {
    /// Counts one triangle of the face, keeping it when it lies in the
    /// picked triangle's plane.
    fn offer(&mut self, world: &[[f64; 3]; 3]) {
        let (area, centre) = area_and_centre(world);
        let Some(n) = normalize(area) else {
            return;
        };
        self.sound += 1;
        let in_plane = world
            .iter()
            .all(|&p| dot(sub(p, self.seed_point), self.seed_normal).abs() <= self.slack);
        if dot(n, self.seed_normal) < self.cos_flat || !in_plane {
            return;
        }
        self.sum = add(self.sum, area);
        self.weighted_centre = add(self.weighted_centre, scale(centre, length(area)));
        self.normals.push(n);
    }
}

/// The flat surface round `pick`: the triangles of its face that lie in the
/// picked triangle's plane, within `flat_degrees` of its direction.
///
/// # Errors
///
/// [`SurfaceMeasureError::NoSuchTriangle`] when `pick` names a mesh or
/// triangle `meshes` does not have, [`SurfaceMeasureError::Degenerate`]
/// when the picked triangle has no area, and
/// [`SurfaceMeasureError::BadTolerance`] when `flat_degrees` is not within
/// 0 to 90.
pub fn surface_at(
    meshes: &[TriangleMesh],
    pick: &SurfacePick,
    flat_degrees: f64,
) -> Result<SurfacePatch, SurfaceMeasureError> {
    if !(0.0..=90.0).contains(&flat_degrees) {
        return Err(SurfaceMeasureError::BadTolerance("flat tolerance"));
    }
    let missing = SurfaceMeasureError::NoSuchTriangle {
        mesh: pick.mesh,
        triangle: pick.triangle,
    };
    let Some(mesh) = meshes.get(pick.mesh) else {
        return Err(missing);
    };
    let Some(tri) = mesh.triangles.get(pick.triangle) else {
        return Err(missing);
    };
    let seed = corners(mesh, tri).ok_or(SurfaceMeasureError::Degenerate)?;
    let (seed_area, seed_centre) = area_and_centre(&seed);
    let seed_normal = normalize(seed_area).ok_or(SurfaceMeasureError::Degenerate)?;
    let face = Face::of(mesh, pick.triangle);
    let mut g = Gather {
        seed_normal,
        seed_point: seed_centre,
        cos_flat: flat_degrees.to_radians().cos(),
        slack: PLANE_SLACK * diagonal(mesh),
        sum: [0.0; 3],
        weighted_centre: [0.0; 3],
        normals: Vec::new(),
        sound: 0,
    };
    for (i, tri) in mesh.triangles.iter().enumerate() {
        if face.holds(mesh, i)
            && let Some(world) = corners(mesh, tri)
        {
            g.offer(&world);
        }
    }
    let normal = normalize(g.sum).ok_or(SurfaceMeasureError::Degenerate)?;
    let doubled = length(g.sum);
    let flatness = g
        .normals
        .iter()
        .map(|&n| dot(n, normal).clamp(-1.0, 1.0).acos().to_degrees())
        .fold(0.0, f64::max);
    Ok(SurfacePatch {
        mesh: pick.mesh,
        face: face.index(),
        whole_face: face.index().is_some() && g.normals.len() == g.sound,
        triangles: g.normals.len(),
        area: doubled / 2.0,
        centroid: scale(g.weighted_centre, 1.0 / doubled),
        normal,
        flatness_degrees: flatness,
    })
}

/// Measures between the surfaces at two picks: the distance between them
/// when they are parallel within `tolerance`, the angle between them
/// otherwise.
///
/// The distance is from `b`'s picked point to the plane of `a`'s surface,
/// so a pair forced parallel still reports a distance that was measured
/// somewhere. The angle is reported either way
/// ([`SurfaceMeasure::angle_degrees`]).
///
/// # Errors
///
/// As [`surface_at`] for either pick;
/// [`SurfaceMeasureError::BadTolerance`] when a tolerance is not within 0
/// to 90 degrees; [`SurfaceMeasureError::BadPoint`] when a picked point is
/// not finite.
pub fn measure_surfaces(
    meshes: &[TriangleMesh],
    a: &SurfacePick,
    b: &SurfacePick,
    tolerance: &SurfaceTolerance,
) -> Result<SurfaceMeasure, SurfaceMeasureError> {
    if !(0.0..=90.0).contains(&tolerance.parallel_degrees) {
        return Err(SurfaceMeasureError::BadTolerance("parallel tolerance"));
    }
    let finite = |p: &SurfacePick| p.point.iter().all(|c| c.is_finite());
    if !finite(a) || !finite(b) {
        return Err(SurfaceMeasureError::BadPoint);
    }
    let first = surface_at(meshes, a, tolerance.flat_degrees)?;
    let second = surface_at(meshes, b, tolerance.flat_degrees)?;
    let cos = dot(first.normal, second.normal).clamp(-1.0, 1.0);
    let angle_degrees = cos.abs().acos().to_degrees();
    let within = angle_degrees <= tolerance.parallel_degrees;
    let relation = if within || tolerance.force_parallel {
        SurfaceRelation::Parallel {
            distance: dot(sub(b.point, first.centroid), first.normal).abs(),
            forced: !within,
        }
    } else {
        SurfaceRelation::Angled {
            degrees: angle_degrees,
        }
    };
    let delta = sub(b.point, a.point);
    Ok(SurfaceMeasure {
        a: first,
        b: second,
        angle_degrees,
        normals_degrees: cos.acos().to_degrees(),
        relation,
        pick_distance: length(delta),
        delta,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)] // Tests fail loudly by design.
#[allow(clippy::single_range_in_vec_init)] // One face spanning every triangle is intended.
mod tests;
