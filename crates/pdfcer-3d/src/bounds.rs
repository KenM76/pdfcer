//! The box around what a render draws, from which a fit frames the model.

use crate::TriangleMesh;
use crate::raster::at;
use crate::render::corners;
use crate::vec3::{length, sub};

/// An axis-aligned box around every point a set of meshes draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    /// The smallest x, y and z.
    pub min: [f64; 3],
    /// The largest x, y and z.
    pub max: [f64; 3],
}

impl Bounds {
    /// The box around the corners of every triangle [`render`](crate::render()) draws (all
    /// three indices in range and every corner finite), or `None` when
    /// there is none.
    ///
    /// ```
    /// use pdfcer_3d::{Bounds, TriangleMesh};
    /// let mut mesh = TriangleMesh::default();
    /// mesh.positions = vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 3.0, 1.0]];
    /// mesh.triangles = vec![[0, 1, 2]];
    /// let b = Bounds::of(&[mesh]).unwrap();
    /// assert_eq!(b.max, [2.0, 3.0, 1.0]);
    /// ```
    pub fn of(meshes: &[TriangleMesh]) -> Option<Bounds> {
        let mut out: Option<Bounds> = None;
        for p in drawn_corners(meshes) {
            let b = out.get_or_insert(Bounds { min: p, max: p });
            for ((lo, hi), c) in b.min.iter_mut().zip(b.max.iter_mut()).zip(p) {
                *lo = lo.min(c);
                *hi = hi.max(c);
            }
        }
        out
    }

    /// The box's centre; finite for any finite box (each end is halved
    /// before the sum, so ends near the largest float do not overflow).
    pub fn centre(&self) -> [f64; 3] {
        [0, 1, 2].map(|i| at(self.min, i) / 2.0 + at(self.max, i) / 2.0)
    }

    /// Half the box's diagonal: the radius of a sphere holding it.
    pub fn radius(&self) -> f64 {
        length(sub(self.max, self.min)) / 2.0
    }
}

/// The corners of every triangle [`render`](crate::render()) draws: those whose three
/// indices are in range and whose corners are all finite ([`corners`]).
pub(crate) fn drawn_corners(
    meshes: &[TriangleMesh],
) -> impl Iterator<Item = [f64; 3]> + Clone + '_ {
    meshes
        .iter()
        .flat_map(|m| m.triangles.iter().filter_map(|t| corners(m, t)).flatten())
}
