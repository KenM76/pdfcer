//! A reusable picking index: a bounding-volume hierarchy over every
//! triangle of a model, built once, so repeated picks (hover, measuring)
//! visit only the triangles whose boxes the ray crosses.

use crate::TriangleMesh;
use crate::pick::{Hit, Ray, intersect};
use crate::render::corners;

/// Triangles per leaf; more costs intersection tests, fewer costs nodes.
const LEAF_SIZE: usize = 4;

/// A triangle the renderer would draw, with its corners resolved.
#[derive(Debug, Clone)]
struct Prim {
    mesh: usize,
    triangle: usize,
    corners: [[f64; 3]; 3],
}

#[derive(Debug, Clone, Copy)]
struct Bounds {
    lo: [f64; 3],
    hi: [f64; 3],
}

#[derive(Debug, Clone)]
enum Node {
    Leaf {
        bounds: Bounds,
        start: usize,
        end: usize,
    },
    Inner {
        bounds: Bounds,
        left: usize,
        right: usize,
    },
}

/// A picking index over a model's meshes ([`PickIndex::new`]): the same
/// answers as [`crate::pick`], faster when one model is picked many times.
///
/// The index copies the triangles' corners, so it does not borrow the
/// meshes; rebuild it when they change.
///
/// # Examples
///
/// ```
/// use pdfcer_3d::{PickIndex, Ray};
/// let index = PickIndex::new(&[]);
/// let ray = Ray { origin: [0.0; 3], direction: [0.0, 0.0, 1.0], min_distance: 0.0 };
/// assert_eq!(index.pick(&ray), None);
/// ```
#[derive(Debug, Clone)]
pub struct PickIndex {
    prims: Vec<Prim>,
    nodes: Vec<Node>,
}

impl PickIndex {
    /// Builds the index over `meshes`. [`Hit::mesh`] indexes this slice.
    /// Triangles the renderer skips (a missing or non-finite corner) are
    /// left out, as [`crate::pick`] skips them.
    #[must_use]
    pub fn new(meshes: &[TriangleMesh]) -> Self {
        let mut prims = Vec::new();
        for (m, mesh) in meshes.iter().enumerate() {
            for (t, tri) in mesh.triangles.iter().enumerate() {
                if let Some(c) = corners(mesh, tri) {
                    prims.push(Prim {
                        mesh: m,
                        triangle: t,
                        corners: c,
                    });
                }
            }
        }
        let mut index = Self {
            prims,
            nodes: Vec::new(),
        };
        if !index.prims.is_empty() {
            let end = index.prims.len();
            index.build(0, end);
        }
        index
    }

    /// The nearest triangle `ray` meets at or beyond [`Ray::min_distance`],
    /// exactly as [`crate::pick`] would answer for the indexed meshes.
    #[must_use]
    pub fn pick(&self, ray: &Ray) -> Option<Hit> {
        self.pick_where(ray, f64::INFINITY, |_| true)
    }

    /// [`Self::pick`] limited to meshes `include` accepts (by index in the
    /// slice given to [`Self::new`]; return `false` for a hidden part) and
    /// to hits no farther than `max_distance` along the ray. Pass
    /// `f64::INFINITY` for no limit.
    #[must_use]
    pub fn pick_where(
        &self,
        ray: &Ray,
        max_distance: f64,
        mut include: impl FnMut(usize) -> bool,
    ) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        let mut limit = max_distance;
        let mut stack = Vec::with_capacity(64);
        if !self.nodes.is_empty() {
            stack.push(0usize);
        }
        while let Some(n) = stack.pop() {
            let Some(node) = self.nodes.get(n) else {
                continue;
            };
            match *node {
                Node::Inner {
                    bounds,
                    left,
                    right,
                } => {
                    if crosses(&bounds, ray, limit) {
                        stack.push(right);
                        stack.push(left);
                    }
                }
                Node::Leaf { bounds, start, end } => {
                    if !crosses(&bounds, ray, limit) {
                        continue;
                    }
                    for p in self.prims.get(start..end).unwrap_or_default() {
                        if !include(p.mesh) {
                            continue;
                        }
                        let Some(hit) = intersect(&p.corners, ray) else {
                            continue;
                        };
                        if hit.distance > limit {
                            continue;
                        }
                        let hit = Hit {
                            mesh: p.mesh,
                            triangle: p.triangle,
                            ..hit
                        };
                        if best.is_none_or(|b| nearer(&hit, &b)) {
                            limit = hit.distance;
                            best = Some(hit);
                        }
                    }
                }
            }
        }
        best
    }

    /// Builds the subtree over `prims[start..end]` and returns its node
    /// index. Median splits halve the range, so depth stays near log2(n).
    fn build(&mut self, start: usize, end: usize) -> usize {
        let range = self.prims.get(start..end).unwrap_or_default();
        let bounds = bounds_of(range.iter().flat_map(|p| p.corners));
        let slot = self.nodes.len();
        if end - start <= LEAF_SIZE {
            self.nodes.push(Node::Leaf { bounds, start, end });
            return slot;
        }
        let cb = bounds_of(range.iter().map(|p| centroid(&p.corners)));
        let axis = longest_axis(&cb);
        let mid = start + (end - start) / 2;
        if let Some(slice) = self.prims.get_mut(start..end) {
            slice.select_nth_unstable_by(mid - start, |a, b| {
                axis_of(centroid(&a.corners), axis).total_cmp(&axis_of(centroid(&b.corners), axis))
            });
        }
        self.nodes.push(Node::Leaf { bounds, start, end });
        let left = self.build(start, mid);
        let right = self.build(mid, end);
        if let Some(node) = self.nodes.get_mut(slot) {
            *node = Node::Inner {
                bounds,
                left,
                right,
            };
        }
        slot
    }
}

/// `a` is nearer than `b`, ties going to the lower (mesh, triangle) as the
/// linear [`crate::pick`] scan resolves them.
fn nearer(a: &Hit, b: &Hit) -> bool {
    a.distance < b.distance
        || (a.distance == b.distance && (a.mesh, a.triangle) < (b.mesh, b.triangle))
}

/// Whether `ray` crosses `b` somewhere in [min_distance, limit]. The test
/// is inclusive so a hit at exactly `limit` can still win a tie.
fn crosses(b: &Bounds, ray: &Ray, limit: f64) -> bool {
    let mut t0 = ray.min_distance;
    let mut t1 = limit;
    for axis in 0..3 {
        let o = axis_of(ray.origin, axis);
        let d = axis_of(ray.direction, axis);
        let (lo, hi) = (axis_of(b.lo, axis), axis_of(b.hi, axis));
        if d == 0.0 {
            if o < lo || o > hi {
                return false;
            }
            continue;
        }
        let (mut a, mut c) = ((lo - o) / d, (hi - o) / d);
        if a > c {
            std::mem::swap(&mut a, &mut c);
        }
        // Widen by a relative epsilon: the triangle test and this one round
        // differently, and the box must never reject what the triangle hits.
        let slack = 1e-9 * (a.abs().max(c.abs()).max(1.0));
        t0 = t0.max(a - slack);
        t1 = t1.min(c + slack);
        if t0 > t1 {
            return false;
        }
    }
    true
}

fn bounds_of(points: impl Iterator<Item = [f64; 3]>) -> Bounds {
    let mut b = Bounds {
        lo: [f64::INFINITY; 3],
        hi: [f64::NEG_INFINITY; 3],
    };
    for p in points {
        for ((lo, hi), v) in b.lo.iter_mut().zip(b.hi.iter_mut()).zip(p) {
            *lo = lo.min(v);
            *hi = hi.max(v);
        }
    }
    b
}

fn centroid(c: &[[f64; 3]; 3]) -> [f64; 3] {
    let [a, b, d] = *c;
    [
        (a[0] + b[0] + d[0]) / 3.0,
        (a[1] + b[1] + d[1]) / 3.0,
        (a[2] + b[2] + d[2]) / 3.0,
    ]
}

fn longest_axis(b: &Bounds) -> usize {
    let ext = |i| axis_of(b.hi, i) - axis_of(b.lo, i);
    let (x, y, z) = (ext(0), ext(1), ext(2));
    if x >= y && x >= z {
        0
    } else if y >= z {
        1
    } else {
        2
    }
}

fn axis_of(v: [f64; 3], axis: usize) -> f64 {
    v.get(axis).copied().unwrap_or(0.0)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use crate::pick::pick;

    /// A deterministic scatter of `n` small triangles in a 10-unit cube.
    fn scatter(n: usize, seed: u64) -> TriangleMesh {
        let mut s = seed;
        let mut next = || {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((s >> 11) as f64 / (1u64 << 53) as f64) * 10.0
        };
        let mut mesh = TriangleMesh::default();
        for i in 0..n {
            let c = [next(), next(), next()];
            for k in 0..3 {
                let mut p = c;
                p[k] += 0.8;
                mesh.positions.push(p);
            }
            let b = (i * 3) as u32;
            mesh.triangles.push([b, b + 1, b + 2]);
        }
        mesh
    }

    fn rays() -> Vec<Ray> {
        let mut out = Vec::new();
        for i in 0..20 {
            for j in 0..20 {
                let (x, y) = (i as f64 * 0.55, j as f64 * 0.55);
                out.push(Ray {
                    origin: [x, y, -5.0],
                    direction: [0.0, 0.0, 1.0],
                    min_distance: 0.0,
                });
                let d = crate::vec3::normalize([0.3, -0.2, 1.0]).unwrap();
                out.push(Ray {
                    origin: [x, y, 5.0],
                    direction: d,
                    min_distance: f64::NEG_INFINITY,
                });
            }
        }
        out
    }

    #[test]
    fn the_index_answers_exactly_as_the_linear_pick() {
        let meshes = vec![scatter(300, 1), scatter(200, 2), TriangleMesh::default()];
        let index = PickIndex::new(&meshes);
        let mut hits = 0;
        for ray in rays() {
            let want = pick(&meshes, &ray);
            hits += usize::from(want.is_some());
            assert_eq!(index.pick(&ray), want, "{ray:?}");
        }
        assert!(hits > 50, "the scatter must actually be hit ({hits})");
    }

    #[test]
    fn a_masked_mesh_is_never_hit() {
        let meshes = vec![scatter(300, 1), scatter(200, 2)];
        let index = PickIndex::new(&meshes);
        let only_second = [meshes[1].clone()];
        for ray in rays() {
            let got = index.pick_where(&ray, f64::INFINITY, |m| m == 1);
            let want = pick(&only_second, &ray).map(|h| Hit { mesh: 1, ..h });
            assert_eq!(got, want, "{ray:?}");
        }
    }

    #[test]
    fn a_maximum_distance_drops_farther_hits() {
        let meshes = vec![scatter(300, 1)];
        let index = PickIndex::new(&meshes);
        let mut dropped = 0;
        for ray in rays() {
            let full = index.pick(&ray);
            let near = index.pick_where(&ray, 8.0, |_| true);
            match full {
                Some(h) if h.distance <= 8.0 => assert_eq!(near, full),
                Some(_) => {
                    dropped += 1;
                    assert_eq!(near, None);
                }
                None => assert_eq!(near, None),
            }
        }
        assert!(dropped > 0, "some hit must lie beyond the limit");
    }

    #[test]
    fn coincident_triangles_tie_to_the_lower_index() {
        let one = scatter(1, 7);
        let meshes = vec![one.clone(), one];
        let p = &meshes[0].positions;
        let c = centroid(&[p[0], p[1], p[2]]);
        let ray = Ray {
            origin: [c[0], c[1], c[2] - 10.0],
            direction: [0.0, 0.0, 1.0],
            min_distance: 0.0,
        };
        let want = pick(&meshes, &ray);
        assert!(want.is_some(), "the ray must meet the triangle");
        assert_eq!(PickIndex::new(&meshes).pick(&ray), want);
    }
}
