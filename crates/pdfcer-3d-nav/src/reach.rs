//! How big the model is, for fitting: the reach of its farthest drawn
//! vertex and its box's radius, both finite and positive for any model.

use pdfcer_3d::vec3::{length, scale, sub};
use pdfcer_3d::{Bounds, TriangleMesh};

/// How far the farthest vertex the meshes' triangles use lies from
/// `bounds`' centre, in model units: the radius of the sphere a fitted view
/// frames. [`radius`] when no triangle has a finite vertex.
///
/// Measured with scaled norms, so a vertex anywhere in the float range has
/// its true distance; a distance beyond the largest float counts as it.
/// Vertices no triangle uses, and indices past the vertex list, are skipped.
///
/// ```
/// use pdfcer_3d::{Bounds, TriangleMesh};
/// use pdfcer_3d_nav::reach;
/// let mut mesh = TriangleMesh::default();
/// mesh.positions = vec![[0.0; 3], [3.0, 4.0, 0.0], [0.0, 1.0, 0.0], [99.0; 3]];
/// mesh.triangles = vec![[0, 1, 2]];
/// let bounds = Bounds { min: [0.0; 3], max: [0.0; 3] };
/// assert_eq!(reach(&[mesh], &bounds), 5.0);
/// ```
pub fn reach(meshes: &[TriangleMesh], bounds: &Bounds) -> f64 {
    let c = bounds.centre();
    let far = meshes
        .iter()
        .flat_map(|m| {
            m.triangles
                .iter()
                .flatten()
                .filter_map(|&i| m.positions.get(i as usize))
        })
        .filter_map(|&p| distance(c, p))
        .fold(0.0_f64, f64::max);
    if far > 0.0 { far } else { radius(bounds) }
}

/// Half the length of `bounds`' diagonal, or 1 for a box with no extent
/// (or one that is not finite). Halved before it is measured, so a box as
/// wide as the float range has its radius (clamped to the largest float).
pub fn radius(bounds: &Bounds) -> f64 {
    if !bounds.min.iter().chain(&bounds.max).all(|c| c.is_finite()) {
        return 1.0;
    }
    match length(sub(scale(bounds.max, 0.5), scale(bounds.min, 0.5))).min(f64::MAX) {
        r if r > 0.0 => r,
        _ => 1.0,
    }
}

/// The distance from `a` to `b`, clamped to the largest float; `None` when
/// either is not finite.
pub(crate) fn distance(a: [f64; 3], b: [f64; 3]) -> Option<f64> {
    if !a.iter().chain(&b).all(|c| c.is_finite()) {
        return None;
    }
    // A difference that overflows is longer than the largest float anyway.
    Some(length(sub(b, a)).min(f64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mesh(corner: f64) -> TriangleMesh {
        let mut m = TriangleMesh::default();
        m.positions = vec![[-corner; 3], [corner; 3], [corner, -corner, corner]];
        m.triangles = vec![[0, 1, 2]];
        m
    }

    #[test]
    fn the_reach_of_a_huge_or_tiny_model_is_its_own() {
        for corner in [1e200, 1e-200] {
            // The box is twice the mesh, so the fallback is told apart.
            let bounds = Bounds {
                min: [-2.0 * corner; 3],
                max: [2.0 * corner; 3],
            };
            let reach = reach(&[mesh(corner)], &bounds);
            let wanted = corner * 3f64.sqrt();
            assert!(
                (reach / wanted - 1.0).abs() < 1e-12,
                "{corner:e}: reach {reach:e}, wanted {wanted:e}"
            );
        }
    }

    #[test]
    fn the_radius_of_a_box_as_wide_as_the_floats_is_finite() {
        let wide = Bounds {
            min: [-f64::MAX, -1.0, -1.0],
            max: [f64::MAX, 1.0, 1.0],
        };
        assert_eq!(radius(&wide), f64::MAX);
        let most = Bounds {
            min: [-0.6 * f64::MAX, 0.0, 0.0],
            max: [0.6 * f64::MAX, 0.0, 0.0],
        };
        assert_eq!(radius(&most), 0.6 * f64::MAX);
        for corner in [1e200, 1e-200] {
            let b = Bounds {
                min: [-corner; 3],
                max: [corner; 3],
            };
            assert!((radius(&b) / (corner * 3f64.sqrt()) - 1.0).abs() < 1e-12);
        }
        let flat = Bounds {
            min: [2.0; 3],
            max: [2.0; 3],
        };
        assert_eq!(radius(&flat), 1.0);
    }
}
