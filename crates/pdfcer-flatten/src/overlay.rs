//! The overlay of the paints' shapes: the atomic regions, each covered by a
//! fixed set of paints. Polygon booleans are `i_overlay`'s.

use i_overlay::core::fill_rule::FillRule as IFill;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::simplify::SimplifyShape;
use i_overlay::float::single::SingleFloatOverlay;

use crate::{Ceiling, FillRule, FlattenError, Limits, Point, Shape};

type Shapes = Vec<Vec<Vec<Point>>>;

/// Regions smaller than this many square device units are dropped: they are
/// the slivers floating-point booleans leave along shared edges.
const SLIVER: f64 = 1e-3;

/// One atomic region before compositing.
pub(crate) struct Atom {
    pub(crate) contours: Vec<Vec<Point>>,
    pub(crate) paints: Vec<usize>,
}

fn signed_area(c: &[Point]) -> f64 {
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

/// Split the plane by every shape in turn. A region that no shape covers is
/// not returned.
///
/// # Errors
///
/// [`FlattenError::TooComplex`] past [`Limits::max_regions`] or
/// [`Limits::max_vertices`].
pub(crate) fn atoms(shapes: &[&Shape], limits: &Limits) -> Result<Vec<Atom>, FlattenError> {
    let mut regions: Vec<(Shapes, Vec<usize>)> = Vec::new();
    let mut covered: Shapes = Vec::new();
    for (i, shape) in shapes.iter().enumerate() {
        let s = normalise(shape);
        if area(&s) < SLIVER {
            continue;
        }
        let mut next = Vec::with_capacity(regions.len() * 2 + 1);
        for (r, paints) in regions {
            let inside = boolean(&r, &s, OverlayRule::Intersect);
            if area(&inside) >= SLIVER {
                let mut with = paints.clone();
                with.push(i);
                next.push((inside, with));
            }
            let outside = boolean(&r, &s, OverlayRule::Difference);
            if area(&outside) >= SLIVER {
                next.push((outside, paints));
            }
        }
        let fresh = boolean(&s, &covered, OverlayRule::Difference);
        if area(&fresh) >= SLIVER {
            next.push((fresh, vec![i]));
        }
        covered = boolean(&covered, &s, OverlayRule::Union);
        regions = next;
        if regions.len() > limits.max_regions {
            return Err(FlattenError::TooComplex(Ceiling::Regions));
        }
        let total = vertices(&covered) + regions.iter().map(|(r, _)| vertices(r)).sum::<usize>();
        if total > limits.max_vertices {
            return Err(FlattenError::TooComplex(Ceiling::Vertices));
        }
    }
    Ok(regions
        .into_iter()
        .map(|(r, paints)| Atom {
            contours: r.into_iter().flatten().collect(),
            paints,
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
