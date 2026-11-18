//! What one picked model edge or vertex of a tessellated model measures.
//!
//! A model keeps triangles, not the curves their faces meet along, so an
//! edge is found: a triangle edge is a model edge where the two faces on
//! either side differ, where only one triangle uses it, or (when the file
//! separates no faces) where the triangles turn more than the flat
//! tolerance. The picked edge runs on through every corner where exactly
//! one more such edge between the same two faces continues it, turning
//! no more than the corner tolerance.

use std::collections::HashMap;

use super::fit::{across, fit_circle, least_eigenvector};
use super::{SurfaceMeasureError, SurfacePick, area_and_centre};
use crate::TriangleMesh;
use crate::render::corners;
use crate::vec3::{add, dot, length, normalize, scale, sub};

/// The curve fitted to a model edge.
///
/// Fitted from the edge's corners, not read from the file (rule 4).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum EdgeShape {
    /// Every corner lies on the line between the ends.
    Line {
        /// From the first end to the second, a unit vector.
        direction: [f64; 3],
    },
    /// The corners lie on a circle.
    Circle {
        /// The circle's centre.
        centre: [f64; 3],
        /// The unit normal of the circle's plane (its sign is arbitrary).
        axis: [f64; 3],
        /// The fitted radius, model units.
        radius: f64,
        /// The farthest a corner lies off the fitted circle, model units.
        residual: f64,
        /// How far round the circle the edge reaches, degrees: 360 for a
        /// closed edge.
        sweep_degrees: f64,
        /// The length of the fitted arc, `radius` times the sweep.
        arc_length: f64,
    },
    /// Neither a line nor a circle.
    Curve,
}

/// What one model edge measures.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct EdgeFigures {
    /// The mesh the edge is on.
    pub mesh: usize,
    /// The faces either side; `None` for a side with no face (the file
    /// separates no faces, or only one triangle uses the edge).
    pub faces: [Option<usize>; 2],
    /// The edge's corners in order; for a closed edge the first is not
    /// repeated at the end.
    pub points: Vec<[f64; 3]>,
    /// The edge closes on itself.
    pub closed: bool,
    /// The summed length of its straight pieces, model units.
    pub length: f64,
    /// The distance between the two ends; 0 for a closed edge.
    pub chord: f64,
    /// The line or circle fitted.
    pub shape: EdgeShape,
}

/// A corner's identity: its position, so corners stored twice weld.
type Corner = [u64; 3];

/// The face and unit normal of each triangle using an edge.
type Users = HashMap<(Corner, Corner), Vec<(Option<usize>, [f64; 3])>>;

fn corner(p: [f64; 3]) -> Corner {
    p.map(|c| (c + 0.0).to_bits())
}

/// The model edges of one mesh, as corner pairs keyed by the two faces.
struct Edges {
    /// Each model edge's two face keys, by its sorted corner pair.
    key: HashMap<(Corner, Corner), [Option<usize>; 2]>,
    /// The model edges at each corner.
    at: HashMap<Corner, Vec<Corner>>,
    position: HashMap<Corner, [f64; 3]>,
    /// The cosine of the sharpest turn an edge runs on through.
    cos_corner: f64,
}

/// The face of each triangle, by `faces` ranges or `triangle_faces`.
fn face_of(mesh: &TriangleMesh) -> Vec<Option<usize>> {
    let mut out = vec![None; mesh.triangles.len()];
    if !mesh.faces.is_empty() {
        for (f, r) in mesh.faces.iter().enumerate() {
            for slot in out.get_mut(r.clone()).into_iter().flatten() {
                *slot = Some(f);
            }
        }
    } else if mesh.triangle_faces.len() == mesh.triangles.len() {
        for (slot, &f) in out.iter_mut().zip(&mesh.triangle_faces) {
            *slot = Some(f as usize);
        }
    }
    out
}

impl Edges {
    fn of(mesh: &TriangleMesh, flat_degrees: f64, corner_degrees: f64) -> Edges {
        let faces = face_of(mesh);
        let mut users: Users = HashMap::new();
        let mut position = HashMap::new();
        for (t, tri) in mesh.triangles.iter().enumerate() {
            let Some(w) = corners(mesh, tri) else {
                continue;
            };
            let Some(n) = normalize(area_and_centre(&w).0) else {
                continue;
            };
            let c = w.map(corner);
            for (k, &p) in c.iter().zip(&w) {
                position.insert(*k, p);
            }
            let face = faces.get(t).copied().flatten();
            for (a, b) in [(c[0], c[1]), (c[1], c[2]), (c[2], c[0])] {
                users
                    .entry((a.min(b), a.max(b)))
                    .or_default()
                    .push((face, n));
            }
        }
        let cos_flat = flat_degrees.to_radians().cos();
        let mut edges = Edges {
            key: HashMap::new(),
            at: HashMap::new(),
            position,
            cos_corner: corner_degrees.to_radians().cos(),
        };
        for (pair, used) in users {
            let key = match used.as_slice() {
                [(f, _)] => [*f, None],
                [(f, n), (g, m)] if f != g || (f.is_none() && dot(*n, *m) < cos_flat) => {
                    [(*f).min(*g), (*f).max(*g)]
                }
                [_, _] => continue,
                _ => [None, None],
            };
            edges.key.insert(pair, key);
            edges.at.entry(pair.0).or_default().push(pair.1);
            edges.at.entry(pair.1).or_default().push(pair.0);
        }
        edges
    }

    fn key(&self, a: Corner, b: Corner) -> Option<[Option<usize>; 2]> {
        self.key.get(&(a.min(b), a.max(b))).copied()
    }

    /// The one other edge with `key` at `at`, not back to `from`.
    fn next(&self, at: Corner, from: Corner, key: [Option<usize>; 2]) -> Option<Corner> {
        let mut onward = self.at.get(&at)?.iter().copied();
        let same: Vec<Corner> = onward
            .by_ref()
            .filter(|&c| self.key(at, c) == Some(key))
            .collect();
        let onward = match same.as_slice() {
            [x, y] if *x == from => *y,
            [x, y] if *y == from => *x,
            _ => return None,
        };
        self.smooth(from, at, onward).then_some(onward)
    }

    /// The edge turns at `at` by no more than the corner tolerance.
    fn smooth(&self, from: Corner, at: Corner, to: Corner) -> bool {
        let p = |k: Corner| self.position.get(&k).copied();
        let (Some(f), Some(a), Some(t)) = (p(from), p(at), p(to)) else {
            return false;
        };
        match (normalize(sub(a, f)), normalize(sub(t, a))) {
            (Some(d), Some(e)) => dot(d, e) >= self.cos_corner,
            _ => false,
        }
    }

    /// The corners along the edge through `a`-`b`, and whether it closes.
    fn chain(&self, a: Corner, b: Corner, key: [Option<usize>; 2]) -> (Vec<Corner>, bool) {
        let mut forward = vec![a, b];
        let limit = self.key.len() + 2;
        while forward.len() <= limit {
            let (Some(&at), Some(&from)) = (forward.last(), forward.iter().rev().nth(1)) else {
                break;
            };
            match self.next(at, from, key) {
                Some(c) if c == a && self.smooth(at, a, b) => return (forward, true),
                Some(c) if c == a => {
                    forward.push(a);
                    return (forward, false);
                }
                Some(c) => forward.push(c),
                None => break,
            }
        }
        let mut back = vec![b, a];
        while back.len() <= limit {
            let (Some(&at), Some(&from)) = (back.last(), back.iter().rev().nth(1)) else {
                break;
            };
            match self.next(at, from, key) {
                Some(c) => back.push(c),
                None => break,
            }
        }
        back.reverse();
        back.truncate(back.len().saturating_sub(2));
        back.extend(forward);
        (back, false)
    }
}

/// The distance from `p` to the segment `a`-`b`.
fn to_segment(p: [f64; 3], a: [f64; 3], b: [f64; 3]) -> f64 {
    let ab = sub(b, a);
    let span = dot(ab, ab);
    let t = if span > 0.0 {
        (dot(sub(p, a), ab) / span).clamp(0.0, 1.0)
    } else {
        0.0
    };
    length(sub(p, add(a, scale(ab, t))))
}

fn picked<'m>(
    meshes: &'m [TriangleMesh],
    pick: &SurfacePick,
) -> Result<(&'m TriangleMesh, [[f64; 3]; 3]), SurfaceMeasureError> {
    let missing = SurfaceMeasureError::NoSuchTriangle {
        mesh: pick.mesh,
        triangle: pick.triangle,
    };
    let mesh = meshes.get(pick.mesh).ok_or(missing.clone())?;
    let tri = mesh.triangles.get(pick.triangle).ok_or(missing)?;
    if !pick.point.iter().all(|c| c.is_finite()) {
        return Err(SurfaceMeasureError::BadPoint);
    }
    let w = corners(mesh, tri).ok_or(SurfaceMeasureError::Degenerate)?;
    Ok((mesh, w))
}

/// The corner of the picked triangle nearest the picked point: a vertex's
/// X, Y, Z.
///
/// # Errors
///
/// [`SurfaceMeasureError::NoSuchTriangle`], [`SurfaceMeasureError::Degenerate`]
/// (a corner is not finite) and [`SurfaceMeasureError::BadPoint`].
pub fn vertex_at(
    meshes: &[TriangleMesh],
    pick: &SurfacePick,
) -> Result<[f64; 3], SurfaceMeasureError> {
    let (_, w) = picked(meshes, pick)?;
    let near = |p: &[f64; 3]| length(sub(*p, pick.point));
    Ok(w.into_iter()
        .min_by(|a, b| near(a).total_cmp(&near(b)))
        .unwrap_or(pick.point))
}

/// Measures the model edge of the picked triangle nearest the picked
/// point.
///
/// `flat_degrees` (0 to 90) is how far two triangles may turn and still be
/// one surface when the file separates no faces; `corner_degrees` (0 to
/// 180) is the sharpest turn the edge runs on through, so a tessellated
/// circle stays one edge while a box corner ends one. A whole loop with
/// no sharper turn is a closed edge.
///
/// # Errors
///
/// As [`vertex_at`]; [`SurfaceMeasureError::BadTolerance`] when
/// `flat_degrees` is not within 0 to 90; [`SurfaceMeasureError::NoEdge`]
/// when no side of the picked triangle is a model edge.
pub fn edge_at(
    meshes: &[TriangleMesh],
    pick: &SurfacePick,
    flat_degrees: f64,
    corner_degrees: f64,
) -> Result<EdgeFigures, SurfaceMeasureError> {
    if !(0.0..=90.0).contains(&flat_degrees) {
        return Err(SurfaceMeasureError::BadTolerance("flat tolerance"));
    }
    if !(0.0..=180.0).contains(&corner_degrees) {
        return Err(SurfaceMeasureError::BadTolerance("corner tolerance"));
    }
    let (mesh, w) = picked(meshes, pick)?;
    let edges = Edges::of(mesh, flat_degrees, corner_degrees);
    let c = w.map(corner);
    let sides = [(0, 1), (1, 2), (2, 0)];
    let (a, b) = sides
        .iter()
        .filter_map(|&(i, j)| Some((*c.get(i)?, *c.get(j)?, *w.get(i)?, *w.get(j)?)))
        .filter(|&(a, b, ..)| edges.key(a, b).is_some())
        .min_by(|x, y| {
            to_segment(pick.point, x.2, x.3).total_cmp(&to_segment(pick.point, y.2, y.3))
        })
        .map(|(a, b, ..)| (a, b))
        .ok_or(SurfaceMeasureError::NoEdge)?;
    let key = edges.key(a, b).ok_or(SurfaceMeasureError::NoEdge)?;
    let (chain, closed) = edges.chain(a, b, key);
    let points: Vec<[f64; 3]> = chain
        .iter()
        .filter_map(|k| edges.position.get(k).copied())
        .collect();
    Ok(figures(pick.mesh, key, points, closed))
}

fn figures(
    mesh: usize,
    faces: [Option<usize>; 2],
    points: Vec<[f64; 3]>,
    closed: bool,
) -> EdgeFigures {
    let mut length_sum: f64 = points
        .windows(2)
        .map(|w| match w {
            [p, q] => length(sub(*q, *p)),
            _ => 0.0,
        })
        .sum();
    let (first, last) = (points.first().copied(), points.last().copied());
    if closed && let (Some(p), Some(q)) = (first, last) {
        length_sum += length(sub(p, q));
    }
    let chord = match (closed, first, last) {
        (false, Some(p), Some(q)) => length(sub(q, p)),
        _ => 0.0,
    };
    let shape = edge_shape(&points, closed, length_sum);
    EdgeFigures {
        mesh,
        faces,
        points,
        closed,
        length: length_sum,
        chord,
        shape,
    }
}

/// How far off a fitted line or circle a corner may lie, as a share of the
/// edge's length: room for coordinates stored as 32-bit floats.
const CURVE_SLACK: f64 = 1e-5;

fn edge_shape(points: &[[f64; 3]], closed: bool, total: f64) -> EdgeShape {
    let slack = CURVE_SLACK * total;
    let (Some(&p), Some(&q)) = (points.first(), points.last()) else {
        return EdgeShape::Curve;
    };
    if !closed && let Some(direction) = normalize(sub(q, p)) {
        let off = points
            .iter()
            .map(|&x| length(sub(sub(x, p), scale(direction, dot(sub(x, p), direction)))))
            .fold(0.0, f64::max);
        if off <= slack {
            return EdgeShape::Line { direction };
        }
    }
    circle(points, closed, slack).unwrap_or(EdgeShape::Curve)
}

fn circle(points: &[[f64; 3]], closed: bool, slack: f64) -> Option<EdgeShape> {
    let n = points.len() as f64;
    let mean = scale(points.iter().fold([0.0; 3], |s, &x| add(s, x)), 1.0 / n);
    let mut m = [[0.0; 3]; 3];
    for &x in points {
        let d = sub(x, mean);
        for (r, &dr) in m.iter_mut().zip(&d) {
            for (c, &dc) in r.iter_mut().zip(&d) {
                *c += dr * dc;
            }
        }
    }
    let axis = least_eigenvector(m);
    let (u, v) = across(axis)?;
    let flat: Vec<(f64, f64)> = points.iter().map(|&x| (dot(x, u), dot(x, v))).collect();
    let (cu, cv, radius) = fit_circle(&flat)?;
    let height = dot(mean, axis);
    let residual = points
        .iter()
        .zip(&flat)
        .map(|(&x, &(a, b))| {
            let off_plane = dot(x, axis) - height;
            ((a - cu).hypot(b - cv) - radius).hypot(off_plane)
        })
        .fold(0.0, f64::max);
    if residual > slack.max(radius * CURVE_SLACK) {
        return None;
    }
    let turn: f64 = flat
        .windows(2)
        .map(|w| match w {
            [(a, b), (c, d)] => {
                let (x0, y0, x1, y1) = (a - cu, b - cv, c - cu, d - cv);
                (x0 * y1 - y0 * x1).atan2(x0 * x1 + y0 * y1)
            }
            _ => 0.0,
        })
        .sum();
    let sweep = if closed {
        std::f64::consts::TAU
    } else {
        turn.abs()
    };
    let centre = add(add(scale(u, cu), scale(v, cv)), scale(axis, height));
    Some(EdgeShape::Circle {
        centre,
        axis,
        radius,
        residual,
        sweep_degrees: sweep.to_degrees(),
        arc_length: radius * sweep,
    })
}
