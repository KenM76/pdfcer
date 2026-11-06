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
//! framed on `(max - min) x (R - min)` [WD 7.8.9.4]) unless that normal
//! lies in the triangle's own plane, where the bit is noise; the stored
//! normal of its planar face, compared geometrically unless the triangle
//! is a sliver; otherwise none. Without a normal the measured default
//! holds: a new apex lying exactly in its parent's plane on the parent's
//! side (`d.z == 0`, `d.y > 0`, a double-sided panel folding back over
//! itself) is folded. A component (a run started from an empty stack) that fails
//! to fit is rewound and retried with the fold inverted at its second
//! triangle, where most measured unsignalled folds sat; failing that, once
//! more inverted at the last sliver before its first failure. A sliver's
//! sign against its face normal is rounding noise: trusting it loses as
//! many measured meshes as it gains. A mesh whose oriented walk does not
//! fit is walked again on the default alone.
//!
//! Two further measured rules. An unsignalled triangle whose continuation
//! edge is already shared by two triangles, while the other edge is open,
//! continues across the open one: the walk never re-enters a closed edge.
//! And a planar face [WD 7.8.9.1] must rebuild flat: a component whose
//! planar-face vertices stray more than [`MAX_BEND`] tolerances from their
//! face's plane fits the arrays by accident and is treated as not fitting;
//! a vertex that leaves its face's running plane fails as it is placed
//! ([`bend`]), so a wrong fold there is caught within a few triangles.
//! A mesh no walk fits is searched last ([`search`]).
//!
//! Only the per-triangle `edge_status` form (three entries per triangle,
//! the first `T` read) is reconstructed; the one-entry-per-triangle form
//! walks differently and is not.

use std::collections::HashMap;

use crate::TriangleMesh;
use crate::vec3::{add, cross, dot, scale, sub};

mod bend;
mod decide;
mod normals;
mod partial;
mod search;

pub(crate) use normals::NormalArrays;
use normals::{NormalMark, NormalReader, Orientation, stored_normals};
pub use partial::PartialMesh;

type V = [f64; 3];

/// Below this sine of its angle at `A` a triangle is a sliver whose
/// winding no normal can orient: the WD assumes none [WD 7.8.9].
const MIN_SINE: f64 = 1e-6;

/// Below this in-plane height, in tolerances, a planar face's normal cannot
/// orient a triangle. The WD quantises each vertex to the tolerance and
/// requires every triangle to be taller than one [WD 7.8.9], so a decoded
/// triangle that is tall in space yet nearly flat *along the face* (its
/// apex frame tilted out of the face by an earlier sliver) has an in-face
/// area of about one quantum: the sign of its winding against the face
/// normal is noise. Measured on the fitting meshes of two CAD assemblies,
/// every signalled planar triangle sits at or above one tolerance
/// (2359/2362); the two false swaps that broke a mesh sat at 0.01 and 0.28.
const MIN_PLANE_HEIGHT: f64 = 0.5;

/// Below this cosine between a fresh record's normal and the triangle's
/// winding, the record's `triangle_normal_reversed` bit is noise. The
/// encoder sets the bit so the frame's Z agrees with the vertex normal
/// [WD 7.8.9.4]; a vertex normal lying in the triangle's plane (a needle
/// one quantum tall standing in a wall whose normal it carries) has no
/// side to agree with, and the bit records the sign of a rounding
/// residue. Measured on two CAD assemblies: every such noise bit sat
/// below 0.003, the nearest correct decision at 0.048.
const BLIND_NORMAL: f64 = 0.01;

/// The height of a triangle measured within the plane of normal `n`:
/// `|w . n| / |n|` is twice its area projected onto that plane, divided by
/// its longest edge. `s` is `w . n` for the winding `w`; zero when either
/// length is.
fn plane_height(s: f64, n: V, edges: [V; 3]) -> f64 {
    let long = edges.iter().map(|v| dot(*v, *v).sqrt()).fold(0.0, f64::max);
    let nn = dot(n, n).sqrt();
    if long > 0.0 && nn > 0.0 {
        s.abs() / nn / long
    } else {
        0.0
    }
}

/// How far, in tolerances, a planar face's vertices may stray from the
/// plane through their centroid. Measured: correct rebuilds stay under 10;
/// accidental fits bend by 96 and more.
const MAX_BEND: f64 = 20.0;

/// `a` over its unscaled length, or `a` itself when that is zero; kept
/// apart from [`crate::vec3::normalize`], which pre-scales vectors below
/// `1e-140` or above about `1e154`, so the walk's arithmetic stays as
/// measured against the encoders.
fn unit(a: V) -> V {
    let l = dot(a, a).sqrt();
    if l > 0.0 { scale(a, 1.0 / l) } else { a }
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

/// The `Z` of `PrcPt::MakeOrthoRep` [WD 12.3] on `X`:
/// `unitize(X x (0, 1, 0))`, or `X x (1, 0, 0)` when that is null, from
/// `X` unitized again as the pseudocode does. Only `Z` is taken: the frame
/// rebuilds `Y` from its own `X` (see [`apex`]).
fn make_ortho_rep_z(x: V) -> Option<V> {
    let x = unitize(x)?;
    unitize(cross(x, [0.0, 1.0, 0.0])).or_else(|| unitize(cross(x, [1.0, 0.0, 0.0])))
}

/// The apex across `[lo hi]` (`lo` the lower-numbered vertex) opposite `w`,
/// `d` already scaled by the tolerance [WD 7.8.9.2], and whether its frame
/// was degenerate. A null `Z` or `Y` takes `MakeOrthoRep(X)`'s `Z` as the
/// WD prescribes, and `Y = unitize(Z x X)` from the frame's own `X`, not
/// the copy `MakeOrthoRep` unitized again: `unitize` keeps a vector's
/// length only to about 1e-8 [WD 12.3 `Length`], the encoder's `Y` carries
/// its own `X`'s error, and the walk amplifies the difference to whole
/// tolerances within a thousand triangles (PDF_Spec
/// `prc__8137__tess_3d_compressed.md` X65). A null `X`, an edge the WD's
/// non-degeneracy rule excludes, is used as it is.
///
/// A degenerate frame is turned a half-turn about `X` when the sub-epsilon
/// residue of `(W - O) x X` points against `MakeOrthoRep`'s `Z`, kept when
/// it points along it, and turned by `turned` only when it is exactly zero:
/// the WD is silent, and real encoders measurably keep that residue's sign
/// (PDF_Spec `prc__8137__tess_3d_compressed.md` X18). `invert` reverses the
/// choice, for the search.
fn apex(lo: V, hi: V, w: V, d: V, turned: bool, invert: bool) -> (V, bool) {
    let o = scale(add(lo, hi), 0.5);
    let x = sub(lo, hi);
    let x = unitize(x).unwrap_or(x);
    let residue = cross(sub(w, o), x);
    let frame = unitize(residue).and_then(|z| Some((unitize(cross(z, x))?, z)));
    let degenerate = frame.is_none();
    let (y, z) = frame
        .or_else(|| {
            make_ortho_rep_z(x)
                .and_then(|z| Some((unitize(cross(z, x))?, z)))
                .map(|(y, z)| {
                    let along = dot(residue, z);
                    let turn = if along == 0.0 { turned } else { along < 0.0 };
                    if turn != invert {
                        (scale(y, -1.0), scale(z, -1.0))
                    } else {
                        (y, z)
                    }
                })
        })
        .unwrap_or_default();
    let p = sub(sub(add(o, scale(x, d[0])), scale(y, d[1])), scale(z, d[2]));
    (p, degenerate)
}

/// The decoded arrays of one `TESS_3D_Compressed`.
#[derive(Clone, Copy)]
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
    /// Whether a `MakeOrthoRep` frame whose residue is exactly zero is
    /// turned a half-turn about `X`; see [`apex`] and [`reconstruct`].
    pub(crate) ortho_turned: bool,
    /// Whether orienting ignores reused normals and treats a sub-tolerance
    /// triangle like any other; see [`reconstruct`].
    pub(crate) legacy_orient: bool,
}

/// Mutable traversal state; `log` records every edge-count increment so a
/// component can be rolled back and retried. `normals` is present when
/// the stored normals orient the walk.
struct Walk<'a> {
    pos: Vec<V>,
    tris: Vec<[u32; 3]>,
    /// The mesh tolerance, for [`MIN_PLANE_HEIGHT`].
    tol: f64,
    edges: HashMap<(u32, u32), u8>,
    log: Vec<(u32, u32)>,
    stack: Vec<(u32, u32, u32)>,
    next: Option<(u32, u32, u32)>,
    slot: usize,
    ri: usize,
    pi: usize,
    normals: Option<NormalReader<'a>>,
    /// The last triangle whose face normal could not orient it.
    sliver: Option<usize>,
    /// Whether the last triangle's fold came from its stored normals.
    signalled: bool,
    /// Whether the last triangle is a sliver, at most a tolerance from
    /// collinear, whose stored decision decoded noise may have corrupted.
    weak: bool,
    /// Whether the last triangle is [`Walk::weak`] and collinear to
    /// [`MIN_SINE`]: the one case a stored decision cannot orient, so the
    /// default fold stands and the search branches on it.
    thin: bool,
    /// Set by the search: reverse the next apex's degenerate-frame turn.
    turn: bool,
    /// Whether the last apex's frame was degenerate.
    degenerate: bool,
    /// Set by the best-fit search: orient a sliver by its corner normals
    /// (see [`Walk::fold`]).
    raw: bool,
    /// Set by the best-fit search: a stored normal that would continue
    /// across a closed edge while the other edge is open gives way, as an
    /// unsignalled fold does (see [`Walk::step`]).
    open_wins: bool,
    /// Whether the last triangle's fold came from a record's reversed bit.
    exact: bool,
    /// Set by the best-fit search: a fold [`Walk::exact`] did not decide is
    /// a search candidate too.
    loose: bool,
    /// Whether any apex so far, rewound or not, would move under the
    /// other [`Arrays::ortho_turned`].
    reads_turned: bool,
    /// Set by [`partial::decode`]: a failing [`walk_into`] leaves the walk
    /// as it stood before the step that failed.
    partial: bool,
    /// Set by [`partial::decode`]: an edge may be shared by any number of
    /// triangles, and stored normals that run out are dropped rather than
    /// failing the step.
    forgiving: bool,
    /// The running planes of the planar faces walked so far.
    planes: bend::Planes,
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
            tol: 0.0,
            edges: HashMap::new(),
            log: Vec::new(),
            stack: Vec::new(),
            next: None,
            slot: 0,
            ri: 0,
            pi: 0,
            normals,
            sliver: None,
            signalled: false,
            weak: false,
            thin: false,
            turn: false,
            degenerate: false,
            raw: false,
            open_wins: false,
            exact: false,
            loose: false,
            reads_turned: false,
            partial: false,
            forgiving: false,
            planes: bend::Planes::default(),
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
        self.degenerate = false;
        let invert = std::mem::take(&mut self.turn);
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
            let (v, degenerate) = apex(lo, hi, ww, d, a.ortho_turned, invert);
            if degenerate && !self.reads_turned {
                self.reads_turned = apex(lo, hi, ww, d, !a.ortho_turned, invert).0 != v;
            }
            self.degenerate = degenerate;
            self.pos.push(v);
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
                        _ => add(scale(add(self.get(v[0])?, self.get(v[1])?), 0.5), d),
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
    /// default `fold` stands, as it does when a fresh record's normal lies
    /// in the triangle's plane ([`BLIND_NORMAL`]), where its decision is
    /// noise and the triangle is left to the search. The WD requires every
    /// triangle's height to exceed the tolerance [WD 7.8.9]; a decoded one
    /// at most that thin ([`Walk::weak`]) still carries its winding unless
    /// it is collinear to [`MIN_SINE`] ([`Walk::thin`]), where quantisation
    /// has erased it and the default stands, unless `legacy`, as it does
    /// for one nearly flat along its planar face ([`MIN_PLANE_HEIGHT`]).
    /// With [`Walk::raw`], such a triangle and a sliver are instead
    /// oriented by the sum of their corner normals, where the triangle has
    /// any.
    fn fold(&mut self, t: [u32; 3], fold: bool, legacy: bool) -> Result<bool, String> {
        let ti = self.tris.len();
        let Some(r) = self.normals.as_mut() else {
            return Ok(fold);
        };
        let Some(o) = r.read(ti, t, &self.pos) else {
            if self.forgiving {
                self.normals = None;
                return Ok(fold);
            }
            return Err("the stored normals run out".into());
        };
        let corners = if self.raw {
            r.corner_sum(&self.pos)
        } else {
            [0.0; 3]
        };
        let thin = self.thin && !legacy;
        if self.raw && !legacy && !matches!(o, Orientation::Unknown) {
            let (a, b, c) = (self.get(t[0])?, self.get(t[1])?, self.get(t[2])?);
            let (ab, ac) = (sub(b, a), sub(c, a));
            let w = cross(ab, ac);
            let sine = (dot(w, w) / (dot(ab, ab) * dot(ac, ac))).sqrt();
            if thin || sine.is_nan() || sine <= MIN_SINE {
                let s = dot(w, corners);
                return Ok(if s == 0.0 { fold } else { s < 0.0 });
            }
        }
        self.decide(o, t, fold, legacy, thin)
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
            if n > 2 && !self.forgiving {
                return Err("an edge is shared by more than two triangles".into());
            }
        }
        self.signalled = false;
        self.exact = false;
        {
            let (p, q, r) = (self.get(ta)?, self.get(tb)?, self.get(tc)?);
            let (pq, pr) = (sub(q, p), sub(r, p));
            let long = [pq, sub(r, q), sub(p, r)]
                .iter()
                .map(|v| dot(*v, *v).sqrt())
                .fold(0.0, f64::max);
            let w = cross(pq, pr);
            let sine = (dot(w, w) / (dot(pq, pq) * dot(pr, pr))).sqrt();
            self.weak = long > 0.0 && dot(w, w).sqrt() / long <= a.tolerance;
            self.thin = self.weak && (sine.is_nan() || sine <= MIN_SINE);
        }
        self.tol = a.tolerance;
        let fold = self.fold(tri, fold, a.legacy_orient)?;
        if !self.forgiving
            && let Some(n) = a.normals.as_ref()
        {
            let t = [self.get(ta)?, self.get(tb)?, self.get(tc)?];
            let w = cross(sub(t[1], t[0]), sub(t[2], t[0]));
            let oriented = (!self.weak).then(|| if fold != flip { scale(w, -1.0) } else { w });
            self.planes
                .place(n, self.tris.len(), t, a.tolerance, oriented)?;
        }
        self.tris.push(tri);
        let (mut left, mut right) = ((tc, tb, ta), (ta, tc, tb));
        if fold != flip {
            std::mem::swap(&mut left, &mut right);
        }
        let closed = |e: (u32, u32, u32)| self.edges.get(&key(e.0, e.1)).copied().unwrap_or(0) >= 2;
        let taken = if status & 2 != 0 { left } else { right };
        let other = if status & 2 != 0 { right } else { left };
        if (!self.signalled || self.open_wins) && status & 3 != 0 && closed(taken) && !closed(other)
        {
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

/// The mesh the stored normals orient, if every array and normal is
/// consumed exactly.
fn oriented_fit(a: &Arrays<'_>) -> Option<TriangleMesh> {
    let w = walk(a, Some(NormalReader::new(a.normals.as_ref()?))).ok()?;
    let Walk {
        pos, tris, normals, ..
    } = w;
    let stored = normals.and_then(|r| r.finish(&pos))?;
    Some(mesh(pos, tris, stored))
}

/// Rebuilds the mesh, or says why the arrays do not fit the traversal.
/// Every slot, reference and point must be consumed exactly.
///
/// A mesh with stored normals is walked oriented by them, which must then
/// be consumed exactly too; failing that, it is walked again on the
/// default fold alone and its normals read afterwards, kept only if they
/// fit.
///
/// The WD's `MakeOrthoRep` fixes an apex frame's axis but not which way
/// round the encoder had it: some producers' meshes fit only with that
/// frame turned a half-turn about `X`. The WD frame is tried first at each
/// stage; the turned one is kept only as a fit the stored normals confirm.
///
/// A rebuilt mesh and whether it is a best fit, or why none was rebuilt.
pub(crate) type Rebuilt = Result<(TriangleMesh, bool), NotRebuilt>;

/// Why no mesh was rebuilt, and what decoded regardless.
#[derive(Debug)]
pub(crate) struct NotRebuilt {
    pub(crate) why: String,
    /// The triangles that decoded anyway; `None` when the arrays are in a
    /// form the traversal does not read, or not even a seed decodes.
    pub(crate) partial: Option<Box<PartialMesh>>,
}

/// [`reconstruct`], falling back to [`search::best_fit`]; the flag is true
/// for a best fit, which another geometry might match as well. When
/// neither fits, [`partial::decode`] salvages what it can.
pub(crate) fn rebuild(a: &Arrays<'_>) -> Rebuilt {
    let why = match reconstruct(a) {
        Ok(m) => return Ok((m, false)),
        Err(e) => e,
    };
    if a.edge_status.len() != a.triangles.saturating_mul(3) {
        return Err(NotRebuilt { why, partial: None });
    }
    match search::best_fit(a) {
        Some(m) => Ok((m, true)),
        None => Err(NotRebuilt {
            partial: partial::decode(a, &why).map(Box::new),
            why,
        }),
    }
}

/// A mesh no stage fits is retried with [`Arrays::legacy_orient`]: some
/// producers' meshes fit only that looser reading.
pub(crate) fn reconstruct(a: &Arrays<'_>) -> Result<TriangleMesh, String> {
    if a.edge_status.len() != a.triangles.saturating_mul(3) {
        return Err("the one-status-per-triangle edge form is not reconstructed".into());
    }
    reconstruct_oriented(a).or_else(|e| {
        let legacy = Arrays {
            legacy_orient: true,
            ..*a
        };
        reconstruct_oriented(&legacy).map_err(|_| e)
    })
}

fn reconstruct_oriented(a: &Arrays<'_>) -> Result<TriangleMesh, String> {
    if let Some(m) = oriented_fit(a) {
        return Ok(m);
    }
    let turned = Arrays {
        ortho_turned: true,
        ..*a
    };
    let Walk { pos, tris, .. } = match walk(a, None) {
        Ok(w) => match oriented_fit(&turned) {
            Some(m) => return Ok(m),
            None => w,
        },
        Err(e) => {
            return search::unique_fit_either(a, &turned).ok_or(e);
        }
    };
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
        triangle_faces: Vec::new(),
        uvs: Vec::new(),
        triangle_uvs: Vec::new(),
    }
}

/// Walks every component. One that fails to fit is rewound and retried
/// with the fold inverted at its second triangle: a fold whose apex is a
/// reference, or whose triangle carries no normal signal, has no other
/// trace in the arrays. Should that fail too, it is retried once more
/// inverted at the last sliver its first walk met before failing (see
/// [`MIN_SINE`]). A component whose planar faces bend (see [`MAX_BEND`])
/// counts as failing.
fn walk<'a>(a: &Arrays<'_>, normals: Option<NormalReader<'a>>) -> Result<Walk<'a>, String> {
    let mut w = Walk::new(a.triangles, normals);
    walk_into(a, &mut w)?;
    if w.slot != a.is_reference.len() || w.ri != a.references.len() || w.pi != a.points.len() {
        return Err("the decoded triangles do not use up the stored arrays".into());
    }
    Ok(w)
}

/// Walks every component into `w` (see [`walk`]). Under [`Walk::partial`]
/// a failure leaves `w` as it stood before the step that failed, so the
/// caller can take that step another way.
fn walk_into(a: &Arrays<'_>, w: &mut Walk<'_>) -> Result<(), String> {
    let t = a.triangles;
    let mut start = w.mark(0);
    let (mut flip_at, mut sliver) = (None, None);
    let mut i = 0;
    while i < t {
        if w.next.is_none() && !(flip_at.is_some() && i == start.tri) {
            start = w.mark(i);
            flip_at = None;
        }
        let before = w.partial.then(|| (w.mark(i), w.next, w.stack.clone()));
        let status = a.edge_status.get(i).copied().unwrap_or(0);
        let stepped = w.step(a, status, flip_at == Some(i)).and_then(|()| {
            if w.next.is_none() && !search::flat(a, &w.pos, &w.tris, start.tri) {
                Err("a planar face does not rebuild flat".to_string())
            } else {
                Ok(())
            }
        });
        match stepped {
            Ok(()) => i += 1,
            Err(e) => {
                let retry = match flip_at {
                    _ if i == start.tri => None,
                    None => {
                        sliver = w.sliver.filter(|&s| s > start.tri + 1);
                        Some(start.tri + 1)
                    }
                    Some(f) if f == start.tri + 1 => sliver.take(),
                    Some(_) => None,
                };
                let Some(f) = retry else {
                    if let Some((m, next, stack)) = before {
                        w.rewind(m);
                        (w.next, w.stack) = (next, stack);
                    }
                    return Err(e);
                };
                w.rewind(start);
                flip_at = Some(f);
                i = start.tri;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)] // Tests fail loudly.
mod tests;
