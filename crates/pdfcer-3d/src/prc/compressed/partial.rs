//! What a mesh no fit rebuilds still yields: the triangles its traversal
//! decoded soundly, then those a forgiving walk decodes past the point the
//! arrays stopped fitting. Drawn only under an opt-in mesh-fit setting and
//! disclosed, since the shape past the first inconsistency is a reading
//! the file does not confirm.

use super::{Arrays, NormalReader, Walk, mesh, walk_into};
use crate::TriangleMesh;

/// A compressed mesh decoded past the point where its arrays stopped
/// fitting the traversal (see [`crate::Tessellation::Compressed`]).
///
/// `mesh.triangles[..sound]` decoded before the first inconsistency; the
/// rest came from a walk that lets an edge be shared by more than two
/// triangles, computes normals once the stored ones run out and stops when
/// the point array does or a triangle reaches more than the sound part's
/// diagonal outside its box (a walk whose frame has gone wrong sends points
/// far off). Stored normals are kept only when every one
/// is consumed; otherwise `mesh.normals` is empty and the renderer
/// computes facet normals.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PartialMesh {
    /// Every triangle decoded, the sound ones first.
    pub mesh: TriangleMesh,
    /// How many leading triangles of `mesh` decoded before the first
    /// inconsistency.
    pub sound: usize,
    /// The entity's triangle count.
    pub total: usize,
    /// Why the sound decode stopped, in a sentence a shell can print.
    pub why: String,
}

impl PartialMesh {
    /// `mesh` cut back to its sound triangles; every per-triangle array is
    /// cut with it, vertices are left in place.
    #[must_use]
    pub fn sound_mesh(&self) -> TriangleMesh {
        let mut m = self.mesh.clone();
        m.triangles.truncate(self.sound);
        m.triangle_normals.truncate(self.sound);
        m.triangle_graphics.truncate(self.sound);
        m.triangle_faces.truncate(self.sound);
        m.triangle_uvs.truncate(self.sound);
        m
    }
}

/// The box the sound vertices span, widened by its own diagonal on every
/// side. A continued walk whose frame has gone wrong sends points far off;
/// a triangle reaching outside this box ends the walk, so what is drawn
/// stays near the part.
struct Reach {
    lo: [f64; 3],
    hi: [f64; 3],
    margin: f64,
}

impl Reach {
    fn of(pos: &[[f64; 3]]) -> Self {
        let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
        for p in pos {
            for ((l, h), &c) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
                *l = l.min(c);
                *h = h.max(c);
            }
        }
        let margin = lo
            .iter()
            .zip(&hi)
            .map(|(l, h)| (h - l).powi(2))
            .sum::<f64>()
            .sqrt();
        Self { lo, hi, margin }
    }

    fn left_by(&self, p: [f64; 3]) -> bool {
        self.lo
            .iter()
            .zip(&self.hi)
            .zip(p)
            .any(|((l, h), c)| !c.is_finite() || c < l - self.margin || c > h + self.margin)
    }
}

/// The partial mesh of arrays no fit rebuilds, or `None` when not even a
/// seed triangle decodes. `why` is the reason the strict rebuild gave.
pub(super) fn decode(a: &Arrays<'_>, why: &str) -> Option<PartialMesh> {
    let normals = a.normals.as_ref().map(NormalReader::new);
    let mut w = Walk::new(a.triangles, normals);
    w.partial = true;
    let stopped = walk_into(a, &mut w).err();
    let sound = w.tris.len();
    let reach = Reach::of(&w.pos);
    w.forgiving = true;
    let mut i = sound;
    while i < a.triangles {
        let status = a.edge_status.get(i).copied().unwrap_or(0);
        let seen = w.pos.len();
        if w.step(a, status, false).is_err() {
            break;
        }
        if w.pos
            .get(seen..)
            .is_some_and(|new| new.iter().any(|p| reach.left_by(*p)))
        {
            w.tris.pop();
            break;
        }
        i += 1;
    }
    if w.tris.is_empty() {
        return None;
    }
    let Walk {
        pos, tris, normals, ..
    } = w;
    let stored = normals.and_then(|r| r.finish(&pos)).unwrap_or_default();
    Some(PartialMesh {
        mesh: mesh(pos, tris, stored),
        sound,
        total: a.triangles,
        why: stopped.unwrap_or_else(|| why.to_owned()),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)] // Tests fail loudly.
mod tests {
    use super::*;

    fn arrays<'a>(
        points: &'a [i64],
        status: &'a [i32],
        t: usize,
        is_ref: &'a [bool],
        refs: &'a [u32],
    ) -> Arrays<'a> {
        Arrays {
            tolerance: 0.5,
            origin: [10.0, 0.0, 0.0],
            points,
            edge_status: status,
            triangles: t,
            is_reference: is_ref,
            references: refs,
            ortho_turned: false,
            legacy_orient: false,
            normals: None,
        }
    }

    /// Seed `[0 1 2]`, apex 3 across `[2 1]`, then a reference to 0 across
    /// `[3 1]` and a reference to 1 across the popped `[2 3]`: the fourth
    /// triangle puts a third face on edge `[3 1]`. The strict walk stops at
    /// three, the forgiving one takes the fourth, and the sound cut leaves
    /// the three.
    #[test]
    fn a_third_triangle_on_an_edge_is_decoded_past_the_stop() {
        let pts = [0, 0, 0, 4, 0, 0, -2, 4, 0, 2, -6, 2];
        let status = [3, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let is_ref = [false, false, false, false, true, true];
        let a = arrays(&pts, &status, 4, &is_ref, &[0, 1]);
        assert!(super::super::reconstruct(&a).is_err());
        let p = decode(&a, "test").unwrap();
        assert_eq!((p.sound, p.total), (3, 4), "{:?}", p.mesh.triangles);
        assert_eq!(p.mesh.triangles.len(), 4);
        assert_eq!(p.sound_mesh().triangles.len(), 3);
        assert_eq!(p.sound_mesh().positions.len(), p.mesh.positions.len());
        assert_eq!(p.why, "an edge is shared by more than two triangles");
    }

    /// Arrays that fit every step but leave points over: the whole mesh is
    /// sound and the strict reason is kept.
    #[test]
    fn leftover_points_keep_every_triangle_sound() {
        let pts = [0, 0, 0, 4, 0, 0, -2, 4, 0, 2, -6, 2, 9, 9, 9];
        let a = arrays(&pts, &[2, 0, 0, 0, 0, 0], 2, &[false; 4], &[]);
        let why = super::super::reconstruct(&a).unwrap_err();
        let p = decode(&a, &why).unwrap();
        assert_eq!((p.sound, p.total, p.mesh.triangles.len()), (2, 2, 2));
        assert_eq!(p.why, why);
    }

    /// The first test's walk with the fourth triangle a reference to 0
    /// across the popped `[2 3]` (sound, `[3 1 0]`), then a fifth apex
    /// across an edge already closed: the strict walk stops at four, the
    /// forgiving one takes the fifth only while its apex stays within a
    /// diagonal of the sound box. Far off, the fifth is cut; nearby, kept.
    #[test]
    fn a_triangle_reaching_far_outside_the_sound_part_ends_the_walk() {
        let is_ref = [false, false, false, false, true, true, false];
        for (st, apex, kept) in [
            (1, [40, 40, 40], 4),
            (1, [1, 1, 1], 5),
            (2, [40, 40, 40], 4),
            (2, [1, 1, 1], 5),
        ] {
            let status = [3, 3, 0, st, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
            let mut pts = vec![0, 0, 0, 4, 0, 0, -2, 4, 0, 2, -6, 2];
            pts.extend(apex);
            let a = arrays(&pts, &status, 5, &is_ref, &[0, 0]);
            let p = decode(&a, "test").unwrap();
            assert_eq!(
                (p.sound, p.total, p.mesh.triangles.len()),
                (4, 5, kept),
                "{st} {apex:?} {}",
                p.why
            );
        }
    }

    /// Points that run out mid-walk stop the forgiving walk too; what
    /// decoded is kept.
    #[test]
    fn points_running_out_end_the_walk() {
        let pts = [0, 0, 0, 4, 0, 0, -2, 4, 0];
        let a = arrays(&pts, &[2, 0, 0, 2, 0, 0, 0, 0, 0], 3, &[false; 5], &[]);
        let p = decode(&a, "x").unwrap();
        assert_eq!((p.sound, p.total, p.mesh.triangles.len()), (1, 3, 1));
        assert_eq!(p.why, "points run out");
    }
}
