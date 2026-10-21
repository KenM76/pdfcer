//! Fitting cubic Béziers to a polyline within a tolerance: Schneider's
//! least-squares fit ("An Algorithm for Automatically Fitting Digitized
//! Curves", Graphics Gems, 1990) with Newton reparameterisation, accepted
//! only when the two-way deviation between the polyline and the curve is
//! within the tolerance.

use crate::vector::geometry::{Point, sub};
use crate::vector::path_geom::{cubic_at, dist_sq_point_segment};

/// Recursion depth past which a range stays as its original lines.
const MAX_DEPTH: u32 = 24;
/// Samples per cubic when measuring the curve against the polyline.
const FLATTEN: usize = 48;
/// Newton passes before a range is split.
const NEWTON_PASSES: usize = 4;

/// One fitted piece, ending at polyline vertex `end`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Piece {
    /// A straight segment.
    Line { end: usize },
    /// A cubic with these two control points.
    Curve { c1: Point, c2: Point, end: usize },
}

impl Piece {
    /// The polyline vertex this piece ends at.
    pub(crate) const fn end(self) -> usize {
        match self {
            Self::Line { end } | Self::Curve { end, .. } => end,
        }
    }
}

fn add(a: Point, b: Point) -> Point {
    Point::new(a.x + b.x, a.y + b.y)
}

fn scale(a: Point, k: f64) -> Point {
    Point::new(a.x * k, a.y * k)
}

fn dot(a: Point, b: Point) -> f64 {
    a.x * b.x + a.y * b.y
}

fn unit(a: Point) -> Option<Point> {
    let len = a.x.hypot(a.y);
    (len > 0.0 && len.is_finite()).then(|| scale(a, 1.0 / len))
}

/// The largest distance either way between the polyline `poly` and the
/// cubic `bez`: from each vertex to the curve, and from the curve to the
/// polyline, the curve taken as `FLATTEN` chords.
pub(crate) fn deviation(poly: &[Point], bez: [Point; 4]) -> f64 {
    let [p0, c1, c2, p3] = bez;
    let samples: Vec<Point> = (0..=FLATTEN)
        .map(|i| cubic_at(p0, c1, c2, p3, i as f64 / FLATTEN as f64))
        .collect();
    let farthest = |from: &[Point], to: &[Point]| {
        from.iter()
            .map(|&p| {
                to.windows(2)
                    .filter_map(|w| match w {
                        [a, b] => Some(dist_sq_point_segment(p, *a, *b)),
                        _ => None,
                    })
                    .fold(f64::INFINITY, f64::min)
            })
            .fold(0.0, f64::max)
    };
    let worst = farthest(poly, &samples).max(farthest(&samples, poly));
    if worst.is_finite() { worst.sqrt() } else { 0.0 }
}

/// The straight cubic from `a` to `b`.
pub(crate) fn straight(a: Point, b: Point) -> [Point; 4] {
    let third = scale(sub(b, a), 1.0 / 3.0);
    [a, add(a, third), sub(b, third), b]
}

/// Fit `points` (no two consecutive equal, at least two) with pieces
/// whose deviation from their stretch of the polyline is at most `tol`.
/// Each corner sharper than `corner_cos` (the cosine of the turn) starts
/// a fresh fit, so it stays a corner.
pub(crate) fn fit(points: &[Point], tol: f64, corner_cos: f64) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut start = 0;
    for k in 1..points.len() {
        let corner = match (points.get(k - 1), points.get(k), points.get(k + 1)) {
            (Some(&a), Some(&b), Some(&c)) => match (unit(sub(b, a)), unit(sub(c, b))) {
                (Some(i), Some(o)) => dot(i, o) < corner_cos,
                _ => true,
            },
            _ => true,
        };
        if corner {
            fit_piece(points, start, k, tol, &mut out);
            start = k;
        }
    }
    out
}

/// Fit the corner-free stretch `first..=last`.
fn fit_piece(points: &[Point], first: usize, last: usize, tol: f64, out: &mut Vec<Piece>) {
    let at = |i: usize| points.get(i).copied();
    let tangents = match (at(first), at(first + 1), at(last.wrapping_sub(1)), at(last)) {
        (Some(a), Some(b), Some(c), Some(d)) => unit(sub(b, a)).zip(unit(sub(c, d))),
        _ => None,
    };
    match tangents {
        Some((t1, t2)) => {
            let fitter = Fitter { points, tol };
            fitter.range(first, last, t1, t2, 0, out);
        }
        None => out.extend((first + 1..=last).map(|end| Piece::Line { end })),
    }
}

struct Fitter<'a> {
    points: &'a [Point],
    tol: f64,
}

impl Fitter<'_> {
    fn at(&self, i: usize) -> Point {
        self.points.get(i).copied().unwrap_or(Point::new(0.0, 0.0))
    }

    fn stretch(&self, first: usize, last: usize) -> &[Point] {
        self.points.get(first..=last).unwrap_or(&[])
    }

    fn fits(&self, first: usize, last: usize, bez: [Point; 4]) -> bool {
        deviation(self.stretch(first, last), bez) <= self.tol
    }

    fn range(
        &self,
        first: usize,
        last: usize,
        t1: Point,
        t2: Point,
        depth: u32,
        out: &mut Vec<Piece>,
    ) {
        if last <= first + 1 || self.fits(first, last, straight(self.at(first), self.at(last))) {
            out.push(Piece::Line { end: last });
            return;
        }
        let mut u = self.chord_params(first, last);
        let mut bez = self.generate(first, last, &u, t1, t2);
        let (mut err, mut split) = self.max_error(first, last, &bez, &u);
        for pass in 0..=NEWTON_PASSES {
            if err <= self.tol && self.fits(first, last, bez) {
                out.push(Piece::Curve {
                    c1: bez[1],
                    c2: bez[2],
                    end: last,
                });
                return;
            }
            if err > 4.0 * self.tol || pass == NEWTON_PASSES {
                break;
            }
            u = self.reparameterise(first, &bez, &u);
            bez = self.generate(first, last, &u, t1, t2);
            (err, split) = self.max_error(first, last, &bez, &u);
        }
        if depth >= MAX_DEPTH {
            out.extend((first + 1..=last).map(|end| Piece::Line { end }));
            return;
        }
        let split = split.clamp(first + 1, last - 1);
        let centre = unit(sub(self.at(split - 1), self.at(split + 1)))
            .or_else(|| unit(sub(self.at(split - 1), self.at(split))))
            .unwrap_or(t2);
        self.range(first, split, t1, centre, depth + 1, out);
        self.range(split, last, scale(centre, -1.0), t2, depth + 1, out);
    }

    /// Chord-length parameters of `first..=last`, from 0 to 1.
    fn chord_params(&self, first: usize, last: usize) -> Vec<f64> {
        let mut u = vec![0.0];
        let mut total = 0.0;
        for i in first + 1..=last {
            total += self.at(i).distance(self.at(i - 1));
            u.push(total);
        }
        if total > 0.0 {
            u.iter_mut().for_each(|v| *v /= total);
        }
        u
    }

    /// The least-squares cubic with end tangents `t1` and `t2` (Schneider's
    /// `GenerateBezier`), falling back to a third of the chord when the
    /// solve is degenerate.
    fn generate(&self, first: usize, last: usize, u: &[f64], t1: Point, t2: Point) -> [Point; 4] {
        let (p0, p3) = (self.at(first), self.at(last));
        let (mut c00, mut c01, mut c11, mut x0, mut x1) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for (k, &t) in u.iter().enumerate() {
            let s = 1.0 - t;
            let (b0, b1, b2, b3) = (s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t);
            let a0 = scale(t1, b1);
            let a1 = scale(t2, b2);
            c00 += dot(a0, a0);
            c01 += dot(a0, a1);
            c11 += dot(a1, a1);
            let base = add(scale(p0, b0 + b1), scale(p3, b2 + b3));
            let tmp = sub(self.at(first + k), base);
            x0 += dot(a0, tmp);
            x1 += dot(a1, tmp);
        }
        let det = c00 * c11 - c01 * c01;
        let (mut al, mut ar) = if det.abs() > f64::EPSILON {
            ((x0 * c11 - x1 * c01) / det, (c00 * x1 - c01 * x0) / det)
        } else {
            (0.0, 0.0)
        };
        let chord = p0.distance(p3);
        if !(al.is_finite() && ar.is_finite()) || al < 1e-6 * chord || ar < 1e-6 * chord {
            (al, ar) = (chord / 3.0, chord / 3.0);
        }
        [p0, add(p0, scale(t1, al)), add(p3, scale(t2, ar)), p3]
    }

    /// The largest distance from a vertex to the curve at its parameter,
    /// and the vertex where it falls.
    fn max_error(&self, first: usize, last: usize, bez: &[Point; 4], u: &[f64]) -> (f64, usize) {
        let mut worst = (0.0, (first + last) / 2);
        for (k, &t) in u.iter().enumerate().take(u.len().saturating_sub(1)).skip(1) {
            let q = cubic_at(bez[0], bez[1], bez[2], bez[3], t);
            let d = q.distance(self.at(first + k));
            if d > worst.0 {
                worst = (d, first + k);
            }
        }
        worst
    }

    /// One Newton step per parameter towards the curve's nearest point.
    fn reparameterise(&self, first: usize, bez: &[Point; 4], u: &[f64]) -> Vec<f64> {
        let [p0, p1, p2, p3] = *bez;
        let d1 = [
            scale(sub(p1, p0), 3.0),
            scale(sub(p2, p1), 3.0),
            scale(sub(p3, p2), 3.0),
        ];
        let d2 = [scale(sub(d1[1], d1[0]), 2.0), scale(sub(d1[2], d1[1]), 2.0)];
        u.iter()
            .enumerate()
            .map(|(k, &t)| {
                let s = 1.0 - t;
                let q = cubic_at(p0, p1, p2, p3, t);
                let q1 = add(
                    add(scale(d1[0], s * s), scale(d1[1], 2.0 * s * t)),
                    scale(d1[2], t * t),
                );
                let q2 = add(scale(d2[0], s), scale(d2[1], t));
                let diff = sub(q, self.at(first + k));
                let den = dot(q1, q1) + dot(diff, q2);
                if den.abs() > f64::EPSILON {
                    (t - dot(diff, q1) / den).clamp(0.0, 1.0)
                } else {
                    t
                }
            })
            .collect()
    }
}
