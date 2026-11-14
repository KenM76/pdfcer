//! Types 4–7 as Gouraud triangles, from the raster's own triangles and
//! patch grid.

use tiny_skia::Transform;

use super::{COLOUR_TOL, Decline, Gouraud, lerp3, max_diff};
use crate::mesh::{Mesh, MeshData, Shade, map, patch_device_extent, subdivision_for};
use crate::shading::ColorRamp;

/// The deepest a parametric mesh triangle is split (4ⁿ pieces).
const MAX_DEPTH: u32 = 4;

/// Types 4–7: the raster's own triangles and patch grid.
pub(super) fn mesh_triangles(
    mesh: &Mesh,
    ramp: Option<&ColorRamp>,
    to_device: Transform,
    max: usize,
) -> Result<Gouraud, Decline> {
    if ramp.is_some_and(|r| !r.is_complete()) {
        return Err(Decline::IncompleteRamp);
    }
    let mut out = Gouraud::default();
    each_triangle(mesh, to_device, |p, s| shaded(&mut out, p, s, ramp, 0, max))?;
    Ok(out)
}

/// Every triangle of `mesh` in device space with its corner shades, in
/// paint order: a triangle mesh's own, or each patch's grid cut in two.
pub(super) fn each_triangle(
    mesh: &Mesh,
    to_device: Transform,
    mut f: impl FnMut([[f32; 2]; 3], [Shade; 3]) -> Result<(), Decline>,
) -> Result<(), Decline> {
    match &mesh.data {
        MeshData::Triangles(tris) => {
            for tri in tris {
                f(tri.xy.map(|p| map(to_device, p)), tri.shade)?;
            }
        }
        MeshData::Patches(patches) => {
            for patch in patches {
                let n = subdivision_for(patch_device_extent(patch, to_device));
                let side = (n + 1) as usize;
                let mut grid: Vec<([f32; 2], Shade)> = Vec::with_capacity(side * side);
                #[allow(clippy::cast_precision_loss)]
                for jv in 0..=n {
                    let v = jv as f32 / n as f32;
                    for iu in 0..=n {
                        let u = iu as f32 / n as f32;
                        grid.push((map(to_device, patch.at(u, v)), patch.shade_at(u, v)));
                    }
                }
                for row in 0..n as usize {
                    for col in 0..n as usize {
                        let i = row * side + col;
                        let at = |k: usize| grid.get(k).copied();
                        let (Some(a), Some(b), Some(c), Some(d)) =
                            (at(i), at(i + 1), at(i + side), at(i + side + 1))
                        else {
                            continue;
                        };
                        f([a.0, b.0, c.0], [a.1, b.1, c.1])?;
                        f([b.0, c.0, d.0], [b.1, c.1, d.1])?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Emit one mesh triangle, splitting a parametric one until the ramp is
/// linear across it. A ramp hole leaves the triangle unpainted, as the
/// raster leaves its pixels.
fn shaded(
    out: &mut Gouraud,
    p: [[f32; 2]; 3],
    s: [Shade; 3],
    ramp: Option<&ColorRamp>,
    depth: u32,
    max: usize,
) -> Result<(), Decline> {
    let rgb = |x: Shade| x.resolve(ramp).map(|c| [c.r, c.g, c.b]);
    let (Some(a), Some(b), Some(c)) = (rgb(s[0]), rgb(s[1]), rgb(s[2])) else {
        return Ok(());
    };
    let parametric = s.iter().all(|x| matches!(x, Shade::Parametric(_)));
    let mid = |(pi, si): ([f32; 2], Shade), (pj, sj): ([f32; 2], Shade)| {
        (
            [(pi[0] + pj[0]) / 2.0, (pi[1] + pj[1]) / 2.0],
            si.lerp(sj, 0.5),
        )
    };
    let (v0, v1, v2) = ((p[0], s[0]), (p[1], s[1]), (p[2], s[2]));
    let (ab, bc, ca) = (mid(v0, v1), mid(v1, v2), mid(v2, v0));
    let bent = parametric
        && depth < MAX_DEPTH
        && [(ab.1, a, b), (bc.1, b, c), (ca.1, c, a)]
            .iter()
            .any(|&(m, x, y)| rgb(m).is_none_or(|m| max_diff(m, lerp3(x, y, 0.5)) > COLOUR_TOL));
    if !bent {
        return out.push(p, [a, b, c], max);
    }
    let d = depth + 1;
    shaded(out, [p[0], ab.0, ca.0], [s[0], ab.1, ca.1], ramp, d, max)?;
    shaded(out, [ab.0, p[1], bc.0], [ab.1, s[1], bc.1], ramp, d, max)?;
    shaded(out, [ca.0, bc.0, p[2]], [ca.1, bc.1, s[2]], ramp, d, max)?;
    shaded(out, [ab.0, bc.0, ca.0], [ab.1, bc.1, ca.1], ramp, d, max)
}
