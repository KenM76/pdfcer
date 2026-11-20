//! The index as flat arrays for a GPU traversal: two texels per node and
//! the triangles in leaf order, positions less a local origin so f32 keeps
//! their precision.

use super::{Node, PickIndex};

/// A node's flat texels: `[lo, a]` and `[hi, b]`. An inner node has
/// `a` = its left child and `b` = its right; a leaf has `a` = −1 − its
/// first triangle and `b` = one past its last.
pub(crate) type FlatNode = [[f32; 4]; 2];

/// One triangle in leaf order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FlatPrim {
    pub(crate) mesh: usize,
    pub(crate) triangle: usize,
    /// Corners less the local origin.
    pub(crate) corners: [[f32; 3]; 3],
}

impl PickIndex {
    /// The nodes (root first) and triangles, positions less `origin`.
    pub(crate) fn flatten(&self, origin: [f64; 3]) -> (Vec<FlatNode>, Vec<FlatPrim>) {
        let local = |p: [f64; 3]| {
            let [x, y, z] = p;
            [
                (x - origin[0]) as f32,
                (y - origin[1]) as f32,
                (z - origin[2]) as f32,
            ]
        };
        let texel = |p: [f64; 3], w: usize, leaf: bool| {
            let [x, y, z] = local(p);
            let w = if leaf { -1.0 - w as f32 } else { w as f32 };
            [x, y, z, w]
        };
        let nodes = self
            .nodes
            .iter()
            .map(|node| match *node {
                Node::Inner {
                    bounds,
                    left,
                    right,
                } => [
                    texel(bounds.lo, left, false),
                    texel(bounds.hi, right, false),
                ],
                Node::Leaf { bounds, start, end } => {
                    [texel(bounds.lo, start, true), texel(bounds.hi, end, false)]
                }
            })
            .collect();
        let prims = self
            .prims
            .iter()
            .map(|p| FlatPrim {
                mesh: p.mesh,
                triangle: p.triangle,
                corners: p.corners.map(local),
            })
            .collect();
        (nodes, prims)
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use super::super::tests::{rays, scatter};
    use super::*;

    type V = [f32; 3];
    fn sub(a: V, b: V) -> V {
        [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
    }
    fn dot(a: V, b: V) -> f32 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }
    fn cross(a: V, b: V) -> V {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }

    /// `scene_crosses` of the trace shader.
    fn crosses(lo: V, hi: V, o: V, d: V, (mut t0, mut t1): (f32, f32), far: f32) -> bool {
        for k in 0..3 {
            if d[k] == 0.0 {
                if o[k] < lo[k] || o[k] > hi[k] {
                    return false;
                }
                continue;
            }
            let (mut a, mut c) = ((lo[k] - o[k]) / d[k], (hi[k] - o[k]) / d[k]);
            if a > c {
                std::mem::swap(&mut a, &mut c);
            }
            let slack = 1e-5 * a.abs().max(c.abs()).max(far);
            t0 = t0.max(a - slack);
            t1 = t1.min(c + slack);
            if t0 > t1 {
                return false;
            }
        }
        true
    }

    /// `scene_intersect` of the trace shader.
    fn intersect(p: &FlatPrim, o: V, d: V, tmin: f32) -> Option<f32> {
        let [c0, c1, c2] = p.corners;
        let (e1, e2) = (sub(c1, c0), sub(c2, c0));
        let pv = cross(d, e2);
        let det = dot(e1, pv);
        if det.abs() < f32::EPSILON * dot(e1, e1).max(dot(e2, e2)) {
            return None;
        }
        let s = sub(o, c0);
        let u = dot(s, pv) / det;
        let q = cross(s, e1);
        let v = dot(d, q) / det;
        let t = dot(e2, q) / det;
        ((0.0..=1.0).contains(&u) && v >= 0.0 && u + v <= 1.0 && t >= tmin).then_some(t)
    }

    /// `scene_first_hit` of the trace shader, every triangle opaque.
    fn first_hit(nodes: &[FlatNode], prims: &[FlatPrim], o: V, d: V, far: f32) -> Option<usize> {
        let (mut stack, mut best, mut limit) = (vec![0usize], None, far);
        while let Some(node) = stack.pop() {
            let [a, b] = nodes[node];
            if !crosses(
                [a[0], a[1], a[2]],
                [b[0], b[1], b[2]],
                o,
                d,
                (0.0, limit),
                far,
            ) {
                continue;
            }
            if a[3] >= 0.0 {
                stack.push(b[3] as usize);
                stack.push(a[3] as usize);
                continue;
            }
            let range = (-a[3] - 1.0) as usize..b[3] as usize;
            for (p, prim) in prims.iter().enumerate().take(range.end).skip(range.start) {
                if let Some(h) = intersect(prim, o, d, 0.0)
                    && h <= limit
                    && (best.is_none() || h < limit)
                {
                    best = Some(p);
                    limit = h;
                }
            }
        }
        best
    }

    #[test]
    fn the_flat_index_finds_what_the_index_finds() {
        let meshes = vec![scatter(300, 1), scatter(200, 2)];
        let index = PickIndex::new(&meshes);
        let origin = [3.0, 4.0, -1.0];
        let (nodes, prims) = index.flatten(origin);
        let mut leaves: Vec<usize> = Vec::new();
        for [a, b] in &nodes {
            if a[3] < 0.0 {
                leaves.extend((-a[3] - 1.0) as usize..b[3] as usize);
            }
        }
        leaves.sort_unstable();
        assert_eq!(leaves, (0..prims.len()).collect::<Vec<_>>());
        let (mut same, mut total, mut hits) = (0, 0, 0);
        for ray in rays().into_iter().filter(|r| r.min_distance == 0.0) {
            let o = [0, 1, 2].map(|k| (ray.origin[k] - origin[k]) as f32);
            let d = ray.direction.map(|v| v as f32);
            let want = index.first_hit(&ray, 1e3, |_| true, |_, _| false);
            let got =
                first_hit(&nodes, &prims, o, d, 1e3).map(|p| (prims[p].mesh, prims[p].triangle));
            total += 1;
            hits += usize::from(got.is_some());
            same += usize::from(want.map(|h| (h.mesh, h.triangle)) == got);
        }
        assert!(
            total > 300 && hits > 100 && same == total,
            "{same} of {total}, {hits} hits"
        );
    }
}
