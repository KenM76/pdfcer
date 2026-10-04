//! Triangle reconstruction for `TESS_3D_Compressed` (173) [WD 7.8.9.7].
//!
//! The WD names the arrays but not the traversal that produced them. The
//! rules below are measured against real PRC files (every component,
//! slot, reference and point consumed exactly; flat faces rebuild flat);
//! the spec RAG's `prc__8137__tess_3d_compressed.md` records the
//! measurement and its provenance.
//!
//! Traversal. A component starts with a seed triangle of three slots;
//! every later triangle takes one slot, its apex. A slot flagged in
//! `point_is_reference` takes the next `point_reference_array` value, a
//! vertex number; any other slot creates the next vertex from the next
//! `point_array` triple. Triangle `[A B C]`, entered across `[A B]`, reads
//! `edge_status & 3`: bit 1 continues across `[C B]`, bit 0 across
//! `[A C]`; with both, `[C B]` is taken and `[A C]` pushed. With neither,
//! pushed edges are popped, skipping any already shared by two triangles;
//! an empty stack starts the next component.
//!
//! Geometry, in units of `tolerance`:
//! - seed: `V0 = origin + d`, `V1 = V0 + d`, `V2 = mid(V0, V1) + d`;
//! - apex across `[P Q]` with the triangle's third vertex `W`
//!   [WD 7.8.9.2]: with `O = (P + Q) * 0.5`, `L` and `H` the lower- and
//!   higher-numbered of `P`, `Q`, `X = unitize(L - H)`,
//!   `Z = unitize((W - O) x X)` and `Y = unitize(Z x X)`, the apex is
//!   `((O + d.x X) - d.y Y) - d.z Z`.
//!
//! Arithmetic. The encoder reinjects every approximated point into its
//! working mesh and frames the next apex on it [WD 7.8.9], so the decoder
//! must repeat its floating point bit for bit: a last-bit difference
//! compounds from apex to apex, and along a chain of thin triangles an
//! exactly normalising decoder drifts thousands of tolerances off.
//! `unitize` is therefore the WD's `PrcPt::Unitize` [WD 12.3], whose length
//! is not exact. `X`'s direction (the WD prints `V1 - V0`, lower to
//! higher), `Y`'s unitize and the summation order are measured; the spec
//! RAG's `prc__8137__tess_3d_compressed.md` §2b M9 records the fit.
//!
//! Fold. Left and right are taken relative to the triangle normal, the
//! cross product of its vertices oriented by one of its stored normals
//! [WD 7.8.9, 7.8.9.1]. A triangle whose walk-order winding
//! `(B - A) x (C - A)` points against that normal is folded: bit 1 and
//! bit 0 swap for its own continuation. Its normal comes from, in order:
//! a record read at this triangle, whose `triangle_normal_reversed` bit
//! gives the orientation exactly (`reversed != (A > B)`, the record being
//! framed on `(max - min) x (R - min)` [WD 7.8.9.4]); the stored normal
//! of its planar face, compared geometrically unless the triangle is a
//! sliver; otherwise none. Without a normal the measured default holds: a
//! new apex lying exactly in its parent's plane on the parent's side
//! (`d.z == 0`, `d.y > 0`, a double-sided panel folding back over itself)
//! is folded. A component (a run started from an empty stack) that fails
//! to fit is rewound and retried once with the fold inverted at its second
//! triangle, where every measured unsignalled fold sat. A mesh whose
//! oriented walk does not fit is walked again on the default alone.
//!
//! Only the per-triangle `edge_status` form (three entries per triangle,
//! the first `T` read) is reconstructed; the one-entry-per-triangle form
//! walks differently and is not.

use std::collections::HashMap;

use crate::TriangleMesh;

mod normals;

pub(crate) use normals::NormalArrays;
use normals::{NormalMark, NormalReader, Orientation, stored_normals};

type V = [f64; 3];

/// Below this sine of its angle at `A` a triangle is a sliver whose
/// winding no normal can orient: the WD assumes none [WD 7.8.9].
const MIN_SINE: f64 = 1e-6;

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V, b: V) -> V {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: V, s: f64) -> V {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn unit(a: V) -> V {
    let l = dot(a, a).sqrt();
    if l > 0.0 { mul(a, 1.0 / l) } else { a }
}
fn key(a: u32, b: u32) -> (u32, u32) {
    if a < b { (a, b) } else { (b, a) }
}

/// `PrcPt::Length` [WD 12.3]: the squared length summed in `f64` and
/// rounded to `f32`, then Newton's square root in `f64`, seeded with that
/// `f32` with its exponent halved. The printed loop never advances its
/// iterate; it is read as iterating until two successive values are equal,
/// `None` (the WD's -1) when 100 steps do not settle.
fn wd_length(a: V) -> Option<f64> {
    let squared = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]) as f32;
    let bits = squared.to_bits();
    let exponent = (bits >> 23) & 0xff;
    let seed = if exponent > 127 {
        f32::from_bits((bits & !(0xff << 23)) | (((exponent - 127) / 2 + 127) << 23))
    } else {
        squared
    };
    let squared = f64::from(squared);
    let mut x = f64::from(seed);
    for _ in 0..100 {
        let next = 0.5 * (x + squared / x);
        if next == x {
            return Some(next);
        }
        x = next;
    }
    None
}

/// `PrcPt::Unitize` [WD 12.3]: each component divided by [`wd_length`];
/// `None` below `FLT_EPSILON`.
fn unitize(a: V) -> Option<V> {
    let l = wd_length(a).filter(|&l| l >= f64::from(f32::EPSILON))?;
    Some([a[0] / l, a[1] / l, a[2] / l])
}

/// `PrcPt::MakeOrthoRep` [WD 12.3] on `X`: `(Y, Z)` with
/// `Z = unitize(X x (0, 1, 0))`, or `X x (1, 0, 0)` when that is null.
fn make_ortho_rep(x: V) -> Option<(V, V)> {
    let x = unitize(x)?;
    let z = unitize(cross(x, [0.0, 1.0, 0.0])).or_else(|| unitize(cross(x, [1.0, 0.0, 0.0])))?;
    Some((unitize(cross(z, x))?, z))
}

/// The apex across `[lo hi]` (`lo` the lower-numbered vertex) opposite `w`,
/// `d` already scaled by the tolerance [WD 7.8.9.2]. A null `Z` or `Y`
/// comes from `MakeOrthoRep(X)` as the WD prescribes; a null `X`, an edge
/// the WD's non-degeneracy rule excludes, is used as it is.
fn apex(lo: V, hi: V, w: V, d: V) -> V {
    let o = mul(add(lo, hi), 0.5);
    let x = sub(lo, hi);
    let x = unitize(x).unwrap_or(x);
    let (y, z) = unitize(cross(sub(w, o), x))
        .and_then(|z| Some((unitize(cross(z, x))?, z)))
        .or_else(|| make_ortho_rep(x))
        .unwrap_or_default();
    sub(sub(add(o, mul(x, d[0])), mul(y, d[1])), mul(z, d[2]))
}

/// The decoded arrays of one `TESS_3D_Compressed`.
pub(crate) struct Arrays<'a> {
    pub(crate) tolerance: f64,
    pub(crate) origin: V,
    pub(crate) points: &'a [i64],
    pub(crate) edge_status: &'a [i32],
    pub(crate) triangles: usize,
    pub(crate) is_reference: &'a [bool],
    pub(crate) references: &'a [u32],
    /// The stored normals, when the mesh has them.
    pub(crate) normals: Option<NormalArrays<'a>>,
}

/// Mutable traversal state; `log` records every edge-count increment so a
/// component can be rolled back and retried. `normals` is present when
/// the stored normals orient the walk.
struct Walk<'a> {
    pos: Vec<V>,
    tris: Vec<[u32; 3]>,
    edges: HashMap<(u32, u32), u8>,
    log: Vec<(u32, u32)>,
    stack: Vec<(u32, u32, u32)>,
    next: Option<(u32, u32, u32)>,
    slot: usize,
    ri: usize,
    pi: usize,
    normals: Option<NormalReader<'a>>,
}

/// Where a component starts, to roll back to.
#[derive(Clone, Copy)]
struct Mark {
    tri: usize,
    pos: usize,
    log: usize,
    slot: usize,
    ri: usize,
    pi: usize,
    normals: Option<NormalMark>,
}

impl<'a> Walk<'a> {
    fn new(t: usize, normals: Option<NormalReader<'a>>) -> Self {
        Walk {
            pos: Vec::new(),
            tris: Vec::with_capacity(t.min(1 << 20)),
            edges: HashMap::new(),
            log: Vec::new(),
            stack: Vec::new(),
            next: None,
            slot: 0,
            ri: 0,
            pi: 0,
            normals,
        }
    }

    fn mark(&self, tri: usize) -> Mark {
        Mark {
            tri,
            pos: self.pos.len(),
            log: self.log.len(),
            slot: self.slot,
            ri: self.ri,
            pi: self.pi,
            normals: self.normals.as_ref().map(NormalReader::mark),
        }
    }

    fn rewind(&mut self, m: Mark) {
        for k in self.log.drain(m.log..) {
            if let Some(n) = self.edges.get_mut(&k) {
                *n = n.saturating_sub(1);
            }
        }
        self.pos.truncate(m.pos);
        self.tris.truncate(m.tri);
        self.stack.clear();
        self.next = None;
        (self.slot, self.ri, self.pi) = (m.slot, m.ri, m.pi);
        if let (Some(r), Some(n)) = (self.normals.as_mut(), m.normals) {
            r.rewind(n);
        }
    }

    // One slot: `Ok(Some(v))` a reference, `Ok(None)` a new point `d`.
    fn take(&mut self, a: &Arrays<'_>) -> Result<Option<u32>, String> {
        let flagged = *a.is_reference.get(self.slot).ok_or("slots run out")?;
        self.slot += 1;
        if !flagged {
            return Ok(None);
        }
        let v = *a.references.get(self.ri).ok_or("references run out")?;
        self.ri += 1;
        self.get(v)?;
        Ok(Some(v))
    }

    fn point(&mut self, a: &Arrays<'_>) -> Result<V, String> {
        let Some(&[x, y, z]) = a.points.get(self.pi..self.pi.saturating_add(3)) else {
            return Err("points run out".into());
        };
        self.pi += 3;
        let tol = a.tolerance;
        Ok([x as f64 * tol, y as f64 * tol, z as f64 * tol])
    }

    fn get(&self, v: u32) -> Result<V, String> {
        self.pos
            .get(v as usize)
            .copied()
            .ok_or_else(|| "a triangle refers to a vertex not yet decoded".to_owned())
    }

    /// The next triangle and its default fold: the seed, or the apex
    /// across the edge to continue from. A new apex lying in its parent's
    /// plane on the parent's side (`d.z == 0`, `d.y > 0`) defaults to
    /// folded.
    fn triangle(&mut self, a: &Arrays<'_>) -> Result<([u32; 3], bool), String> {
        if let Some((p, q, w)) = self.next.take() {
            if let Some(r) = self.take(a)? {
                return Ok(([p, q, r], false));
            }
            let (pp, qq, ww) = (self.get(p)?, self.get(q)?, self.get(w)?);
            let (lo, hi) = if p < q { (pp, qq) } else { (qq, pp) };
            let fold = matches!(
                a.points.get(self.pi..self.pi.saturating_add(3)),
                Some(&[_, dy, 0]) if dy > 0
            );
            let d = self.point(a)?;
            self.pos.push(apex(lo, hi, ww, d));
            return Ok(([p, q, (self.pos.len() - 1) as u32], fold));
        }
        let mut v = [0u32; 3];
        for k in 0..3 {
            let id = match self.take(a)? {
                Some(r) => r,
                None => {
                    let d = self.point(a)?;
                    let p = match k {
                        0 => add(a.origin, d),
                        1 => add(self.get(v[0])?, d),
                        _ => add(mul(add(self.get(v[0])?, self.get(v[1])?), 0.5), d),
                    };
                    self.pos.push(p);
                    (self.pos.len() - 1) as u32
                }
            };
            if let Some(s) = v.get_mut(k) {
                *s = id;
            }
        }
        Ok((v, false))
    }

    /// Whether `t` folds: its winding `(B - A) x (C - A)` points against
    /// its stored normal [WD 7.8.9, 7.8.9.1]. Without a usable normal the
    /// default `fold` stands.
    fn fold(&mut self, t: [u32; 3], fold: bool) -> Result<bool, String> {
        let ti = self.tris.len();
        let Some(r) = self.normals.as_mut() else {
            return Ok(fold);
        };
        let o = r
            .read(ti, t, &self.pos)
            .ok_or("the stored normals run out")?;
        Ok(match o {
            Orientation::Reversed(rev) => rev != (t[0] > t[1]),
            Orientation::Normal(n) => {
                let (a, b, c) = (self.get(t[0])?, self.get(t[1])?, self.get(t[2])?);
                let (ab, ac) = (sub(b, a), sub(c, a));
                let w = cross(ab, ac);
                let sine = (dot(w, w) / (dot(ab, ab) * dot(ac, ac))).sqrt();
                let s = dot(w, n);
                if sine > MIN_SINE && s != 0.0 {
                    s < 0.0
                } else {
                    fold
                }
            }
            Orientation::Unknown => fold,
        })
    }

    /// Adds one triangle; `flip` inverts the fold decision.
    fn step(&mut self, a: &Arrays<'_>, status: i32, flip: bool) -> Result<(), String> {
        let (tri, fold) = self.triangle(a)?;
        let [ta, tb, tc] = tri;
        for (x, y) in [(ta, tb), (tb, tc), (tc, ta)] {
            let n = self.edges.entry(key(x, y)).or_insert(0);
            *n = n.saturating_add(1);
            let n = *n;
            self.log.push(key(x, y));
            if n > 2 {
                return Err("an edge is shared by more than two triangles".into());
            }
        }
        let fold = self.fold(tri, fold)?;
        self.tris.push(tri);
        let (mut left, mut right) = ((tc, tb, ta), (ta, tc, tb));
        if fold != flip {
            std::mem::swap(&mut left, &mut right);
        }
        match (status & 2 != 0, status & 1 != 0) {
            (true, true) => {
                self.next = Some(left);
                self.stack.push(right);
            }
            (true, false) => self.next = Some(left),
            (false, true) => self.next = Some(right),
            (false, false) => {}
        }
        while self.next.is_none() {
            let Some(e) = self.stack.pop() else { break };
            if self.edges.get(&key(e.0, e.1)).copied().unwrap_or(0) < 2 {
                self.next = Some(e);
            }
        }
        Ok(())
    }
}

/// Rebuilds the mesh, or says why the arrays do not fit the traversal.
/// Every slot, reference and point must be consumed exactly.
///
/// A mesh with stored normals is walked oriented by them, which must then
/// be consumed exactly too; failing that, it is walked again on the
/// default fold alone and its normals read afterwards, kept only if they
/// fit.
pub(crate) fn reconstruct(a: &Arrays<'_>) -> Result<TriangleMesh, String> {
    if a.edge_status.len() != a.triangles.saturating_mul(3) {
        return Err("the one-status-per-triangle edge form is not reconstructed".into());
    }
    if let Some(n) = a.normals.as_ref()
        && let Ok(w) = walk(a, Some(NormalReader::new(n)))
    {
        let Walk {
            pos, tris, normals, ..
        } = w;
        if let Some(stored) = normals.and_then(|r| r.finish(&pos)) {
            return Ok(mesh(pos, tris, stored));
        }
    }
    let Walk { pos, tris, .. } = walk(a, None)?;
    let stored = a
        .normals
        .as_ref()
        .and_then(|n| stored_normals(&pos, &tris, n))
        .unwrap_or_default();
    Ok(mesh(pos, tris, stored))
}

fn mesh(
    positions: Vec<V>,
    triangles: Vec<[u32; 3]>,
    stored: (Vec<V>, Vec<[u32; 3]>),
) -> TriangleMesh {
    TriangleMesh {
        positions,
        triangles,
        faces: Vec::new(),
        normals_recalculated: false,
        normals: stored.0,
        triangle_normals: stored.1,
        triangle_graphics: Vec::new(),
        uvs: Vec::new(),
        triangle_uvs: Vec::new(),
    }
}

/// Walks every component. One that fails to fit is rewound and retried
/// once with the fold inverted at its second triangle: a fold whose apex
/// is a reference, or whose triangle carries no normal signal, has no
/// other trace in the arrays.
fn walk<'a>(a: &Arrays<'_>, normals: Option<NormalReader<'a>>) -> Result<Walk<'a>, String> {
    let t = a.triangles;
    let mut w = Walk::new(t, normals);
    let mut start = w.mark(0);
    let mut retried = false;
    let mut i = 0;
    while i < t {
        if w.next.is_none() && !(retried && i == start.tri) {
            start = w.mark(i);
            retried = false;
        }
        let status = a.edge_status.get(i).copied().unwrap_or(0);
        let flip = retried && i == start.tri + 1;
        match w.step(a, status, flip) {
            Ok(()) => i += 1,
            Err(e) if retried || i == start.tri => return Err(e),
            Err(_) => {
                w.rewind(start);
                retried = true;
                i = start.tri;
            }
        }
    }
    if w.slot != a.is_reference.len() || w.ri != a.references.len() || w.pi != a.points.len() {
        return Err("the decoded triangles do not use up the stored arrays".into());
    }
    Ok(w)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)] // Tests fail loudly.
mod tests {
    use super::*;

    const S: f64 = std::f64::consts::SQRT_2;

    fn run(
        points: &[i64],
        status: &[i32],
        t: usize,
        is_ref: &[bool],
        refs: &[u32],
    ) -> Result<TriangleMesh, String> {
        reconstruct(&Arrays {
            tolerance: 0.5,
            origin: [10.0, 0.0, 0.0],
            points,
            edge_status: status,
            triangles: t,
            is_reference: is_ref,
            references: refs,
            normals: None,
        })
    }

    fn close(a: V, b: V) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-9)
    }

    /// Seed `[0 1 2]`, then the apex across `[2 1]`: `X` runs from vertex
    /// 2 to vertex 1, and each of `d`'s three components lands on its own
    /// axis with its own sign. Tolerance 0.5 halves every step.
    #[test]
    fn a_seed_and_one_apex_rebuild() {
        let pts = [0, 0, 0, 4, 0, 0, -2, 4, 0, 2, -6, 2];
        let m = run(&pts, &[2, 0, 0, 0, 0, 0], 2, &[false; 4], &[]).unwrap();
        assert_eq!(m.triangles, [[0, 1, 2], [2, 1, 3]]);
        let p: Vec<V> = m
            .positions
            .iter()
            .map(|v| [v[0] - 10.0, v[1], v[2]])
            .collect();
        assert!(close(p[0], [0.0, 0.0, 0.0]) && close(p[1], [2.0, 0.0, 0.0]));
        assert!(close(p[2], [0.0, 2.0, 0.0]), "{:?}", p[2]);
        assert!(close(p[3], [1.0 + 2.0 * S, 1.0 + S, -1.0]), "{:?}", p[3]);
    }

    /// Bit 0 alone continues across `[A C]`, keeping `W = B`.
    #[test]
    fn the_right_edge_is_entered_from_a_to_c() {
        let pts = [0, 0, 0, 4, 0, 0, -2, 4, 0, 0, -2, 0];
        let m = run(&pts, &[1, 0, 0, 0, 0, 0], 2, &[false; 4], &[]).unwrap();
        assert_eq!(m.triangles[1], [0, 2, 3]);
    }

    /// A reference slot reuses a vertex; a closed pushed edge is skipped
    /// and the next component starts from a fresh seed.
    #[test]
    fn references_and_components() {
        let pts = [0; 9 + 9];
        let is_ref = [false, false, false, true, false, false, false];
        let m = run(&pts, &[3, 0, 0, 0, 0, 0, 0, 0, 0], 3, &is_ref, &[0]).unwrap();
        assert_eq!(m.triangles, [[0, 1, 2], [2, 1, 0], [3, 4, 5]]);
    }

    /// A double-sided panel: four coplanar vertices, four triangles. The
    /// second triangle's apex folds back onto the seed's side, so its own
    /// continuation takes `[A C]` first; the last two apexes are the seed's
    /// first two vertices.
    #[test]
    fn a_folded_apex_swaps_its_continuation() {
        let pts = [0, 0, 0, 4, 0, 0, -2, 4, 0, 1, 3, 0];
        let is_ref = [false, false, false, false, true, true];
        let m = run(
            &pts,
            &[3, 3, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            4,
            &is_ref,
            &[0, 1],
        )
        .unwrap();
        assert_eq!(m.triangles, [[0, 1, 2], [2, 1, 3], [2, 3, 0], [0, 3, 1]]);
        // The same panel unfolded is rescued by the component retry.
        let unfolded = [0, 0, 0, 4, 0, 0, -2, 4, 0, 1, -3, 0];
        let m = run(
            &unfolded,
            &[3, 3, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            4,
            &is_ref,
            &[0, 1],
        )
        .unwrap();
        assert_eq!(m.triangles, [[0, 1, 2], [2, 1, 3], [2, 3, 0], [0, 3, 1]]);
    }

    #[test]
    fn a_fold_past_the_retry_point_is_detected() {
        let pts = [0, 0, 0, 4, 0, 0, -2, 4, 0, 0, -3, 1, 1, 3, 0];
        let is_ref = [false, false, false, false, false, true];
        let m = run(
            &pts,
            &[3, 3, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            4,
            &is_ref,
            &[0],
        )
        .unwrap();
        assert_eq!(m.triangles[3], [3, 4, 0]);
    }

    /// A panel whose fold apex is a reference carries no fold signal: the
    /// unfolded walk closes an edge it still needs, so the component is
    /// rolled back and retried folded at its second triangle.
    #[test]
    fn a_reference_fold_is_found_by_retrying_the_component() {
        let pts: Vec<i64> = (1..=30).collect();
        let mut is_ref = vec![false; 9];
        is_ref.extend([true, true, false, true, true, true]);
        let mut status = vec![0; 21];
        status[3..7].copy_from_slice(&[3, 3, 2, 0]);
        let m = run(&pts, &status, 7, &is_ref, &[0, 3, 6, 0, 3]).unwrap();
        assert_eq!(
            m.triangles[3..],
            [[0, 3, 9], [9, 3, 6], [9, 6, 0], [0, 6, 3]]
        );
    }

    /// Four triangles, the third's apex `(1, 3, 1)` off its parent's plane
    /// so the default rule sees no fold; `bits`/`angles` are the stored
    /// normals, `planar` the one face's flag.
    fn oriented(bits: &[u8], angles: &[i32], planar: bool) -> TriangleMesh {
        let binary: Vec<bool> = bits.iter().map(|&b| b == 1).collect();
        let pts = [
            0,
            0,
            0,
            4,
            0,
            0,
            -2,
            4,
            0,
            0,
            -3,
            i64::from(!planar),
            1,
            3,
            1,
        ];
        let mut is_ref = [false; 6];
        is_ref[5] = true;
        reconstruct(&Arrays {
            tolerance: 0.5,
            origin: [10.0, 0.0, 0.0],
            points: &pts,
            edge_status: &[3, 3, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            triangles: 4,
            is_reference: &is_ref,
            references: &[0],
            normals: Some(NormalArrays {
                bits: 10,
                binary: &binary,
                angles,
                planar: &[planar],
                face_of: &[0; 4],
            }),
        })
        .unwrap()
    }

    const FOLDED: [[u32; 3]; 4] = [[0, 1, 2], [2, 1, 3], [3, 1, 4], [3, 4, 0]];

    /// A record read at a triangle orients it exactly: the third
    /// triangle's fresh record at vertex 4 has `reversed` clear while its
    /// winding runs `max` to `min`, so it folds [WD 7.8.9.1]; vertex 3's
    /// set bit, read at `[2 1 3]`, keeps the second unfolded.
    #[test]
    fn a_reversed_bit_orients_the_walk() {
        let mut bits = Vec::new();
        for rev in [0, 0, 0, 1, 0] {
            bits.extend([0, rev, 0, 0]);
        }
        let angles: Vec<i32> = [0, 1023].repeat(5);
        let m = oriented(&bits, &angles, false);
        assert_eq!(m.triangles, FOLDED);
        assert_eq!(m.normals.len(), 5);
        // Without the normals the default rule leaves it unfolded.
        let plain = run(
            &[0, 0, 0, 4, 0, 0, -2, 4, 0, 0, -3, 1, 1, 3, 1],
            &[3, 3, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            4,
            &[false, false, false, false, false, true],
            &[0],
        )
        .unwrap();
        assert_eq!(plain.triangles[3], [4, 1, 0]);
    }

    /// On a planar face only the seed reads a record; the third triangle
    /// winds against that face normal, so it folds [WD 7.8.9].
    #[test]
    fn a_planar_face_normal_orients_the_walk() {
        let m = oriented(&[0, 0, 0, 0], &[0, 1023], true);
        assert_eq!(m.triangles, FOLDED);
        assert_eq!(m.triangle_normals, [[0; 3]; 4]);
    }

    /// Normals the oriented walk cannot use up send the mesh back to the
    /// default rule, which keeps its triangles and drops the normals.
    #[test]
    fn normals_that_do_not_fit_fall_back_to_the_default_fold() {
        let m = oriented(&[0, 0, 0, 0, 1], &[0, 1023], true);
        assert_eq!(m.triangles[3], [4, 1, 0]);
        assert!(m.normals.is_empty());
    }

    #[test]
    fn arrays_that_do_not_fit_are_refused() {
        let pts = [0; 9];
        let ok = [false; 3];
        assert!(run(&pts, &[0, 0, 0], 1, &ok, &[]).is_ok());
        assert!(run(&pts, &[0], 1, &ok, &[]).is_err(), "T form");
        assert!(
            run(&[0; 12], &[0, 0, 0], 1, &ok, &[]).is_err(),
            "points left"
        );
        assert!(
            run(&[0; 6], &[0, 0, 0], 1, &ok, &[]).is_err(),
            "points short"
        );
        assert!(
            run(&pts, &[0, 0, 0], 1, &[false; 4], &[]).is_err(),
            "slots left"
        );
        let fwd = [false, false, true];
        assert!(
            run(&[0; 6], &[0, 0, 0], 1, &fwd, &[7]).is_err(),
            "forward ref"
        );
    }

    /// `PrcPt::Length` rounds the squared length to `f32` and takes
    /// Newton's root from a halved exponent, which can settle an ulp off
    /// the correctly rounded root; `Unitize` refuses lengths below
    /// `FLT_EPSILON` [WD 12.3].
    #[test]
    fn length_and_unitize_follow_the_wd_pseudocode() {
        assert_eq!(wd_length([3.0, 4.0, 0.0]), Some(5.0));
        let newton = f64::from_bits(S.to_bits() - 1);
        assert_eq!(wd_length([1.0, 1.0, 0.0]), Some(newton));
        let long = 1.0 + 2f64.powi(-30);
        assert_eq!(wd_length([long, 0.0, 0.0]), Some(1.0));
        assert_eq!(unitize([0.0, long, 0.0]), Some([0.0, long, 0.0]));
        assert_eq!(unitize([1e-8, 0.0, 0.0]), None);
        assert_eq!(unitize([0.0; 3]), None);
    }

    /// A `W` on the edge's line leaves `Z` null, so the frame comes from
    /// `MakeOrthoRep(X)`: `Z` from `X x (0, 1, 0)`, or from `X x (1, 0, 0)`
    /// when `X` is along `Y` [WD 7.8.9.2, 12.3].
    #[test]
    fn a_null_z_takes_the_ortho_rep_of_x() {
        let d = [1.0, 2.0, 3.0];
        let p = apex([0.0; 3], [-2.0, 0.0, 0.0], [5.0, 0.0, 0.0], d);
        assert_eq!(p, [0.0, -2.0, -3.0]);
        let p = apex([0.0; 3], [0.0, -2.0, 0.0], [0.0, 5.0, 0.0], d);
        assert_eq!(p, [-2.0, 0.0, 3.0]);
    }

    /// A strip of thin, non-planar triangles compressed by an encoder
    /// written out from the WD: each point is expressed in the frame of the
    /// working mesh, rounded to the tolerance and reinjected [WD 7.8.9,
    /// 7.8.9.2], with `PrcPt::Length` and `Unitize` transcribed from the
    /// pseudocode [WD 12.3]. The decoder must land on that working mesh bit
    /// for bit, which keeps every vertex within the rounding bound of its
    /// source.
    #[test]
    fn a_strip_decodes_onto_the_encoders_working_mesh() {
        fn length(v: V) -> f64 {
            let f = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) as f32;
            let mut bits = f.to_bits();
            let exponent = (bits >> 23) & 0xff;
            if exponent > 127 {
                bits = (bits & 0x807f_ffff) | (((exponent - 127) / 2 + 127) << 23);
            }
            let (squared, mut x0) = (f64::from(f), f64::from(f32::from_bits(bits)));
            for _ in 0..100 {
                let xi = 0.5 * (x0 + squared / x0);
                if xi == x0 {
                    return xi;
                }
                x0 = xi;
            }
            -1.0
        }
        fn unitized(v: V) -> V {
            let l = length(v);
            assert!(l >= f64::from(f32::EPSILON), "null axis");
            [v[0] / l, v[1] / l, v[2] / l]
        }
        let (tol, n) = (1e-4, 48);
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut jitter = |scale: f64| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((state >> 11) as f64 / (1u64 << 53) as f64 - 0.5) * scale
        };
        // Two rails 4 apart, 0.05 between a rail's points, 0.02 of relief.
        let source: Vec<V> = (0..n)
            .map(|i| {
                let (along, rail) = (i as f64 * 0.025, (i % 2) as f64 * 4.0);
                [along + jitter(0.01), rail + jitter(0.01), jitter(0.02)]
            })
            .collect();
        let round = |v: f64| (v / tol).round() as i64;
        let (mut points, mut work) = (Vec::new(), Vec::<V>::new());
        for (i, target) in source.iter().take(3).enumerate() {
            let base = match i {
                0 => [0.0; 3],
                1 => work[0],
                _ => mul(add(work[0], work[1]), 0.5),
            };
            let d = [0, 1, 2].map(|c| round(target[c] - base[c]));
            points.extend(d);
            work.push(add(base, d.map(|c| c as f64 * tol)));
        }
        // Triangle k enters across vertices k and k + 1, opposite k - 1,
        // and continues right on odd k, left on even k, unless it folds.
        let mut status = vec![0; 3 * (n - 2)];
        status[0] = 2;
        for k in 1..n - 2 {
            let (lo, hi, w) = (work[k], work[k + 1], work[k - 1]);
            let o = mul(add(hi, lo), 0.5);
            let x = unitized(sub(lo, hi));
            let z = unitized(cross(sub(w, o), x));
            let y = unitized(cross(z, x));
            let r = sub(source[k + 2], o);
            let d = [round(dot(r, x)), round(-dot(r, y)), round(-dot(r, z))];
            points.extend(d);
            let [dx, dy, dz] = d.map(|c| c as f64 * tol);
            work.push(sub(sub(add(o, mul(x, dx)), mul(y, dy)), mul(z, dz)));
            if k < n - 3 {
                let fold = d[2] == 0 && d[1] > 0;
                status[k] = if (k % 2 == 1) != fold { 1 } else { 2 };
            }
        }
        let m = reconstruct(&Arrays {
            tolerance: tol,
            origin: [0.0; 3],
            points: &points,
            edge_status: &status,
            triangles: n - 2,
            is_reference: &vec![false; n],
            references: &[],
            normals: None,
        })
        .unwrap();
        assert_eq!(m.triangles[1..4], [[2, 1, 3], [2, 3, 4], [4, 3, 5]]);
        assert_eq!(m.positions, work);
        for (p, s) in m.positions.iter().zip(&source) {
            let off = dot(sub(*p, *s), sub(*p, *s)).sqrt() / tol;
            assert!(off < 0.87, "{off} tolerances off");
        }
    }
}
