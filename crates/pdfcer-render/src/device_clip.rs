//! Keeps every path handed to tiny-skia inside its fixed-point range.
//!
//! tiny-skia 0.11 rasterises in fixed point. Measured on a 1100 px pixmap
//! (`examples/deep_zoom_pixels.rs`): an anti-aliased fill whose device bounds
//! pass about ±2^27 px panics, and a non-anti-aliased one past about ±2^30
//! paints wrong pixels with no error. A deep region render puts page-wide
//! paths there. A path whose device bounds exceed [`GUARD`] is therefore
//! mapped to device space in `f64` and clipped, contour by contour, to the
//! target plus a margin before it is rasterised.
//!
//! Clipping each closed contour separately to a convex rectangle preserves
//! the winding number of every point inside the rectangle, so non-zero and
//! even-odd fills paint the same pixels as the unclipped path would have.

use std::borrow::Cow;

use tiny_skia::{
    FillRule, Mask, Paint, Path, PathBuilder, PathSegment, PathStroker, Pixmap, PixmapMut, Point,
    Stroke, Transform,
};

/// Device-space extent, in pixels, beyond which a path is pre-clipped.
/// Far below the ±2^27 failure point and far above any pixmap edge
/// ([`crate::MAX_PIXMAP_EDGE`]), so ordinary renders never take the clip.
pub(crate) const GUARD: f64 = 4_194_304.0;

/// Pixels kept outside the target on every side, so anti-aliased edges and
/// the clip boundary itself never land on a visible pixel.
const MARGIN: f64 = 4.0;

/// Maximum distance, in device pixels, between a curve and its flattening.
const FLAT: f64 = 0.05;

/// Subdivision depth cap for one curve. A curve spanning 2^31 px reaches
/// [`FLAT`] in about 18 halvings.
const MAX_DEPTH: u32 = 32;

type P = (f64, f64);

/// What to hand tiny-skia for a fill.
pub(crate) enum Fit<'a> {
    /// In range: draw `path` under the transform unchanged.
    AsIs(&'a Path, Transform),
    /// Pre-clipped, already in device space: draw under the identity, with
    /// the paint's shader moved by the original transform.
    Device(Path),
    /// Nothing of the path reaches the target.
    Nothing,
}

fn map(ts: Transform, x: f32, y: f32) -> P {
    let (x, y) = (f64::from(x), f64::from(y));
    (
        f64::from(ts.sx) * x + f64::from(ts.kx) * y + f64::from(ts.tx),
        f64::from(ts.ky) * x + f64::from(ts.sy) * y + f64::from(ts.ty),
    )
}

/// Whether `path`'s bounds, grown by `pad` user units, leave ±[`GUARD`]
/// under `ts`. NaN counts as out of range.
fn exceeds(path: &Path, ts: Transform, pad: f32) -> bool {
    let b = path.bounds();
    let (l, t, r, btm) = (
        b.left() - pad,
        b.top() - pad,
        b.right() + pad,
        b.bottom() + pad,
    );
    [(l, t), (r, t), (l, btm), (r, btm)].iter().any(|&(x, y)| {
        let (dx, dy) = map(ts, x, y);
        !(dx.abs() <= GUARD && dy.abs() <= GUARD)
    })
}

#[derive(Clone, Copy)]
struct Window {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl Window {
    fn around(width: u32, height: u32) -> Self {
        Self {
            x0: -MARGIN,
            y0: -MARGIN,
            x1: f64::from(width) + MARGIN,
            y1: f64::from(height) + MARGIN,
        }
    }

    fn misses(&self, pts: &[P]) -> bool {
        pts.iter().all(|p| p.0 < self.x0)
            || pts.iter().all(|p| p.0 > self.x1)
            || pts.iter().all(|p| p.1 < self.y0)
            || pts.iter().all(|p| p.1 > self.y1)
    }
}

/// Decides how a fill of `path` under `ts` reaches a `width` x `height`
/// target.
pub(crate) fn fit(path: &Path, ts: Transform, width: u32, height: u32) -> Fit<'_> {
    if !exceeds(path, ts, 0.0) {
        return Fit::AsIs(path, ts);
    }
    let win = Window::around(width, height);
    let mut pb = PathBuilder::new();
    let mut contour: Vec<P> = Vec::new();
    let mut last: P = (0.0, 0.0);
    for seg in path.segments() {
        match seg {
            PathSegment::MoveTo(p) => {
                emit(&mut contour, win, &mut pb);
                last = map(ts, p.x, p.y);
                contour.push(last);
            }
            PathSegment::LineTo(p) => {
                last = map(ts, p.x, p.y);
                contour.push(last);
            }
            PathSegment::QuadTo(c, p) => {
                let (c, p) = (map(ts, c.x, c.y), map(ts, p.x, p.y));
                let c1 = (
                    last.0 + 2.0 / 3.0 * (c.0 - last.0),
                    last.1 + 2.0 / 3.0 * (c.1 - last.1),
                );
                let c2 = (p.0 + 2.0 / 3.0 * (c.0 - p.0), p.1 + 2.0 / 3.0 * (c.1 - p.1));
                flatten([last, c1, c2, p], win, 0, &mut contour);
                last = p;
            }
            PathSegment::CubicTo(c1, c2, p) => {
                let pts = [
                    last,
                    map(ts, c1.x, c1.y),
                    map(ts, c2.x, c2.y),
                    map(ts, p.x, p.y),
                ];
                flatten(pts, win, 0, &mut contour);
                last = pts[3];
            }
            PathSegment::Close => {
                let start = contour.first().copied();
                emit(&mut contour, win, &mut pb);
                if let Some(s) = start {
                    last = s;
                    contour.push(s);
                }
            }
        }
    }
    emit(&mut contour, win, &mut pb);
    pb.finish().map_or(Fit::Nothing, Fit::Device)
}

/// Appends a flattening of cubic `c` to `out`, excluding its start point.
/// A piece whose control hull misses the window becomes its chord: the
/// region between them lies inside the hull, so outside the window, and no
/// winding number inside the window changes.
fn flatten(c: [P; 4], win: Window, depth: u32, out: &mut Vec<P>) {
    if depth >= MAX_DEPTH || win.misses(&c) || flat(c) {
        out.push(c[3]);
        return;
    }
    let mid = |a: P, b: P| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    let (ab, bc, cd) = (mid(c[0], c[1]), mid(c[1], c[2]), mid(c[2], c[3]));
    let (abc, bcd) = (mid(ab, bc), mid(bc, cd));
    let m = mid(abc, bcd);
    flatten([c[0], ab, abc, m], win, depth + 1, out);
    flatten([m, bcd, cd, c[3]], win, depth + 1, out);
}

fn flat(c: [P; 4]) -> bool {
    let (dx, dy) = (c[3].0 - c[0].0, c[3].1 - c[0].1);
    let len = dx.hypot(dy);
    let dist = |p: P| {
        if len < 1e-9 {
            (p.0 - c[0].0).hypot(p.1 - c[0].1)
        } else {
            ((p.0 - c[0].0) * dy - (p.1 - c[0].1) * dx).abs() / len
        }
    };
    dist(c[1]) <= FLAT && dist(c[2]) <= FLAT
}

/// Clips the implicitly closed polygon `contour` to `win` and appends the
/// result to `pb` as one closed contour. Empties `contour`.
fn emit(contour: &mut Vec<P>, win: Window, pb: &mut PathBuilder) {
    let mut poly = std::mem::take(contour);
    for edge in 0..4 {
        if poly.len() < 3 {
            return;
        }
        poly = clip_edge(&poly, win, edge);
    }
    let mut pts = poly.iter().map(|&(x, y)| {
        #[allow(clippy::cast_possible_truncation)] // within the window, so well inside f32
        Point::from_xy(x as f32, y as f32)
    });
    let Some(first) = pts.next() else { return };
    if poly.len() < 3 {
        return;
    }
    pb.move_to(first.x, first.y);
    for p in pts {
        pb.line_to(p.x, p.y);
    }
    pb.close();
}

/// One Sutherland–Hodgman pass: keeps the part of `poly` on the inner side
/// of window edge `edge` (0 left, 1 right, 2 top, 3 bottom).
fn clip_edge(poly: &[P], win: Window, edge: u8) -> Vec<P> {
    let inside = |p: P| match edge {
        0 => p.0 >= win.x0,
        1 => p.0 <= win.x1,
        2 => p.1 >= win.y0,
        _ => p.1 <= win.y1,
    };
    let cross = |a: P, b: P| {
        let (k, horizontal) = match edge {
            0 => (win.x0, false),
            1 => (win.x1, false),
            2 => (win.y0, true),
            _ => (win.y1, true),
        };
        if horizontal {
            let t = (k - a.1) / (b.1 - a.1);
            (a.0 + t * (b.0 - a.0), k)
        } else {
            let t = (k - a.0) / (b.0 - a.0);
            (k, a.1 + t * (b.1 - a.1))
        }
    };
    let mut out = Vec::with_capacity(poly.len() + 4);
    let Some(&tail) = poly.last() else {
        return out;
    };
    let mut prev = tail;
    for &cur in poly {
        match (inside(prev), inside(cur)) {
            (true, true) => out.push(cur),
            (true, false) => out.push(cross(prev, cur)),
            (false, true) => {
                out.push(cross(prev, cur));
                out.push(cur);
            }
            (false, false) => {}
        }
        prev = cur;
    }
    out
}

/// For an out-of-range stroke, its outline in user space, to be filled
/// (non-zero) through [`fit`]. `None` when the stroke is in range and should
/// go to tiny-skia unchanged; `Some(None)` when there is nothing to draw.
fn stroke_outline(path: &Path, stroke: &Stroke, ts: Transform) -> Option<Option<Path>> {
    let pad = stroke.width * stroke.miter_limit.max(1.0) + 1.0;
    if !exceeds(path, ts, pad) {
        return None;
    }
    let res = PathStroker::compute_resolution_scale(&ts);
    let dashed = match &stroke.dash {
        Some(d) => match path.dash(d, res) {
            Some(p) => Cow::Owned(p),
            None => return Some(None),
        },
        None => Cow::Borrowed(path),
    };
    let mut solid = stroke.clone();
    solid.dash = None;
    if solid.width <= 0.0 {
        // A zero-width stroke is a one-pixel hairline in device space.
        solid.width = 1.0 / res;
    }
    Some(dashed.stroke(&solid, res))
}

/// `fill_path` / `stroke_path` that pre-clip out-of-range geometry.
pub(crate) trait FitPaint {
    /// [`PixmapMut::fill_path`], through [`fit`].
    fn fill_path_fit(
        &mut self,
        path: &Path,
        paint: &Paint,
        rule: FillRule,
        ts: Transform,
        mask: Option<&Mask>,
    );
    /// [`PixmapMut::stroke_path`]; an out-of-range stroke is outlined and
    /// filled through [`fit`].
    fn stroke_path_fit(
        &mut self,
        path: &Path,
        paint: &Paint,
        stroke: &Stroke,
        ts: Transform,
        mask: Option<&Mask>,
    );
}

impl FitPaint for PixmapMut<'_> {
    fn fill_path_fit(
        &mut self,
        path: &Path,
        paint: &Paint,
        rule: FillRule,
        ts: Transform,
        mask: Option<&Mask>,
    ) {
        match fit(path, ts, self.width(), self.height()) {
            Fit::AsIs(p, t) => self.fill_path(p, paint, rule, t, mask),
            Fit::Device(p) => {
                let mut paint = paint.clone();
                paint.shader.transform(ts);
                self.fill_path(&p, &paint, rule, Transform::identity(), mask);
            }
            Fit::Nothing => {}
        }
    }

    fn stroke_path_fit(
        &mut self,
        path: &Path,
        paint: &Paint,
        stroke: &Stroke,
        ts: Transform,
        mask: Option<&Mask>,
    ) {
        match stroke_outline(path, stroke, ts) {
            None => self.stroke_path(path, paint, stroke, ts, mask),
            Some(Some(outline)) => self.fill_path_fit(&outline, paint, FillRule::Winding, ts, mask),
            Some(None) => {}
        }
    }
}

impl FitPaint for Pixmap {
    fn fill_path_fit(
        &mut self,
        path: &Path,
        paint: &Paint,
        rule: FillRule,
        ts: Transform,
        mask: Option<&Mask>,
    ) {
        self.as_mut().fill_path_fit(path, paint, rule, ts, mask);
    }

    fn stroke_path_fit(
        &mut self,
        path: &Path,
        paint: &Paint,
        stroke: &Stroke,
        ts: Transform,
        mask: Option<&Mask>,
    ) {
        self.as_mut().stroke_path_fit(path, paint, stroke, ts, mask);
    }
}

/// [`Mask::fill_path`] through [`fit`].
pub(crate) trait FitMask {
    /// See [`FitMask`].
    fn fill_path_fit(&mut self, path: &Path, rule: FillRule, anti_alias: bool, ts: Transform);
}

impl FitMask for Mask {
    fn fill_path_fit(&mut self, path: &Path, rule: FillRule, anti_alias: bool, ts: Transform) {
        match fit(path, ts, self.width(), self.height()) {
            Fit::AsIs(p, t) => self.fill_path(p, rule, anti_alias, t),
            Fit::Device(p) => self.fill_path(&p, rule, anti_alias, Transform::identity()),
            Fit::Nothing => {}
        }
    }
}

#[cfg(test)]
// Test code: a failed expectation should panic with its message.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    fn square(l: f32, t: f32, r: f32, b: f32) -> Path {
        PathBuilder::from_rect(tiny_skia::Rect::from_ltrb(l, t, r, b).expect("rect"))
    }

    #[test]
    fn in_range_paths_are_untouched() {
        let p = square(0.0, 0.0, 10.0, 10.0);
        assert!(matches!(
            fit(&p, Transform::from_scale(100.0, 100.0), 64, 64),
            Fit::AsIs(..)
        ));
    }

    #[test]
    fn a_huge_fill_is_clipped_to_the_window() {
        let p = square(-1.0e9, -1.0e9, 50.0, 1.0e9);
        let Fit::Device(c) = fit(&p, Transform::identity(), 100, 100) else {
            panic!("expected a device-space clip");
        };
        let b = c.bounds();
        assert_eq!(
            (b.left(), b.top(), b.right(), b.bottom()),
            (-4.0, -4.0, 50.0, 104.0)
        );
    }

    #[test]
    fn a_huge_path_wholly_outside_draws_nothing() {
        let p = square(1.0e9, 0.0, 2.0e9, 10.0);
        assert!(matches!(
            fit(&p, Transform::identity(), 100, 100),
            Fit::Nothing
        ));
    }

    #[test]
    fn a_hole_survives_clipping_under_even_odd() {
        // Outer square far beyond range, inner hole inside the window.
        let mut pb = PathBuilder::new();
        pb.push_rect(tiny_skia::Rect::from_ltrb(-1.0e9, -1.0e9, 1.0e9, 1.0e9).expect("r"));
        pb.push_rect(tiny_skia::Rect::from_ltrb(20.0, 20.0, 80.0, 80.0).expect("r"));
        let p = pb.finish().expect("path");
        let mut m = Mask::new(100, 100).expect("mask");
        m.fill_path_fit(&p, FillRule::EvenOdd, true, Transform::identity());
        let at = |x: u32, y: u32| m.data()[(y * 100 + x) as usize];
        assert_eq!((at(5, 5), at(50, 50), at(95, 95)), (255, 0, 255));
    }

    #[test]
    fn a_huge_circle_edge_lands_where_it_should() {
        // Radius 2^30 px, far past tiny-skia's range, with its rightmost
        // point exactly on x = 0 before a translation of (50, 50).
        let r = 1_073_741_824.0_f32;
        let p = PathBuilder::from_circle(-r, 0.0, r).expect("circle");
        let ts = Transform::from_translate(50.0, 50.0);
        let mut m = Mask::new(100, 100).expect("mask");
        m.fill_path_fit(&p, FillRule::Winding, true, ts);
        let at = |x: u32| m.data()[(50 * 100 + x) as usize];
        assert_eq!((at(45), at(55)), (255, 0));
    }

    #[test]
    fn a_huge_stroke_is_outlined_and_clipped() {
        let mut pb = PathBuilder::new();
        pb.move_to(-1.0e10, 50.0);
        pb.line_to(1.0e10, 50.0);
        let p = pb.finish().expect("path");
        let stroke = Stroke {
            width: 10.0,
            ..Stroke::default()
        };
        let mut pm = Pixmap::new(100, 100).expect("pixmap");
        let mut paint = Paint::default();
        paint.set_color_rgba8(0, 0, 0, 255);
        pm.stroke_path_fit(&p, &paint, &stroke, Transform::identity(), None);
        let a = |y: u32| pm.pixel(50, y).map_or(0, |c| c.alpha());
        assert_eq!((a(40), a(50), a(60)), (0, 255, 0));
    }
}
