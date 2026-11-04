//! Model-space matrices and their application to meshes.

/// A 4×4 matrix, `m[row][col]`, acting on column vectors.
pub type Matrix = [[f64; 4]; 4];

/// The identity matrix.
pub const IDENTITY: Matrix = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// `a × b`.
pub fn multiply(a: &Matrix, b: &Matrix) -> Matrix {
    let mut m = [[0.0; 4]; 4];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..4)
                .map(|k| {
                    a.get(i).and_then(|r| r.get(k)).copied().unwrap_or(0.0)
                        * b.get(k).and_then(|r| r.get(j)).copied().unwrap_or(0.0)
                })
                .sum();
        }
    }
    m
}

/// `m × (p, 1)`, divided by the homogeneous coordinate when it is neither
/// 0 nor 1.
pub fn transform_point(m: &Matrix, p: [f64; 3]) -> [f64; 3] {
    let v = [p[0], p[1], p[2], 1.0];
    let row = |i: usize| -> f64 {
        m.get(i)
            .map_or(0.0, |r| r.iter().zip(v).map(|(a, b)| a * b).sum())
    };
    let w = row(3);
    let s = if w == 0.0 || w == 1.0 { 1.0 } else { 1.0 / w };
    [row(0) * s, row(1) * s, row(2) * s]
}

/// The determinant of rows `rows` and columns `cols` of `m`.
fn minor3(m: &Matrix, rows: [usize; 3], cols: [usize; 3]) -> f64 {
    let a = |i: usize, j: usize| {
        let (r, c) = (rows.get(i).copied(), cols.get(j).copied());
        r.zip(c)
            .and_then(|(r, c)| m.get(r).and_then(|row| row.get(c)))
            .copied()
            .unwrap_or(0.0)
    };
    a(0, 0) * (a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1))
        - a(0, 1) * (a(1, 0) * a(2, 2) - a(1, 2) * a(2, 0))
        + a(0, 2) * (a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0))
}

/// A value whose sign is the orientation of the point map `m`: the 4×4
/// determinant, or the upper 3×3 one when that is 0 or not a number.
fn orientation(m: &Matrix) -> f64 {
    let last = |j: usize| m.get(3).and_then(|r| r.get(j)).copied().unwrap_or(0.0);
    let det4: f64 = (0..4)
        .map(|j| {
            let cols = match j {
                0 => [1, 2, 3],
                1 => [0, 2, 3],
                2 => [0, 1, 3],
                _ => [0, 1, 2],
            };
            // Cofactor sign (-1)^(3 + j) for row 3.
            let sign = if j % 2 == 1 { 1.0 } else { -1.0 };
            sign * last(j) * minor3(m, [0, 1, 2], cols)
        })
        .sum();
    if det4 == 0.0 || det4.is_nan() {
        minor3(m, [0, 1, 2], [0, 1, 2])
    } else {
        det4
    }
}

impl crate::TriangleMesh {
    /// This mesh with every position mapped through `m` and every stored
    /// normal through its inverse transpose (renormalised); a mirroring `m`
    /// reverses each triangle so the outside stays counter-clockwise.
    ///
    /// Mirroring is the sign of the full 4×4 determinant: the point map
    /// `p ↦ (Ap + b) / w` has Jacobian determinant `det(m) / w⁴`, so its
    /// orientation is that sign wherever `w ≠ 0`, whatever the sign of `w`.
    /// A last row of zeros (no divide, see [`transform_point`]) uses the
    /// upper 3×3 determinant instead.
    ///
    /// ```
    /// # use pdfcer_3d::{IDENTITY, TriangleMesh};
    /// let mut mirror = IDENTITY;
    /// mirror[0][0] = -1.0;
    /// let mut mesh = TriangleMesh::default();
    /// mesh.positions = vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    /// mesh.triangles = vec![[0, 1, 2]];
    /// let placed = mesh.transformed(&mirror);
    /// assert_eq!(placed.positions[0], [-1.0, 0.0, 0.0]);
    /// assert_eq!(placed.triangles[0], [0, 2, 1]);
    /// ```
    #[must_use]
    pub fn transformed(&self, m: &Matrix) -> Self {
        let mut out = self.clone();
        for p in &mut out.positions {
            *p = transform_point(m, *p);
        }
        let a = |i: usize, j: usize| m.get(i).and_then(|r| r.get(j)).copied().unwrap_or(0.0);
        let det = orientation(m);
        // Normals take the inverse transpose; the cofactor matrix is det
        // times it, so its sign is corrected and the result renormalised.
        let cof = |i: usize, j: usize| {
            let (r0, r1) = ((i + 1) % 3, (i + 2) % 3);
            let (c0, c1) = ((j + 1) % 3, (j + 2) % 3);
            a(r0, c0) * a(r1, c1) - a(r0, c1) * a(r1, c0)
        };
        let sign = if det < 0.0 { -1.0 } else { 1.0 };
        for n in &mut out.normals {
            let v = [0, 1, 2].map(|i| {
                sign * n
                    .iter()
                    .enumerate()
                    .map(|(j, c)| cof(i, j) * c)
                    .sum::<f64>()
            });
            let len = v.iter().map(|c| c * c).sum::<f64>().sqrt();
            *n = if len > 0.0 && len.is_finite() {
                v.map(|c| c / len)
            } else {
                v
            };
        }
        if det < 0.0 {
            for t in &mut out.triangles {
                t.swap(1, 2);
            }
            for t in &mut out.triangle_normals {
                t.swap(1, 2);
            }
            for t in out.triangle_uvs.iter_mut().flatten().flatten() {
                t.swap(1, 2);
            }
        }
        out
    }
}

#[cfg(test)]
// A wrong index is the test failure itself.
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::TriangleMesh;

    fn triangle(z: f64) -> TriangleMesh {
        TriangleMesh {
            positions: vec![[0.0, 0.0, z], [1.0, 0.0, z], [0.0, 1.0, z]],
            normals: vec![[0.0, 0.0, 1.0]],
            triangles: vec![[0, 1, 2]],
            ..TriangleMesh::default()
        }
    }

    fn with_last_row(row: [f64; 4]) -> Matrix {
        let mut m = IDENTITY;
        m[3] = row;
        m
    }

    #[test]
    fn a_negative_uniform_w_mirrors_and_flips_the_normal() {
        let placed = triangle(0.0).transformed(&with_last_row([0.0, 0.0, 0.0, -1.0]));
        assert_eq!(placed.positions[1], [-1.0, 0.0, 0.0]);
        assert_eq!(placed.triangles[0], [0, 2, 1]);
        assert_eq!(placed.normals[0], [0.0, 0.0, -1.0]);
    }

    /// `w = z - 3` has 4×4 determinant -3, so the map mirrors on both sides
    /// of `w = 0`: the sign of `w` does not decide it.
    #[test]
    fn a_projective_row_flips_by_its_determinant_on_both_sides_of_w_zero() {
        let m = with_last_row([0.0, 0.0, 1.0, -3.0]);
        for z in [1.0, 4.0] {
            assert_eq!(
                triangle(z).transformed(&m).triangles[0],
                [0, 2, 1],
                "z = {z}"
            );
        }
        // `w = 3 - z`: determinant +3, negative `w` past z = 3, no flip.
        let m = with_last_row([0.0, 0.0, -1.0, 3.0]);
        for z in [1.0, 4.0] {
            assert_eq!(
                triangle(z).transformed(&m).triangles[0],
                [0, 1, 2],
                "z = {z}"
            );
        }
    }

    #[test]
    fn a_zero_last_row_keeps_the_3x3_rule() {
        let mut m = with_last_row([0.0; 4]);
        assert_eq!(triangle(1.0).transformed(&m).triangles[0], [0, 1, 2]);
        m[0][0] = -1.0;
        assert_eq!(triangle(1.0).transformed(&m).triangles[0], [0, 2, 1]);
    }
}
