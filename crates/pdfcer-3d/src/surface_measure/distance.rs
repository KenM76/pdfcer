//! The distance between two picked entities (vertex, edge or face): the
//! nearest, the farthest and centre to centre.

use super::edge::{edge_at, vertex_at};
use super::face::resolve_face;
use super::{EdgeShape, FaceSource, SurfaceMeasureError, SurfacePick};
use crate::TriangleMesh;
use crate::vec3::{add, cross, dot, length, normalize, scale, sub};

/// What a pick stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MeasureEntity {
    /// The picked triangle's corner nearest the pick.
    Vertex,
    /// The model edge nearest the pick ([`edge_at`]).
    Edge,
    /// The face the pick is on, as [`super::face_figures`] resolves it:
    /// the file's face, or the flat patch round the pick when that face is
    /// not one surface.
    Face,
}

/// One picked entity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Selection {
    /// Where the model was picked.
    pub pick: SurfacePick,
    /// What the pick stands for.
    pub entity: MeasureEntity,
}

/// A distance and the two points it runs between.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Span {
    /// The distance, model units.
    pub length: f64,
    /// The point on the first entity.
    pub from: [f64; 3],
    /// The point on the second entity.
    pub to: [f64; 3],
}

impl Span {
    fn between(from: [f64; 3], to: [f64; 3]) -> Span {
        Span {
            length: length(sub(to, from)),
            from,
            to,
        }
    }

    /// `to - from`: the distance along X, Y and Z.
    #[must_use]
    pub fn delta(&self) -> [f64; 3] {
        sub(self.to, self.from)
    }

    /// The span projected onto a plane with normal `normal` (any length):
    /// `to` moves along `normal` until `to - from` lies in the plane.
    /// `None` when `normal` has no length.
    #[must_use]
    pub fn projected(&self, normal: [f64; 3]) -> Option<Span> {
        let n = normalize(normal)?;
        let d = self.delta();
        Some(Span::between(
            self.from,
            add(self.from, sub(d, scale(n, dot(d, n)))),
        ))
    }

    /// The signed length of the span along `direction` (any length): the
    /// part [`Span::projected`] removes. `None` when `direction` has no
    /// length.
    #[must_use]
    pub fn along(&self, direction: [f64; 3]) -> Option<f64> {
        Some(dot(self.delta(), normalize(direction)?))
    }
}

/// The distances between two entities.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct EntityDistance {
    /// The nearest two points; 0 when the entities touch or cross.
    pub minimum: Span,
    /// The farthest two corners. A circle is its tessellated corners, so a
    /// curved entity's maximum can fall short of the true curve's.
    pub maximum: Span,
    /// Between the entities' centres when both have one: a vertex is its
    /// own centre, a circular edge has its fitted centre (rule 4).
    pub centres: Option<Span>,
    /// Which triangles the first entity was measured from, when it is a
    /// face: the same set [`super::face_figures`] reports for its pick.
    pub a_source: Option<FaceSource>,
    /// The same for the second entity.
    pub b_source: Option<FaceSource>,
}

/// One entity as the points, segments and triangles it is made of.
struct Shape {
    pieces: Vec<Vec<[f64; 3]>>,
    centre: Option<[f64; 3]>,
    source: Option<FaceSource>,
}

fn shape_of(
    meshes: &[TriangleMesh],
    s: &Selection,
    flat_degrees: f64,
    corner_degrees: f64,
) -> Result<Shape, SurfaceMeasureError> {
    match s.entity {
        MeasureEntity::Vertex => {
            let p = vertex_at(meshes, &s.pick)?;
            Ok(Shape {
                pieces: vec![vec![p]],
                centre: Some(p),
                source: None,
            })
        }
        MeasureEntity::Edge => {
            let e = edge_at(meshes, &s.pick, flat_degrees, corner_degrees)?;
            let mut pieces: Vec<Vec<[f64; 3]>> = e.points.windows(2).map(<[_]>::to_vec).collect();
            if let (true, Some(&p), Some(&q)) = (e.closed, e.points.first(), e.points.last()) {
                pieces.push(vec![q, p]);
            }
            let centre = match e.shape {
                EdgeShape::Circle { centre, .. } => Some(centre),
                _ => None,
            };
            Ok(Shape {
                pieces,
                centre,
                source: None,
            })
        }
        _ => {
            let (tris, figures) = resolve_face(meshes, &s.pick, flat_degrees)?;
            Ok(Shape {
                pieces: tris.into_iter().map(|t| t.to_vec()).collect(),
                centre: None,
                source: Some(figures.source),
            })
        }
    }
}

/// Measures between two picked entities.
///
/// `flat_degrees` and `corner_degrees` find an edge as [`edge_at`] does.
///
/// # Errors
///
/// Those of [`vertex_at`], [`edge_at`] and, for a face, those of
/// [`super::face_figures`].
pub fn entity_distance(
    meshes: &[TriangleMesh],
    a: &Selection,
    b: &Selection,
    flat_degrees: f64,
    corner_degrees: f64,
) -> Result<EntityDistance, SurfaceMeasureError> {
    let a = shape_of(meshes, a, flat_degrees, corner_degrees)?;
    let b = shape_of(meshes, b, flat_degrees, corner_degrees)?;
    let minimum = nearest(&a.pieces, &b.pieces).ok_or(SurfaceMeasureError::Degenerate)?;
    let maximum = farthest(&a.pieces, &b.pieces).ok_or(SurfaceMeasureError::Degenerate)?;
    let centres = match (a.centre, b.centre) {
        (Some(p), Some(q)) => Some(Span::between(p, q)),
        _ => None,
    };
    Ok(EntityDistance {
        minimum,
        maximum,
        centres,
        a_source: a.source,
        b_source: b.source,
    })
}

fn farthest(a: &[Vec<[f64; 3]>], b: &[Vec<[f64; 3]>]) -> Option<Span> {
    let corners = |x: &[Vec<[f64; 3]>]| x.iter().flatten().copied().collect::<Vec<_>>();
    let (pa, pb) = (corners(a), corners(b));
    pa.iter()
        .flat_map(|&p| pb.iter().map(move |&q| Span::between(p, q)))
        .max_by(|x, y| x.length.total_cmp(&y.length))
}

/// The axis-aligned box round a piece.
fn bounds(piece: &[[f64; 3]]) -> ([f64; 3], [f64; 3]) {
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    for p in piece {
        for ((l, h), &c) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
            *l = l.min(c);
            *h = h.max(c);
        }
    }
    (lo, hi)
}

/// The least distance two boxes allow between their contents.
fn box_gap(a: &([f64; 3], [f64; 3]), b: &([f64; 3], [f64; 3])) -> f64 {
    let mut gap = [0.0; 3];
    for (((g, &alo), &ahi), (&blo, &bhi)) in
        gap.iter_mut().zip(&a.0).zip(&a.1).zip(b.0.iter().zip(&b.1))
    {
        *g = (blo - ahi).max(alo - bhi).max(0.0);
    }
    length(gap)
}

fn nearest(a: &[Vec<[f64; 3]>], b: &[Vec<[f64; 3]>]) -> Option<Span> {
    let boxes_b: Vec<_> = b.iter().map(|q| bounds(q)).collect();
    let mut best: Option<Span> = None;
    for p in a {
        let bp = bounds(p);
        for (q, bq) in b.iter().zip(&boxes_b) {
            if best.is_some_and(|s| box_gap(&bp, bq) >= s.length) {
                continue;
            }
            if let Some(s) = piece_distance(p, q)
                && best.is_none_or(|b| s.length < b.length)
            {
                best = Some(s);
            }
        }
    }
    best
}

/// The nearest points of two points, segments or triangles.
fn piece_distance(p: &[[f64; 3]], q: &[[f64; 3]]) -> Option<Span> {
    let mut best: Option<Span> = None;
    let mut keep = |s: Span| {
        if best.is_none_or(|b| s.length < b.length) {
            best = Some(s);
        }
    };
    for &x in p {
        keep(Span::between(x, closest_on(q, x)?));
    }
    for &y in q {
        let x = closest_on(p, y)?;
        keep(Span::between(x, y));
    }
    for (a0, a1) in sides(p) {
        for (b0, b1) in sides(q) {
            let (x, y) = segment_segment(a0, a1, b0, b1);
            keep(Span::between(x, y));
        }
    }
    if let [t0, t1, t2] = *q {
        for (a0, a1) in sides(p) {
            if let Some(x) = crossing(a0, a1, [t0, t1, t2]) {
                keep(Span::between(x, x));
            }
        }
    }
    if let [t0, t1, t2] = *p {
        for (b0, b1) in sides(q) {
            if let Some(y) = crossing(b0, b1, [t0, t1, t2]) {
                keep(Span::between(y, y));
            }
        }
    }
    best
}

/// A piece's edges: none for a point, one for a segment, three for a
/// triangle.
fn sides(p: &[[f64; 3]]) -> Vec<([f64; 3], [f64; 3])> {
    match *p {
        [a, b] => vec![(a, b)],
        [a, b, c] => vec![(a, b), (b, c), (c, a)],
        _ => Vec::new(),
    }
}

/// The point of a piece nearest `x`.
fn closest_on(piece: &[[f64; 3]], x: [f64; 3]) -> Option<[f64; 3]> {
    match *piece {
        [a] => Some(a),
        [a, b] => Some(on_segment(x, a, b)),
        [a, b, c] => Some(on_triangle(x, a, b, c)),
        _ => None,
    }
}

fn on_segment(x: [f64; 3], a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    let ab = sub(b, a);
    let span = dot(ab, ab);
    let t = if span > 0.0 {
        (dot(sub(x, a), ab) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    add(a, scale(ab, t))
}

/// The point of triangle `a b c` nearest `p` (Ericson, Real-Time Collision
/// Detection, 5.1.5).
fn on_triangle(p: [f64; 3], a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> [f64; 3] {
    let (ab, ac, ap) = (sub(b, a), sub(c, a), sub(p, a));
    let (d1, d2) = (dot(ab, ap), dot(ac, ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = sub(p, b);
    let (d3, d4) = (dot(ab, bp), dot(ac, bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return add(a, scale(ab, d1 / (d1 - d3)));
    }
    let cp = sub(p, c);
    let (d5, d6) = (dot(ab, cp), dot(ac, cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return add(a, scale(ac, d2 / (d2 - d6)));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return add(b, scale(sub(c, b), w));
    }
    let denom = va + vb + vc;
    if denom == 0.0 {
        return a;
    }
    let (v, w) = (vb / denom, vc / denom);
    add(a, add(scale(ab, v), scale(ac, w)))
}

/// The nearest points of segments `p0 p1` and `q0 q1` (Ericson 5.1.9).
fn segment_segment(p0: [f64; 3], p1: [f64; 3], q0: [f64; 3], q1: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let (d1, d2, r) = (sub(p1, p0), sub(q1, q0), sub(p0, q0));
    let (a, e, f) = (dot(d1, d1), dot(d2, d2), dot(d2, r));
    let (s, t) = if a <= 0.0 && e <= 0.0 {
        (0.0, 0.0)
    } else if a <= 0.0 {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = dot(d1, r);
        if e <= 0.0 {
            ((-c / a).clamp(0.0, 1.0), 0.0)
        } else {
            let b = dot(d1, d2);
            let denom = a * e - b * b;
            let mut s = if denom > 0.0 {
                ((b * f - c * e) / denom).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let mut t = (b * s + f) / e;
            if t < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            }
            (s, t)
        }
    };
    (add(p0, scale(d1, s)), add(q0, scale(d2, t)))
}

/// Where segment `p q` passes through triangle `t`, if it does.
fn crossing(p: [f64; 3], q: [f64; 3], t: [[f64; 3]; 3]) -> Option<[f64; 3]> {
    let [a, b, c] = t;
    let (ab, ac, d) = (sub(b, a), sub(c, a), sub(q, p));
    let h = cross(d, ac);
    let det = dot(ab, h);
    if det.abs() <= f64::EPSILON * dot(ab, ab).max(dot(ac, ac)) * dot(d, d).sqrt() {
        return None;
    }
    let s = sub(p, a);
    let u = dot(s, h) / det;
    let qv = cross(s, ab);
    let v = dot(d, qv) / det;
    let t = dot(ac, qv) / det;
    ((0.0..=1.0).contains(&u) && v >= 0.0 && u + v <= 1.0 && (0.0..=1.0).contains(&t))
        .then(|| add(p, scale(d, t)))
}
