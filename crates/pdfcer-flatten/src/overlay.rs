//! The overlay of the paints' shapes: the atomic regions, each covered by a
//! fixed set of paints. Polygon booleans are `i_overlay`'s.

use i_overlay::core::fill_rule::FillRule as IFill;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::simplify::SimplifyShape;
use i_overlay::float::single::SingleFloatOverlay;

use crate::punch::{self, Meet};
use crate::{Ceiling, FillRule, FlattenError, Limits, Point, Shape};

/// i_overlay's shape set: each shape an outer contour and its holes.
pub(crate) type Shapes = Vec<Vec<Vec<Point>>>;

/// Regions smaller than this many square device units are dropped: they are
/// the slivers floating-point booleans leave along shared edges.
const SLIVER: f64 = 1e-3;

/// One atomic region before compositing.
pub(crate) struct Atom {
    pub(crate) contours: Vec<Vec<Point>>,
    pub(crate) paints: Vec<usize>,
}

/// The shoelace area of a closed contour: positive when counter-clockwise,
/// so its sign is the contour's winding.
pub(crate) fn signed_area(c: &[Point]) -> f64 {
    let mut s = 0.0;
    let mut prev = c.last().copied().unwrap_or([0.0, 0.0]);
    for &p in c {
        s += prev[0] * p[1] - p[0] * prev[1];
        prev = p;
    }
    s / 2.0
}

/// The area of an i_overlay result: each shape is an outer contour and its
/// holes.
fn area(shapes: &Shapes) -> f64 {
    shapes
        .iter()
        .map(|shape| {
            let mut it = shape.iter();
            let outer = it.next().map_or(0.0, |c| signed_area(c).abs());
            outer - it.map(|c| signed_area(c).abs()).sum::<f64>()
        })
        .sum()
}

fn vertices(shapes: &Shapes) -> usize {
    shapes.iter().flatten().map(Vec::len).sum()
}

fn normalise(shape: &Shape) -> Shapes {
    let finite: Vec<Vec<Point>> = shape
        .contours
        .iter()
        .filter(|c| c.len() >= 3 && c.iter().all(|p| p[0].is_finite() && p[1].is_finite()))
        .cloned()
        .collect();
    if finite.is_empty() {
        return Vec::new();
    }
    let rule = match shape.rule {
        FillRule::NonZero => IFill::NonZero,
        FillRule::EvenOdd => IFill::EvenOdd,
    };
    finite.simplify_shape(rule)
}

fn boolean(a: &Shapes, b: &Shapes, rule: OverlayRule) -> Shapes {
    if a.is_empty() {
        return match rule {
            OverlayRule::Union => b.clone(),
            _ => Vec::new(),
        };
    }
    if b.is_empty() {
        return match rule {
            OverlayRule::Intersect => Vec::new(),
            _ => a.clone(),
        };
    }
    a.overlay(b, rule, IFill::EvenOdd)
}

/// [`Shape::intersection`].
pub(crate) fn intersection(a: &Shape, b: &Shape) -> Shape {
    let both = boolean(&normalise(a), &normalise(b), OverlayRule::Intersect);
    Shape {
        contours: both.into_iter().flatten().collect(),
        rule: FillRule::EvenOdd,
    }
}

/// `[x0, y0, x1, y1]`.
pub(crate) type Bbox = [f64; 4];

fn bbox(shapes: &Shapes) -> Bbox {
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for p in shapes.iter().flatten().flatten() {
        b = [
            b[0].min(p[0]),
            b[1].min(p[1]),
            b[2].max(p[0]),
            b[3].max(p[1]),
        ];
    }
    b
}

/// Whether two boxes share area. Shapes whose boxes do not cannot share
/// area either, so their booleans are known without running them.
fn meets(a: &Bbox, b: &Bbox) -> bool {
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

/// A region under construction and what [`atoms`] needs to know about it
/// without walking its contours.
struct Piece {
    shape: Shapes,
    bbox: Bbox,
    vertices: usize,
    paints: Vec<usize>,
}

impl Piece {
    fn new(shape: Shapes, paints: Vec<usize>) -> Self {
        Self {
            bbox: bbox(&shape),
            vertices: vertices(&shape),
            shape,
            paints,
        }
    }
}

/// [`boolean`], charged to [`Limits::max_work`]: every vertex fed to a
/// boolean is one unit.
struct Budget {
    spent: usize,
    limit: usize,
}

impl Budget {
    fn charge(&mut self, units: usize) -> Result<(), FlattenError> {
        self.spent = self.spent.saturating_add(units);
        if self.spent > self.limit {
            return Err(FlattenError::TooComplex(Ceiling::Work));
        }
        Ok(())
    }

    fn boolean(
        &mut self,
        a: &Shapes,
        b: &Shapes,
        rule: OverlayRule,
    ) -> Result<Shapes, FlattenError> {
        self.charge(vertices(a).saturating_add(vertices(b)))?;
        let out = boolean(a, b, rule);
        self.charge(vertices(&out))?;
        Ok(out)
    }
}

/// The pieces a step produces, refused the moment they pass
/// [`Limits::max_regions`] or [`Limits::max_vertices`], before the rest of
/// the step's booleans run: one boolean between two shapes that cross at
/// nearly every edge can cost tens of microseconds a vertex.
struct Step<'a> {
    pieces: Vec<Piece>,
    vertices: usize,
    limits: &'a Limits,
}

impl Step<'_> {
    fn push(&mut self, piece: Piece) -> Result<(), FlattenError> {
        self.vertices = self.vertices.saturating_add(piece.vertices);
        self.pieces.push(piece);
        if self.pieces.len() > self.limits.max_regions {
            return Err(FlattenError::TooComplex(Ceiling::Regions));
        }
        if self.vertices > self.limits.max_vertices {
            return Err(FlattenError::TooComplex(Ceiling::Vertices));
        }
        Ok(())
    }

    /// Refuse now if the pieces not yet cut, `rest` vertices, cannot fit:
    /// each passes through or splits into parts that share its boundary,
    /// so it carries at least its vertices into the step.
    fn fits(&self, rest: usize) -> Result<(), FlattenError> {
        if self.vertices.saturating_add(rest) > self.limits.max_vertices {
            return Err(FlattenError::TooComplex(Ceiling::Vertices));
        }
        Ok(())
    }
}

/// Cut every piece by `s` (paint `i`, box `sb`) into `next`, and return
/// the part of `s` no piece covers. A piece whose box misses `sb` passes
/// through untouched; the uncovered part is `s` less every piece's
/// intersection with it, which is `s` less the union of the pieces.
fn cut(
    pieces: Vec<Piece>,
    s: &Shapes,
    sb: &Bbox,
    i: usize,
    budget: &mut Budget,
    next: &mut Step<'_>,
) -> Result<Shapes, FlattenError> {
    let mut fresh = s.clone();
    // Set once `s` is found inside one piece: the pieces are disjoint, so
    // no other piece meets it.
    let mut placed = false;
    let mut rest: usize = pieces.iter().map(|p| p.vertices).sum();
    for piece in pieces {
        rest = rest.saturating_sub(piece.vertices);
        if placed || !meets(&piece.bbox, sb) {
            next.push(piece)?;
            continue;
        }
        match punch::meet(&piece.shape, s, sb) {
            Meet::Apart => {
                next.push(piece)?;
                continue;
            }
            Meet::Within => {
                let mut with = piece.paints.clone();
                with.push(i);
                next.push(Piece::new(s.clone(), with))?;
                let shape = punch::punched(piece.shape, s);
                next.push(Piece::new(shape, piece.paints))?;
                fresh.clear();
                placed = true;
                continue;
            }
            Meet::Unknown => {}
        }
        next.fits(rest.saturating_add(piece.vertices))?;
        let inside = budget.boolean(&piece.shape, s, OverlayRule::Intersect)?;
        if inside.is_empty() {
            next.push(piece)?;
            continue;
        }
        let outside = budget.boolean(&piece.shape, s, OverlayRule::Difference)?;
        if !fresh.is_empty() {
            fresh = budget.boolean(&fresh, &inside, OverlayRule::Difference)?;
        }
        if area(&inside) >= SLIVER {
            let mut with = piece.paints.clone();
            with.push(i);
            next.push(Piece::new(inside, with))?;
        }
        if area(&outside) >= SLIVER {
            next.push(Piece::new(outside, piece.paints))?;
        }
    }
    Ok(fresh)
}

/// Split the plane by every shape in turn. A region that no shape covers is
/// not returned.
///
/// # Errors
///
/// [`FlattenError::TooComplex`] past [`Limits::max_regions`],
/// [`Limits::max_vertices`] or [`Limits::max_work`].
pub(crate) fn atoms(shapes: &[&Shape], limits: &Limits) -> Result<Vec<Atom>, FlattenError> {
    let mut pieces: Vec<Piece> = Vec::new();
    let mut budget = Budget {
        spent: 0,
        limit: limits.max_work,
    };
    for (i, shape) in shapes.iter().enumerate() {
        let s = normalise(shape);
        if area(&s) < SLIVER {
            continue;
        }
        let sb = bbox(&s);
        let mut next = Step {
            pieces: Vec::with_capacity(pieces.len() + 2),
            vertices: 0,
            limits,
        };
        let fresh = cut(pieces, &s, &sb, i, &mut budget, &mut next)?;
        if area(&fresh) >= SLIVER {
            next.push(Piece::new(fresh, vec![i]))?;
        }
        pieces = next.pieces;
    }
    Ok(pieces
        .into_iter()
        .map(|p| Atom {
            contours: p.shape.into_iter().flatten().collect(),
            paints: p.paints,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    // Tests fail by panicking.
    #![allow(clippy::indexing_slicing)]
    use super::*;

    fn square(x: f64, y: f64, w: f64) -> Shape {
        Shape {
            contours: vec![vec![[x, y], [x + w, y], [x + w, y + w], [x, y + w]]],
            rule: FillRule::NonZero,
        }
    }

    #[test]
    fn two_overlapping_squares_make_three_atoms_of_the_right_area() {
        let (a, b) = (square(0.0, 0.0, 2.0), square(1.0, 1.0, 2.0));
        let atoms = atoms(&[&a, &b], &Limits::default()).unwrap_or_default();
        assert_eq!(atoms.len(), 3);
        let area_of = |ps: &[usize]| {
            atoms
                .iter()
                .find(|t| t.paints == ps)
                .map(|t| t.contours.iter().map(|c| signed_area(c)).sum::<f64>().abs())
                .unwrap_or(-1.0)
        };
        assert!((area_of(&[0, 1]) - 1.0).abs() < 1e-6);
        assert!((area_of(&[0]) - 3.0).abs() < 1e-6);
        assert!((area_of(&[1]) - 3.0).abs() < 1e-6);
    }

    #[test]
    fn an_even_odd_ring_has_a_hole_and_a_non_zero_one_does_not() {
        let mut ring = square(0.0, 0.0, 4.0);
        ring.contours
            .push(vec![[1.0, 1.0], [3.0, 1.0], [3.0, 3.0], [1.0, 3.0]]);
        ring.rule = FillRule::EvenOdd;
        assert!((area(&normalise(&ring)) - 12.0).abs() < 1e-6);
        ring.rule = FillRule::NonZero;
        assert!((area(&normalise(&ring)) - 16.0).abs() < 1e-6);
    }

    #[test]
    fn the_region_ceiling_refuses() {
        let shapes: Vec<Shape> = (0..6)
            .map(|i| square(f64::from(i) * 0.5, 0.0, 4.0))
            .collect();
        let refs: Vec<&Shape> = shapes.iter().collect();
        let limits = Limits {
            max_regions: 4,
            ..Limits::default()
        };
        assert!(matches!(
            atoms(&refs, &limits),
            Err(FlattenError::TooComplex(Ceiling::Regions))
        ));
    }
}
