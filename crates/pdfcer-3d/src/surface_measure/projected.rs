//! The area of a picked face projected onto a plane.

use super::face::face_triangles;
use super::{Face, SurfaceMeasureError, SurfacePick, area_and_centre};
use crate::TriangleMesh;
use crate::vec3::{dot, normalize};

/// A face's area projected onto the plane square to a direction, model
/// units squared, split by which way each triangle faces.
///
/// A face that never turns past edge-on to the direction (a plane, a
/// shallow dish) lies wholly on one side, and that side's figure is its
/// shadow's area. A face that turns past edge-on (a whole cylinder seen
/// side-on) has both, and where its two sides overlap in the projection
/// each counts once per side, so neither figure alone is its shadow.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct ProjectedArea {
    /// Projected area of the triangles whose outward side faces along the
    /// direction.
    pub toward: f64,
    /// Projected area of the triangles facing against it.
    pub away: f64,
}

/// Projects the whole face `pick` lies on (as [`super::face_figures`])
/// onto the plane square to `direction` (any length).
///
/// # Errors
///
/// [`SurfaceMeasureError::NoSuchTriangle`] for a pick `meshes` does not
/// have, [`SurfaceMeasureError::Degenerate`] for a face with no area,
/// [`SurfaceMeasureError::BadDirection`] for a zero or non-finite
/// `direction`.
pub fn face_projected_area(
    meshes: &[TriangleMesh],
    pick: &SurfacePick,
    direction: [f64; 3],
) -> Result<ProjectedArea, SurfaceMeasureError> {
    let d = normalize(direction)
        .filter(|d| d.iter().all(|c| c.is_finite()))
        .ok_or(SurfaceMeasureError::BadDirection)?;
    let missing = SurfaceMeasureError::NoSuchTriangle {
        mesh: pick.mesh,
        triangle: pick.triangle,
    };
    let mesh = meshes.get(pick.mesh).ok_or(missing.clone())?;
    if pick.triangle >= mesh.triangles.len() {
        return Err(missing);
    }
    let tris = face_triangles(mesh, &Face::of(mesh, pick.triangle));
    if tris.is_empty() {
        return Err(SurfaceMeasureError::Degenerate);
    }
    let (mut toward, mut away) = (0.0, 0.0);
    for w in &tris {
        let along = dot(area_and_centre(w).0, d) / 2.0;
        if along > 0.0 {
            toward += along;
        } else {
            away -= along;
        }
    }
    Ok(ProjectedArea { toward, away })
}
