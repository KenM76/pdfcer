//! Mesh export: binary STL and Wavefront OBJ.
//!
//! Both write coordinates as stored, in the file's units, without the
//! representation item's placement. A triangle whose index is out of range
//! is skipped.

use std::fmt::Write as _;

use crate::vec3::sub;
use crate::{PrcError, TriangleMesh};

fn corners(m: &TriangleMesh) -> impl Iterator<Item = [[f64; 3]; 3]> + '_ {
    m.triangles.iter().filter_map(|t| {
        Some([
            *m.positions.get(t[0] as usize)?,
            *m.positions.get(t[1] as usize)?,
            *m.positions.get(t[2] as usize)?,
        ])
    })
}

/// The unit normal of a counter-clockwise triangle; zero when degenerate.
fn normal([a, b, c]: [[f64; 3]; 3]) -> [f64; 3] {
    let (u, v) = (sub(b, a), sub(c, a));
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len > 0.0 && len.is_finite() {
        [n[0] / len, n[1] / len, n[2] / len]
    } else {
        [0.0; 3]
    }
}

/// Binary STL of every mesh's triangles, with facet normals computed from
/// the winding. Coordinates narrow to `f32`, as the format requires.
///
/// # Errors
/// [`PrcError::TooLarge`] past `u32::MAX` triangles, the format's count
/// field.
///
/// # Examples
/// ```
/// let stl = pdfcer_3d::to_stl(&[]).unwrap();
/// assert_eq!(stl.len(), 84);
/// ```
pub fn to_stl(meshes: &[TriangleMesh]) -> Result<Vec<u8>, PrcError> {
    let n: usize = meshes.iter().map(|m| corners(m).count()).sum();
    let count = u32::try_from(n).map_err(|_| PrcError::TooLarge {
        limit: u32::MAX as usize,
    })?;
    let mut out = Vec::with_capacity(84 + 50 * n);
    let mut header = [b' '; 80];
    let tag = b"pdfcer PRC tessellation";
    header.iter_mut().zip(tag).for_each(|(h, t)| *h = *t);
    out.extend_from_slice(&header);
    out.extend_from_slice(&count.to_le_bytes());
    for m in meshes {
        for tri in corners(m) {
            for v in std::iter::once(normal(tri)).chain(tri) {
                for c in v {
                    out.extend_from_slice(&(c as f32).to_le_bytes());
                }
            }
            out.extend_from_slice(&[0, 0]);
        }
    }
    Ok(out)
}

/// Wavefront OBJ: one `o` object per mesh and one `g` group per face, with
/// 1-based vertex indices; a mesh's stored normals are written as `vn` and
/// each corner names its own (`f v//vn`).
///
/// # Examples
/// ```
/// assert_eq!(pdfcer_3d::to_obj(&[]), "# pdfcer PRC tessellation\n");
/// ```
pub fn to_obj(meshes: &[TriangleMesh]) -> String {
    let mut s = String::from("# pdfcer PRC tessellation\n");
    let (mut base, mut nbase) = (1usize, 1usize);
    for (i, m) in meshes.iter().enumerate() {
        let _ = writeln!(s, "o mesh{i}");
        for [x, y, z] in &m.positions {
            let _ = writeln!(s, "v {x} {y} {z}");
        }
        let with_normals = m.triangle_normals.len() == m.triangles.len();
        if with_normals {
            for [x, y, z] in &m.normals {
                // `+ 0.0` prints a negative zero as `0`.
                let _ = writeln!(s, "vn {} {} {}", x + 0.0, y + 0.0, z + 0.0);
            }
        }
        let in_range = |t: &[u32; 3]| t.iter().all(|&k| (k as usize) < m.positions.len());
        let face = |s: &mut String, k: usize| {
            let Some(t) = m.triangles.get(k).filter(|t| in_range(t)) else {
                return;
            };
            let n = m
                .triangle_normals
                .get(k)
                .filter(|n| with_normals && n.iter().all(|&j| (j as usize) < m.normals.len()));
            s.push('f');
            for c in 0..3 {
                let v = base + t.get(c).copied().unwrap_or(0) as usize;
                let _ = match n.and_then(|n| n.get(c)) {
                    Some(&j) => write!(s, " {v}//{}", nbase + j as usize),
                    None => write!(s, " {v}"),
                };
            }
            s.push('\n');
        };
        if m.faces.is_empty() {
            (0..m.triangles.len()).for_each(|k| face(&mut s, k));
        }
        for (j, r) in m.faces.iter().enumerate() {
            let _ = writeln!(s, "g mesh{i}_face{j}");
            r.clone()
                .filter(|&k| k < m.triangles.len())
                .for_each(|k| face(&mut s, k));
        }
        base += m.positions.len();
        if with_normals {
            nbase += m.normals.len();
        }
    }
    s
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn quad() -> TriangleMesh {
        TriangleMesh {
            positions: vec![[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]],
            triangles: vec![[0, 1, 2], [0, 2, 3], [0, 9, 1]],
            faces: vec![0..1, 1..3],
            normals_recalculated: false,
            normals: Vec::new(),
            triangle_normals: Vec::new(),
            triangle_graphics: Vec::new(),
        }
    }

    fn f32_at(b: &[u8], at: usize) -> f32 {
        f32::from_le_bytes(b[at..at + 4].try_into().unwrap())
    }

    #[test]
    fn stl_counts_valid_triangles_and_computes_normals() {
        let stl = to_stl(&[quad(), quad()]).unwrap();
        assert_eq!(&stl[..6], b"pdfcer");
        assert_eq!(u32::from_le_bytes(stl[80..84].try_into().unwrap()), 4);
        assert_eq!(stl.len(), 84 + 4 * 50);
        // First facet: normal +Z, then vertex 2 = (1, 1, 0).
        assert_eq!(f32_at(&stl, 84 + 8), 1.0);
        assert_eq!(f32_at(&stl, 84 + 36), 1.0);
        assert_eq!(f32_at(&stl, 84 + 40), 1.0);
        assert_eq!(&stl[84 + 48..84 + 50], &[0, 0]);
    }

    #[test]
    fn a_degenerate_triangle_has_a_zero_normal() {
        assert_eq!(normal([[1., 1., 1.]; 3]), [0.0; 3]);
        assert_eq!(
            normal([[0., 0., 0.], [0., 1., 0.], [1., 0., 0.]]),
            [0., 0., -1.]
        );
    }

    #[test]
    fn obj_offsets_indices_per_mesh_and_groups_faces() {
        let obj = to_obj(&[quad(), quad()]);
        let lines: Vec<&str> = obj.lines().collect();
        assert_eq!(lines.iter().filter(|l| l.starts_with("v ")).count(), 8);
        assert_eq!(lines[2], "v 0 0 0");
        assert!(lines.contains(&"g mesh0_face1"));
        let faces: Vec<&&str> = lines.iter().filter(|l| l.starts_with("f ")).collect();
        assert_eq!(faces, [&"f 1 2 3", &"f 1 3 4", &"f 5 6 7", &"f 5 7 8"]);
    }

    #[test]
    fn obj_writes_stored_normals_and_offsets_them_per_mesh() {
        let mut m = quad();
        m.normals = vec![[0., 0., 1.], [0., 0., -1.]];
        m.triangle_normals = vec![[0, 1, 0], [1, 1, 0], [0, 0, 0]];
        let obj = to_obj(&[quad(), m.clone(), m]);
        let lines: Vec<&str> = obj.lines().collect();
        assert_eq!(lines.iter().filter(|l| l.starts_with("vn ")).count(), 4);
        assert!(lines.contains(&"vn 0 0 -1"));
        let faces: Vec<&&str> = lines.iter().filter(|l| l.starts_with("f ")).collect();
        assert_eq!(
            faces[0], &"f 1 2 3",
            "a mesh without normals keeps plain corners"
        );
        assert_eq!(faces[2], &"f 5//1 6//2 7//1");
        assert_eq!(faces[5], &"f 9//4 11//4 12//3");
    }

    #[test]
    fn obj_without_faces_lists_every_triangle() {
        let mut m = quad();
        m.faces.clear();
        let obj = to_obj(&[m]);
        assert_eq!(obj.lines().filter(|l| l.starts_with("f ")).count(), 2);
        assert!(!obj.contains("g "));
    }
}
