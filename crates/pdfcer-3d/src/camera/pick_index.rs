//! A reusable picking index: a bounding-volume hierarchy over every
//! triangle of a model, built once, so repeated picks (hover, measuring)
//! visit only the triangles whose boxes the ray crosses.

use crate::TriangleMesh;
use crate::camera::pick::{Hit, Ray, intersect_weighted};
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

/// What [`PickIndex::walk`] does after a hit.
enum Step {
    Go,
    /// Ignore hits farther than this from now on.
    Shorten(f64),
    Stop,
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
        include: impl FnMut(usize) -> bool,
    ) -> Option<Hit> {
        self.first_hit(ray, max_distance, include, |_, _| false)
    }

    /// The nearest hit no farther than `max_distance` that is not `clear`,
    /// on a mesh `include` accepts: the closest-hit query of a ray that
    /// passes through cut-out holes. `clear` is given the hit and its
    /// barycentric weights on the triangle's corners (in
    /// [`TriangleMesh::triangles`] order, summing to 1), so it can sample a
    /// texture's alpha there; return `true` for a hole. Ties go to the
    /// lower (mesh, triangle), as in [`Self::pick`].
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_3d::{PickIndex, Ray, TriangleMesh};
    /// let mut mesh = TriangleMesh::default();
    /// mesh.positions = vec![[0.0, 0.0, 1.0], [4.0, 0.0, 1.0], [0.0, 4.0, 1.0]];
    /// mesh.triangles = vec![[0, 1, 2]];
    /// let index = PickIndex::new(&[mesh]);
    /// let ray = Ray { origin: [1.0, 1.0, 0.0], direction: [0.0, 0.0, 1.0], min_distance: 0.0 };
    /// // A hole wherever the first corner's weight is under a half.
    /// let hit = index.first_hit(&ray, f64::INFINITY, |_| true, |_, w| w[0] < 0.5);
    /// assert_eq!(hit.map(|h| h.distance), Some(1.0));
    /// ```
    #[must_use]
    pub fn first_hit(
        &self,
        ray: &Ray,
        max_distance: f64,
        include: impl FnMut(usize) -> bool,
        mut clear: impl FnMut(&Hit, [f64; 3]) -> bool,
    ) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        self.walk(ray, max_distance, include, |hit, w| {
            if best.is_some_and(|b| !nearer(&hit, &b)) || clear(&hit, w) {
                return Step::Go;
            }
            best = Some(hit);
            Step::Shorten(hit.distance)
        });
        best
    }

    /// The share of light (0 to 1) that gets along `ray` from
    /// [`Ray::min_distance`] to `max_distance` past the meshes `include`
    /// accepts: the any-hit query of a shadow or visibility ray. It is the
    /// product of `share` over every triangle met in that range, each given
    /// the hit and its barycentric weights as in [`Self::first_hit`];
    /// return 0 for an opaque surface, `1 − alpha` for a see-through one,
    /// 1 for a hole. A ray along a shared edge meets both triangles. Stops
    /// at the first share of 0; a share outside 0 to 1 is clamped.
    #[must_use]
    pub fn transmittance(
        &self,
        ray: &Ray,
        max_distance: f64,
        include: impl FnMut(usize) -> bool,
        mut share: impl FnMut(&Hit, [f64; 3]) -> f64,
    ) -> f64 {
        let mut through = 1.0;
        self.walk(ray, max_distance, include, |hit, w| {
            let s = share(&hit, w);
            through *= if s.is_nan() { 0.0 } else { s.clamp(0.0, 1.0) };
            if through == 0.0 { Step::Stop } else { Step::Go }
        });
        through
    }

    /// Calls `visit` with every hit on an included mesh no farther than the
    /// current limit, which `visit` can shorten or end.
    fn walk(
        &self,
        ray: &Ray,
        max_distance: f64,
        mut include: impl FnMut(usize) -> bool,
        mut visit: impl FnMut(Hit, [f64; 3]) -> Step,
    ) {
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
                        let Some((hit, w)) = intersect_weighted(&p.corners, ray) else {
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
                        match visit(hit, w) {
                            Step::Go => {}
                            Step::Shorten(d) => limit = d,
                            Step::Stop => return,
                        }
                    }
                }
            }
        }
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

/// Each corner is divided before summing, so corners near the float range
/// cannot overflow the sum.
fn centroid(&[a, b, c]: &[[f64; 3]; 3]) -> [f64; 3] {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    let [cx, cy, cz] = c;
    [
        ax / 3.0 + bx / 3.0 + cx / 3.0,
        ay / 3.0 + by / 3.0 + cy / 3.0,
        az / 3.0 + bz / 3.0 + cz / 3.0,
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
    use crate::camera::pick::pick;
    use crate::render::corners;

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
    fn a_centroid_near_the_float_range_is_finite() {
        let m = [f64::MAX / 1.5; 3];
        assert!(centroid(&[m, m, m]).iter().all(|c| c.is_finite()));
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

    /// Every (hit, weights) a linear scan of `meshes` finds for `ray`, in
    /// (mesh, triangle) order.
    fn brute(meshes: &[TriangleMesh], ray: &Ray, max: f64) -> Vec<(Hit, [f64; 3])> {
        let mut out = Vec::new();
        for (m, mesh) in meshes.iter().enumerate() {
            for (t, tri) in mesh.triangles.iter().enumerate() {
                let c = corners(mesh, tri);
                if let Some((h, w)) = c.and_then(|c| intersect_weighted(&c, ray))
                    && h.distance <= max
                {
                    out.push((
                        Hit {
                            mesh: m,
                            triangle: t,
                            ..h
                        },
                        w,
                    ));
                }
            }
        }
        out
    }

    /// A share that is a power of two, so any product order is exact.
    fn share(h: &Hit, w: [f64; 3]) -> f64 {
        match (h.mesh * 7 + h.triangle) % 4 {
            0 => 0.5,
            1 => 0.25,
            2 if w[0] < 0.4 => 1.0,
            2 => 0.0,
            _ => 1.0,
        }
    }

    fn hole(h: &Hit, w: [f64; 3]) -> bool {
        h.triangle.is_multiple_of(3) || w[1] > 0.6
    }

    #[test]
    fn first_hit_matches_a_brute_force_scan_with_holes() {
        let meshes = vec![scatter(300, 1), scatter(200, 2)];
        let index = PickIndex::new(&meshes);
        let (mut skipped, mut hits) = (0, 0);
        for ray in rays() {
            for max in [f64::INFINITY, 8.0] {
                let all = brute(&meshes, &ray, max);
                let want = all
                    .iter()
                    .filter(|(h, w)| h.mesh == 1 || !hole(h, *w))
                    .map(|(h, _)| *h)
                    .reduce(|a, b| if nearer(&b, &a) { b } else { a });
                let nearest = all
                    .iter()
                    .map(|(h, _)| *h)
                    .reduce(|a, b| if nearer(&b, &a) { b } else { a });
                skipped += usize::from(want != nearest);
                hits += usize::from(want.is_some());
                let got = index.first_hit(&ray, max, |_| true, |h, w| h.mesh == 0 && hole(h, w));
                assert_eq!(got, want, "{ray:?} {max}");
            }
        }
        assert!(
            hits > 50 && skipped > 10,
            "holes must change answers ({hits}, {skipped})"
        );
    }

    #[test]
    fn transmittance_matches_a_brute_force_product() {
        let meshes = vec![scatter(300, 1), scatter(200, 2)];
        let index = PickIndex::new(&meshes);
        let (mut partial, mut blocked) = (0, 0);
        for ray in rays() {
            for max in [f64::INFINITY, 8.0] {
                let want: f64 = brute(&meshes, &ray, max)
                    .iter()
                    .filter(|(h, _)| h.mesh == 0)
                    .map(|(h, w)| share(h, *w))
                    .product();
                partial += usize::from(want > 0.0 && want < 1.0);
                blocked += usize::from(want == 0.0);
                let got = index.transmittance(&ray, max, |m| m == 0, share);
                assert_eq!(got.to_bits(), want.to_bits(), "{ray:?} {max}");
            }
        }
        assert!(
            partial > 10 && blocked > 10,
            "both outcomes must occur ({partial}, {blocked})"
        );
    }

    #[test]
    fn transmittance_stops_at_the_first_opaque_surface() {
        let meshes = vec![scatter(300, 1)];
        let index = PickIndex::new(&meshes);
        for ray in rays() {
            let mut calls = 0;
            let t = index.transmittance(
                &ray,
                f64::INFINITY,
                |_| true,
                |_, _| {
                    calls += 1;
                    0.0
                },
            );
            assert!(calls <= 1, "{calls} calls after an opaque hit");
            assert_eq!(t, if calls == 0 { 1.0 } else { 0.0 });
        }
    }

    #[test]
    fn a_share_outside_the_unit_range_is_clamped() {
        let meshes = vec![scatter(300, 1)];
        let index = PickIndex::new(&meshes);
        for ray in rays() {
            for (bad, want) in [(7.0, 1.0), (-2.0, 0.0), (f64::NAN, 0.0)] {
                let hit = index.pick(&ray).is_some();
                let t = index.transmittance(&ray, f64::INFINITY, |_| true, |_, _| bad);
                assert_eq!(t, if hit { want } else { 1.0 });
            }
        }
    }

    #[test]
    fn weights_reproduce_the_hit_point() {
        let meshes = vec![scatter(300, 1)];
        let index = PickIndex::new(&meshes);
        for ray in rays() {
            let _ = index.first_hit(
                &ray,
                f64::INFINITY,
                |_| true,
                |h, w| {
                    let tri = meshes[0].triangles[h.triangle];
                    let c = corners(&meshes[0], &tri).unwrap();
                    for (axis, want) in h.point.iter().enumerate() {
                        let p: f64 = w.iter().zip(&c).map(|(k, v)| k * v[axis]).sum();
                        assert!((p - want).abs() < 1e-9, "{p} vs {:?}", h.point);
                    }
                    assert!((w.iter().sum::<f64>() - 1.0).abs() < 1e-12);
                    false
                },
            );
        }
    }
}
