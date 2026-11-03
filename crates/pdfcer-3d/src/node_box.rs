//! Each placement's bounding box in its own axes, for the bounding-box
//! render modes (ISO 32000-1 Table 308: "aligned with the axes of the local
//! coordinate space for that node").

use crate::{Matrix, TriangleMesh, transform_point};

/// A placed node's bounding box: the box around its tessellation's points
/// in the tessellation's own axes, carried into model space by the
/// placement. A box is per placement (a leaf of the model tree), not per
/// group node; the standard does not say which nodes have one.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[non_exhaustive]
pub struct NodeBox {
    /// The eight corners in model space: corner `i` takes the local
    /// maximum on x when bit 0 of `i` is set, y bit 1, z bit 2, else the
    /// minimum.
    pub corners: [[f64; 3]; 8],
    /// The placement it bounds, an index into the placements
    /// [`crate::AssembledModel::tree`]'s ranges index; `None` for a mesh
    /// drawn unplaced.
    pub placement: Option<usize>,
}

impl NodeBox {
    /// The box around every finite point `mesh`'s triangles use, carried by
    /// `matrix`; `None` when there is no such point.
    pub(crate) fn of(
        mesh: &TriangleMesh,
        matrix: &Matrix,
        placement: Option<usize>,
    ) -> Option<Self> {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        let points = mesh.triangles.iter().flatten();
        for p in points.filter_map(|&i| mesh.positions.get(i as usize)) {
            if p.iter().all(|c| c.is_finite()) {
                for ((l, h), &c) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
                    *l = l.min(c);
                    *h = h.max(c);
                }
            }
        }
        if lo.iter().any(|c| !c.is_finite()) {
            return None;
        }
        let mut corners = [[0.0; 3]; 8];
        for (i, corner) in corners.iter_mut().enumerate() {
            let mut local = lo;
            for (axis, c) in local.iter_mut().enumerate() {
                if i & (1 << axis) != 0 {
                    *c = hi.get(axis).copied().unwrap_or(*c);
                }
            }
            *corner = transform_point(matrix, local);
        }
        Some(NodeBox { corners, placement })
    }

    /// The twelve edges, as corner index pairs.
    pub const EDGES: [[usize; 2]; 12] = [
        [0, 1],
        [2, 3],
        [4, 5],
        [6, 7],
        [0, 2],
        [1, 3],
        [4, 6],
        [5, 7],
        [0, 4],
        [1, 5],
        [2, 6],
        [3, 7],
    ];

    /// The six faces as two triangles each, corner indices, outward
    /// counter-clockwise for a right-handed placement.
    #[cfg(feature = "render")]
    pub(crate) const TRIANGLES: [[u32; 3]; 12] = [
        [0, 2, 3],
        [0, 3, 1],
        [4, 5, 7],
        [4, 7, 6],
        [0, 1, 5],
        [0, 5, 4],
        [2, 6, 7],
        [2, 7, 3],
        [0, 4, 6],
        [0, 6, 2],
        [1, 3, 7],
        [1, 7, 5],
    ];
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)] // Tests fail loudly by design.
mod tests {
    use super::*;

    #[test]
    fn a_rotated_placement_carries_the_local_box() {
        let mesh = TriangleMesh {
            positions: vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 1.0, 3.0]],
            triangles: vec![[0, 1, 2]],
            ..TriangleMesh::default()
        };
        // A quarter turn about z, then 10 along x (columns are axes).
        let m: Matrix = [
            [0.0, -1.0, 0.0, 10.0],
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let b = NodeBox::of(&mesh, &m, Some(4)).unwrap_or_default();
        assert_eq!(b.placement, Some(4));
        assert_eq!(b.corners[0], transform_point(&m, [0.0, 0.0, 0.0]));
        assert_eq!(b.corners[7], transform_point(&m, [2.0, 1.0, 3.0]));
        assert!(NodeBox::of(&TriangleMesh::default(), &m, None).is_none());
    }
}
