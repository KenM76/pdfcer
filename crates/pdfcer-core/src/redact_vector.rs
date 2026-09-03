//! # Redaction of vector paths — cut the geometry out, never clip it (ISO 32000-1 §12.5.6.23)
//!
//! The third kind of surgery under [`crate::redact`]: glyphs are removed from
//! show strings, image samples are overwritten in a grid, and **path geometry
//! is cut** so that no painted segment or filled area lies inside a
//! redaction region. §12.5.6.23's *"remove all traces of the specified
//! content"* is the requirement; the same clause's ban on hiding image data
//! with clipping is read here as binding on vector content too, because a
//! path under a clip is a path whose bytes survive.
//!
//! ## What "cut" means, per painting operator
//!
//! A path object is `m … l … c … re … h` construction followed by exactly one
//! painting operator (§8.5). Each is rewritten in the coordinate system it
//! was authored in, so the bytes around it — the CTM, the colour, the line
//! width — are untouched:
//!
//! | operator | rewrite |
//! |---|---|
//! | `S`, `s` (stroke) | every segment is **cut** against the region **expanded by the stroke width**: lines by Liang–Barsky, cubics by recursive subdivision (a piece whose control hull is wholly outside is kept as a curve; wholly inside is dropped; straddling is split until one or the other, or until it is smaller than the tolerance, when it is dropped). A closed subpath is opened at the cuts. |
//! | `f`, `F`, `f*` (fill) | the region is subtracted by **decomposing the complement into four strips** (left, right, above, below, bounded by the path's own box) and clipping every subpath to each strip with Sutherland–Hodgman. Each non-empty strip becomes its own path object painted with the same operator. A subpath wholly inside a strip is kept exactly, curves and all; one that crosses a strip edge is flattened first. |
//! | `B`, `B*`, `b`, `b*` (fill + stroke) | both of the above, against the **expanded** region, emitted as a fill object followed by a stroke object. |
//! | `n` | ends a path without painting — a clip definition, not content. Never touched. |
//!
//! ## Why strips rather than a polygon boolean
//!
//! `polygon − rect` in general needs a robust boolean-operations library
//! (self-intersections, winding rules, coincident edges), and a wrong
//! boolean is a *silent* wrong picture. `polygon ∩ convex-rect` needs only
//! Sutherland–Hodgman, which is exact for any subject polygon — concave,
//! self-intersecting, multi-subpath — in the one property that matters here:
//! **the winding number of every point inside the clip rectangle is
//! preserved**, so both nonzero and even-odd fills stay correct. The
//! complement of a rectangle within a bounding box is four rectangles, so
//! four clips give the difference exactly, at the cost of a few extra path
//! objects for the paths that cross the region. Nothing crosses the region
//! on most pages, and a path wholly inside it is simply deleted.
//!
//! ## Over-cover, stated
//!
//! - A stroke's ink extends half its width beyond the centreline, plus a
//!   projecting cap or a miter. The cutting region for a stroke is therefore
//!   the mark expanded by **one full stroke width** (in page space, under the
//!   CTM's larger scale factor), never less. Where the width is zero (the
//!   thinnest device line), one unit is used.
//! - A cubic piece smaller than the flattening tolerance that still straddles
//!   the boundary is **dropped**, not kept.
//! - A path under a **singular CTM** (zero-area placement) that touches a
//!   region is dropped whole: it cannot be inverted back into its own
//!   coordinates, and a zero-area path is at most a hairline nobody loses.
//!
//! ## The clip-marked path object (`W`/`W*` before the paint)
//!
//! §8.5.4: the clip takes effect **after** painting, from the path as
//! constructed. Rewriting the path would rewrite the clip and cut every
//! later object on the page to a smaller window — content that was never
//! marked would vanish. So a clip-marked object is emitted as **the cut
//! paint first, then the ORIGINAL construction with `W n`**: the paint is
//! what the mark covers, the clip is what the rest of the page depends on.
//! The original geometry therefore survives in the stream as a clip. It is
//! not painted content; it is counted ([`Cut::clip_kept`]) and disclosed,
//! because a clip shaped like a secret is a residual an operator should hear
//! about.
//!
//! ## Numerics
//!
//! Everything is computed in page space (the region is axis-aligned there),
//! then mapped back through the inverse CTM for emission. Flattening
//! tolerance is [`FLATNESS`] page units; the boundary tests carry an
//! [`EPS`] so a segment lying exactly on the region's edge is treated as
//! inside (over-cover).
//!
//! ## Spec sources
//!
//! - `iso32000__s__12.5.6.23.md` — "remove all traces", the clipping ban.
//! - `iso32000__s__8.5.md` — path construction/painting operators, the
//!   one-painting-operator-per-path-object rule, `W`'s deferred effect.
//! - `iso32000__ref__redaction_removal.md` §3 — surgery-and-re-emit rather
//!   than normalising the stream.

use crate::redact::{Mat, RegionBox};
use crate::writer::content::emit_number;

/// Flattening tolerance, page units: the largest distance a flattened chord
/// may sit from its cubic. A twentieth of a point is below any print or
/// screen resolution pdfcer renders at.
const FLATNESS: f64 = 0.05;

/// Boundary slack: a coordinate within this of a region edge counts as on
/// the redacted side.
const EPS: f64 = 1e-6;

/// Recursion ceiling for cubic subdivision (2^12 pieces per curve).
const MAX_SPLIT_DEPTH: u32 = 12;

/// A point in page space or user space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct P {
    pub x: f64,
    pub y: f64,
}

impl P {
    const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    fn lerp(self, o: Self, t: f64) -> Self {
        Self::new(self.x + (o.x - self.x) * t, self.y + (o.y - self.y) * t)
    }

    fn map(self, m: Mat) -> Self {
        let (x, y) = m.apply(self.x, self.y);
        Self::new(x, y)
    }

    fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

/// One segment after the subpath's start point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Seg {
    Line(P),
    Cubic(P, P, P),
}

/// A subpath as constructed: start point, segments, closure.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Subpath {
    pub start: Option<P>,
    pub segs: Vec<Seg>,
    pub closed: bool,
}

impl Subpath {
    fn end(&self) -> Option<P> {
        match self.segs.last() {
            Some(Seg::Line(p) | Seg::Cubic(_, _, p)) => Some(*p),
            None => self.start,
        }
    }

    fn map(&self, m: Mat) -> Self {
        Self {
            start: self.start.map(|p| p.map(m)),
            segs: self
                .segs
                .iter()
                .map(|s| match s {
                    Seg::Line(p) => Seg::Line(p.map(m)),
                    Seg::Cubic(a, b, c) => Seg::Cubic(a.map(m), b.map(m), c.map(m)),
                })
                .collect(),
            closed: self.closed,
        }
    }

    /// Control-hull bounding box.
    fn bbox(&self) -> Option<Rect> {
        let mut r: Option<Rect> = None;
        let mut grow = |p: P| {
            if !p.is_finite() {
                return;
            }
            r = Some(match r {
                None => Rect {
                    x0: p.x,
                    y0: p.y,
                    x1: p.x,
                    y1: p.y,
                },
                Some(b) => Rect {
                    x0: b.x0.min(p.x),
                    y0: b.y0.min(p.y),
                    x1: b.x1.max(p.x),
                    y1: b.y1.max(p.y),
                },
            });
        };
        if let Some(s) = self.start {
            grow(s);
        }
        for s in &self.segs {
            match s {
                Seg::Line(p) => grow(*p),
                Seg::Cubic(a, b, c) => {
                    grow(*a);
                    grow(*b);
                    grow(*c);
                }
            }
        }
        r
    }

    /// The subpath as a polyline (closing segment appended when closed).
    fn flatten(&self) -> Vec<P> {
        let Some(start) = self.start else {
            return Vec::new();
        };
        let mut out = vec![start];
        let mut cur = start;
        for s in &self.segs {
            match *s {
                Seg::Line(p) => {
                    out.push(p);
                    cur = p;
                }
                Seg::Cubic(a, b, c) => {
                    flatten_cubic(cur, a, b, c, &mut out);
                    cur = c;
                }
            }
        }
        out
    }
}

/// An axis-aligned rectangle in page space.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Rect {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl Rect {
    fn from_region(r: RegionBox) -> Self {
        Self {
            x0: r.min_x,
            y0: r.min_y,
            x1: r.max_x,
            y1: r.max_y,
        }
    }

    fn expand(self, m: f64) -> Self {
        Self {
            x0: self.x0 - m,
            y0: self.y0 - m,
            x1: self.x1 + m,
            y1: self.y1 + m,
        }
    }

    fn intersects(self, o: Self) -> bool {
        self.x0 < o.x1 + EPS && o.x0 < self.x1 + EPS && self.y0 < o.y1 + EPS && o.y0 < self.y1 + EPS
    }

    fn contains_rect(self, o: Self) -> bool {
        self.x0 - EPS <= o.x0
            && o.x1 <= self.x1 + EPS
            && self.y0 - EPS <= o.y0
            && o.y1 <= self.y1 + EPS
    }

    fn union(self, o: Self) -> Self {
        Self {
            x0: self.x0.min(o.x0),
            y0: self.y0.min(o.y0),
            x1: self.x1.max(o.x1),
            y1: self.y1.max(o.y1),
        }
    }
}

/// Adaptive flattening of one cubic (de Casteljau halving until the
/// control polygon is within [`FLATNESS`] of the chord). Appends every
/// point after `p0`.
fn flatten_cubic(p0: P, p1: P, p2: P, p3: P, out: &mut Vec<P>) {
    fn rec(p0: P, p1: P, p2: P, p3: P, depth: u32, out: &mut Vec<P>) {
        // Distance of the control points from the chord, the standard
        // flatness estimate.
        let dx = p3.x - p0.x;
        let dy = p3.y - p0.y;
        let d1 = ((p1.x - p3.x) * dy - (p1.y - p3.y) * dx).abs();
        let d2 = ((p2.x - p3.x) * dy - (p2.y - p3.y) * dx).abs();
        let flat = (d1 + d2) * (d1 + d2) <= FLATNESS * FLATNESS * (dx * dx + dy * dy);
        if flat
            || depth >= MAX_SPLIT_DEPTH
            || !(p0.is_finite() && p1.is_finite() && p2.is_finite() && p3.is_finite())
        {
            out.push(p3);
            return;
        }
        let (l, r) = split_cubic(p0, p1, p2, p3, 0.5);
        rec(l.0, l.1, l.2, l.3, depth + 1, out);
        rec(r.0, r.1, r.2, r.3, depth + 1, out);
    }
    rec(p0, p1, p2, p3, 0, out);
}

/// A cubic's four control points.
type Cubic = (P, P, P, P);

/// De Casteljau split of a cubic at `t`.
fn split_cubic(p0: P, p1: P, p2: P, p3: P, t: f64) -> (Cubic, Cubic) {
    let p01 = p0.lerp(p1, t);
    let p12 = p1.lerp(p2, t);
    let p23 = p2.lerp(p3, t);
    let p012 = p01.lerp(p12, t);
    let p123 = p12.lerp(p23, t);
    let mid = p012.lerp(p123, t);
    ((p0, p01, p012, mid), (mid, p123, p23, p3))
}

fn hull(p0: P, p1: P, p2: P, p3: P) -> Rect {
    Rect {
        x0: p0.x.min(p1.x).min(p2.x).min(p3.x),
        y0: p0.y.min(p1.y).min(p2.y).min(p3.y),
        x1: p0.x.max(p1.x).max(p2.x).max(p3.x),
        y1: p0.y.max(p1.y).max(p2.y).max(p3.y),
    }
}

// ---------------------------------------------------------------------------
// Strokes: keep the parts OUTSIDE the region
// ---------------------------------------------------------------------------

/// Liang–Barsky: the parameter interval `[t0, t1]` of segment `a→b` that
/// lies INSIDE `r`, or `None` when the segment misses it.
fn inside_interval(a: P, b: P, r: Rect) -> Option<(f64, f64)> {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let mut t0 = 0.0f64;
    let mut t1 = 1.0f64;
    for (p, q) in [
        (-dx, a.x - r.x0),
        (dx, r.x1 - a.x),
        (-dy, a.y - r.y0),
        (dy, r.y1 - a.y),
    ] {
        if p.abs() < 1e-12 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return None;
            }
        }
    }
    Some((t0, t1))
}

/// Cut a stroked subpath (already in page space) against `r`, returning the
/// open pieces that lie outside. A closed subpath is first given its
/// closing segment.
fn cut_stroke(sp: &Subpath, r: Rect) -> Vec<Subpath> {
    let Some(start) = sp.start else {
        return Vec::new();
    };
    let mut segs = sp.segs.clone();
    if sp.closed {
        segs.push(Seg::Line(start));
    }
    let mut pieces: Vec<Subpath> = Vec::new();
    let mut current: Option<Subpath> = None;
    let mut cur = start;
    let push_line =
        |current: &mut Option<Subpath>, pieces: &mut Vec<Subpath>, a: P, b: P| match current {
            Some(c) if c.end() == Some(a) => c.segs.push(Seg::Line(b)),
            _ => {
                if let Some(c) = current.take() {
                    pieces.push(c);
                }
                *current = Some(Subpath {
                    start: Some(a),
                    segs: vec![Seg::Line(b)],
                    closed: false,
                });
            }
        };
    let push_cubic =
        |current: &mut Option<Subpath>, pieces: &mut Vec<Subpath>, a: P, c1: P, c2: P, b: P| {
            match current {
                Some(c) if c.end() == Some(a) => c.segs.push(Seg::Cubic(c1, c2, b)),
                _ => {
                    if let Some(c) = current.take() {
                        pieces.push(c);
                    }
                    *current = Some(Subpath {
                        start: Some(a),
                        segs: vec![Seg::Cubic(c1, c2, b)],
                        closed: false,
                    });
                }
            }
        };
    for s in segs {
        match s {
            Seg::Line(b) => {
                let a = cur;
                cur = b;
                if !(a.is_finite() && b.is_finite()) {
                    continue;
                }
                match inside_interval(a, b, r) {
                    None => push_line(&mut current, &mut pieces, a, b),
                    Some((t0, t1)) => {
                        if t0 > EPS {
                            push_line(&mut current, &mut pieces, a, a.lerp(b, t0));
                        }
                        if t1 < 1.0 - EPS {
                            // The piece after the region starts a new open
                            // subpath (there is a gap).
                            if let Some(c) = current.take() {
                                pieces.push(c);
                            }
                            push_line(&mut current, &mut pieces, a.lerp(b, t1), b);
                        } else if let Some(c) = current.take() {
                            pieces.push(c);
                        }
                    }
                }
            }
            Seg::Cubic(c1, c2, b) => {
                let a = cur;
                cur = b;
                if !(a.is_finite() && c1.is_finite() && c2.is_finite() && b.is_finite()) {
                    continue;
                }
                cut_cubic(a, c1, c2, b, r, 0, &mut current, &mut pieces, &push_cubic);
            }
        }
    }
    if let Some(c) = current.take() {
        pieces.push(c);
    }
    pieces
}

/// How a kept cubic piece is appended to the current run of pieces.
type PushCubic<'a> = &'a dyn Fn(&mut Option<Subpath>, &mut Vec<Subpath>, P, P, P, P);

/// Recursive cubic cutting: keep hull-outside pieces as curves, drop
/// hull-inside pieces, split the rest.
#[allow(clippy::too_many_arguments)]
fn cut_cubic(
    p0: P,
    p1: P,
    p2: P,
    p3: P,
    r: Rect,
    depth: u32,
    current: &mut Option<Subpath>,
    pieces: &mut Vec<Subpath>,
    push: PushCubic<'_>,
) {
    let h = hull(p0, p1, p2, p3);
    if !h.intersects(r) {
        push(current, pieces, p0, p1, p2, p3);
        return;
    }
    if r.contains_rect(h) {
        // Wholly inside: dropped, and the run of kept pieces is broken.
        if let Some(c) = current.take() {
            pieces.push(c);
        }
        return;
    }
    let tiny = (h.x1 - h.x0) <= FLATNESS && (h.y1 - h.y0) <= FLATNESS;
    if depth >= MAX_SPLIT_DEPTH || tiny {
        // Straddling and too small to resolve: dropped (over-cover).
        if let Some(c) = current.take() {
            pieces.push(c);
        }
        return;
    }
    let (l, rr) = split_cubic(p0, p1, p2, p3, 0.5);
    cut_cubic(l.0, l.1, l.2, l.3, r, depth + 1, current, pieces, push);
    cut_cubic(rr.0, rr.1, rr.2, rr.3, r, depth + 1, current, pieces, push);
}

// ---------------------------------------------------------------------------
// Fills: keep the parts inside each complement strip
// ---------------------------------------------------------------------------

/// One edge of the clip rectangle, as a half-plane.
#[derive(Clone, Copy)]
enum Edge {
    Left(f64),
    Right(f64),
    Bottom(f64),
    Top(f64),
}

impl Edge {
    fn inside(self, p: P) -> bool {
        match self {
            Self::Left(x) => p.x >= x,
            Self::Right(x) => p.x <= x,
            Self::Bottom(y) => p.y >= y,
            Self::Top(y) => p.y <= y,
        }
    }

    /// Where segment `a→b` meets this edge's line. Only called when `a`
    /// and `b` are on opposite sides, so the divisor is non-zero.
    fn cross(self, a: P, b: P) -> P {
        match self {
            Self::Left(x) | Self::Right(x) => {
                let t = (x - a.x) / (b.x - a.x);
                P::new(x, a.y + (b.y - a.y) * t)
            }
            Self::Bottom(y) | Self::Top(y) => {
                let t = (y - a.y) / (b.y - a.y);
                P::new(a.x + (b.x - a.x) * t, y)
            }
        }
    }
}

/// Sutherland–Hodgman clip of a closed polygon to `r`.
fn clip_polygon(poly: &[P], r: Rect) -> Vec<P> {
    let mut input: Vec<P> = poly.iter().copied().filter(|p| p.is_finite()).collect();
    for edge in [
        Edge::Left(r.x0),
        Edge::Right(r.x1),
        Edge::Bottom(r.y0),
        Edge::Top(r.y1),
    ] {
        let Some(&last) = input.last() else {
            break;
        };
        let mut output = Vec::with_capacity(input.len() + 4);
        let mut prev = last;
        for &curr in &input {
            let ci = edge.inside(curr);
            let pi = edge.inside(prev);
            if ci {
                if !pi {
                    output.push(edge.cross(prev, curr));
                }
                output.push(curr);
            } else if pi {
                output.push(edge.cross(prev, curr));
            }
            prev = curr;
        }
        input = output;
    }
    input
}

/// Consecutive pairs of a closed polyline, wrapping from last to first.
fn closed_edges(poly: &[P]) -> impl Iterator<Item = (P, P)> + '_ {
    poly.iter()
        .copied()
        .zip(poly.iter().copied().cycle().skip(1))
        .take(poly.len())
}

/// Signed area (shoelace); zero for a degenerate result.
fn area(poly: &[P]) -> f64 {
    if poly.len() < 3 {
        return 0.0;
    }
    let a: f64 = closed_edges(poly).map(|(p, q)| p.x * q.y - q.x * p.y).sum();
    a / 2.0
}

/// The subpaths of a filled path clipped to `strip`: exact where a subpath
/// lies wholly inside, flattened-and-clipped where it crosses, and absent
/// where it contributes no area.
fn fill_in_strip(subpaths: &[Subpath], strip: Rect) -> Vec<Subpath> {
    let mut out = Vec::new();
    for sp in subpaths {
        let Some(bb) = sp.bbox() else {
            continue;
        };
        if strip.contains_rect(bb) {
            out.push(Subpath {
                closed: true,
                ..sp.clone()
            });
            continue;
        }
        let poly = clip_polygon(&sp.flatten(), strip);
        if area(&poly).abs() <= EPS {
            continue;
        }
        let mut it = poly.into_iter();
        let Some(start) = it.next() else {
            continue;
        };
        out.push(Subpath {
            start: Some(start),
            segs: it.map(Seg::Line).collect(),
            closed: true,
        });
    }
    out
}

/// Winding number of `p` with respect to the closed polyline `poly`.
fn winding(poly: &[P], p: P) -> i32 {
    let mut w = 0i32;
    if poly.len() < 2 {
        return 0;
    }
    for (a, b) in closed_edges(poly) {
        if a.y <= p.y {
            if b.y > p.y && (b.x - a.x) * (p.y - a.y) - (p.x - a.x) * (b.y - a.y) > 0.0 {
                w += 1;
            }
        } else if b.y <= p.y && (b.x - a.x) * (p.y - a.y) - (p.x - a.x) * (b.y - a.y) < 0.0 {
            w -= 1;
        }
    }
    w
}

/// Does the geometry itself meet `r` — not merely its bounding box?
///
/// True when any flattened edge enters `r` (Liang–Barsky says so), or when
/// `fill` and a corner of `r` lies inside the filled area under the given
/// rule. A stroke that passes beside the region, or a ring whose hole
/// contains it, is left alone — the object is only rewritten when a cut
/// would change what is painted.
fn touches(page: &[Subpath], r: Rect, fill: bool, even_odd: bool) -> bool {
    let polys: Vec<Vec<P>> = page.iter().map(Subpath::flatten).collect();
    for (sp, poly) in page.iter().zip(&polys) {
        let n = poly.len();
        // An open stroke has no closing edge; a fill is implicitly closed.
        let edges = if sp.closed || fill {
            n
        } else {
            n.saturating_sub(1)
        };
        if closed_edges(poly)
            .take(edges)
            .any(|(a, b)| a.is_finite() && b.is_finite() && inside_interval(a, b, r).is_some())
        {
            return true;
        }
    }
    if fill {
        for corner in [
            P::new(r.x0, r.y0),
            P::new(r.x1, r.y0),
            P::new(r.x0, r.y1),
            P::new(r.x1, r.y1),
            P::new((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0),
        ] {
            let w: i32 = polys.iter().map(|p| winding(p, corner)).sum();
            let inside = if even_odd {
                polys.iter().map(|p| winding(p, corner)).sum::<i32>() % 2 != 0
            } else {
                w != 0
            };
            if inside {
                return true;
            }
        }
    }
    false
}

/// The four rectangles of `outer − hole`. Strips that would be empty are
/// omitted.
fn complement_strips(outer: Rect, hole: Rect) -> Vec<Rect> {
    let mut v = Vec::with_capacity(4);
    if hole.x0 > outer.x0 {
        v.push(Rect {
            x0: outer.x0,
            y0: outer.y0,
            x1: hole.x0,
            y1: outer.y1,
        });
    }
    if hole.x1 < outer.x1 {
        v.push(Rect {
            x0: hole.x1,
            y0: outer.y0,
            x1: outer.x1,
            y1: outer.y1,
        });
    }
    let mx0 = hole.x0.max(outer.x0);
    let mx1 = hole.x1.min(outer.x1);
    if mx0 < mx1 {
        if hole.y0 > outer.y0 {
            v.push(Rect {
                x0: mx0,
                y0: outer.y0,
                x1: mx1,
                y1: hole.y0,
            });
        }
        if hole.y1 < outer.y1 {
            v.push(Rect {
                x0: mx0,
                y0: hole.y1,
                x1: mx1,
                y1: outer.y1,
            });
        }
    }
    v
}

// ---------------------------------------------------------------------------
// The path object and its rewrite
// ---------------------------------------------------------------------------

/// What the painting operator does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Paint {
    pub stroke: bool,
    pub fill: bool,
    pub even_odd: bool,
    /// `s`/`b`/`b*`: close the current subpath before painting.
    pub close: bool,
}

impl Paint {
    /// Classify a painting operator; `None` for anything else (including
    /// `n`, which paints nothing).
    pub(crate) fn from_op(op: &[u8]) -> Option<Self> {
        Some(match op {
            b"S" => Self {
                stroke: true,
                fill: false,
                even_odd: false,
                close: false,
            },
            b"s" => Self {
                stroke: true,
                fill: false,
                even_odd: false,
                close: true,
            },
            b"f" | b"F" => Self {
                stroke: false,
                fill: true,
                even_odd: false,
                close: false,
            },
            b"f*" => Self {
                stroke: false,
                fill: true,
                even_odd: true,
                close: false,
            },
            b"B" => Self {
                stroke: true,
                fill: true,
                even_odd: false,
                close: false,
            },
            b"B*" => Self {
                stroke: true,
                fill: true,
                even_odd: true,
                close: false,
            },
            b"b" => Self {
                stroke: true,
                fill: true,
                even_odd: false,
                close: true,
            },
            b"b*" => Self {
                stroke: true,
                fill: true,
                even_odd: true,
                close: true,
            },
            _ => return None,
        })
    }
}

/// A path object as the content interpreter recorded it, in the
/// coordinate system it was authored in.
#[derive(Debug, Clone, Default)]
pub(crate) struct PathRecord {
    /// Byte offset of the first construction operand in the decoded buffer.
    pub start: Option<usize>,
    /// The CTM in force (constant across a path object — §8.2 forbids `cm`
    /// inside one).
    pub ctm: Mat,
    pub subpaths: Vec<Subpath>,
    /// `W` / `W*` seen before the paint.
    pub clip: Option<&'static [u8]>,
    /// Byte offset of that `W`/`W*` operator, so the ORIGINAL construction
    /// bytes (which end there) can be re-emitted as the clip.
    pub clip_start: Option<usize>,
    /// Something other than a construction or clip operator appeared inside
    /// the object; the bytes cannot be replaced as a unit.
    pub dirty: bool,
}

impl PathRecord {
    /// Note the object's first construction operand and the CTM in force.
    /// Idempotent: only the FIRST operator of an object sets them, which is
    /// what makes the byte span replaceable as a unit and makes the CTM the
    /// one §8.2 guarantees is constant across the object.
    pub(crate) fn begin(&mut self, start: usize, ctm: Mat) {
        if self.start.is_none() {
            self.start = Some(start);
            self.ctm = ctm;
        }
    }

    fn cur_point(&self) -> P {
        self.subpaths
            .last()
            .and_then(Subpath::end)
            .unwrap_or(P::new(0.0, 0.0))
    }

    /// `m`: start a new subpath at `(x, y)` (§8.5.2.1).
    pub(crate) fn move_to(&mut self, x: f64, y: f64) {
        self.subpaths.push(Subpath {
            start: Some(P::new(x, y)),
            segs: Vec::new(),
            closed: false,
        });
    }

    /// `l`: a straight segment to `(x, y)`.
    ///
    /// §8.5.2 makes `l` without a current point an error; it is treated as
    /// a `m` so the rest of the object still parses and can still be cut,
    /// which is safer than dropping the object's remaining geometry.
    pub(crate) fn line_to(&mut self, x: f64, y: f64) {
        if self.subpaths.last().and_then(|s| s.start).is_none() {
            self.move_to(x, y);
            return;
        }
        if let Some(sp) = self.subpaths.last_mut() {
            sp.segs.push(Seg::Line(P::new(x, y)));
        }
    }

    /// `c`: a cubic Bézier with both control points given.
    pub(crate) fn curve_to(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, x3: f64, y3: f64) {
        if self.subpaths.last().and_then(|s| s.start).is_none() {
            self.move_to(x1, y1);
        }
        if let Some(sp) = self.subpaths.last_mut() {
            sp.segs
                .push(Seg::Cubic(P::new(x1, y1), P::new(x2, y2), P::new(x3, y3)));
        }
    }

    /// `v`: first control point is the current point.
    pub(crate) fn curve_v(&mut self, x2: f64, y2: f64, x3: f64, y3: f64) {
        let c = self.cur_point();
        self.curve_to(c.x, c.y, x2, y2, x3, y3);
    }

    /// `y`: second control point is the end point.
    pub(crate) fn curve_y(&mut self, x1: f64, y1: f64, x3: f64, y3: f64) {
        self.curve_to(x1, y1, x3, y3, x3, y3);
    }

    /// `h`: close the current subpath. Recorded as a flag rather than a
    /// segment so a closed subpath re-emits as `h` and a cut one can be
    /// opened at the cut without inventing a closing line.
    pub(crate) fn close(&mut self) {
        if let Some(s) = self.subpaths.last_mut() {
            s.closed = true;
        }
    }

    /// `re`: the §8.5.2.1 expansion `x y m (x+w) y l (x+w) (y+h) l x (y+h) l h`,
    /// recorded as those four segments so every later step sees one shape
    /// of subpath.
    pub(crate) fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.move_to(x, y);
        self.line_to(x + w, y);
        self.line_to(x + w, y + h);
        self.line_to(x, y + h);
        self.close();
    }

    /// Page-space bounding box of the whole object.
    pub(crate) fn page_bbox(&self) -> Option<(f64, f64, f64, f64)> {
        let mut r: Option<Rect> = None;
        for sp in &self.subpaths {
            if let Some(b) = sp.map(self.ctm).bbox() {
                r = Some(r.map_or(b, |x| x.union(b)));
            }
        }
        r.map(|b| (b.x0, b.y0, b.x1, b.y1))
    }
}

/// The outcome of cutting one path object.
#[derive(Debug, Default)]
pub(crate) struct Cut {
    /// Replacement bytes for the object (construction through paint).
    pub bytes: Vec<u8>,
    /// The object was wholly inside a region: nothing of it is painted.
    pub dropped_whole: bool,
    /// A clip-marked object: the original construction was kept as a clip.
    pub clip_kept: bool,
}

/// `bytes` without trailing PDF whitespace (§7.2.2), so a re-emitted
/// construction joins its operator with exactly one space.
fn trim_ws(mut bytes: &[u8]) -> &[u8] {
    while let Some((&last, rest)) = bytes.split_last() {
        if matches!(last, b' ' | b'\n' | b'\r' | b'\t' | b'\x0C' | b'\0') {
            bytes = rest;
        } else {
            break;
        }
    }
    bytes
}

/// The larger scale factor of `m` (how much a user-space length can grow
/// under it), for expanding the region by a stroke width.
fn scale_of(m: Mat) -> f64 {
    (m.a * m.a + m.b * m.b)
        .sqrt()
        .max((m.c * m.c + m.d * m.d).sqrt())
}

fn invert(m: Mat) -> Option<Mat> {
    let det = m.a * m.d - m.b * m.c;
    if det.abs() < 1e-12 || !det.is_finite() {
        return None;
    }
    Some(Mat {
        a: m.d / det,
        b: -m.b / det,
        c: -m.c / det,
        d: m.a / det,
        e: (m.c * m.f - m.d * m.e) / det,
        f: (m.b * m.e - m.a * m.f) / det,
    })
}

/// Emit subpaths (already back in user coordinates) as construction ops.
fn emit_subpaths(out: &mut Vec<u8>, subpaths: &[Subpath]) {
    for sp in subpaths {
        let Some(start) = sp.start else {
            continue;
        };
        emit_number(out, start.x);
        out.push(b' ');
        emit_number(out, start.y);
        out.extend_from_slice(b" m\n");
        for s in &sp.segs {
            match s {
                Seg::Line(p) => {
                    emit_number(out, p.x);
                    out.push(b' ');
                    emit_number(out, p.y);
                    out.extend_from_slice(b" l\n");
                }
                Seg::Cubic(a, b, c) => {
                    for (i, p) in [a, b, c].into_iter().enumerate() {
                        if i > 0 {
                            out.push(b' ');
                        }
                        emit_number(out, p.x);
                        out.push(b' ');
                        emit_number(out, p.y);
                    }
                    out.extend_from_slice(b" c\n");
                }
            }
        }
        if sp.closed {
            out.extend_from_slice(b"h\n");
        }
    }
}

/// Rewrite `record` (painted by `paint`, with user-space line width
/// `line_width`) so nothing it paints lies inside any of `regions`.
///
/// Returns `None` when the object touches no region (leave the bytes
/// alone). `original` is the object's original bytes, needed only for the
/// clip-marked case.
pub(crate) fn cut_path(
    record: &PathRecord,
    paint: Paint,
    line_width: f64,
    regions: &[RegionBox],
    original: &[u8],
) -> Option<Cut> {
    let (bx0, by0, bx1, by1) = record.page_bbox()?;
    let original = trim_ws(original);
    let bbox = Rect {
        x0: bx0,
        y0: by0,
        x1: bx1,
        y1: by1,
    };
    // Stroke expansion: one full width in page units (never less than one).
    let expand = if paint.stroke {
        (line_width.max(1.0) * scale_of(record.ctm)).max(1.0)
    } else {
        0.0
    };
    let mut hit: Vec<Rect> = regions
        .iter()
        .map(|r| Rect::from_region(*r).expand(expand))
        .filter(|r| r.intersects(bbox))
        .collect();
    if hit.is_empty() {
        return None;
    }
    // Work in page space.
    let mut page: Vec<Subpath> = record.subpaths.iter().map(|s| s.map(record.ctm)).collect();
    if paint.close {
        for sp in &mut page {
            sp.closed = true;
        }
    }
    // The box met a region; does the geometry? A stroke passing beside
    // the mark, or a fill whose hole surrounds it, is not touched.
    hit.retain(|r| touches(&page, *r, paint.fill, paint.even_odd));
    if hit.is_empty() {
        return None;
    }
    let mut cut = Cut::default();
    let Some(inv) = invert(record.ctm) else {
        // Zero-area placement touching a region: drop it whole.
        cut.dropped_whole = true;
        if let Some(clip) = record.clip {
            cut.bytes.extend_from_slice(original);
            cut.bytes.extend_from_slice(b" ");
            cut.bytes.extend_from_slice(clip);
            cut.bytes.extend_from_slice(b" n\n");
            cut.clip_kept = true;
        }
        return Some(cut);
    };
    let whole_inside = hit.iter().any(|r| r.contains_rect(bbox));
    let mut out = Vec::new();
    if !whole_inside {
        // Regions are applied one after another; the survivors of one cut
        // are the input of the next.
        hit.sort_by(|a, b| a.x0.total_cmp(&b.x0));
        if paint.fill {
            let mut fills: Vec<Vec<Subpath>> = vec![page.clone()];
            for r in &hit {
                let mut next = Vec::new();
                for group in &fills {
                    let gb = group.iter().filter_map(Subpath::bbox).reduce(Rect::union);
                    let Some(gb) = gb else {
                        continue;
                    };
                    if !gb.intersects(*r) {
                        next.push(group.clone());
                        continue;
                    }
                    for strip in complement_strips(gb.expand(EPS), *r) {
                        let clipped = fill_in_strip(group, strip);
                        if !clipped.is_empty() {
                            next.push(clipped);
                        }
                    }
                }
                fills = next;
            }
            let op: &[u8] = if paint.even_odd { b"f*" } else { b"f" };
            for group in &fills {
                let user: Vec<Subpath> = group.iter().map(|s| s.map(inv)).collect();
                emit_subpaths(&mut out, &user);
                out.extend_from_slice(op);
                out.push(b'\n');
            }
        }
        if paint.stroke {
            let mut pieces: Vec<Subpath> = page.clone();
            for r in &hit {
                pieces = pieces.iter().flat_map(|sp| cut_stroke(sp, *r)).collect();
            }
            if !pieces.is_empty() {
                let user: Vec<Subpath> = pieces.iter().map(|s| s.map(inv)).collect();
                emit_subpaths(&mut out, &user);
                out.extend_from_slice(b"S\n");
            }
        }
    }
    if out.is_empty() {
        cut.dropped_whole = true;
    }
    cut.bytes = out;
    if let Some(clip) = record.clip {
        // Paint first (as cut), then the ORIGINAL geometry as the clip, so
        // the rest of the page keeps the window it was authored under.
        cut.bytes.extend_from_slice(original);
        cut.bytes.extend_from_slice(b" ");
        cut.bytes.extend_from_slice(clip);
        cut.bytes.extend_from_slice(b" n\n");
        cut.clip_kept = true;
    }
    Some(cut)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp)]
mod tests {
    use super::*;

    fn region(x0: f64, y0: f64, x1: f64, y1: f64) -> RegionBox {
        RegionBox {
            min_x: x0,
            min_y: y0,
            max_x: x1,
            max_y: y1,
        }
    }

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Rect {
        Rect { x0, y0, x1, y1 }
    }

    #[test]
    fn a_line_through_the_region_becomes_two_pieces() {
        let sp = Subpath {
            start: Some(P::new(0.0, 50.0)),
            segs: vec![Seg::Line(P::new(200.0, 50.0))],
            closed: false,
        };
        let pieces = cut_stroke(&sp, rect(80.0, 0.0, 120.0, 100.0));
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0].start, Some(P::new(0.0, 50.0)));
        assert_eq!(pieces[0].segs, vec![Seg::Line(P::new(80.0, 50.0))]);
        assert_eq!(pieces[1].start, Some(P::new(120.0, 50.0)));
        assert_eq!(pieces[1].segs, vec![Seg::Line(P::new(200.0, 50.0))]);
    }

    #[test]
    fn a_line_inside_the_region_vanishes_and_one_outside_is_kept_whole() {
        let inside = Subpath {
            start: Some(P::new(90.0, 50.0)),
            segs: vec![Seg::Line(P::new(110.0, 50.0))],
            closed: false,
        };
        assert!(cut_stroke(&inside, rect(80.0, 0.0, 120.0, 100.0)).is_empty());
        let outside = Subpath {
            start: Some(P::new(0.0, 50.0)),
            segs: vec![Seg::Line(P::new(70.0, 50.0))],
            closed: false,
        };
        let kept = cut_stroke(&outside, rect(80.0, 0.0, 120.0, 100.0));
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0], outside);
    }

    #[test]
    fn a_closed_square_cut_at_one_corner_opens_into_a_polyline_outside() {
        let sq = Subpath {
            start: Some(P::new(0.0, 0.0)),
            segs: vec![
                Seg::Line(P::new(100.0, 0.0)),
                Seg::Line(P::new(100.0, 100.0)),
                Seg::Line(P::new(0.0, 100.0)),
            ],
            closed: true,
        };
        // Region over the top-right corner.
        let pieces = cut_stroke(&sq, rect(80.0, 80.0, 120.0, 120.0));
        // One run: (0,0)→(100,0)→(100,80) then a gap, then (80,100)→(0,100)→(0,0).
        assert_eq!(pieces.len(), 2, "{pieces:?}");
        for piece in &pieces {
            assert!(!piece.closed);
            let pts = piece.flatten();
            for p in pts {
                assert!(!(p.x > 80.0 + EPS && p.y > 80.0 + EPS), "{p:?} is inside");
            }
        }
    }

    #[test]
    fn a_cubic_straddling_the_region_is_split_and_the_inside_dropped() {
        // A wide arc from (0,0) to (200,0) bulging up to y≈75.
        let sp = Subpath {
            start: Some(P::new(0.0, 0.0)),
            segs: vec![Seg::Cubic(
                P::new(50.0, 100.0),
                P::new(150.0, 100.0),
                P::new(200.0, 0.0),
            )],
            closed: false,
        };
        let r = rect(90.0, 0.0, 110.0, 200.0);
        let pieces = cut_stroke(&sp, r);
        assert_eq!(pieces.len(), 2, "{pieces:?}");
        // Every kept piece's hull is outside the region.
        for piece in &pieces {
            let b = piece.bbox().unwrap();
            assert!(
                !b.intersects(rect(90.0 + 1.0, 0.0, 110.0 - 1.0, 200.0)),
                "{b:?}"
            );
            assert!(
                piece.segs.iter().all(|s| matches!(s, Seg::Cubic(..))),
                "curves stay curves"
            );
        }
    }

    #[test]
    fn clip_polygon_keeps_the_winding_of_a_surrounding_polygon() {
        // A big square surrounding the clip rect → the clip rect itself.
        let poly = [
            P::new(0.0, 0.0),
            P::new(100.0, 0.0),
            P::new(100.0, 100.0),
            P::new(0.0, 100.0),
        ];
        let out = clip_polygon(&poly, rect(20.0, 20.0, 40.0, 40.0));
        assert!((area(&out).abs() - 400.0).abs() < 1e-9, "{out:?}");
        // Same orientation as the input.
        assert!(area(&out) > 0.0);
        // A polygon entirely outside → no area.
        let far = [
            P::new(200.0, 200.0),
            P::new(300.0, 200.0),
            P::new(300.0, 300.0),
        ];
        assert!(area(&clip_polygon(&far, rect(0.0, 0.0, 100.0, 100.0))).abs() < 1e-9);
    }

    #[test]
    fn complement_strips_partition_the_outer_box() {
        let outer = rect(0.0, 0.0, 100.0, 100.0);
        let hole = rect(40.0, 40.0, 60.0, 60.0);
        let strips = complement_strips(outer, hole);
        assert_eq!(strips.len(), 4);
        let total: f64 = strips.iter().map(|s| (s.x1 - s.x0) * (s.y1 - s.y0)).sum();
        assert!((total - (10_000.0 - 400.0)).abs() < 1e-9);
        // A hole that reaches an edge yields fewer strips.
        assert_eq!(
            complement_strips(outer, rect(0.0, 40.0, 60.0, 60.0)).len(),
            3
        );
        // A hole covering the whole box yields none.
        assert!(complement_strips(outer, rect(-1.0, -1.0, 101.0, 101.0)).is_empty());
    }

    #[test]
    fn cut_path_fills_around_the_region_and_strokes_with_expansion() {
        let mut rec = PathRecord::default();
        rec.begin(0, Mat::IDENTITY);
        rec.rect(0.0, 0.0, 100.0, 100.0);
        // A fill: four strips around a central hole.
        let cut = cut_path(
            &rec,
            Paint::from_op(b"f").unwrap(),
            1.0,
            &[region(40.0, 40.0, 60.0, 60.0)],
            b"",
        )
        .unwrap();
        let text = String::from_utf8(cut.bytes.clone()).unwrap();
        assert_eq!(
            text.matches(" f\n").count() + text.matches("\nf\n").count(),
            4,
            "{text}"
        );
        assert!(!cut.dropped_whole);
        assert!(!text.contains("S\n"));
        // A stroke of the same square: the region is expanded by the width.
        let cut = cut_path(
            &rec,
            Paint::from_op(b"S").unwrap(),
            4.0,
            &[region(90.0, 90.0, 120.0, 120.0)],
            b"",
        )
        .unwrap();
        let text = String::from_utf8(cut.bytes.clone()).unwrap();
        assert!(text.ends_with("S\n"), "{text}");
        // The cut lands at 90 − 4 = 86 on both edges.
        assert!(text.contains("100 86 l"), "{text}");
        assert!(text.contains("86 100 m"), "{text}");
    }

    #[test]
    fn a_path_wholly_inside_the_region_is_dropped_and_a_clip_is_kept() {
        let mut rec = PathRecord::default();
        rec.begin(0, Mat::IDENTITY);
        rec.rect(10.0, 10.0, 5.0, 5.0);
        let cut = cut_path(
            &rec,
            Paint::from_op(b"f").unwrap(),
            1.0,
            &[region(0.0, 0.0, 100.0, 100.0)],
            b"",
        )
        .unwrap();
        assert!(cut.dropped_whole);
        assert!(cut.bytes.is_empty());

        rec.clip = Some(b"W");
        let cut = cut_path(
            &rec,
            Paint::from_op(b"n").unwrap_or(Paint {
                stroke: false,
                fill: false,
                even_odd: false,
                close: false,
            }),
            1.0,
            &[region(0.0, 0.0, 100.0, 100.0)],
            b"10 10 5 5 re",
        )
        .unwrap();
        assert!(cut.clip_kept);
        assert_eq!(cut.bytes, b"10 10 5 5 re W n\n");
    }

    #[test]
    fn a_stroke_beside_the_region_and_a_ring_around_it_are_left_alone() {
        // The line's box spans the region but the line passes below it.
        let mut rec = PathRecord::default();
        rec.begin(0, Mat::IDENTITY);
        rec.move_to(0.0, 0.0);
        rec.line_to(300.0, 100.0);
        assert!(
            cut_path(
                &rec,
                Paint::from_op(b"S").unwrap(),
                1.0,
                &[region(60.0, 80.0, 120.0, 140.0)],
                b""
            )
            .is_none()
        );
        // A ring (even-odd) whose hole contains the region: no fill there.
        let mut ring = PathRecord::default();
        ring.begin(0, Mat::IDENTITY);
        ring.rect(0.0, 0.0, 200.0, 200.0);
        ring.rect(50.0, 50.0, 100.0, 100.0);
        assert!(
            cut_path(
                &ring,
                Paint::from_op(b"f*").unwrap(),
                1.0,
                &[region(80.0, 80.0, 120.0, 120.0)],
                b""
            )
            .is_none()
        );
        // The same ring under nonzero is SOLID (same orientation), so it
        // is cut.
        assert!(
            cut_path(
                &ring,
                Paint::from_op(b"f").unwrap(),
                1.0,
                &[region(80.0, 80.0, 120.0, 120.0)],
                b""
            )
            .is_some()
        );
    }

    #[test]
    fn a_path_touching_no_region_is_left_alone() {
        let mut rec = PathRecord::default();
        rec.begin(0, Mat::IDENTITY);
        rec.rect(0.0, 0.0, 10.0, 10.0);
        assert!(
            cut_path(
                &rec,
                Paint::from_op(b"f").unwrap(),
                1.0,
                &[region(50.0, 50.0, 60.0, 60.0)],
                b""
            )
            .is_none()
        );
    }

    #[test]
    fn the_rewrite_is_emitted_in_the_authored_coordinates() {
        // CTM scales by 2 and translates by (100, 0): user (0..50) → page
        // (100..200). A region at page x 150..160 must cut at user x 25..30.
        let mut rec = PathRecord::default();
        rec.begin(
            0,
            Mat {
                a: 2.0,
                b: 0.0,
                c: 0.0,
                d: 2.0,
                e: 100.0,
                f: 0.0,
            },
        );
        rec.move_to(0.0, 10.0);
        rec.line_to(50.0, 10.0);
        let cut = cut_path(
            &rec,
            Paint::from_op(b"S").unwrap(),
            0.0,
            &[region(150.0, 0.0, 160.0, 100.0)],
            b"",
        )
        .unwrap();
        let text = String::from_utf8(cut.bytes).unwrap();
        // Width 0 counts as one user unit; under the ×2 CTM that is two page
        // units of expansion → the cuts land at user 24 and 31.
        assert!(text.contains("0 10 m\n24 10 l\n"), "{text}");
        assert!(text.contains("31 10 m\n50 10 l\n"), "{text}");
    }
}
