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
//! - apex across `[P Q]` with the triangle's third vertex `W`: with
//!   `O = mid(P, Q)`, `X` the unit vector from the higher-numbered to the
//!   lower-numbered of `P`, `Q`, `Z = unit((W - O) x X)` and `Y = Z x X`,
//!   the apex is `O + d.x X - d.y Y - d.z Z`.
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

/// The decoded arrays of one `TESS_3D_Compressed`.
pub(crate) struct Arrays<'a> {
    pub(crate) tolerance: f64,
    pub(crate) origin: V,
    pub(crate) points: &'a [i64],
    pub(crate) edge_status: &'a [i32],
    pub(crate) triangles: usize,
    pub(crate) is_reference: &'a [bool],
    pub(crate) references: &'a [u32],
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
                        let o = mul(add(pp, qq), 0.5);
                        let x = if p > q {
                            unit(sub(qq, pp))
                        } else {
                            unit(sub(pp, qq))
                        };
                        let z = unit(cross(sub(ww, o), x));
                        let y = cross(z, x);
                        if let Some(&[_, dy, dz]) = a.points.get(self.pi..self.pi.saturating_add(3))
                        {
                            fold = dz == 0 && dy > 0;
                        }
                        let d = self.point(a)?;
                        self.pos
                            .push(add(o, sub(sub(mul(x, d[0]), mul(y, d[1])), mul(z, d[2]))));
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
    Ok(TriangleMesh {
        positions: pos,
        triangles: tris,
        faces: Vec::new(),
        normals_recalculated: false,
        triangle_graphics: Vec::new(),
    })
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
}
