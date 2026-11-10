//! The interpreter side of the page-wide coverage ceiling
//! ([`crate::page_budget::COVERAGE_PER_CANVAS`], ARCHITECTURE.md §10.1).
//! Each fill, stroke, shading and image is charged the device area it can
//! reach before it paints. Once the ceiling is spent, everything left on
//! the page is skipped and counted in `Diagnostics::content_truncated`.

use tiny_skia::{Path, PathSegment, Rect, Transform};

use super::Interpreter;
use crate::canvas::Canvas;
use crate::page_budget::Refusal;

impl Interpreter<'_> {
    /// Charge a paint whose device-space bounds are `bounds` (`None`: the
    /// whole clip) and whose painted area cannot exceed `cap` pixels.
    /// Returns `true` when the paint must be skipped.
    pub(super) fn coverage_spent(
        &mut self,
        bounds: Option<Rect>,
        cap: Option<u64>,
        canvas: &Canvas<'_>,
    ) -> bool {
        let (w, h) = (canvas.width(), canvas.height());
        let area = reach(bounds, w, h, self.gs.current.clip_bbox);
        let area = cap.map_or(area, |c| area.min(c));
        if self.budget.cover(area, u64::from(w) * u64::from(h)) {
            return false;
        }
        self.diag.content_truncated += 1;
        self.diag.note(Refusal::Coverage.note());
        true
    }

    /// [`Self::coverage_spent`] for a path painted under `ctm`. A stroke is
    /// charged no more than its device length times its width, so long
    /// diagonal hairlines are not charged for their whole bounding box.
    pub(super) fn path_coverage_spent(
        &mut self,
        path: &Path,
        ctm: Transform,
        fill: bool,
        stroke: bool,
        canvas: &Canvas<'_>,
    ) -> bool {
        // `n` paints nothing; CAD exporters set a page-sized clip with it
        // before every stroke.
        if !fill && !stroke {
            return false;
        }
        let width = stroke.then(|| device_width(self.gs.current.line_width, ctm));
        let pad = width.unwrap_or(0.0) / 2.0 + 1.0;
        let bounds = path.bounds().transform(ctm).and_then(|b| {
            Rect::from_ltrb(
                b.left() - pad,
                b.top() - pad,
                b.right() + pad,
                b.bottom() + pad,
            )
        });
        let cap = match width {
            Some(w) if !fill => Some(stroke_area(path, ctm, w)),
            _ => None,
        };
        self.coverage_spent(bounds, cap, canvas)
    }
}

/// Pixels of `bounds` that land on a `w` x `h` canvas inside `clip`
/// (`(left, top, right, bottom)` in device pixels).
fn reach(bounds: Option<Rect>, w: u32, h: u32, clip: Option<(f32, f32, f32, f32)>) -> u64 {
    let (mut l, mut t, mut r, mut b) = (0.0f32, 0.0f32, w as f32, h as f32);
    if let Some(bb) = bounds {
        (l, t, r, b) = (
            l.max(bb.left()),
            t.max(bb.top()),
            r.min(bb.right()),
            b.min(bb.bottom()),
        );
    }
    if let Some((cl, ct, cr, cb)) = clip {
        (l, t, r, b) = (l.max(cl), t.max(ct), r.min(cr), b.min(cb));
    }
    if r <= l || b <= t {
        return 0;
    }
    // Both spans are within the canvas, so each fits in a u32 after `ceil`.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let px = |v: f32| v.ceil() as u64;
    px(r - l) * px(b - t)
}

/// A user-space line width in device pixels: the larger axis scale, and
/// at least one pixel, since a zero-width line still paints a hairline.
fn device_width(width: f32, ctm: Transform) -> f32 {
    let sx = ctm.sx.hypot(ctm.ky);
    let sy = ctm.kx.hypot(ctm.sy);
    (width * sx.max(sy)).max(1.0)
}

/// An upper bound on the pixels a stroke of device width `width` paints:
/// the device length of the path's control polygon times `width + 2`.
fn stroke_area(path: &Path, ctm: Transform, width: f32) -> u64 {
    let map = |p: tiny_skia::Point| {
        let mut q = [p];
        ctm.map_points(&mut q);
        q[0]
    };
    let mut length = 0.0f64;
    let (mut last, mut start) = (None, None);
    let mut step = |from: &mut Option<tiny_skia::Point>, to: tiny_skia::Point| {
        if let Some(f) = *from {
            length += f64::from(f.distance(to));
        }
        *from = Some(to);
    };
    for seg in path.segments() {
        match seg {
            PathSegment::MoveTo(p) => {
                last = Some(map(p));
                start = last;
            }
            PathSegment::LineTo(p) => step(&mut last, map(p)),
            PathSegment::QuadTo(a, p) => {
                step(&mut last, map(a));
                step(&mut last, map(p));
            }
            PathSegment::CubicTo(a, b, p) => {
                step(&mut last, map(a));
                step(&mut last, map(b));
                step(&mut last, map(p));
            }
            PathSegment::Close => {
                if let Some(s) = start {
                    step(&mut last, s);
                }
            }
        }
    }
    let area = length * f64::from(width + 2.0);
    // Saturating float-to-int: a non-finite or huge length charges the most.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let area = area.ceil() as u64;
    area
}

#[cfg(test)]
#[allow(clippy::expect_used)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use tiny_skia::PathBuilder;

    #[test]
    fn reach_clamps_to_the_canvas_and_the_clip() {
        let r = Rect::from_ltrb(-50.0, -50.0, 500.0, 500.0);
        assert_eq!(reach(r, 100, 200, None), 100 * 200);
        assert_eq!(reach(r, 100, 200, Some((10.0, 10.0, 20.0, 30.0))), 10 * 20);
        assert_eq!(
            reach(Rect::from_ltrb(150.0, 0.0, 160.0, 10.0), 100, 100, None),
            0
        );
        assert_eq!(reach(None, 100, 100, None), 10_000);
    }

    #[test]
    fn a_diagonal_hairline_is_charged_by_length_not_bbox() {
        let mut pb = PathBuilder::new();
        pb.move_to(0.0, 0.0);
        pb.line_to(1000.0, 1000.0);
        let path = pb.finish().expect("two points make a path");
        let area = stroke_area(&path, Transform::identity(), 1.0);
        assert!((4242..=4244).contains(&area), "{area}");
    }

    #[test]
    fn device_width_follows_the_ctm_scale() {
        assert!((device_width(2.0, Transform::from_scale(10.0, 10.0)) - 20.0).abs() < 1e-4);
        assert!((device_width(0.0, Transform::identity()) - 1.0).abs() < 1e-6);
    }
}
