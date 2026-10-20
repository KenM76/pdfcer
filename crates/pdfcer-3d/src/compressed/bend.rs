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
/// (oriented to agree), the sum of their corners, and their count.
#[derive(Clone, Copy, Default)]
struct Plane {
    area: V,
    corners: V,
    count: usize,
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
    /// triangle needs no further bookkeeping.
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
    ) -> Result<(), String> {
        while self.log.len() > ti {
            if let Some((f, area, corners)) = self.log.pop().flatten()
                && let Some(e) = self.faces.get_mut(f)
            {
                e.area = sub(e.area, area);
                e.corners = sub(e.corners, corners);
                e.count -= 1;
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
        }
        self.log.push(Some((f, area, corners)));
        Ok(())
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
        p.place(&n, 0, tri(0.0, [0.0, 100.0, 0.0]), 1.0).unwrap();
        p.place(&n, 1, tri(0.0, [100.0, 100.0, 5.0]), 1.0).unwrap();
    }

    #[test]
    fn a_vertex_off_its_planar_face_fails() {
        let n = arrays(&[true]);
        let mut p = Planes::default();
        p.place(&n, 0, tri(0.0, [0.0, 100.0, 0.0]), 1.0).unwrap();
        let bent = p.place(&n, 1, tri(0.0, [50.0, 30.0, 25.0]), 1.0);
        assert!(bent.is_err());
    }

    #[test]
    fn a_curved_face_is_not_checked() {
        let n = arrays(&[false]);
        let mut p = Planes::default();
        p.place(&n, 0, tri(0.0, [0.0, 100.0, 0.0]), 1.0).unwrap();
        p.place(&n, 1, tri(0.0, [50.0, 30.0, 500.0]), 1.0).unwrap();
    }

    #[test]
    fn the_allowance_grows_with_distance_from_the_centroid() {
        let n = arrays(&[true]);
        let mut p = Planes::default();
        p.place(&n, 0, tri(0.0, [0.0, 100.0, 0.0]), 1.0).unwrap();
        // 25 tolerances off is a bend beside the centroid, not 5 diameters away.
        p.place(&n, 1, tri(0.0, [600.0, 30.0, 25.0]), 1.0).unwrap();
    }

    #[test]
    fn a_rewound_triangle_leaves_its_face() {
        let n = arrays(&[true]);
        let mut p = Planes::default();
        p.place(&n, 0, tri(0.0, [0.0, 100.0, 0.0]), 1.0).unwrap();
        p.place(&n, 1, tri(0.0, [100.0, 100.0, 0.0]), 1.0).unwrap();
        // Walked again from triangle 0 on another plane: the old ones are gone.
        p.place(&n, 0, tri(100.0, [0.0, 100.0, 100.0]), 1.0)
            .unwrap();
        p.place(&n, 1, tri(100.0, [50.0, 30.0, 100.0]), 1.0)
            .unwrap();
    }
}
