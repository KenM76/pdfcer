//! The integer edges `tiny-skia` 0.11 scans a fill from,
//! built as it builds them, for a per-pixel coverage evaluation
//! ([`super::fill_coverage`]) to reproduce its coverage exactly.
//!
//! Ported from `tiny-skia` 0.11.4 (`edge.rs`, `edge_builder.rs`,
//! `line_clipper.rs`, `scan/path.rs`, `scan/path_aa.rs`), itself a port of
//! Skia. Copyright 2006 The Android Open Source Project, 2011 Google Inc.,
//! 2020 Yevhenii Reizner; BSD-3-Clause, see `THIRD_PARTY_LICENSES.md`.
//!
//! Line-only paths under the identity transform, as `device_clip::fit`
//! hands the CPU replay; anything else is `None`.

use tiny_skia::{FillRule, IntRect, Path, PathSegment, Point, Rect};

/// `DrawTiler::MAX_DIMENSIONS`: a target wider or taller is filled one
/// tile of at most this many pixels a side at a time, the path translated
/// to each tile's origin and back.
pub(crate) const DRAW_TILE: u32 = 8191;

/// Supersampling shift of the anti-aliased scan: 4 × 4 samples a pixel.
pub(crate) const SHIFT: i32 = 2;

/// One line edge: `x` (16.16 fixed, supersampled) on row `first_y`,
/// stepping `dx` a row, through row `last_y`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LineEdge {
    pub x: i32,
    pub dx: i32,
    pub first_y: i32,
    pub last_y: i32,
    pub winding: i32,
}

impl LineEdge {
    /// `x` on supersampled row `y`, the walk's running sum.
    pub(crate) fn x_at(&self, y: i32) -> i32 {
        self.x
            .wrapping_add(y.wrapping_sub(self.first_y).wrapping_mul(self.dx))
    }

    fn new(p0: Point, p1: Point, shift: i32) -> Option<Self> {
        let scale = (1i32 << (shift + 6)) as f32;
        let (mut x0, mut y0) = ((p0.x * scale) as i32, (p0.y * scale) as i32);
        let (mut x1, mut y1) = ((p1.x * scale) as i32, (p1.y * scale) as i32);
        let mut winding = 1;
        if y0 > y1 {
            core::mem::swap(&mut x0, &mut x1);
            core::mem::swap(&mut y0, &mut y1);
            winding = -1;
        }
        let top = (y0 + 32) >> 6;
        let bottom = (y1 + 32) >> 6;
        if top == bottom {
            return None;
        }
        let slope = fdot6_div(x1 - x0, y1 - y0);
        let dy = shl(top, 6) + 32 - y0;
        let mul = ((i64::from(slope) * i64::from(dy)) >> 16) as i32;
        Some(LineEdge {
            x: shl(x0 + mul, 10),
            dx: slope,
            first_y: top,
            last_y: bottom - 1,
            winding,
        })
    }
}

fn shl(v: i32, s: i32) -> i32 {
    ((v as u32) << s) as i32
}

fn fdot6_div(a: i32, b: i32) -> i32 {
    if i16::try_from(a).is_ok() {
        shl(a, 16).checked_div(b).unwrap_or(0)
    } else {
        let v = (i64::from(a) << 16).checked_div(i64::from(b)).unwrap_or(0);
        v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
    }
}

/// A fill's edges in the scan's starting order, the rows it
/// walks and the pixels it writes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FillEdges {
    pub edges: Vec<LineEdge>,
    pub even_odd: bool,
    /// [`SHIFT`] anti-aliased, 0 aliased: rows and columns are then pixels.
    pub shift: i32,
    /// Supersampled rows walked, `start_y..stop_y`.
    pub start_y: i32,
    pub stop_y: i32,
    /// Pixels written: `bounds ∩ clip`.
    pub area: IntRect,
}

/// The edges `Pixmap::fill_path` / `Mask::fill_path` scan for `path`
/// filled under the identity into a `width` × `height` target; `None`
/// when it draws nothing or takes a branch not reproduced (curves, a
/// target over 8,191 pixels).
pub(crate) fn build(
    path: &Path,
    rule: FillRule,
    size: (u32, u32),
    anti_alias: bool,
) -> Option<FillEdges> {
    if skips(path) {
        return None;
    }
    scan(path, rule, size, anti_alias)
}

/// Whether `fill_path` returns before scanning: an empty or hairline
/// path, or coordinates too big for its maths.
pub(crate) fn skips(path: &Path) -> bool {
    let b = path.bounds();
    let nearly_zero = |v: f32| v.abs() <= 1.0 / 4096.0;
    nearly_zero(b.width()) || nearly_zero(b.height()) || too_big(&b)
}

/// The scan of one `width` × `height` target, past [`skips`]: what
/// `scan::path_aa::fill_path` (`scan::path::fill_path` aliased) walks for
/// one `DrawTiler` tile.
pub(crate) fn scan(
    path: &Path,
    rule: FillRule,
    (width, height): (u32, u32),
    anti_alias: bool,
) -> Option<FillEdges> {
    if width > DRAW_TILE || height > DRAW_TILE || width == 0 || height == 0 {
        return None;
    }
    let b = path.bounds();
    let (ir, shift) = if anti_alias {
        let r = Rect::from_ltrb(
            b.left().floor(),
            b.top().floor(),
            b.right().ceil(),
            b.bottom().ceil(),
        )?;
        (r.round_out()?, SHIFT)
    } else {
        (conservative_round(&b)?, 0)
    };
    let clip = IntRect::from_xywh(0, 0, width, height)?;
    let area = ir.intersect(&clip)?;
    let contained =
        ir.x() >= 0 && ir.y() >= 0 && ir.right() <= clip.right() && ir.bottom() <= clip.bottom();
    let clip_rect = Rect::from_ltrb(0.0, 0.0, width as f32, height as f32)?;
    let mut edges = Vec::new();
    for (p0, p1) in lines(path)? {
        if contained {
            push_line(&mut edges, p0, p1, shift);
        } else {
            let mut pts = [Point::zero(); 4];
            let n = clip_line([p0, p1], &clip_rect, &mut pts);
            for w in pts.get(..n).unwrap_or(&[]).windows(2) {
                if let [a, b] = w {
                    push_line(&mut edges, *a, *b, shift);
                }
            }
        }
    }
    if edges.len() < 2 {
        return None;
    }
    edges.sort_by_key(|e| (e.first_y, e.x));
    let (mut start_y, mut stop_y) = (shl(ir.y(), shift), shl(ir.bottom(), shift));
    if !contained {
        start_y = start_y.max(0);
        stop_y = stop_y.min(shl(height as i32, shift));
    }
    if start_y < 0 || stop_y < 0 {
        return None;
    }
    Some(FillEdges {
        edges,
        even_odd: rule == FillRule::EvenOdd,
        shift,
        start_y,
        stop_y,
        area,
    })
}

/// `scan::path::conservative_round_to_int`: each side moved out by
/// `0.5 + 1.5 / 64` in `f64`, then rounded inward.
fn conservative_round(b: &Rect) -> Option<IntRect> {
    const BIAS: f64 = 0.5 + 1.5 / 64.0;
    // Saturating casts, as `i32::saturate_from`.
    let down = |v: f32| (f64::from(v) - BIAS).ceil() as i32;
    let up = |v: f32| (f64::from(v) + BIAS).floor() as i32;
    IntRect::from_ltrb(down(b.left()), down(b.top()), up(b.right()), up(b.bottom()))
}

fn too_big(b: &Rect) -> bool {
    let max = f32::MAX * 0.25;
    !(b.left() >= -max && b.top() >= -max && b.right() <= max && b.bottom() <= max)
}

/// The path's lines in `edge_iter` order, each subpath closed; `None` for
/// a curve.
fn lines(path: &Path) -> Option<Vec<(Point, Point)>> {
    let mut out = Vec::new();
    let (mut start, mut last) = (Point::zero(), Point::zero());
    let mut open = false;
    for seg in path.segments() {
        match seg {
            PathSegment::MoveTo(p) => {
                if open {
                    out.push((last, start));
                }
                open = false;
                start = p;
                last = p;
            }
            PathSegment::LineTo(p) => {
                out.push((last, p));
                last = p;
                open = true;
            }
            PathSegment::Close => {
                if open {
                    out.push((last, start));
                }
                open = false;
                last = start;
            }
            PathSegment::QuadTo(..) | PathSegment::CubicTo(..) => return None,
        }
    }
    if open {
        out.push((last, start));
    }
    Some(out)
}

fn push_line(edges: &mut Vec<LineEdge>, p0: Point, p1: Point, shift: i32) {
    let Some(edge) = LineEdge::new(p0, p1, shift) else {
        return;
    };
    let combine = match edges.last_mut() {
        Some(last) if edge.dx == 0 => combine_vertical(&edge, last),
        _ => Combine::No,
    };
    match combine {
        Combine::Total => {
            edges.pop();
        }
        Combine::Partial => {}
        Combine::No => edges.push(edge),
    }
}

#[derive(PartialEq)]
enum Combine {
    No,
    Partial,
    Total,
}

fn combine_vertical(edge: &LineEdge, last: &mut LineEdge) -> Combine {
    if last.dx != 0 || edge.x != last.x {
        return Combine::No;
    }
    if edge.winding == last.winding {
        return if edge.last_y + 1 == last.first_y {
            last.first_y = edge.first_y;
            Combine::Partial
        } else if edge.first_y == last.last_y + 1 {
            last.last_y = edge.last_y;
            Combine::Partial
        } else {
            Combine::No
        };
    }
    if edge.first_y == last.first_y {
        return if edge.last_y == last.last_y {
            Combine::Total
        } else if edge.last_y < last.last_y {
            last.first_y = edge.last_y + 1;
            Combine::Partial
        } else {
            last.first_y = last.last_y + 1;
            last.last_y = edge.last_y;
            last.winding = edge.winding;
            Combine::Partial
        };
    }
    if edge.last_y == last.last_y {
        if edge.first_y > last.first_y {
            last.last_y = edge.first_y - 1;
        } else {
            last.last_y = last.first_y - 1;
            last.first_y = edge.first_y;
            last.winding = edge.winding;
        }
        return Combine::Partial;
    }
    Combine::No
}

/// `line_clipper::clip` with right-culling off: the pieces of `src` inside
/// `clip`, its parts left or right of it moved onto that side; the points
/// written to `out`, their count returned (0 or 2..=4).
fn clip_line(src: [Point; 2], clip: &Rect, out: &mut [Point; 4]) -> usize {
    let (i0, i1) = if src[0].y < src[1].y { (0, 1) } else { (1, 0) };
    if at(&src, i1).y <= clip.top() || at(&src, i0).y >= clip.bottom() {
        return 0;
    }
    let mut tmp = src;
    if at(&src, i0).y < clip.top() {
        put(
            &mut tmp,
            i0,
            Point::from_xy(sect_h(&src, clip.top()), clip.top()),
        );
    }
    if at(&tmp, i1).y > clip.bottom() {
        put(
            &mut tmp,
            i1,
            Point::from_xy(sect_h(&src, clip.bottom()), clip.bottom()),
        );
    }
    let (i0, i1, mut reverse) = if src[0].x < src[1].x {
        (0, 1, false)
    } else {
        (1, 0, true)
    };
    let mut res = [Point::zero(); 4];
    let count;
    if at(&tmp, i1).x <= clip.left() || at(&tmp, i0).x >= clip.right() {
        let x = if at(&tmp, i1).x <= clip.left() {
            clip.left()
        } else {
            clip.right()
        };
        res[0] = Point::from_xy(x, tmp[0].y);
        res[1] = Point::from_xy(x, tmp[1].y);
        reverse = false;
        count = 1;
    } else {
        let mut k = 0;
        if at(&tmp, i0).x < clip.left() {
            res[0] = Point::from_xy(clip.left(), at(&tmp, i0).y);
            res[1] = Point::from_xy(clip.left(), sect_v_pinned(&tmp, clip.left()));
            k = 1;
        } else {
            res[0] = at(&tmp, i0);
        }
        k += 1;
        if at(&tmp, i1).x > clip.right() {
            put(
                &mut res,
                k,
                Point::from_xy(clip.right(), sect_v_pinned(&tmp, clip.right())),
            );
            k += 1;
            put(&mut res, k, Point::from_xy(clip.right(), at(&tmp, i1).y));
        } else {
            put(&mut res, k, at(&tmp, i1));
        }
        count = k;
    }
    for i in 0..=count {
        put(out, if reverse { count - i } else { i }, at(&res, i));
    }
    count + 1
}

/// `a[i]`, or the origin past the end.
fn at<const N: usize>(a: &[Point; N], i: usize) -> Point {
    a.get(i).copied().unwrap_or(Point::zero())
}

/// `a[i] = p`, ignored past the end.
fn put<const N: usize>(a: &mut [Point; N], i: usize, p: Point) {
    if let Some(slot) = a.get_mut(i) {
        *slot = p;
    }
}

/// `value` held between two limits given in either order; NaN passes.
fn pin(value: f64, a: f64, b: f64) -> f64 {
    let (lo, hi) = if b < a { (b, a) } else { (a, b) };
    if value < lo {
        lo
    } else if value > hi {
        hi
    } else {
        value
    }
}

fn sect_h(src: &[Point; 2], y: f32) -> f32 {
    let [p0, p1] = *src;
    if (p1.y - p0.y).abs() <= 1.0 / 4096.0 {
        return (p0.x + p1.x) * 0.5;
    }
    let (x0, y0) = (f64::from(p0.x), f64::from(p0.y));
    let (x1, y1) = (f64::from(p1.x), f64::from(p1.y));
    pin(x0 + (f64::from(y) - y0) * (x1 - x0) / (y1 - y0), x0, x1) as f32
}

fn sect_v_pinned(src: &[Point; 2], x: f32) -> f32 {
    let [p0, p1] = *src;
    let y = if (p1.x - p0.x).abs() <= 1.0 / 4096.0 {
        (p0.y + p1.y) * 0.5
    } else {
        let (x0, y0) = (f64::from(p0.x), f64::from(p0.y));
        let (x1, y1) = (f64::from(p1.x), f64::from(p1.y));
        (y0 + (f64::from(x) - x0) * (y1 - y0) / (x1 - x0)) as f32
    };
    pin(f64::from(y), f64::from(p0.y), f64::from(p1.y)) as f32
}
