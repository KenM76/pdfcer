//! Page-space path geometry for point hit-testing: fill containment
//! (§8.5.3 winding rules), outline proximity and cubic flattening.

use super::clip::transformed_bounds;
use super::decompose::{FillRule, PathObject, Segment, Subpath};
use super::geometry::{Bounds, Matrix, Point};
use super::hit::FLATTEN_STEPS;

/// Whether `point` hits a path object: inside its fill (if filled) or
/// within the stroke/clip proximity threshold of its outline.
pub(super) fn path_hit(path: &PathObject, point: Point, tolerance: f64) -> bool {
    // A cheap bbox reject first (the object's page bbox widened by the
    // stroke half-width and tolerance).
    let half = stroke_half_width(path);
    if !path.page_bbox.inflate(half + tolerance).contains(point) {
        return false;
    }

    if let Some(rule) = path.style.fill {
        // A closed subpath whose box misses the point adds winding 0 and an
        // even crossing count, so only boxes containing it can matter.
        let near = page_subpaths_near(path, point, 0.0);
        if point_inside(&near, point, rule) {
            return true;
        }
    }

    let threshold = if path.style.stroke {
        half + tolerance
    } else {
        // A filled-only or `n` path: no stroke, but a near-edge click
        // should still land, so use the tolerance alone as the proximity
        // band.
        tolerance
    };
    let near = page_subpaths_near(path, point, threshold);
    outline_within(&near, point, threshold)
}

/// Slack on a culling box, absorbing the rounding of the inverse mapping.
/// Page space is in points.
const CULL_SLACK: f64 = 1e-6;

/// The path's subpaths in page space, keeping only those whose user-space
/// control-polygon hull meets the user-space box enclosing the page-space
/// square of half-side `reach` around `point`; the rest are never
/// transformed, so a dense path (one object of 10^5 subpaths) costs a click
/// only the subpaths near it. The hull contains every flattened vertex, and
/// the box contains the preimage of every page point within `reach`, so
/// culling never drops a subpath that could hit. A CTM without a finite
/// inverse keeps every subpath.
pub(super) fn page_subpaths_near(path: &PathObject, point: Point, reach: f64) -> Vec<Subpath> {
    let page_box = Bounds {
        min: point,
        max: point,
    }
    .inflate(reach + CULL_SLACK);
    let query = path
        .ctm
        .inverse()
        .map(|inv| transformed_bounds(page_box, inv))
        .filter(|b| b.min.is_finite() && b.max.is_finite());
    path.subpaths
        .iter()
        .filter(|s| query.is_none_or(|q| overlaps(hull_bounds(s), q)))
        .map(|s| s.transformed(path.ctm))
        .collect()
}

/// Whether two boxes share a point (edges count).
fn overlaps(a: Bounds, b: Bounds) -> bool {
    a.min.x <= b.max.x && b.min.x <= a.max.x && a.min.y <= b.max.y && b.min.y <= a.max.y
}

/// User-space box of a subpath's anchors and control points. Non-finite
/// points are skipped, as `flatten` drops them.
fn hull_bounds(sp: &Subpath) -> Bounds {
    let mut b = Bounds::EMPTY;
    let mut add = |p: Point| {
        if p.is_finite() {
            b = b.union_point(p);
        }
    };
    add(sp.start);
    for seg in &sp.segments {
        match *seg {
            Segment::Line { to } => add(to),
            Segment::Cubic { c1, c2, to } => {
                add(c1);
                add(c2);
                add(to);
            }
        }
    }
    b
}

/// The user-space line width scaled into page space by the object's CTM,
/// halved — the distance the stroke extends either side of the path
/// centerline (§8.4.3.2). A width-0 hairline gets a tiny nominal value so
/// it is still selectable.
pub(super) fn stroke_half_width(path: &PathObject) -> f64 {
    if !path.style.stroke {
        return 0.0;
    }
    let scale = ctm_scale(path.ctm);
    let w = if path.line_width <= 0.0 {
        0.1
    } else {
        path.line_width
    };
    (w * scale) / 2.0
}

/// A scalar page-space scale estimate for a CTM — the square root of the
/// absolute determinant (the geometric-mean linear scale). Used to map a
/// user-space line width into page space for stroke proximity. A
/// degenerate/non-finite CTM yields a harmless 1.0.
pub(super) fn ctm_scale(ctm: Matrix) -> f64 {
    let d = ctm.determinant().abs();
    if d.is_finite() && d > 0.0 {
        d.sqrt()
    } else {
        1.0
    }
}

/// Whether `point` is inside the region the subpaths fill, under `rule`
/// (every subpath treated as closed — a fill implicitly closes, §8.5.3.1).
pub(super) fn point_inside(subpaths: &[Subpath], point: Point, rule: FillRule) -> bool {
    let mut winding = 0i32;
    let mut crossings = 0u32;
    for sp in subpaths {
        let poly = flatten(sp);
        accumulate_crossings(&poly, point, &mut winding, &mut crossings);
    }
    match rule {
        FillRule::NonZero => winding != 0,
        FillRule::EvenOdd => crossings % 2 == 1,
    }
}

/// Whether `point` is within `threshold` of any outline segment (stroke /
/// clip proximity). Closed subpaths include their closing edge.
pub(super) fn outline_within(subpaths: &[Subpath], point: Point, threshold: f64) -> bool {
    let t2 = threshold * threshold;
    for sp in subpaths {
        let poly = flatten(sp);
        let n = poly.len();
        if n == 0 {
            continue;
        }
        for w in poly.windows(2) {
            let [a, b] = w else { continue };
            if dist_sq_point_segment(point, *a, *b) <= t2 {
                return true;
            }
        }
        // Closing edge, for a closed subpath (a stroked `h`/`re`/`s`).
        if sp.closed
            && n >= 2
            && let (Some(&last), Some(&firstp)) = (poly.last(), poly.first())
            && dist_sq_point_segment(point, last, firstp) <= t2
        {
            return true;
        }
    }
    false
}

/// Flatten one subpath (page space) to a polyline of on-curve vertices,
/// cubics subdivided into [`FLATTEN_STEPS`] chords. Non-finite vertices
/// are dropped (a hostile operand cannot poison the ray cast).
pub(super) fn flatten(sp: &Subpath) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::new();
    let push = |p: Point, out: &mut Vec<Point>| {
        if p.is_finite() {
            out.push(p);
        }
    };
    push(sp.start, &mut out);
    let mut from = sp.start;
    for seg in &sp.segments {
        match *seg {
            Segment::Line { to } => {
                push(to, &mut out);
                from = to;
            }
            Segment::Cubic { c1, c2, to } => {
                for step in 1..=FLATTEN_STEPS {
                    let t = step as f64 / FLATTEN_STEPS as f64;
                    push(cubic_at(from, c1, c2, to, t), &mut out);
                }
                from = to;
            }
        }
    }
    out
}

/// A cubic Bézier point at parameter `t` (de Casteljau, closed form).
pub(super) fn cubic_at(p0: Point, c1: Point, c2: Point, p3: Point, t: f64) -> Point {
    let u = 1.0 - t;
    let w0 = u * u * u;
    let w1 = 3.0 * u * u * t;
    let w2 = 3.0 * u * t * t;
    let w3 = t * t * t;
    Point::new(
        w0 * p0.x + w1 * c1.x + w2 * c2.x + w3 * p3.x,
        w0 * p0.y + w1 * c1.y + w2 * c2.y + w3 * p3.y,
    )
}

/// Fold one closed polygon's edge crossings of the ray `y = point.y,
/// x ≥ point.x` into the running winding number (signed, for nonzero) and
/// crossing count (unsigned, for even-odd). Standard robust half-open
/// (`[y0, y1)`) crossing test.
pub(super) fn accumulate_crossings(
    poly: &[Point],
    point: Point,
    winding: &mut i32,
    crossings: &mut u32,
) {
    if poly.len() < 2 {
        return;
    }
    // Every consecutive pair, plus the closing edge (last → first) so the
    // polygon is treated as closed (a fill implicitly closes).
    let closing = match (poly.first(), poly.last()) {
        (Some(&f), Some(&l)) => Some((l, f)),
        _ => None,
    };
    let pairs = poly.windows(2).filter_map(|w| match w {
        [a, b] => Some((*a, *b)),
        _ => None,
    });
    for (a, b) in pairs.chain(closing) {
        // Half-open interval avoids double-counting a vertex on the ray.
        let a_below = a.y <= point.y;
        let b_below = b.y <= point.y;
        if a_below == b_below {
            continue;
        }
        // The edge crosses the horizontal line through `point`; find the x
        // of the intersection.
        let dy = b.y - a.y;
        if dy == 0.0 {
            continue;
        }
        let t = (point.y - a.y) / dy;
        let x = a.x + t * (b.x - a.x);
        if x >= point.x {
            *crossings += 1;
            if b.y > a.y {
                *winding += 1; // upward edge
            } else {
                *winding -= 1; // downward edge
            }
        }
    }
}

/// Squared distance from `p` to the segment `a`–`b` (avoids a `sqrt` in
/// the proximity loop). A degenerate segment (`a == b`) reduces to the
/// point distance.
pub(super) fn dist_sq_point_segment(p: Point, a: Point, b: Point) -> f64 {
    let vx = b.x - a.x;
    let vy = b.y - a.y;
    let wx = p.x - a.x;
    let wy = p.y - a.y;
    let len2 = vx * vx + vy * vy;
    if len2 <= 0.0 {
        return wx * wx + wy * wy;
    }
    let t = ((wx * vx + wy * vy) / len2).clamp(0.0, 1.0);
    let dx = wx - t * vx;
    let dy = wy - t * vy;
    dx * dx + dy * dy
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)] // Tests fail loudly.
mod tests {
    use super::*;
    use crate::content::ContentStream;
    use crate::vector::decompose::{NoXObjects, VectorObject, decompose};

    fn paths(src: &str) -> Vec<PathObject> {
        let cs = ContentStream::parse(src.as_bytes().to_vec()).unwrap();
        decompose(&cs, Matrix::IDENTITY, &NoXObjects)
            .objects
            .into_iter()
            .map(|o| match o {
                VectorObject::Path(p) => p,
                other => panic!("not a path: {other:?}"),
            })
            .collect()
    }

    /// The unculled answer: every subpath transformed and tested.
    fn reference(path: &PathObject, point: Point, tolerance: f64) -> bool {
        let half = stroke_half_width(path);
        if !path.page_bbox.inflate(half + tolerance).contains(point) {
            return false;
        }
        let all = path.page_subpaths();
        if let Some(rule) = path.style.fill
            && point_inside(&all, point, rule)
        {
            return true;
        }
        let threshold = if path.style.stroke {
            half + tolerance
        } else {
            tolerance
        };
        outline_within(&all, point, threshold)
    }

    #[test]
    fn culling_never_changes_a_hit_under_a_rotated_scaled_ctm() {
        // Nested rings (even-odd and nonzero), a curve, open strokes and a
        // clip-only path, under a CTM that rotates, shears and scales.
        let body = "0 0 m 100 0 l 100 100 l 0 100 l h 20 20 m 80 20 l 80 80 l 20 80 l h \
                    40 40 m 60 40 l 60 60 l 40 60 l h 10 120 m 30 160 70 160 90 120 c \
                    0 200 m 100 200 l 50 230 m 50 260 l";
        let ctm = "0.8 0.5 -0.4 0.9 120 40 cm";
        let src = format!(
            "q {ctm} {body} f* Q q {ctm} {body} B Q q {ctm} 2 w {body} S Q q {ctm} {body} n Q"
        );
        let all = paths(&src);
        assert_eq!(all.len(), 4);
        let mut checked = 0;
        for path in &all {
            for ix in 0..90 {
                for iy in 0..90 {
                    let p = Point::new(-60.0 + f64::from(ix) * 3.7, -10.0 + f64::from(iy) * 3.9);
                    for tol in [0.0, 1.5] {
                        assert_eq!(
                            path_hit(path, p, tol),
                            reference(path, p, tol),
                            "{p:?} {tol}"
                        );
                        checked += 1;
                    }
                }
            }
        }
        assert_eq!(checked, 4 * 90 * 90 * 2);
    }

    #[test]
    fn a_dense_path_transforms_only_the_subpaths_near_the_point() {
        let mut src = String::from("q 2 0 0 2 0 0 cm ");
        for i in 0..2000 {
            src.push_str(&format!("{i} 0 m {i} 10 l "));
        }
        src.push_str("S Q");
        let path = paths(&src).remove(0);
        let at = Point::new(1001.0, 5.0);
        let near = page_subpaths_near(&path, at, 1.0);
        assert!((1..=3).contains(&near.len()), "{}", near.len());
        assert!(path_hit(&path, at, 1.0));
        assert!(!path_hit(&path, Point::new(1001.0, 25.0), 1.0));
    }
}
