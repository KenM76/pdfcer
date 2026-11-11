//! Which probes sit inside a solid, where they would see only the solid's
//! dark interior and dim the surfaces around them. Winding is untrusted, so
//! the test is a vote: a probe is inside a closed mesh when most of a fixed
//! set of rays cross that mesh an odd number of times, and it is unusable
//! when most rays hit something almost at once (embedded in a surface).

use std::collections::BTreeMap;

use super::sphere_directions;
use crate::TriangleMesh;
use crate::camera::pick::Ray;
use crate::camera::pick_index::PickIndex;

/// Rays per probe in the vote; odd, so a majority always exists.
const VOTE_RAYS: usize = 7;
/// A hit nearer than this share of the probe spacing counts as short.
const SHORT_HIT: f64 = 0.05;

/// Per mesh, whether it is closed: every edge, with corners matched by
/// position, borders exactly two triangles. Only a closed mesh has an
/// inside.
pub(crate) fn closed_meshes(meshes: &[TriangleMesh]) -> Vec<bool> {
    meshes.iter().map(is_closed).collect()
}

fn is_closed(mesh: &TriangleMesh) -> bool {
    let mut ids: BTreeMap<[u64; 3], u32> = BTreeMap::new();
    let mut welded = Vec::with_capacity(mesh.positions.len());
    for p in &mesh.positions {
        let next = ids.len() as u32;
        welded.push(*ids.entry(p.map(f64::to_bits)).or_insert(next));
    }
    let mut edges = Vec::with_capacity(mesh.triangles.len() * 3);
    for t in &mesh.triangles {
        let Some(c) = t
            .iter()
            .map(|&i| welded.get(i as usize).copied())
            .collect::<Option<Vec<u32>>>()
        else {
            return false;
        };
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            let (Some(&a), Some(&b)) = (c.get(a), c.get(b)) else {
                return false;
            };
            if a != b {
                edges.push((a.min(b), a.max(b)));
            }
        }
    }
    if edges.is_empty() {
        return false;
    }
    edges.sort_unstable();
    edges.chunk_by(|x, y| x == y).all(|run| run.len() == 2)
}

/// Whether a probe at `p` can be used: not inside a closed mesh and not
/// embedded in a surface. `far` bounds every ray; `spacing` is the
/// lattice's smallest probe spacing.
pub(crate) fn usable(
    index: &PickIndex,
    closed: &[bool],
    p: [f64; 3],
    spacing: f64,
    far: f64,
) -> bool {
    let mut votes: BTreeMap<usize, usize> = BTreeMap::new();
    let mut short = 0;
    let eps = 1e-9 * far;
    for direction in sphere_directions(VOTE_RAYS, true) {
        let ray = Ray {
            origin: p,
            direction,
            min_distance: eps,
        };
        if index
            .first_hit(&ray, far, |_| true, |_, _| false)
            .is_some_and(|h| h.distance < SHORT_HIT * spacing)
        {
            short += 1;
        }
        let mut odd: BTreeMap<usize, bool> = BTreeMap::new();
        let include = |m: usize| closed.get(m).copied().unwrap_or(false);
        let _ = index.transmittance(&ray, far, include, |h, _| {
            let e = odd.entry(h.mesh).or_insert(false);
            *e = !*e;
            1.0
        });
        for (mesh, is_odd) in odd {
            if is_odd {
                *votes.entry(mesh).or_insert(0) += 1;
            }
        }
    }
    let majority = VOTE_RAYS / 2 + 1;
    short < majority && votes.values().all(|&v| v < majority)
}
