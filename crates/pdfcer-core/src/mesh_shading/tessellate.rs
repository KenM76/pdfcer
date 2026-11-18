//! Tessellate type 6/7 patches into a triangle list.
//!
//! The same grid the renderer paints (`MSH32`): an `n × n` lattice in
//! `(u, v)`, `v` outer and `u` inner, two triangles per cell, geometry from
//! the tensor surface (`MSH28`) and colour bilinear over the corners
//! (§8.7.4.5.7). Only the cell size is the caller's.

use super::{MAX_TRIANGLES, MeshError, MeshPatch, MeshTriangle, MeshVertex};

/// The renderer's cap on cells per patch side.
const MAX_SUBDIVISION: u32 = 64;

/// How finely [`tessellate_patches`] cuts each patch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tessellation {
    cell: f64,
}

impl Tessellation {
    /// Cells of at most `cell` units on a side, in the shading's target
    /// space, up to 64 per patch side. A non-finite or non-positive size
    /// reads as the default.
    #[must_use]
    pub fn with_cell_size(cell: f64) -> Self {
        if cell.is_finite() && cell > 0.0 {
            Self { cell }
        } else {
            Self::default()
        }
    }

    /// The cell size in force.
    #[must_use]
    pub const fn cell_size(&self) -> f64 {
        self.cell
    }

    /// Cells per side for a patch whose control points span `extent`.
    fn cells_for(&self, extent: f64) -> u32 {
        if !extent.is_finite() || extent <= 0.0 {
            return 1;
        }
        let n = (extent / self.cell).ceil().min(f64::from(MAX_SUBDIVISION));
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        // clamped to 1..=64 above
        let n = n as u32;
        n.clamp(1, MAX_SUBDIVISION)
    }
}

impl Default for Tessellation {
    /// One unit per cell: the renderer's density (4 device pixels a cell)
    /// at 400% zoom, so a tessellated mesh is never coarser than what the
    /// renderer draws at that zoom or below.
    fn default() -> Self {
        Self { cell: 1.0 }
    }
}

/// Cut patches into triangles, in paint order.
///
/// # Errors
///
/// [`MeshError::TooManyTriangles`] when the result would pass `cap`
/// (clamped to [`MAX_TRIANGLES`](super::MAX_TRIANGLES)); nothing partial
/// is returned.
pub fn tessellate_patches(
    patches: &[MeshPatch],
    tessellation: Tessellation,
    cap: usize,
) -> Result<Vec<MeshTriangle>, MeshError> {
    let cap = cap.min(MAX_TRIANGLES);
    let mut out: Vec<MeshTriangle> = Vec::new();
    for patch in patches {
        let n = tessellation.cells_for(extent(patch));
        let cells = (n as usize) * (n as usize) * 2;
        if out.len().saturating_add(cells) > cap {
            return Err(MeshError::TooManyTriangles { cap });
        }
        let side = n as usize + 1;
        let mut grid: Vec<MeshVertex> = Vec::with_capacity(side * side);
        for jv in 0..=n {
            let v = f64::from(jv) / f64::from(n);
            for iu in 0..=n {
                let u = f64::from(iu) / f64::from(n);
                grid.push(MeshVertex {
                    xy: surface_at(patch, u, v),
                    color: color_at(patch, u, v),
                });
            }
        }
        let rows: Vec<&[MeshVertex]> = grid.chunks_exact(side).collect();
        for pair in rows.windows(2) {
            let [row, below] = pair else { continue };
            for (top, bottom) in row.windows(2).zip(below.windows(2)) {
                let ([a, b], [c, d]) = (top, bottom) else {
                    continue;
                };
                out.push([a.clone(), b.clone(), c.clone()]);
                out.push([b.clone(), c.clone(), d.clone()]);
            }
        }
    }
    Ok(out)
}

/// The larger side of the control points' bounding box.
fn extent(patch: &MeshPatch) -> f64 {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for pt in patch.p.iter().flatten() {
        lo = [lo[0].min(pt[0]), lo[1].min(pt[1])];
        hi = [hi[0].max(pt[0]), hi[1].max(pt[1])];
    }
    (hi[0] - lo[0]).max(hi[1] - lo[1])
}

/// The cubic Bernstein basis (`MSH28`).
fn bernstein(t: f64) -> [f64; 4] {
    let s = 1.0 - t;
    [s * s * s, 3.0 * t * s * s, 3.0 * t * t * s, t * t * t]
}

/// `S(u, v) = Σ Σ pᵢⱼ Bᵢ(u) Bⱼ(v)` (`MSH28`).
fn surface_at(patch: &MeshPatch, u: f64, v: f64) -> [f64; 2] {
    let bu = bernstein(u);
    let bv = bernstein(v);
    let mut out = [0.0; 2];
    for (col, bui) in patch.p.iter().zip(bu) {
        for (pt, bvj) in col.iter().zip(bv) {
            let w = bui * bvj;
            out[0] = w.mul_add(pt[0], out[0]);
            out[1] = w.mul_add(pt[1], out[1]);
        }
    }
    out
}

/// Bilinear colour, corners in `MSH24` order `c00 c03 c33 c30`.
fn color_at(patch: &MeshPatch, u: f64, v: f64) -> Vec<f64> {
    let [c00, c03, c33, c30] = &patch.corner;
    let lerp = |a: f64, b: f64, t: f64| t.mul_add(b - a, a);
    c00.iter()
        .zip(c03)
        .zip(c33.iter().zip(c30))
        .map(|((&a, &b), (&c, &d))| lerp(lerp(a, b, v), lerp(d, c, v), u))
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn flat_patch(size: f64, colours: [f64; 4]) -> MeshPatch {
        let mut p = [[[0.0; 2]; 4]; 4];
        for (i, col) in p.iter_mut().enumerate() {
            for (j, pt) in col.iter_mut().enumerate() {
                *pt = [size * i as f64 / 3.0, size * j as f64 / 3.0];
            }
        }
        MeshPatch {
            p,
            corner: colours.map(|c| vec![c]),
        }
    }

    #[test]
    fn a_planar_patch_tessellates_onto_its_own_bilinear_colours() {
        let tris = tessellate_patches(
            &[flat_patch(4.0, [0.0, 1.0, 1.0, 0.0])],
            Tessellation::default(),
            MAX_TRIANGLES,
        )
        .unwrap();
        assert_eq!(tris.len(), 4 * 4 * 2);
        for v in tris.iter().flatten() {
            // c00 = c30 = 0, c03 = c33 = 1: colour is v, and v is y / 4.
            assert!((v.color[0] - v.xy[1] / 4.0).abs() < 1e-12, "{v:?}");
        }
    }

    #[test]
    fn past_the_cap_tessellation_declines() {
        let err = tessellate_patches(&[flat_patch(400.0, [0.0; 4])], Tessellation::default(), 100)
            .unwrap_err();
        assert_eq!(err, MeshError::TooManyTriangles { cap: 100 });
    }
}
