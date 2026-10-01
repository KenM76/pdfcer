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
//! Fold. A triangle whose new apex lies exactly in its parent's plane on
//! the parent's side (`d.z == 0`, `d.y > 0`: a double-sided panel folding
//! back over itself) swaps bit 1 and bit 0 for its own continuation. This
//! is measured on zero-thickness panels. A fold whose apex is a reference
//! carries no such signal: a component (a run started from an empty stack)
//! that fails to fit is rewound and retried once with the fold inverted at
//! its second triangle, which is where every measured reference fold sat.
//!
//! Only the per-triangle `edge_status` form (three entries per triangle,
//! the first `T` read) is reconstructed; the one-entry-per-triangle form
//! walks differently and is not.

use std::collections::HashMap;

use crate::TriangleMesh;

type V = [f64; 3];

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
/// component can be rolled back and retried.
struct Walk {
    pos: Vec<V>,
    tris: Vec<[u32; 3]>,
    edges: HashMap<(u32, u32), u8>,
    log: Vec<(u32, u32)>,
    stack: Vec<(u32, u32, u32)>,
    next: Option<(u32, u32, u32)>,
    slot: usize,
    ri: usize,
    pi: usize,
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
}

impl Walk {
    fn mark(&self, tri: usize) -> Mark {
        Mark {
            tri,
            pos: self.pos.len(),
            log: self.log.len(),
            slot: self.slot,
            ri: self.ri,
            pi: self.pi,
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
            .ok_or_else(|| format!("reference to vertex {v} before it exists"))
    }

    /// Adds one triangle; `flip` inverts the fold decision.
    fn step(&mut self, a: &Arrays<'_>, status: i32, flip: bool) -> Result<(), String> {
        let mut fold = false;
        let tri = match self.next.take() {
            Some((p, q, w)) => {
                let r = match self.take(a)? {
                    Some(r) => r,
                    None => {
                        let (pp, qq, ww) = (self.get(p)?, self.get(q)?, self.get(w)?);
                        let (lo, hi) = if p < q { (pp, qq) } else { (qq, pp) };
                        if let Some(&[_, dy, dz]) = a.points.get(self.pi..self.pi.saturating_add(3))
                        {
                            fold = dz == 0 && dy > 0;
                        }
                        let d = self.point(a)?;
                        self.pos.push(apex(lo, hi, ww, d));
                        (self.pos.len() - 1) as u32
                    }
                };
                [p, q, r]
            }
            None => {
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
                v
            }
        };
        let [ta, tb, tc] = tri;
        for (x, y) in [(ta, tb), (tb, tc), (tc, ta)] {
            let n = self.edges.entry(key(x, y)).or_insert(0);
            *n = n.saturating_add(1);
            let n = *n;
            self.log.push(key(x, y));
            if n > 2 {
                return Err(format!("edge {x}-{y} shared by more than two triangles"));
            }
        }
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
pub(crate) fn reconstruct(a: &Arrays<'_>) -> Result<TriangleMesh, String> {
    let t = a.triangles;
    if a.edge_status.len() != t.saturating_mul(3) {
        return Err("the one-status-per-triangle edge form is not reconstructed".into());
    }
    let mut w = Walk {
        pos: Vec::new(),
        tris: Vec::with_capacity(t.min(1 << 20)),
        edges: HashMap::new(),
        log: Vec::new(),
        stack: Vec::new(),
        next: None,
        slot: 0,
        ri: 0,
        pi: 0,
    };
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
    let Walk {
        pos,
        tris,
        slot,
        ri,
        pi,
        ..
    } = w;
    if slot != a.is_reference.len() || ri != a.references.len() || pi != a.points.len() {
        return Err(format!(
            "arrays left over: slots {slot}/{}, references {ri}/{}, point values {pi}/{}",
            a.is_reference.len(),
            a.references.len(),
            a.points.len()
        ));
    }
    let (normals, triangle_normals) = a
        .normals
        .as_ref()
        .and_then(|n| stored_normals(&pos, &tris, n))
        .unwrap_or_default();
    Ok(TriangleMesh {
        positions: pos,
        triangles: tris,
        faces: Vec::new(),
        normals_recalculated: false,
        normals,
        triangle_normals,
        triangle_graphics: Vec::new(),
    })
}

/// The stored-normal arrays of a mesh that does not ask for recalculation
/// [WD 7.8.9.3].
pub(crate) struct NormalArrays<'a> {
    /// `normal_angle_number_of_bits`.
    pub(crate) bits: u32,
    /// `normal_binary_data`.
    pub(crate) binary: &'a [bool],
    /// `normal_angle_array`: theta then phi per stored normal.
    pub(crate) angles: &'a [i32],
    /// `is_face_planar`, per face.
    pub(crate) planar: &'a [bool],
    /// The face of each triangle.
    pub(crate) face_of: &'a [u32],
}

/// Width in bits of a normal reference on a vertex that already stores `n`
/// normals: one more than the bit length of `n - 2`, and 1 for `n <= 2`.
/// The WD's width rule is ambiguous; this formula is measured on a real
/// file (every count from 1 to 6, plus 8, 12 and 14) — spec RAG
/// `prc__8137__tess_3d_compressed.md` §5a N5.
fn reference_bits(n: usize) -> u32 {
    1 + (usize::BITS - n.saturating_sub(2).leading_zeros())
}

/// One stored normal record: `triangle_normal_reversed`, `x_is_reversed`,
/// `y_is_reversed`, theta, phi, and the triangle whose frame it is in.
type Record = (bool, bool, bool, i32, i32, [u32; 3]);

/// Decodes the stored vertex normals [WD 7.8.9.3-7.8.9.4], returning
/// `(normals, triangle_normals)` in [`TriangleMesh`]'s shape, or `None`
/// when the arrays do not fit (the mesh then has no stored normals).
///
/// Each triangle `[P Q R]`, entered across `[P Q]`, visits its corners as
/// `min(P, Q)`, `max(P, Q)`, `R`. A vertex met for the first time reads
/// `has_multiple_normal` and a record. Met again, a vertex without
/// multiple normals reads nothing and reuses its normal (the WD's prose,
/// not its pseudocode); one with multiple normals reads `is_a_reference`
/// and then either a record or a reference, an index counted back from
/// the vertex's most recently stored normal. On a planar face only the
/// first corner of its first triangle is visited, and every corner of the
/// face takes that normal. A record is three bits (reversed, x reversed,
/// y reversed) and two angles. These rules are measured on a real file:
/// they consume both arrays exactly on 347 of its 348 meshes.
fn stored_normals(
    pos: &[V],
    tris: &[[u32; 3]],
    a: &NormalArrays<'_>,
) -> Option<(Vec<V>, Vec<[u32; 3]>)> {
    let mut bits = a.binary.iter().copied();
    let mut angles = a.angles.iter().copied();
    let mut records: Vec<Record> = Vec::new();
    let mut by_vertex: Vec<(bool, Vec<u32>)> = vec![(false, Vec::new()); pos.len()];
    let mut by_face: Vec<Option<u32>> = vec![None; a.planar.len()];
    let mut corners = Vec::with_capacity(tris.len());
    for (ti, t) in tris.iter().enumerate() {
        let face = *a.face_of.get(ti)? as usize;
        let planar = a.planar.get(face).copied().unwrap_or(false);
        if planar && let Some(n) = by_face.get(face).copied().flatten() {
            corners.push([n; 3]);
            continue;
        }
        let lo = usize::from(t[0] > t[1]);
        let (p, q) = if lo == 0 { (t[0], t[1]) } else { (t[1], t[0]) };
        let order = [(lo, p), (1 - lo, q), (2, t[2])];
        let mut out = [0u32; 3];
        for (slot, v) in order {
            let (multi, stored) = by_vertex.get_mut(v as usize)?;
            let mut record = |bits: &mut dyn Iterator<Item = bool>| -> Option<u32> {
                let r = (bits.next()?, bits.next()?, bits.next()?);
                let (th, ph) = (angles.next()?, angles.next()?);
                records.push((r.0, r.1, r.2, th, ph, *t));
                u32::try_from(records.len() - 1).ok()
            };
            let n = if stored.is_empty() {
                *multi = bits.next()?;
                let n = record(&mut bits)?;
                stored.push(n);
                n
            } else if !*multi {
                *stored.first()?
            } else if bits.next()? {
                let w = reference_bits(stored.len());
                let mut idx = 0usize;
                for i in 0..w {
                    idx |= usize::from(bits.next()?) << i;
                }
                *stored.get(stored.len().checked_sub(1 + idx)?)?
            } else {
                let n = record(&mut bits)?;
                stored.push(n);
                n
            };
            if planar {
                *by_face.get_mut(face)? = Some(n);
                out = [n; 3];
                break;
            }
            *out.get_mut(slot)? = n;
        }
        corners.push(out);
    }
    if bits.next().is_some() || angles.next().is_some() {
        return None;
    }
    let step = std::f64::consts::FRAC_PI_2 / f64::from((1u32 << a.bits).saturating_sub(1).max(1));
    let normals = records
        .iter()
        .map(|&(rev, xr, yr, th, ph, t)| decode(pos, t, rev, xr, yr, th, ph, step))
        .collect();
    Some((normals, corners))
}

/// One record's normal in model space: the local frame of the triangle it
/// was read in [WD 7.8.9.4], `Z` its normal (reversed when flagged), and
/// the spherical angles `(cos phi cos theta, cos phi sin theta, sin phi)`
/// with the flagged axes negated. A degenerate triangle gives the zero
/// vector, which the renderer treats as no normal.
#[allow(clippy::too_many_arguments)] // One record's fields, decoded together.
fn decode(pos: &[V], t: [u32; 3], rev: bool, xr: bool, yr: bool, th: i32, ph: i32, step: f64) -> V {
    let (lo, hi) = (t[0].min(t[1]), t[0].max(t[1]));
    let p = |i: u32| pos.get(i as usize).copied().unwrap_or([0.0; 3]);
    let (p0, p1, p2) = (p(lo), p(hi), p(t[2]));
    let (v1, v2, v3) = (unit(sub(p1, p0)), unit(sub(p2, p0)), unit(sub(p2, p1)));
    let half = std::f64::consts::FRAC_PI_2;
    let angle = |a: V, b: V| dot(a, b).clamp(-1.0, 1.0).acos() - half;
    let (t1, t2, t3) = (
        angle(v1, v2),
        angle(v3, mul(v1, -1.0)),
        angle(mul(v2, -1.0), mul(v3, -1.0)),
    );
    let (x, z) = if t1 < t2 && t1 < t3 {
        (v1, cross(v1, v2))
    } else if t2 < t3 {
        (v3, mul(cross(v3, v1), -1.0))
    } else {
        (mul(v2, -1.0), cross(v2, v3))
    };
    let z = unit(z);
    if dot(z, z) < 0.5 {
        return [0.0; 3];
    }
    let z = if rev { mul(z, -1.0) } else { z };
    let y = cross(z, x);
    let (th, ph) = (f64::from(th) * step, f64::from(ph) * step);
    let lx = ph.cos() * th.cos() * if xr { -1.0 } else { 1.0 };
    let ly = ph.cos() * th.sin() * if yr { -1.0 } else { 1.0 };
    add(add(mul(x, lx), mul(y, ly)), mul(z, ph.sin()))
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

    fn normals(
        pos: &[V],
        tris: &[[u32; 3]],
        binary: &[u8],
        angles: &[i32],
        planar: bool,
    ) -> Option<(Vec<V>, Vec<[u32; 3]>)> {
        let binary: Vec<bool> = binary.iter().map(|&b| b == 1).collect();
        stored_normals(
            pos,
            tris,
            &NormalArrays {
                bits: 10,
                binary: &binary,
                angles,
                planar: &[planar],
                face_of: &vec![0; tris.len()],
            },
        )
    }

    const QUAD: [V; 4] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 1.0, 0.0],
    ];

    /// Corners are read `min`, `max`, apex but returned in the triangle's
    /// own order. Each record lands in the frame `X = -V2`, `Z` the face
    /// normal: phi 0 points along `X`, phi at full scale along `Z`, and
    /// the reversed flag flips `Z`.
    #[test]
    fn records_decode_in_the_triangle_frame() {
        let (n, c) = normals(
            &QUAD,
            &[[1, 0, 2]],
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0],
            &[0, 0, 0, 1023, 0, 1023],
            false,
        )
        .unwrap();
        assert_eq!(c, [[1, 0, 2]]);
        assert!(close(n[0], [0.0, -1.0, 0.0]), "{:?}", n[0]);
        assert!(close(n[1], [0.0, 0.0, 1.0]), "{:?}", n[1]);
        assert!(close(n[2], [0.0, 0.0, -1.0]), "{:?}", n[2]);
    }

    /// A vertex with multiple normals references one by index from its
    /// most recent, or stores another; a vertex with one normal reuses it
    /// without reading; leftover bits refuse the whole mesh.
    #[test]
    fn references_reuse_and_new_records() {
        let tris = [[0, 1, 2], [2, 1, 3]];
        let first = [0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0];
        let a = [0, 1023];
        let angles: Vec<i32> = a.iter().cycle().take(8).copied().collect();
        let mut bits = first.to_vec();
        bits.extend([1, 0, 0, 0, 0, 0]);
        let (n, c) = normals(&QUAD, &tris, &bits, &angles, false).unwrap();
        assert_eq!(c, [[0, 1, 2], [2, 1, 3]]);
        assert_eq!(n.len(), 4);
        let mut bits = first.to_vec();
        bits.extend([0, 0, 0, 0, 0, 0, 0, 0]);
        let angles: Vec<i32> = a.iter().cycle().take(10).copied().collect();
        let (n, c) = normals(&QUAD, &tris, &bits, &angles, false).unwrap();
        assert_eq!(c, [[0, 1, 2], [2, 3, 4]]);
        assert_eq!(n.len(), 5);
        // Index 0 is vertex 1's most recent normal, 3, not its first.
        let mut more = tris.to_vec();
        more.push([3, 1, 0]);
        let mut three = bits.clone();
        three.extend([1, 0]);
        let (_, c) = normals(&QUAD, &more, &three, &angles, false).unwrap();
        assert_eq!(c[2], [4, 3, 0]);
        bits.push(0);
        assert!(normals(&QUAD, &tris, &bits, &angles, false).is_none());
    }

    /// The reference width at every measured stored count.
    #[test]
    fn reference_width_matches_the_measured_counts() {
        let measured = [
            (1, 1),
            (2, 1),
            (3, 2),
            (4, 3),
            (5, 3),
            (6, 4),
            (8, 4),
            (12, 5),
            (14, 5),
        ];
        for (n, w) in measured {
            assert_eq!(reference_bits(n), w, "n = {n}");
        }
    }

    /// A planar face reads one record, at the first corner of its first
    /// triangle, and every corner takes it.
    #[test]
    fn a_planar_face_stores_one_normal() {
        let tris = [[0, 1, 2], [2, 1, 3]];
        let (n, c) = normals(&QUAD, &tris, &[0, 0, 0, 0], &[0, 1023], true).unwrap();
        assert_eq!(c, [[0; 3]; 2]);
        assert!(close(n[0], [0.0, 0.0, 1.0]), "{:?}", n[0]);
    }
}
