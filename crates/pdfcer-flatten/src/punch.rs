//! The overlay's fast path for a paint that lies wholly inside one face of
//! a region and touches none of its boundary — a glyph on a background:
//! the cut is then exact without a polygon boolean, at the cost of one scan
//! of the region's edges instead of a boolean's sort.

use crate::Point;
use crate::overlay::{Bbox, Shapes, signed_area};

/// Where a paint lies against a region whose box it meets.
pub(crate) enum Meet {
    /// In the region's interior, clear of its boundary.
    Within,
    /// Clear of the region altogether.
    Apart,
    /// Some edge of the region comes near the paint's box: run the
    /// booleans.
    Unknown,
}

/// Whether segment `a`–`b`'s box meets `bx`, edges included. Conservative:
/// a segment that misses `bx` but whose box meets it says yes.
fn near(a: Point, b: Point, bx: &Bbox) -> bool {
    a[0].min(b[0]) <= bx[2]
        && bx[0] <= a[0].max(b[0])
        && a[1].min(b[1]) <= bx[3]
        && bx[1] <= a[1].max(b[1])
}

/// Even-odd crossing count of `p` against every contour.
fn inside(region: &[Vec<Vec<Point>>], p: Point) -> bool {
    let mut odd = false;
    for c in region.iter().flatten() {
        let mut prev = match c.last() {
            Some(&q) => q,
            None => continue,
        };
        for &q in c {
            if (q[1] > p[1]) != (prev[1] > p[1]) {
                let x = q[0] + (p[1] - q[1]) / (prev[1] - q[1]) * (prev[0] - q[0]);
                if p[0] < x {
                    odd = !odd;
                }
            }
            prev = q;
        }
    }
    odd
}

/// Where `paint` (box `pb`) lies against `region`. Exact when it answers
/// [`Meet::Within`] or [`Meet::Apart`]: no edge of `region` comes within
/// `pb`, so every point of `pb` is on the same side of the boundary as
/// the paint's first vertex.
pub(crate) fn meet(region: &Shapes, paint: &Shapes, pb: &Bbox) -> Meet {
    for c in region.iter().flatten() {
        let mut prev = match c.last() {
            Some(&q) => q,
            None => continue,
        };
        for &q in c {
            if near(prev, q, pb) {
                return Meet::Unknown;
            }
            prev = q;
        }
    }
    let Some(&probe) = paint.iter().flatten().flatten().next() else {
        return Meet::Unknown;
    };
    if inside(region, probe) {
        Meet::Within
    } else {
        Meet::Apart
    }
}

/// `region` less a `paint` that is [`Meet::Within`] it: the paint's outer
/// contours become holes of the region shape it sits in, and the paint's
/// holes become islands. Under even-odd every contour keeps its meaning;
/// the grouping keeps [`crate::overlay`]'s area arithmetic (first contour
/// outer, the rest holes) true.
pub(crate) fn punched(mut region: Shapes, paint: &Shapes) -> Shapes {
    let Some(&probe) = paint.iter().flatten().flatten().next() else {
        return region;
    };
    let host = region
        .iter()
        .position(|shape| inside(std::slice::from_ref(shape), probe));
    let Some(host) = host.and_then(|h| region.get_mut(h)) else {
        return region;
    };
    let ccw = host.first().is_some_and(|c| signed_area(c) > 0.0);
    // Holes wind against the host's outer and islands with it, as the
    // booleans' own output does.
    let oriented = |c: &Vec<Point>, with: bool| {
        let mut c = c.clone();
        if (signed_area(&c) > 0.0) != (ccw == with) {
            c.reverse();
        }
        c
    };
    let mut islands = Vec::new();
    for shape in paint {
        let mut it = shape.iter();
        if let Some(outer) = it.next() {
            host.push(oriented(outer, false));
        }
        islands.extend(it.map(|hole| vec![oriented(hole, true)]));
    }
    region.extend(islands);
    region
}
