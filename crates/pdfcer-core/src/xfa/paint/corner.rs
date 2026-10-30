//! Border corners with a radius (XFA 3.3 `corner` p.663-664, `edge` p.687).
//!
//! Each edge is trimmed back from both ends by the adjacent corner's radius
//! whatever that corner's join, inversion or presence, so an invisible
//! corner leaves a gap (p.687). A corner is stroked in its own thickness and
//! colour: `round` as a quarter arc, convex or (inverted) concave about the
//! rectangle's corner point; `square` as a right angle, or (inverted) a
//! square notch `radius` deep. The concave shapes are pdfcer's reading of
//! "concave" (gap G-D18) and are disclosed as [`PaintNoteKind::Corner`].

use super::{Edge, LayoutItem, LayoutRect, PaintNoteKind, Painter, XfaNode, length};
use crate::writer::content::{LineCap, LineJoin, Paint};

/// The cubic Bézier control-point fraction for a quarter circle.
const KAPPA: f64 = 0.552_284_749_830_793_4;

/// One corner of a border, resolved.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Corner {
    radius: f64,
    inverted: bool,
    round: bool,
    line: Edge,
}

/// Where one corner's stroke runs: from `start` on the incoming edge, past
/// the rectangle's corner point `at`, to `end` on the outgoing edge.
#[derive(Debug, Clone, Copy)]
struct Arm {
    start: (f64, f64),
    at: (f64, f64),
    end: (f64, f64),
}

impl Arm {
    /// The inner point an inverted corner bends around.
    fn inner(self) -> (f64, f64) {
        (
            self.start.0 + self.end.0 - self.at.0,
            self.start.1 + self.end.1 - self.at.1,
        )
    }
}

/// Whether `border` has a corner with a radius, the only case
/// [`Painter::rounded`] is needed for.
pub(super) fn any_radius(border: &XfaNode) -> bool {
    border
        .children_named("corner")
        .any(|c| length(c, "radius").unwrap_or(0.0) > 0.0)
}

impl Painter<'_> {
    /// The four corners (top-left, top-right, bottom-right, bottom-left;
    /// p.38), the last supplied repeated and the defaults with none.
    pub(super) fn corners(&mut self, item: &LayoutItem, border: &XfaNode) -> [Corner; 4] {
        let nodes: Vec<_> = border.children_named("corner").collect();
        let one = |p: &mut Self, i: usize| match nodes.get(i).or(nodes.last()) {
            Some(c) => p.corner(item, c),
            None => Corner {
                radius: 0.0,
                inverted: false,
                round: false,
                line: Edge {
                    thickness: 0.5,
                    rgb: [0.0; 3],
                    drawn: true,
                    three_d: None,
                },
            },
        };
        [one(self, 0), one(self, 1), one(self, 2), one(self, 3)]
    }

    fn corner(&mut self, item: &LayoutItem, c: &XfaNode) -> Corner {
        let radius = length(c, "radius").unwrap_or(0.0).max(0.0);
        let inverted = c.attr("inverted") == Some("1");
        if inverted && radius > 0.0 {
            self.note(item, PaintNoteKind::Corner);
        }
        Corner {
            radius,
            inverted,
            round: c.attr("join") == Some("round"),
            line: self.edge(item, c),
        }
    }

    /// Fill and stroke a border whose corners have a radius: the fill
    /// follows the corner shapes, each edge is trimmed by its corners' radii.
    pub(super) fn rounded(
        &mut self,
        r: LayoutRect,
        rgb: Option<[f64; 3]>,
        sides: &[Edge],
        corners: [Corner; 4],
        offset: f64,
    ) {
        let limit = (r.w.min(r.h) / 2.0).max(0.0);
        let corners = corners.map(|c| Corner {
            radius: c.radius.min(limit),
            ..c
        });
        if let Some(rgb) = rgb {
            let arms = self.arms(r, &corners, [0.0; 4]);
            self.b.set_fill_rgb(rgb[0], rgb[1], rgb[2]);
            self.b.move_to(arms[0].start.0, arms[0].start.1);
            for (c, a) in corners.iter().zip(arms) {
                self.b.line_to(a.start.0, a.start.1);
                self.arm_path(*c, a);
            }
            self.b.close_subpath();
            self.b.paint(Paint::Fill);
        }
        if sides.len() < 4 {
            return;
        }
        let d: [f64; 4] =
            std::array::from_fn(|i| sides.get(i).map_or(0.0, |s| s.thickness) * offset);
        let arms = self.arms(r, &corners, d);
        let after = arms.iter().zip(&corners).cycle().skip(1);
        for ((s, (a, c)), (b, n)) in sides.iter().zip(arms.iter().zip(&corners)).zip(after) {
            self.side(Some(*s), a.end, b.start, [*c, *n]);
        }
        for (c, a) in corners.iter().zip(arms) {
            if c.line.drawn && c.radius > 0.0 {
                self.stroke_style(c.line, LineCap::Butt);
                self.b.set_line_join(LineJoin::Miter);
                self.b.move_to(a.start.0, a.start.1);
                self.arm_path(*c, a);
                self.b.paint(Paint::Stroke);
            }
        }
    }

    /// The four corner arms of `r`, each edge moved outward by its `d`.
    fn arms(&self, r: LayoutRect, corners: &[Corner; 4], d: [f64; 4]) -> [Arm; 4] {
        let [dt, dr, db, dl] = d;
        let top = self.height - r.y + dt;
        let right = r.x + r.w + dr;
        let bottom = self.height - r.y - r.h - db;
        let left = r.x - dl;
        // Clockwise: the incoming then outgoing direction at each corner.
        let at = [(left, top), (right, top), (right, bottom), (left, bottom)];
        let dirs = [
            ((0.0, -1.0), (1.0, 0.0)),
            ((-1.0, 0.0), (0.0, -1.0)),
            ((0.0, 1.0), (-1.0, 0.0)),
            ((1.0, 0.0), (0.0, 1.0)),
        ];
        let blank = Arm {
            start: (0.0, 0.0),
            at: (0.0, 0.0),
            end: (0.0, 0.0),
        };
        let mut out = [blank; 4];
        for (((o, p), (a, b)), c) in out.iter_mut().zip(at).zip(dirs).zip(corners) {
            let k = c.radius;
            *o = Arm {
                start: (p.0 + a.0 * k, p.1 + a.1 * k),
                at: p,
                end: (p.0 + b.0 * k, p.1 + b.1 * k),
            };
        }
        out
    }

    /// Continue the current path through corner `c` from `a.start` to
    /// `a.end`.
    fn arm_path(&mut self, c: Corner, a: Arm) {
        let via = if c.inverted { a.inner() } else { a.at };
        if c.radius <= 0.0 {
            self.b.line_to(a.at.0, a.at.1);
        } else if c.round {
            let c1 = (
                a.start.0 + KAPPA * (via.0 - a.start.0),
                a.start.1 + KAPPA * (via.1 - a.start.1),
            );
            let c2 = (
                a.end.0 + KAPPA * (via.0 - a.end.0),
                a.end.1 + KAPPA * (via.1 - a.end.1),
            );
            self.b.curve_to(c1.0, c1.1, c2.0, c2.1, a.end.0, a.end.1);
        } else {
            self.b.line_to(via.0, via.1);
            self.b.line_to(a.end.0, a.end.1);
        }
    }

    /// Stroke one trimmed side from `a` to `b`. An end meeting a corner with
    /// no radius projects half its thickness, so the sides close the corner.
    fn side(&mut self, s: Option<Edge>, a: (f64, f64), b: (f64, f64), ends: [Corner; 2]) {
        let Some(s) = s.filter(|s| s.drawn) else {
            return;
        };
        let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        if len <= 0.0 {
            return;
        }
        let u = ((b.0 - a.0) / len, (b.1 - a.1) / len);
        let grow = |c: Corner| {
            if c.radius > 0.0 {
                0.0
            } else {
                s.thickness / 2.0
            }
        };
        let (ga, gb) = (grow(ends[0]), grow(ends[1]));
        self.stroke_style(s, LineCap::Butt);
        self.b.move_to(a.0 - u.0 * ga, a.1 - u.1 * ga);
        self.b.line_to(b.0 + u.0 * gb, b.1 + u.1 * gb);
        self.b.paint(Paint::Stroke);
    }
}
