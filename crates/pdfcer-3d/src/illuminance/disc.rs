//! Where a reading's samples land on the picked mesh.

use super::IlluminanceQuery;
use crate::camera::pick::{Hit, Ray};
use crate::camera::pick_index::PickIndex;
use crate::vec3::{add, cross, dot, normalize, scale};

/// A sample is kept when its face is within 60 degrees of the disc's normal.
const MIN_FACING: f64 = 0.5;
/// How far from its plane a point may be and still be on the surface, as a
/// share of the model's size.
const TOLERANCE: f64 = 1e-5;

/// A point on the mesh and the unit normal of the side measured.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Spot {
    pub(super) point: [f64; 3],
    pub(super) normal: [f64; 3],
}

/// The spots `query` measures, `asked` of them at most (1 for a point):
/// the centre first, then the disc filled evenly (a sunflower pattern).
/// `normal` is the query's, unit; `far` exceeds the model's size. `None`
/// when the centre is not on the mesh.
pub(super) fn spots(
    index: &PickIndex,
    far: f64,
    query: &IlluminanceQuery,
    normal: [f64; 3],
    asked: usize,
) -> Option<Vec<Spot>> {
    let tolerance = TOLERANCE * far;
    let centre = land(index, query.mesh, query.point, normal, tolerance)?;
    // The side of the face the query's normal is on.
    let side = if dot(centre.normal, normal) < 0.0 {
        -1.0
    } else {
        1.0
    };
    let spot = |hit: &Hit| Spot {
        point: hit.point,
        normal: scale(hit.normal, side),
    };
    if asked <= 1 {
        return Some(vec![spot(&centre)]);
    }
    let (u, v) = tangents(normal)?;
    let reach = query.radius.max(tolerance);
    // The golden angle: successive samples never line up.
    const TURN: f64 = 2.399_963_229_728_653;
    let mut out = Vec::with_capacity(asked);
    for i in 0..asked {
        let r = query.radius * ((i as f64 + 0.5) / asked as f64).sqrt();
        let (s, c) = (TURN * i as f64).sin_cos();
        let q = add(query.point, add(scale(u, r * c), scale(v, r * s)));
        let kept = land(index, query.mesh, q, normal, reach)
            .filter(|hit| side * dot(hit.normal, normal) >= MIN_FACING);
        out.extend(kept.as_ref().map(spot));
    }
    Some(out)
}

/// Where `q` meets mesh `mesh` along `n`, either way, within `reach`: the
/// nearer of the two.
fn land(index: &PickIndex, mesh: usize, q: [f64; 3], n: [f64; 3], reach: f64) -> Option<Hit> {
    // Each ray starts a hair on the far side of `q`, so a point on the
    // surface is found by both.
    let hair = reach * 1e-6;
    let cast = |way: f64| {
        let ray = Ray {
            origin: add(q, scale(n, -way * hair)),
            direction: scale(n, way),
            min_distance: 0.0,
        };
        index.pick_where(&ray, reach + hair, |m| m == mesh)
    };
    match (cast(-1.0), cast(1.0)) {
        (Some(a), Some(b)) => Some(if a.distance <= b.distance { a } else { b }),
        (a, b) => a.or(b),
    }
}

/// Two unit vectors square to unit `n` and to each other.
fn tangents(n: [f64; 3]) -> Option<([f64; 3], [f64; 3])> {
    let axis = if n[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let u = normalize(cross(n, axis))?;
    Some((u, cross(n, u)))
}
