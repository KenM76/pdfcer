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

impl crate::TriangleMesh {
    /// This mesh with every position mapped through `m` and every stored
    /// normal through its inverse transpose (renormalised); a mirroring `m`
    /// (negative determinant) reverses each triangle so the outside stays
    /// counter-clockwise.
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
        let det = a(0, 0) * (a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1))
            - a(0, 1) * (a(1, 0) * a(2, 2) - a(1, 2) * a(2, 0))
            + a(0, 2) * (a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0));
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
