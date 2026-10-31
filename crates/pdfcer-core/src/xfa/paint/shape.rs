//! A draw's `rectangle`, `line` or `arc` value (XFA 3.3 p.604 arc, p.770
//! line, p.818 rectangle; slice `adobe_ext__xfa_appearance_drawing.md`
//! §5.1-§5.3, gap G-D9), drawn in its content region.

use crate::writer::content::{LineCap, Paint};

use super::{Edge, LayoutItem, LayoutRect, PaintNoteKind, Painter, XfaNode};

/// How big a `circular="1"` arc is (gap G-D9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ArcCircle {
    /// The circle inscribed in the region: diameter = the smaller side.
    #[default]
    Diameter,
    /// A radius of the smaller side, centred on the region.
    Radius,
}

/// How a filled arc of less than 360 degrees is closed (gap G-D9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ArcFill {
    /// Through the centre (a pie slice).
    #[default]
    Pie,
    /// Straight between the ends (a chord).
    Chord,
}

/// `node`'s `rectangle`, `line` or `arc` value.
pub(super) fn value_shape(node: &XfaNode) -> Option<&XfaNode> {
    node.child("value")?
        .children
        .iter()
        .find(|c| matches!(c.tag.as_str(), "rectangle" | "line" | "arc"))
}

/// A finite numeric attribute, or `None` (absent) / `Err` (unreadable).
fn number(node: &XfaNode, name: &str) -> Result<Option<f64>, ()> {
    node.attr(name)
        .map_or(Ok(None), |v| match v.trim().parse::<f64>() {
            Ok(n) if n.is_finite() => Ok(Some(n)),
            _ => Err(()),
        })
}

impl Painter<'_> {
    /// Paint `s`, a draw's shape value, in its content region `r`.
    pub(super) fn shape(&mut self, item: &LayoutItem, s: &XfaNode, r: LayoutRect) {
        match s.tag.as_str() {
            "rectangle" => self.border(item, s, r),
            "line" => self.line(item, s, r),
            _ => self.arc(item, s, r),
        }
    }

    /// The shape's one `edge` (default 0.5pt black), with its `cap`.
    fn shape_edge(&mut self, item: &LayoutItem, s: &XfaNode) -> (Edge, LineCap) {
        let Some(e) = s.child("edge") else {
            return (Edge::PLAIN, LineCap::ProjectingSquare);
        };
        let edge = self.edge(item, e);
        if edge.three_d.is_some() && edge.drawn {
            self.note(item, PaintNoteKind::StrokeApproximated);
        }
        let cap = match e.attr("cap") {
            None | Some("square") => LineCap::ProjectingSquare,
            Some("butt") => LineCap::Butt,
            Some("round") => LineCap::Round,
            Some(_) => {
                self.note(item, PaintNoteKind::Shape);
                LineCap::ProjectingSquare
            }
        };
        (edge, cap)
    }

    /// Corner to corner of `r`: `slope="\"` from top-left, `"/"` from
    /// bottom-left; `hand` moves it half its thickness to the left or right
    /// of that direction.
    fn line(&mut self, item: &LayoutItem, s: &XfaNode, r: LayoutRect) {
        let (edge, cap) = self.shape_edge(item, s);
        if !edge.drawn {
            return;
        }
        let (top, bottom) = (self.height - r.y, self.height - r.y - r.h.max(0.0));
        let (x0, x1) = (r.x, r.x + r.w.max(0.0));
        let ((ax, ay), (bx, by)) = match s.attr("slope") {
            Some("/") => ((x0, bottom), (x1, top)),
            None | Some("\\") => ((x0, top), (x1, bottom)),
            Some(_) => {
                self.note(item, PaintNoteKind::Shape);
                ((x0, top), (x1, bottom))
            }
        };
        let side = match s.attr("hand") {
            Some("left") => 0.5,
            Some("right") => -0.5,
            _ => 0.0,
        };
        let (dx, dy) = (bx - ax, by - ay);
        let len = dx.hypot(dy);
        let (ox, oy) = if len > 0.0 {
            let k = side * edge.thickness / len;
            (-dy * k, dx * k)
        } else {
            (0.0, 0.0)
        };
        self.stroke_style(edge, cap);
        self.b.move_to(ax + ox, ay + oy);
        self.b.line_to(bx + ox, by + oy);
        self.b.paint(Paint::Stroke);
    }

    /// The arc inscribed in `r` from `startAngle` through `sweepAngle`
    /// degrees, counter-clockwise from 3 o'clock (p.37, p.604).
    fn arc(&mut self, item: &LayoutItem, s: &XfaNode, r: LayoutRect) {
        let (mut start, mut sweep) = (0.0, 360.0);
        match (number(s, "startAngle"), number(s, "sweepAngle")) {
            (Ok(a), Ok(b)) => {
                start = a.unwrap_or(0.0).rem_euclid(360.0);
                sweep = b.unwrap_or(360.0).clamp(-360.0, 360.0);
            }
            _ => self.note(item, PaintNoteKind::Shape),
        }
        let (mut rx, mut ry) = (r.w.max(0.0) / 2.0, r.h.max(0.0) / 2.0);
        if s.attr("circular") == Some("1") {
            let m = rx.min(ry);
            let radius = match self.opts.arc_circle {
                ArcCircle::Diameter => m,
                ArcCircle::Radius => 2.0 * m,
            };
            (rx, ry) = (radius, radius);
        }
        let c = (
            r.x + r.w.max(0.0) / 2.0,
            self.height - r.y - r.h.max(0.0) / 2.0,
        );
        let whole = sweep.abs() >= 360.0;
        if let Some(rgb) = self.fill_rgb(item, s) {
            self.b.set_fill_rgb(rgb[0], rgb[1], rgb[2]);
            if !whole && self.opts.arc_fill == ArcFill::Pie {
                self.b.move_to(c.0, c.1);
                let p = point(c, rx, ry, start.to_radians());
                self.b.line_to(p.0, p.1);
            } else {
                let p = point(c, rx, ry, start.to_radians());
                self.b.move_to(p.0, p.1);
            }
            self.arc_path(c, rx, ry, start, sweep);
            self.b.close_subpath();
            self.b.paint(Paint::Fill);
        }
        let (edge, cap) = self.shape_edge(item, s);
        if !edge.drawn || sweep == 0.0 {
            return;
        }
        self.stroke_style(edge, cap);
        let p = point(c, rx, ry, start.to_radians());
        self.b.move_to(p.0, p.1);
        self.arc_path(c, rx, ry, start, sweep);
        if whole {
            self.b.close_subpath();
        }
        self.b.paint(Paint::Stroke);
    }

    /// Bezier segments of at most 90 degrees from the current point.
    fn arc_path(&mut self, c: (f64, f64), rx: f64, ry: f64, start: f64, sweep: f64) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 1..=4
        let n = (sweep.abs() / 90.0).ceil().max(1.0) as usize;
        #[allow(clippy::cast_precision_loss)] // n <= 4
        let step = sweep.to_radians() / n as f64;
        let k = 4.0 / 3.0 * (step / 4.0).tan();
        let mut a = start.to_radians();
        for _ in 0..n {
            let b = a + step;
            let (p0, p3) = (point(c, rx, ry, a), point(c, rx, ry, b));
            let c1 = (p0.0 - k * rx * a.sin(), p0.1 + k * ry * a.cos());
            let c2 = (p3.0 + k * rx * b.sin(), p3.1 - k * ry * b.cos());
            self.b.curve_to(c1.0, c1.1, c2.0, c2.1, p3.0, p3.1);
            a = b;
        }
    }
}

fn point(c: (f64, f64), rx: f64, ry: f64, a: f64) -> (f64, f64) {
    (c.0 + rx * a.cos(), c.1 + ry * a.sin())
}
