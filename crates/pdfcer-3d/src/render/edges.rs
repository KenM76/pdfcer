//! A mesh's edges and vertices for the render modes that draw them, and the
//! per-frame silhouette test (ISO 32000-1 Table 307 `CV`).
//!
//! Positions are welded by their exact bits, so a mesh whose faces repeat
//! a shared corner still has one edge between them. Each edge keeps the
//! unit normals of the faces beside it, oriented as one consistent surface
//! (a face wound against its neighbour is flipped). An open edge, or one
//! three or more faces share, keeps its face's normal and its negation: it
//! always reads as one face towards the viewer and one away, so it is
//! always a silhouette.

use std::collections::HashMap;

use crate::TriangleMesh;
use crate::render::{at3, corners};
use crate::vec3::{add, cross, dot, normalize, scale, sub};

/// One welded edge.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Edge {
    pub(crate) ends: [[f64; 3]; 2],
    /// Each end's vertex normal (area-weighted face normals; zero when
    /// they cancel).
    pub(crate) vertex_normals: [[f64; 3]; 2],
    /// The two faces' unit normals.
    pub(crate) normals: [[f64; 3]; 2],
    /// Whether the faces meet at the crease angle or more, or the edge is
    /// open or non-manifold.
    pub(crate) crease: bool,
}

/// A welded vertex and its normal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Vertex {
    pub(crate) at: [f64; 3],
    pub(crate) normal: [f64; 3],
}

/// A mesh's welded edges and vertices, in first-use order.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct MeshEdges {
    pub(crate) edges: Vec<Edge>,
    pub(crate) vertices: Vec<Vertex>,
}

/// An edge while it is gathered: its welded ends, the first face's normal
/// and direction, the second face's normal, how many faces share it.
struct Gather {
    ends: [u32; 2],
    first: ([f64; 3], bool),
    second: Option<[f64; 3]>,
    faces: u32,
}

/// `mesh`'s edges and vertices; `crease_cos` as
/// [`crate::RenderMode`]'s crease angle sets [`Edge::crease`].
pub(crate) fn of(mesh: &TriangleMesh, crease_cos: f64) -> MeshEdges {
    let mut welder = Welder::default();
    for tri in &mesh.triangles {
        if let Some(world) = corners(mesh, tri) {
            welder.add(&world);
        }
    }
    welder.finish(crease_cos)
}

/// The edges and vertices gathered so far.
#[derive(Default)]
struct Welder {
    weld: HashMap<[u64; 3], u32>,
    points: Vec<[f64; 3]>,
    /// Per vertex, the sum of its faces' area vectors.
    sums: Vec<[f64; 3]>,
    index: HashMap<[u32; 2], usize>,
    gathered: Vec<Gather>,
}

impl Welder {
    /// Adds one triangle; a degenerate one adds nothing.
    fn add(&mut self, world: &[[f64; 3]; 3]) {
        let area = cross(
            sub(at3(world, 1), at3(world, 0)),
            sub(at3(world, 2), at3(world, 0)),
        );
        let Some(n) = normalize(area) else {
            return;
        };
        let ids = [0, 1, 2].map(|k| self.vertex(at3(world, k)));
        for &id in &ids {
            if let Some(s) = self.sums.get_mut(id as usize) {
                *s = add(*s, area);
            }
        }
        for k in 0..3 {
            let (a, b) = (at_id(ids, k), at_id(ids, (k + 1) % 3));
            if a != b {
                self.edge(a, b, n);
            }
        }
    }

    fn vertex(&mut self, p: [f64; 3]) -> u32 {
        let (points, sums) = (&mut self.points, &mut self.sums);
        *self.weld.entry(p.map(f64::to_bits)).or_insert_with(|| {
            points.push(p);
            sums.push([0.0; 3]);
            (points.len() - 1) as u32
        })
    }

    /// Records the edge from `a` to `b` of a face with normal `n`.
    fn edge(&mut self, a: u32, b: u32, n: [f64; 3]) {
        let key = [a.min(b), a.max(b)];
        let forward = a < b;
        match self.index.get(&key).and_then(|&i| self.gathered.get_mut(i)) {
            Some(g) => {
                g.faces += 1;
                if g.second.is_none() {
                    let consistent = forward != g.first.1;
                    g.second = Some(if consistent { n } else { scale(n, -1.0) });
                }
            }
            None => {
                self.index.insert(key, self.gathered.len());
                self.gathered.push(Gather {
                    ends: key,
                    first: (n, forward),
                    second: None,
                    faces: 1,
                });
            }
        }
    }

    fn finish(self, crease_cos: f64) -> MeshEdges {
        let normal_of = |id: u32| {
            self.sums
                .get(id as usize)
                .and_then(|&s| normalize(s))
                .unwrap_or([0.0; 3])
        };
        let point_of = |id: u32| self.points.get(id as usize).copied().unwrap_or([0.0; 3]);
        let edges = self
            .gathered
            .iter()
            .map(|g| {
                let n1 = g.first.0;
                let (n2, crease) = match g.second.filter(|_| g.faces == 2) {
                    Some(n2) => (n2, dot(n1, n2) <= crease_cos),
                    None => (scale(n1, -1.0), true),
                };
                Edge {
                    ends: g.ends.map(point_of),
                    vertex_normals: g.ends.map(normal_of),
                    normals: [n1, n2],
                    crease,
                }
            })
            .collect();
        let vertices = (0..self.points.len() as u32)
            .map(|id| Vertex {
                at: point_of(id),
                normal: normal_of(id),
            })
            .collect();
        MeshEdges { edges, vertices }
    }
}

fn at_id(ids: [u32; 3], k: usize) -> u32 {
    ids.get(k).copied().unwrap_or(0)
}

/// Whether a face with normal `n` faces a viewer along `v` (towards the
/// eye).
fn faces(n: [f64; 3], v: [f64; 3]) -> bool {
    dot(n, v) > 0.0
}

/// Whether `edge` is a silhouette seen along `v` (from its midpoint
/// towards the eye): one face towards the viewer and one away, or both
/// towards and their normals at least the crease angle apart. The line
/// shader's kind-1 test, the same expression.
pub(crate) fn is_silhouette(edge: &Edge, v: [f64; 3], crease_cos: f64) -> bool {
    let [n1, n2] = edge.normals;
    let (f1, f2) = (faces(n1, v), faces(n2, v));
    f1 != f2 || (f1 && dot(n1, n2) <= crease_cos)
}

/// The factor a shaded edge or vertex colour is scaled by: 0.25 plus 0.75
/// of how squarely its normal faces the viewer along `towards`. The
/// standard says these modes "apply lighting" without saying how; this is
/// a headlight, so the reading does not depend on the scheme.
pub(crate) fn shade_factor(normal: [f64; 3], towards: [f64; 3]) -> f64 {
    0.25 + 0.75 * dot(normal, towards).abs()
}

/// `rgba` with its colour scaled by `k`, alpha kept.
pub(crate) fn shaded(rgba: [u8; 4], k: f64) -> [u8; 4] {
    let [r, g, b, a] = rgba;
    let s = |c: u8| (f64::from(c) * k).round().clamp(0.0, 255.0) as u8;
    [s(r), s(g), s(b), a]
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)] // Tests fail loudly by design.
mod tests {
    use super::*;

    /// Two triangles sharing the edge (1,0,0)-(0,1,0), folded by `z`, with
    /// the shared corners repeated so only welding joins them.
    fn fold(z: f64, flip_second: bool) -> TriangleMesh {
        let second = if flip_second { [3, 5, 4] } else { [3, 4, 5] };
        TriangleMesh {
            positions: vec![
                [0., 0., 0.],
                [1., 0., 0.],
                [0., 1., 0.],
                [1., 0., 0.],
                [1., 1., z],
                [0., 1., 0.],
            ],
            triangles: vec![[0, 1, 2], second],
            ..TriangleMesh::default()
        }
    }

    fn shared(e: &MeshEdges) -> Edge {
        e.edges
            .iter()
            .copied()
            .find(|e| !e.crease || e.normals[1] != scale(e.normals[0], -1.0))
            .unwrap_or_else(|| e.edges[1])
    }

    #[test]
    fn welded_corners_join_two_faces_and_a_flat_fold_is_no_crease() {
        let e = of(&fold(0.0, false), 45f64.to_radians().cos());
        assert_eq!(e.vertices.len(), 4);
        assert_eq!(e.edges.len(), 5);
        let open = e.edges.iter().filter(|e| e.crease).count();
        assert_eq!(open, 4, "the outline is open");
        let s = shared(&e);
        assert!(!s.crease);
        assert_eq!(s.normals[0], s.normals[1]);
    }

    #[test]
    fn a_sharp_fold_is_a_crease_and_a_flipped_face_is_turned_back() {
        let cos = 45f64.to_radians().cos();
        let sharp = of(&fold(-2.0, false), cos);
        assert_eq!(sharp.edges.iter().filter(|e| e.crease).count(), 5);
        let flipped = of(&fold(0.0, true), cos);
        let s = shared(&flipped);
        assert_eq!(
            s.normals[0], s.normals[1],
            "wound against, read as one surface"
        );
    }

    #[test]
    fn silhouettes_follow_facing_and_the_crease_angle() {
        let edge = Edge {
            ends: [[0.0; 3]; 2],
            vertex_normals: [[0.0; 3]; 2],
            normals: [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0]],
            crease: true,
        };
        let cos = 45f64.to_radians().cos();
        assert!(is_silhouette(&edge, [0.0, 0.0, 1.0], cos), "one faces away");
        assert!(
            is_silhouette(&edge, [1.0, 0.0, 1.0], cos),
            "90° apart, both front"
        );
        assert!(
            !is_silhouette(&edge, [1.0, 0.0, 1.0], -0.5),
            "under a 120° crease"
        );
        assert!(!is_silhouette(&edge, [-1.0, 0.0, -1.0], cos), "both away");
    }
}
