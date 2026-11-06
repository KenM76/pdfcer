//! The box around what a render draws, from which a fit frames the model.

use crate::TriangleMesh;
use crate::render::corners;
use crate::render::raster::at;
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
        Bounds::of_coloured(meshes, &[])
    }

    /// [`Bounds::of`] for meshes drawn in `colours` (mesh `i` in
    /// `colours[i]`, as [`render_coloured`](crate::render_coloured) takes
    /// them): a mesh whose colour is fully transparent (alpha 0) is not
    /// drawn, so it is not boxed. A mesh past the end of `colours`, or with
    /// no colour, is boxed.
    ///
    /// ```
    /// use pdfcer_3d::{Bounds, TriangleMesh};
    /// let mut near = TriangleMesh::default();
    /// near.positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    /// near.triangles = vec![[0, 1, 2]];
    /// let mut far = near.clone();
    /// far.positions[1] = [1e6, 0.0, 0.0];
    /// let b = Bounds::of_coloured(&[near, far], &[None, Some([9, 9, 9, 0])]).unwrap();
    /// assert_eq!(b.max, [1.0, 1.0, 0.0]);
    /// ```
    pub fn of_coloured(meshes: &[TriangleMesh], colours: &[Option<[u8; 4]>]) -> Option<Bounds> {
        let mut out: Option<Bounds> = None;
        for p in drawn_corners(meshes, colours) {
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

/// The corners of every triangle [`render_coloured`](crate::render_coloured)
/// draws with `colours`: those of a mesh not fully transparent whose three
/// indices are in range and whose corners are all finite ([`corners`]).
pub(crate) fn drawn_corners<'a>(
    meshes: &'a [TriangleMesh],
    colours: &'a [Option<[u8; 4]>],
) -> impl Iterator<Item = [f64; 3]> + Clone + 'a {
    meshes
        .iter()
        .enumerate()
        .filter(|&(i, _)| !colours.get(i).copied().flatten().is_some_and(|c| c[3] == 0))
        .flat_map(|(_, m)| m.triangles.iter().filter_map(|t| corners(m, t)).flatten())
}
