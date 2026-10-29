//! The tessellation section (305): one `TESS_3D` (172) per primitive, each
//! a single `TESS_Face` (174) of plain triangles [WD 7.3.7, 7.8.5, 7.8.9].

use super::validate::Plan;
use super::{BitWriter, MeshScene, PrcWriteError, ScenePrimitive};
use crate::vec3;

const TESSELLATION: u32 = 305;
const TESS_3D: u32 = 172;
const TESS_FACE: u32 = 174;
/// `PRC_FACETESSDATA_Triangle` [WD 7.8.9.2].
const TRIANGLE: u32 = 0x2;

/// The written section and its counts.
pub(super) struct Section {
    pub(super) bytes: Vec<u8>,
    pub(super) triangles: usize,
    pub(super) flat_normals: usize,
}

/// Every planned primitive's mesh, in plan order.
pub(super) fn section(scene: &MeshScene, plan: &Plan) -> Result<Section, PrcWriteError> {
    let mut w = BitWriter::default();
    w.uint(TESSELLATION).uint(0).bit(true);
    w.uint(plan.tessellations.len() as u32);
    let (mut triangles, mut flat_normals) = (0, 0);
    for &(m, p) in &plan.tessellations {
        let Some(prim) = scene.meshes.get(m).and_then(|m| m.primitives.get(p)) else {
            continue;
        };
        triangles += prim.triangles.len();
        flat_normals += usize::from(prim.normals.is_empty());
        tess_3d(&mut w, prim);
    }
    w.uint(0);
    let bytes = w
        .finish()
        .ok_or(PrcWriteError::BadNumber("position or normal"))?;
    Ok(Section {
        bytes,
        triangles,
        flat_normals,
    })
}

/// The unit normal of triangle `t`, or +z for a degenerate one.
fn face_normal(p: &ScenePrimitive, t: [u32; 3]) -> [f64; 3] {
    let at = |i: u32| p.positions.get(i as usize).copied().unwrap_or([0.0; 3]);
    let (a, b, c) = (at(t[0]), at(t[1]), at(t[2]));
    let n = vec3::cross(vec3::sub(b, a), vec3::sub(c, a));
    vec3::normalize(n).unwrap_or([0.0, 0.0, 1.0])
}

fn tess_3d(w: &mut BitWriter, p: &ScenePrimitive) {
    // Not calculated; the coordinates.
    w.uint(TESS_3D).bit(false);
    w.uint(3 * p.positions.len() as u32);
    w.doubles(p.positions.iter().flatten().copied());
    // Has faces; no loops (8137 >= 7039); normals stored, not to be
    // recalculated (8137 >= 7047).
    w.bit(true).bit(false).bit(false);
    let flat = p.normals.is_empty();
    if flat {
        w.uint(3 * p.triangles.len() as u32);
        for &t in &p.triangles {
            w.doubles(face_normal(p, t));
        }
    } else {
        w.uint(3 * p.normals.len() as u32);
        w.doubles(p.normals.iter().flatten().copied());
    }
    // No wire; the (normal, point) offset pair of every corner.
    w.uint(0).uint(6 * p.triangles.len() as u32);
    for (k, t) in p.triangles.iter().enumerate() {
        for v in t {
            let n = if flat { k as u32 } else { *v };
            w.uint(3 * n).uint(3 * v);
        }
    }
    // One face: no line attributes or wires, plain triangles from index 0,
    // no textures, no vertex colours.
    w.uint(1).uint(TESS_FACE).uint(0).uint(0).uint(0);
    w.uint(TRIANGLE)
        .uint(0)
        .uint(1)
        .uint(p.triangles.len() as u32);
    w.uint(0).bit(false);
    // No texture coordinates.
    w.uint(0);
}
