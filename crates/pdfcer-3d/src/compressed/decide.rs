//! How a walked triangle's stored normal decides its fold [WD 7.8.9,
//! 7.8.9.1]: the per-[`Orientation`] half of [`Walk::fold`].

use super::normals::Orientation;
use super::{BLIND_NORMAL, MIN_PLANE_HEIGHT, MIN_SINE, Walk, cross, dot, plane_height, sub};

impl Walk<'_> {
    /// Whether `t`, read as `o`, folds; `fold` is the default and `thin`
    /// whether it is collinear. A planar face whose normal is
    /// [`Orientation::Borrowed`] or [`Orientation::Blind`] is oriented by
    /// its own first tall triangle ([`super::bend::Planes::oriented`])
    /// where one was walked; before that, a borrowed normal is used as
    /// read and a blind one leaves the default to the search.
    pub(super) fn decide(
        &mut self,
        o: Orientation,
        t: [u32; 3],
        fold: bool,
        legacy: bool,
        thin: bool,
    ) -> Result<bool, String> {
        let ti = self.tris.len();
        Ok(match o {
            Orientation::Reversed(..) if thin => {
                if let Some(r) = self.normals.as_mut() {
                    r.blind(ti);
                }
                fold
            }
            Orientation::Reused(_) if legacy => fold,
            Orientation::Reversed(rev, n) => {
                let (a, b, c) = (self.get(t[0])?, self.get(t[1])?, self.get(t[2])?);
                let w = cross(sub(b, a), sub(c, a));
                if dot(n, w).abs() < BLIND_NORMAL * dot(w, w).sqrt() {
                    self.sliver = Some(ti);
                    return Ok(fold);
                }
                self.signalled = true;
                self.exact = true;
                rev != (t[0] > t[1])
            }
            Orientation::Borrowed(face, _) | Orientation::Blind(face)
                if let Some(f) = self.planes.oriented(face, ti) =>
            {
                let (a, b, c) = (self.get(t[0])?, self.get(t[1])?, self.get(t[2])?);
                let s = dot(cross(sub(b, a), sub(c, a)), f);
                if s == 0.0 || thin {
                    self.sliver = Some(ti);
                    fold
                } else {
                    self.signalled = true;
                    s < 0.0
                }
            }
            Orientation::Normal(n) | Orientation::Reused(n) | Orientation::Borrowed(_, n) => {
                let (a, b, c) = (self.get(t[0])?, self.get(t[1])?, self.get(t[2])?);
                let (ab, ac) = (sub(b, a), sub(c, a));
                let w = cross(ab, ac);
                let sine = (dot(w, w) / (dot(ab, ab) * dot(ac, ac))).sqrt();
                let s = dot(w, n);
                let tall =
                    legacy || plane_height(s, n, [ab, ac, sub(c, b)]) > MIN_PLANE_HEIGHT * self.tol;
                if sine > MIN_SINE && s != 0.0 && !thin && tall {
                    self.signalled = true;
                    s < 0.0
                } else {
                    self.sliver = Some(ti);
                    fold
                }
            }
            Orientation::Blind(_) => {
                self.sliver = Some(ti);
                fold
            }
            Orientation::Unknown => fold,
        })
    }
}
