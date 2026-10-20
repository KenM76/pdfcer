//! The planar-face check made while walking: a planar face [WD 7.8.9.1]
//! must rebuild flat, so a vertex that leaves its face's running plane
//! fails the step that placed it rather than the whole walk afterwards.
//!
//! It is [`MAX_BEND`](super::MAX_BEND) applied incrementally. The
//! post-hoc check in [`flat`](super::search::flat) stays: it judges the
//! finished face against its final plane, this one only bounds how far a
//! new vertex may stray from the plane of the triangles placed so far.
//! Measured on real producers, a wrong fold on a planar face surfaces here
//! within a few triangles, where the search's candidate window still holds
//! it; walked on, it surfaced hundreds of triangles later or never.

use super::normals::NormalArrays;
use super::{V, add, cross, dot, mul, sub, unit};

/// How far, in tolerances, a new vertex may stray from its planar face's
/// running plane at the face's centroid.
const BEND: f64 = 10.0;
/// How much the allowance grows per running face diameter of distance
/// from the centroid: the running plane is tilted by its own rounding, and
/// the tilt grows with distance. Measured with [`BEND`]: correct rebuilds
/// of both samples pass at 3, and one more door mesh fits.
const BEND_SLOPE: f64 = 3.0;

/// Per planar face, the running sum of its triangles' area vectors
/// (oriented to agree), the sum of their corners, and their count; and
/// the first of its triangles taller than a tolerance, as walked: its
/// index and its normal oriented by its fold.
#[derive(Clone, Copy, Default)]
struct Plane {
    area: V,
    corners: V,
    count: usize,
    first: Option<(usize, V)>,
}

/// The running planes of a walk's planar faces, kept in step with its
/// triangles.
#[derive(Default)]
pub(super) struct Planes {
    faces: Vec<Plane>,
    /// Per triangle walked, the face and contribution it added.
    log: Vec<Option<(usize, V, V)>>,
}

impl Planes {
    /// Checks triangle `ti` with corners `t` against its face's running
    /// plane, then adds it. Triangles at or after `ti` from an earlier,
    /// rewound attempt are taken out first, so a walk rewound to any
    /// triangle needs no further bookkeeping. `oriented` is the triangle's
    /// normal as folded, `None` for one no taller than a tolerance.
    ///
    /// # Errors
    ///
    /// When a corner strays more than its allowance (see [`BEND`]).
    pub(super) fn place(
        &mut self,
        n: &NormalArrays<'_>,
        ti: usize,
        t: [V; 3],
        tol: f64,
        oriented: Option<V>,
    ) -> Result<(), String> {
        while self.log.len() > ti {
            let i = self.log.len() - 1;
            if let Some((f, area, corners)) = self.log.pop().flatten()
                && let Some(e) = self.faces.get_mut(f)
            {
                e.area = sub(e.area, area);
                e.corners = sub(e.corners, corners);
                e.count -= 1;
                if e.first.is_some_and(|(at, _)| at == i) {
                    e.first = None;
                }
            }
        }
        let face = n.face_of.get(ti).map(|&f| f as usize);
        let Some(f) = face.filter(|&f| n.planar.get(f).copied().unwrap_or(false)) else {
            self.log.push(None);
            return Ok(());
        };
        if self.faces.len() <= f {
            self.faces.resize(f + 1, Plane::default());
        }
        let plane = self.faces.get(f).copied().unwrap_or_default();
        let ww = dot(plane.area, plane.area);
        if plane.count > 0 && ww > 0.0 {
            let normal = unit(plane.area);
            #[allow(clippy::cast_precision_loss)] // A face's triangle count.
            let centre = mul(plane.corners, 1.0 / (3 * plane.count) as f64);
            let diameter = ww.sqrt().sqrt();
            for v in t {
                let off = sub(v, centre);
                let allow = BEND * (1.0 + BEND_SLOPE * dot(off, off).sqrt() / diameter);
                if dot(off, normal).abs() > allow * tol {
                    return Err("a planar face bends while rebuilding".into());
                }
            }
        }
        let mut area = cross(sub(t[1], t[0]), sub(t[2], t[0]));
        if dot(area, plane.area) < 0.0 {
            area = mul(area, -1.0);
        }
        let corners = add(t[0], add(t[1], t[2]));
        if let Some(e) = self.faces.get_mut(f) {
            e.area = add(e.area, area);
            e.corners = add(e.corners, corners);
            e.count += 1;
            if e.first.is_none() {
                e.first = oriented.map(|o| (ti, o));
            }
        }
        self.log.push(Some((f, area, corners)));
        Ok(())
    }

    /// The oriented normal of planar face `f`'s first tall triangle, if
    /// one was placed before triangle `ti`.
    ///
    /// A planar face reads one stored normal, at the first corner of its
    /// first triangle [WD 7.8.9.1]. When that vertex has one normal,
    /// already read at another face, the face borrows it, and that
    /// normal's sign can be the other face's. Measured over every mesh of
    /// both samples that decodes, orienting a borrowing face by its own
    /// first triangle agrees with the borrowed sign 6,856 times of 6,856;
    /// it differs only where the borrowed sign folds a wall wrongly. A
    /// face whose stored normal was decoded at a collinear triangle has no
    /// trustworthy direction at all and is oriented the same way.
    pub(super) fn oriented(&self, f: usize, ti: usize) -> Option<V> {
        let (at, n) = self.faces.get(f)?.first?;
        (at < ti).then_some(n)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Tests fail loudly.
mod tests {
    use super::*;

    const FACES: [u32; 4] = [0, 0, 0, 0];

    fn arrays(planar: &[bool]) -> NormalArrays<'_> {
        NormalArrays {
            bits: 10,
            binary: &[],
            angles: &[],
            planar,
            face_of: &FACES,
        }
    }

    /// A triangle of the plane `z = h` with its apex at `apex`.
    fn tri(h: f64, apex: V) -> [V; 3] {
        [[0.0, 0.0, h], [100.0, 0.0, h], apex]
    }

    #[test]
    fn a_flat_planar_face_passes() {
        let n = arrays(&[true]);
        let mut p = Planes::default();
        p.place(&n, 0, tri(0.0, [0.0, 100.0, 0.0]), 1.0, None)
            .unwrap();
        p.place(&n, 1, tri(0.0, [100.0, 100.0, 5.0]), 1.0, None)
            .unwrap();
    }

    #[test]
    fn a_vertex_off_its_planar_face_fails() {
        let n = arrays(&[true]);
        let mut p = Planes::default();
        p.place(&n, 0, tri(0.0, [0.0, 100.0, 0.0]), 1.0, None)
            .unwrap();
        let bent = p.place(&n, 1, tri(0.0, [50.0, 30.0, 25.0]), 1.0, None);
        assert!(bent.is_err());
    }

    #[test]
    fn a_curved_face_is_not_checked() {
        let n = arrays(&[false]);
        let mut p = Planes::default();
        p.place(&n, 0, tri(0.0, [0.0, 100.0, 0.0]), 1.0, None)
            .unwrap();
        p.place(&n, 1, tri(0.0, [50.0, 30.0, 500.0]), 1.0, None)
            .unwrap();
    }

    #[test]
    fn the_allowance_grows_with_distance_from_the_centroid() {
        let n = arrays(&[true]);
        let mut p = Planes::default();
        p.place(&n, 0, tri(0.0, [0.0, 100.0, 0.0]), 1.0, None)
            .unwrap();
        // 25 tolerances off is a bend beside the centroid, not 5 diameters away.
        p.place(&n, 1, tri(0.0, [600.0, 30.0, 25.0]), 1.0, None)
            .unwrap();
    }

    #[test]
    fn a_rewound_triangle_leaves_its_face() {
        let n = arrays(&[true]);
        let mut p = Planes::default();
        p.place(&n, 0, tri(0.0, [0.0, 100.0, 0.0]), 1.0, None)
            .unwrap();
        p.place(&n, 1, tri(0.0, [100.0, 100.0, 0.0]), 1.0, None)
            .unwrap();
        // Walked again from triangle 0 on another plane: the old ones are gone.
        p.place(&n, 0, tri(100.0, [0.0, 100.0, 100.0]), 1.0, None)
            .unwrap();
        p.place(&n, 1, tri(100.0, [50.0, 30.0, 100.0]), 1.0, None)
            .unwrap();
    }

    #[test]
    fn a_face_is_oriented_by_its_first_tall_triangle() {
        let n = arrays(&[true]);
        let mut p = Planes::default();
        p.place(&n, 0, tri(0.0, [0.0, 100.0, 0.0]), 1.0, None)
            .unwrap();
        p.place(
            &n,
            1,
            tri(0.0, [100.0, 100.0, 0.0]),
            1.0,
            Some([0.0, 0.0, -1.0]),
        )
        .unwrap();
        p.place(
            &n,
            2,
            tri(0.0, [50.0, 100.0, 0.0]),
            1.0,
            Some([0.0, 0.0, 1.0]),
        )
        .unwrap();
        // Not yet visible at the triangle that set it; the first one wins.
        assert_eq!(p.oriented(0, 1), None);
        assert_eq!(p.oriented(0, 3), Some([0.0, 0.0, -1.0]));
        // Rewound past it, the face has no orientation until re-placed.
        p.place(&n, 1, tri(0.0, [100.0, 100.0, 0.0]), 1.0, None)
            .unwrap();
        assert_eq!(p.oriented(0, 2), None);
    }
}
