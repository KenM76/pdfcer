//! The stored normals of a `TESS_3D_Compressed`, read triangle by
//! triangle as the traversal produces them [WD 7.8.9.3-7.8.9.4].
//!
//! Each triangle `[P Q R]`, entered across `[P Q]`, visits its corners as
//! `min(P, Q)`, `max(P, Q)`, `R`. A vertex met for the first time reads
//! `has_multiple_normal` and a record. Met again, a vertex without
//! multiple normals reads nothing and reuses its normal (the WD's prose,
//! not its pseudocode); one with multiple normals reads `is_a_reference`
//! and then either a record or a reference, an index counted back from
//! the vertex's most recently stored normal. On a planar face only the
//! first corner of its first triangle is visited, and every corner of the
//! face takes that normal. A record is three bits (reversed, x reversed,
//! y reversed) and two angles. These rules are measured on real files;
//! the spec RAG's `prc__8137__tess_3d_compressed.md` §5a records them.
//!
//! Read in step with the walk, the records also orient it: see
//! [`Orientation`].

use super::{V, add, cross, dot, mul, sub, unit};

/// The stored-normal arrays of a mesh that does not ask for recalculation
/// [WD 7.8.9.3].
#[derive(Clone, Copy)]
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

/// What a triangle's stored normals say about its winding.
///
/// "The triangle normal is determined by cross-product on its vertices,
/// oriented in conjunction with one of its 3 normals", and left and right
/// for the continuation follow from that normal [WD 7.8.9, 7.8.9.1].
pub(super) enum Orientation {
    /// A record read at this triangle: its `triangle_normal_reversed`,
    /// which is relative to `(max - min) x (R - min)` of the triangle's
    /// own corners [WD 7.8.9.4], so it gives the orientation exactly.
    Reversed(bool),
    /// The normal of a planar face read at an earlier triangle.
    Normal(V),
    /// The sum of a curved triangle's corner normals, every one read at an
    /// earlier triangle: the WD orients each triangle by "one of its 3
    /// normals" whether read fresh or reused [WD 7.8.9].
    Reused(V),
    /// Nothing new was read and no face normal applies.
    Unknown,
}

enum Undo {
    First(u32),
    Pushed(u32),
    Face(usize),
}

/// Reader state to roll back to.
#[derive(Clone, Copy)]
pub(super) struct NormalMark {
    bit: usize,
    angle: usize,
    records: usize,
    corners: usize,
    undo: usize,
}

/// Reads the stored normals one triangle at a time.
pub(super) struct NormalReader<'a> {
    a: &'a NormalArrays<'a>,
    bit: usize,
    angle: usize,
    records: Vec<Record>,
    by_vertex: Vec<(bool, Vec<u32>)>,
    by_face: Vec<Option<u32>>,
    corners: Vec<[u32; 3]>,
    undo: Vec<Undo>,
}

impl<'a> NormalReader<'a> {
    /// A reader at the start of `a`'s bit and angle streams.
    pub(super) fn new(a: &'a NormalArrays<'a>) -> Self {
        Self {
            a,
            bit: 0,
            angle: 0,
            records: Vec::new(),
            by_vertex: Vec::new(),
            by_face: vec![None; a.planar.len()],
            corners: Vec::new(),
            undo: Vec::new(),
        }
    }

    /// The reader's position, for rewinding a component that fails to fit.
    pub(super) fn mark(&self) -> NormalMark {
        NormalMark {
            bit: self.bit,
            angle: self.angle,
            records: self.records.len(),
            corners: self.corners.len(),
            undo: self.undo.len(),
        }
    }

    /// Returns to `m`, forgetting every vertex and face first read since.
    pub(super) fn rewind(&mut self, m: NormalMark) {
        for u in self.undo.drain(m.undo..).rev() {
            match u {
                Undo::First(v) => {
                    if let Some(s) = self.by_vertex.get_mut(v as usize) {
                        *s = (false, Vec::new());
                    }
                }
                Undo::Pushed(v) => {
                    if let Some(s) = self.by_vertex.get_mut(v as usize) {
                        s.1.pop();
                    }
                }
                Undo::Face(f) => {
                    if let Some(s) = self.by_face.get_mut(f) {
                        *s = None;
                    }
                }
            }
        }
        (self.bit, self.angle) = (m.bit, m.angle);
        self.records.truncate(m.records);
        self.corners.truncate(m.corners);
    }

    fn bit(&mut self) -> Option<bool> {
        let b = *self.a.binary.get(self.bit)?;
        self.bit += 1;
        Some(b)
    }

    fn record(&mut self, t: [u32; 3]) -> Option<u32> {
        let r = (self.bit()?, self.bit()?, self.bit()?);
        let th = *self.a.angles.get(self.angle)?;
        let ph = *self.a.angles.get(self.angle + 1)?;
        self.angle += 2;
        self.records.push((r.0, r.1, r.2, th, ph, t));
        u32::try_from(self.records.len() - 1).ok()
    }

    /// The normal at corner `v` of `t`, and whether a record was read.
    fn corner(&mut self, v: u32, t: [u32; 3]) -> Option<(u32, bool)> {
        let vi = v as usize;
        if self.by_vertex.len() <= vi {
            self.by_vertex.resize(vi + 1, (false, Vec::new()));
        }
        let (multi, count) = self.by_vertex.get(vi).map(|s| (s.0, s.1.len()))?;
        let stored = |me: &Self, back: usize| -> Option<u32> {
            let s = &me.by_vertex.get(vi)?.1;
            s.get(s.len().checked_sub(1 + back)?).copied()
        };
        if count == 0 {
            let m = self.bit()?;
            let n = self.record(t)?;
            *self.by_vertex.get_mut(vi)? = (m, vec![n]);
            self.undo.push(Undo::First(v));
            Some((n, true))
        } else if !multi {
            Some((stored(self, count - 1)?, false))
        } else if self.bit()? {
            let mut idx = 0usize;
            for i in 0..reference_bits(count) {
                idx |= usize::from(self.bit()?) << i;
            }
            Some((stored(self, idx)?, false))
        } else {
            let n = self.record(t)?;
            self.by_vertex.get_mut(vi)?.1.push(n);
            self.undo.push(Undo::Pushed(v));
            Some((n, true))
        }
    }

    /// Reads triangle `ti`, `t`, entered across `[t0 t1]`; `None` when the
    /// arrays run out.
    pub(super) fn read(&mut self, ti: usize, t: [u32; 3], pos: &[V]) -> Option<Orientation> {
        let face = *self.a.face_of.get(ti)? as usize;
        let planar = self.a.planar.get(face).copied().unwrap_or(false);
        if planar && let Some(n) = self.by_face.get(face).copied().flatten() {
            self.corners.push([n; 3]);
            return Some(Orientation::Normal(self.normal(n, pos)));
        }
        let lo = usize::from(t[0] > t[1]);
        let (p, q) = if lo == 0 { (t[0], t[1]) } else { (t[1], t[0]) };
        let mut out = [0u32; 3];
        let mut fresh = None;
        for (slot, v) in [(lo, p), (1 - lo, q), (2, t[2])] {
            let (n, new) = self.corner(v, t)?;
            if new && fresh.is_none() {
                fresh = self.records.get(n as usize).map(|r| r.0);
            }
            if planar {
                *self.by_face.get_mut(face)? = Some(n);
                self.undo.push(Undo::Face(face));
                out = [n; 3];
                break;
            }
            *out.get_mut(slot)? = n;
        }
        self.corners.push(out);
        if let Some(rev) = fresh {
            return Some(Orientation::Reversed(rev));
        }
        if planar {
            return Some(Orientation::Unknown);
        }
        let n = out
            .iter()
            .fold([0.0; 3], |n, &c| add(n, self.normal(c, pos)));
        Some(Orientation::Reused(n))
    }

    fn step(&self) -> f64 {
        std::f64::consts::FRAC_PI_2 / f64::from((1u32 << self.a.bits).saturating_sub(1).max(1))
    }

    fn normal(&self, n: u32, pos: &[V]) -> V {
        self.records
            .get(n as usize)
            .map_or([0.0; 3], |r| decode(pos, r, self.step()))
    }

    /// `(normals, triangle_normals)` in [`crate::TriangleMesh`]'s shape,
    /// or `None` when bits or angles are left over.
    pub(super) fn finish(self, pos: &[V]) -> Option<(Vec<V>, Vec<[u32; 3]>)> {
        if self.bit != self.a.binary.len() || self.angle != self.a.angles.len() {
            return None;
        }
        let step = self.step();
        let normals = self.records.iter().map(|r| decode(pos, r, step)).collect();
        Some((normals, self.corners))
    }
}

/// Reads the stored normals of an already reconstructed mesh, or `None`
/// when the arrays do not fit it.
pub(super) fn stored_normals(
    pos: &[V],
    tris: &[[u32; 3]],
    a: &NormalArrays<'_>,
) -> Option<(Vec<V>, Vec<[u32; 3]>)> {
    let mut r = NormalReader::new(a);
    for (ti, t) in tris.iter().enumerate() {
        r.read(ti, *t, pos)?;
    }
    r.finish(pos)
}

/// One record's normal in model space: the local frame of the triangle it
/// was read in [WD 7.8.9.4], `Z` its normal (reversed when flagged), and
/// the spherical angles `(cos phi cos theta, cos phi sin theta, sin phi)`
/// with the flagged axes negated. A degenerate triangle gives the zero
/// vector, which the renderer treats as no normal.
fn decode(pos: &[V], r: &Record, step: f64) -> V {
    let &(rev, xr, yr, th, ph, t) = r;
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

    fn close(a: V, b: V) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-9)
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
