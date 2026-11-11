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
pub(crate) enum Fit {
    /// In the target's device space: draw under the identity, with the
    /// paint's shader moved by the original transform.
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
    /// The target plus [`MARGIN`], in the unbanded space `origin` rows up.
    fn around(width: u32, height: u32, origin: f64) -> Self {
        Self {
            x0: -MARGIN,
            y0: origin - MARGIN,
            x1: f64::from(width) + MARGIN,
            y1: origin + f64::from(height) + MARGIN,
        }
    }

    fn misses(&self, pts: &[P]) -> bool {
        pts.iter().all(|p| p.0 < self.x0)
            || pts.iter().all(|p| p.0 > self.x1)
            || pts.iter().all(|p| p.1 < self.y0)
            || pts.iter().all(|p| p.1 > self.y1)
    }
}

/// Device rows at which every in-range edge is cut, in the target's
/// *unbanded* device space.
///
/// tiny-skia derives a line's fixed-point slope from the endpoints it is
/// handed and chops a line that leaves the target, so a line crossing a
/// row-band edge would rasterise differently in the band than in the whole
/// page. Cutting every edge at absolute multiples of this many rows, before
/// tiny-skia sees it, gives whole and banded replays the same edges up to
/// an integer row shift. [`crate::DisplayList::replay_rows`] bands must
/// start on a multiple of it. Decision 223.
pub(crate) const ROW_LATTICE: u32 = 64;

thread_local! {
    /// Rows between the unbanded device space `fit` cuts in and the target
    /// it draws into. Non-zero only inside [`with_row_origin`].
    static ROW_ORIGIN: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Run `f` with every fit on this thread drawing rows `origin..` of the
/// unbanded device space into its target's row 0. Restored on unwind.
pub(crate) fn with_row_origin<R>(origin: u32, f: impl FnOnce() -> R) -> R {
    struct Restore(u32);
    impl Drop for Restore {
        fn drop(&mut self) {
            ROW_ORIGIN.with(|o| o.set(self.0));
        }
    }
    let _restore = Restore(ROW_ORIGIN.with(|o| o.replace(origin)));
    f()
}

fn row_origin() -> u32 {
    ROW_ORIGIN.with(std::cell::Cell::get)
}

/// Decides how a fill of `path` under `ts` reaches a `width` x `height`
/// target. `ts` maps to the unbanded device space; the result is in the
/// target's, [`with_row_origin`] rows up.
pub(crate) fn fit(path: &Path, ts: Transform, width: u32, height: u32) -> Fit {
    let origin = f64::from(row_origin());
    if !exceeds(path, ts, 0.0) {
        return in_range(path, ts).map_or(Fit::Nothing, Fit::Device);
    }
    let win = Window::around(width, height, origin);
    let mut pb = PathBuilder::new();
    let mut contour: Vec<P> = Vec::new();
    let mut last: P = (0.0, 0.0);
    for seg in path.segments() {
        match seg {
            PathSegment::MoveTo(p) => {
                emit(&mut contour, win, origin, &mut pb);
                last = map(ts, p.x, p.y);
                contour.push(last);
            }
            PathSegment::LineTo(p) => {
                last = map(ts, p.x, p.y);
                contour.push(last);
            }
            PathSegment::QuadTo(c, p) => {
                let (c, p) = (map(ts, c.x, c.y), map(ts, p.x, p.y));
                flatten(quad_to_cubic(last, c, p), win, 0, &mut contour);
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
                emit(&mut contour, win, origin, &mut pb);
                if let Some(s) = start {
                    last = s;
                    contour.push(s);
                }
            }
        }
    }
    emit(&mut contour, win, origin, &mut pb);
    pb.finish().map_or(Fit::Nothing, Fit::Device)
}

fn quad_to_cubic(p0: P, c: P, p: P) -> [P; 4] {
    let c1 = (
        p0.0 + 2.0 / 3.0 * (c.0 - p0.0),
        p0.1 + 2.0 / 3.0 * (c.1 - p0.1),
    );
    let c2 = (p.0 + 2.0 / 3.0 * (c.0 - p.0), p.1 + 2.0 / 3.0 * (c.1 - p.1));
    [p0, c1, c2, p]
}

/// `path` under `ts` as device-space lines: curves flattened to [`FLAT`],
/// every edge cut at the [`ROW_LATTICE`], then moved up by the row origin.
///
/// tiny-skia also chops a *curve* where it leaves the target and re-flattens
/// the pieces (up to 48 levels apart on an edge pixel). Subdivision here is
/// decided under the transform's linear part only, so targets that differ
/// by a translation flatten a curve into the same polyline. Points are
/// rounded to `f32` in the unbanded space and the integer origin subtracted
/// after, which is exact, so a band's edges are the whole page's shifted.
fn in_range(path: &Path, ts: Transform) -> Option<Path> {
    let lin = Transform::from_row(ts.sx, ts.ky, ts.kx, ts.sy, 0.0, 0.0);
    let mut out = LatticeOut {
        pb: PathBuilder::new(),
        t: (f64::from(ts.tx), f64::from(ts.ty)),
        origin: row_origin(),
        pen: (0.0, 0.0),
    };
    let all = Window {
        x0: f64::NEG_INFINITY,
        y0: f64::NEG_INFINITY,
        x1: f64::INFINITY,
        y1: f64::INFINITY,
    };
    let (mut last, mut start): (P, P) = ((0.0, 0.0), (0.0, 0.0));
    let mut pts: Vec<P> = Vec::new();
    for seg in path.segments() {
        pts.clear();
        match seg {
            PathSegment::MoveTo(p) => {
                last = map(lin, p.x, p.y);
                start = last;
                out.move_to(last);
                continue;
            }
            PathSegment::LineTo(p) => pts.push(map(lin, p.x, p.y)),
            PathSegment::QuadTo(c, p) => {
                let (c, p) = (map(lin, c.x, c.y), map(lin, p.x, p.y));
                flatten(quad_to_cubic(last, c, p), all, 0, &mut pts);
            }
            PathSegment::CubicTo(c1, c2, p) => {
                let c = [
                    last,
                    map(lin, c1.x, c1.y),
                    map(lin, c2.x, c2.y),
                    map(lin, p.x, p.y),
                ];
                flatten(c, all, 0, &mut pts);
            }
            PathSegment::Close => {
                out.line_to(start);
                out.pb.close();
                last = start;
                continue;
            }
        }
        for &p in &pts {
            out.line_to(p);
        }
        if let Some(&p) = pts.last() {
            last = p;
        }
    }
    out.pb.finish()
}

/// Path output for [`in_range`]: lattice cuts, `f32` rounding, row origin.
struct LatticeOut {
    pb: PathBuilder,
    /// The transform's translation, added in `f64` before rounding.
    t: P,
    origin: u32,
    /// Last point, in the unbanded device space.
    pen: P,
}

impl LatticeOut {
    fn device(&self, p: P) -> P {
        (p.0 + self.t.0, p.1 + self.t.1)
    }

    fn put(&mut self, d: P, first: bool) {
        #[allow(clippy::cast_possible_truncation)] // in range: `exceeds` was false
        let (x, y) = (d.0 as f32, d.1 as f32);
        #[allow(clippy::cast_precision_loss)] // a row index, exact in f32
        let y = y - self.origin as f32;
        if first {
            self.pb.move_to(x, y);
        } else {
            self.pb.line_to(x, y);
        }
    }

    fn move_to(&mut self, p: P) {
        self.pen = self.device(p);
        self.put(self.pen, true);
    }

    /// A line from the pen to `p`, with a vertex wherever it crosses a
    /// lattice row. Crossings are computed from the two endpoints alone,
    /// so every target computes the same ones.
    fn line_to(&mut self, p: P) {
        let (a, b) = (self.pen, self.device(p));
        let l = f64::from(ROW_LATTICE);
        let (lo, hi) = (a.1.min(b.1), a.1.max(b.1));
        let (first, last) = ((lo / l).floor() + 1.0, (hi / l).ceil() - 1.0);
        // In range, so |y| < GUARD and the row count is bounded.
        #[allow(clippy::cast_possible_truncation)]
        let rows = (first as i64)..=(last as i64);
        #[allow(clippy::cast_precision_loss)] // |k| < GUARD / 64
        let at = |k: i64| {
            let y = k as f64 * l;
            (a.0 + (y - a.1) * (b.0 - a.0) / (b.1 - a.1), y)
        };
        if b.1 > a.1 {
            for k in rows {
                self.put(at(k), false);
            }
        } else {
            for k in rows.rev() {
                self.put(at(k), false);
            }
        }
        self.put(b, false);
        self.pen = b;
    }
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
fn emit(contour: &mut Vec<P>, win: Window, origin: f64, pb: &mut PathBuilder) {
    let mut poly = std::mem::take(contour);
    for edge in 0..4 {
        if poly.len() < 3 {
            return;
        }
        poly = clip_edge(&poly, win, edge);
    }
    let mut pts = poly.iter().map(|&(x, y)| {
        #[allow(clippy::cast_possible_truncation)] // within the window, so well inside f32
        Point::from_xy(x as f32, (y - origin) as f32)
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

/// A stroke's outline in user space, to be filled (non-zero) under `ts`:
/// dashed, then stroked at `ts`'s resolution scale, as tiny-skia builds it
/// for a stroke wider than a device pixel. `None` when there is nothing to
/// draw.
///
/// Every stroke route fills this outline, whatever its width. tiny-skia
/// draws a stroke at most one device pixel wide as a coverage-scaled
/// hairline, which inks one pixel per major-axis step, so a diagonal or a
/// curve gets up to 1/√2 of the ink per unit length that an axis-aligned
/// run gets. A filled outline inks every direction by area.
/// `PathStroker` ignores `Stroke::dash`, so the dash is applied here.
pub(crate) fn stroke_outline(path: &Path, stroke: &Stroke, ts: Transform) -> Option<Path> {
    let res = PathStroker::compute_resolution_scale(&ts);
    let dashed = match &stroke.dash {
        Some(d) => Cow::Owned(path.dash(d, res)?),
        None => Cow::Borrowed(path),
    };
    let mut solid = stroke.clone();
    solid.dash = None;
    if solid.width <= 0.0 {
        // A zero-width stroke is a one-pixel hairline in device space.
        solid.width = 1.0 / res;
    }
    dashed.stroke(&solid, res)
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
    /// A stroke, as its [`stroke_outline`] filled through [`fit`].
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
            Fit::Device(p) => {
                let mut paint = paint.clone();
                #[allow(clippy::cast_precision_loss)] // a row index
                let origin = row_origin() as f32;
                paint.shader.transform(ts.post_translate(0.0, -origin));
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
        if let Some(outline) = stroke_outline(path, stroke, ts) {
            self.fill_path_fit(&outline, paint, FillRule::Winding, ts, mask);
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
    fn in_range_edges_are_cut_at_the_row_lattice() {
        let mut pb = PathBuilder::new();
        pb.move_to(0.0, 10.0);
        pb.line_to(100.0, 210.0);
        pb.close();
        let p = pb.finish().expect("path");
        let Fit::Device(d) = fit(&p, Transform::identity(), 256, 256) else {
            panic!("expected a device path");
        };
        let ys: Vec<f32> = d.points().iter().map(|q| q.y).collect();
        assert_eq!(
            ys,
            [10.0, 64.0, 128.0, 192.0, 210.0, 192.0, 128.0, 64.0, 10.0]
        );
        assert_eq!(d.points()[1].x, 27.0);
    }

    #[test]
    fn a_band_gets_the_whole_pages_edges_shifted_by_its_origin() {
        let mut pb = PathBuilder::new();
        pb.move_to(3.3, 1.7);
        pb.cubic_to(80.1, 20.2, 10.9, 300.3, 97.7, 250.1);
        pb.close();
        let p = pb.finish().expect("path");
        let ts = Transform::from_row(1.3, 0.1, -0.2, 1.1, 0.37, 0.61);
        let Fit::Device(whole) = fit(&p, ts, 128, 320) else {
            panic!("whole");
        };
        let Fit::Device(band) = with_row_origin(128, || fit(&p, ts, 128, 64)) else {
            panic!("band");
        };
        let shifted: Vec<(f32, f32)> = whole.points().iter().map(|q| (q.x, q.y - 128.0)).collect();
        let banded: Vec<(f32, f32)> = band.points().iter().map(|q| (q.x, q.y)).collect();
        assert_eq!(banded, shifted);
        assert_eq!(row_origin(), 0, "the origin is restored");
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
