//! A `fill`'s type (p.734): solid, `linear` or `radial` gradient, `pattern`
//! hatching or `stipple`. Geometry is unstated (gaps G-F1, G-F2, G-D5): a
//! gradient spans the rectangle the fill paints, a radial one as the
//! ellipse inscribed in it (centre to edge midpoints), interpolated in
//! DeviceRGB; hatch lines are [`HATCH_STEP`] apart and [`HATCH_WIDTH`]
//! wide. Spec RAG `adobe_ext__xfa_fills_and_fonts` §1,
//! `adobe_ext__xfa_appearance_drawing` §1.5.

use super::{LayoutItem, LayoutRect, Painter, Presence, Stipple, XfaNode};
use crate::object::{Dict, Name, Object};
use crate::writer::content::Paint;

/// The distance between hatch lines, points (application-defined, p.806).
const HATCH_STEP: f64 = 4.0;
/// The width of a hatch line, points.
const HATCH_WIDTH: f64 = 0.5;

/// A resolved fill.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Fill {
    Solid([f64; 3]),
    /// From `start` at one side to `end` at the other; `dir` is the unit
    /// direction of travel in default user space (y up).
    Linear {
        start: [f64; 3],
        end: [f64; 3],
        dir: (f64, f64),
    },
    /// `start` at the centre to `end` at the edge.
    Radial {
        start: [f64; 3],
        end: [f64; 3],
    },
    /// `fg` lines over `bg`; `lines` holds which of horizontal, vertical,
    /// rising and falling diagonals are drawn.
    Pattern {
        bg: [f64; 3],
        fg: [f64; 3],
        lines: [bool; 4],
    },
}

impl Painter<'_> {
    /// What `node`'s `fill` child paints, `None` for nothing. A fill with
    /// no `color` starts from white; a type child's own colour defaults to
    /// black (gap G-D4).
    pub(super) fn fill_of(&mut self, item: &LayoutItem, node: &XfaNode) -> Option<Fill> {
        let f = match node.child("fill") {
            None => {
                return match self.opts.missing_fill {
                    super::MissingFill::None => None,
                    super::MissingFill::White => Some(Fill::Solid([1.0; 3])),
                };
            }
            Some(f) if f.presence() != Presence::Visible => return None,
            Some(f) => f,
        };
        let base = self.color(item, f, [1.0; 3]);
        let Some(kind) = f
            .children
            .iter()
            .find(|c| matches!(c.tag.as_str(), "linear" | "radial" | "pattern" | "stipple"))
        else {
            return Some(Fill::Solid(base));
        };
        let own = self.color(item, kind, [0.0; 3]);
        let ty = kind.attr("type");
        Some(match kind.tag.as_str() {
            "linear" => Fill::Linear {
                start: base,
                end: own,
                dir: match ty {
                    Some("toLeft") => (-1.0, 0.0),
                    Some("toTop") => (0.0, 1.0),
                    Some("toBottom") => (0.0, -1.0),
                    _ => (1.0, 0.0),
                },
            },
            "radial" if ty == Some("toCenter") => Fill::Radial {
                start: own,
                end: base,
            },
            "radial" => Fill::Radial {
                start: base,
                end: own,
            },
            "pattern" => Fill::Pattern {
                bg: base,
                fg: own,
                lines: match ty {
                    Some("crossDiagonal") => [false, false, true, true],
                    Some("diagonalLeft") => [false, false, false, true],
                    Some("diagonalRight") => [false, false, true, false],
                    Some("horizontal") => [true, false, false, false],
                    Some("vertical") => [false, true, false, false],
                    _ => [true, true, false, false],
                },
            },
            _ => Fill::Solid(self.stippled(kind, base, own)),
        })
    }

    /// A stipple as the uniform blend of its colour over the fill colour
    /// at `rate` percent (gap G-F2), per [`Stipple`] (gap G-D6).
    fn stippled(&self, stipple: &XfaNode, base: [f64; 3], own: [f64; 3]) -> [f64; 3] {
        let rate = stipple
            .attr("rate")
            .and_then(|r| r.trim().parse::<f64>().ok())
            .unwrap_or(50.0);
        let (rate, under) = match self.opts.stipple {
            Stipple::Spec => (rate.clamp(0.0, 100.0), base),
            Stipple::Viewer if [25.0, 50.0, 75.0].contains(&rate) => (rate, [1.0; 3]),
            Stipple::Viewer => (100.0, [1.0; 3]),
        };
        let t = rate / 100.0;
        let [u0, u1, u2] = under;
        let [o0, o1, o2] = own;
        [u0 + (o0 - u0) * t, u1 + (o1 - u1) * t, u2 + (o2 - u2) * t]
    }

    /// Paint `fill` inside the path `path` draws; `bx` is the rectangle the
    /// fill spans (gap G-F1).
    pub(super) fn paint_fill(&mut self, fill: Fill, bx: LayoutRect, path: impl FnOnce(&mut Self)) {
        if let Fill::Solid(rgb) = fill {
            self.b.set_fill_rgb(rgb[0], rgb[1], rgb[2]);
            path(self);
            self.b.paint(Paint::Fill);
            return;
        }
        self.b.save_state();
        path(self);
        self.b.clip_nonzero();
        self.b.paint(Paint::NoPaint);
        let (x0, y0) = (bx.x, self.height - bx.y - bx.h);
        let (cx, cy) = (x0 + bx.w / 2.0, y0 + bx.h / 2.0);
        match fill {
            Fill::Linear { start, end, dir } => {
                let (hw, hh) = (bx.w / 2.0 * dir.0, bx.h / 2.0 * dir.1);
                let coords = [cx - hw, cy - hh, cx + hw, cy + hh];
                self.shade(2, &coords, start, end);
            }
            Fill::Radial { start, end } if bx.w > 0.0 && bx.h > 0.0 => {
                self.b
                    .concat_matrix(bx.w / 2.0, 0.0, 0.0, bx.h / 2.0, cx, cy);
                self.shade(3, &[0.0, 0.0, 0.0, 0.0, 0.0, 1.0], start, end);
            }
            Fill::Radial { end, .. } => self.fill(bx, end),
            Fill::Pattern { bg, fg, lines } => {
                self.fill(bx, bg);
                self.hatch(x0, y0, bx.w, bx.h, fg, lines);
            }
            Fill::Solid(_) => {}
        }
        self.b.restore_state();
    }

    /// `sh` with a new axial (2) or radial (3) shading from `c0` to `c1`,
    /// extended past both ends (ISO 32000-2 §8.7.4.5.3-4, §7.10.3).
    fn shade(&mut self, ty: i64, coords: &[f64], c0: [f64; 3], c1: [f64; 3]) {
        let nums = |v: &[f64]| Object::Array(v.iter().map(|&n| Object::Real(n)).collect());
        let mut func = Dict::new();
        func.insert(Name::from(b"FunctionType"), Object::Integer(2));
        func.insert(Name::from(b"Domain"), nums(&[0.0, 1.0]));
        func.insert(Name::from(b"C0"), nums(&c0));
        func.insert(Name::from(b"C1"), nums(&c1));
        func.insert(Name::from(b"N"), Object::Integer(1));
        let mut sh = Dict::new();
        sh.insert(Name::from(b"ShadingType"), Object::Integer(ty));
        sh.insert(
            Name::from(b"ColorSpace"),
            Object::Name(Name::from(b"DeviceRGB")),
        );
        sh.insert(Name::from(b"Coords"), nums(coords));
        sh.insert(
            Name::from(b"Extend"),
            Object::Array(vec![Object::Boolean(true), Object::Boolean(true)]),
        );
        sh.insert(Name::from(b"Function"), Object::Dict(func));
        let name = format!("/XS{} sh\n", self.shadings.len());
        self.shadings.push(sh);
        self.b.append_raw(name.as_bytes());
    }

    /// Hatch lines over `w` x `h` at (`x0`, `y0`), bottom-left origin.
    fn hatch(&mut self, x0: f64, y0: f64, w: f64, h: f64, rgb: [f64; 3], lines: [bool; 4]) {
        self.b.set_line_width(HATCH_WIDTH);
        self.b.set_stroke_rgb(rgb[0], rgb[1], rgb[2]);
        let steps = |len: f64, step: f64| {
            (0..)
                .map(move |i| f64::from(i) * step)
                .take_while(move |&d| d <= len)
        };
        if lines[0] {
            for d in steps(h, HATCH_STEP) {
                self.b.move_to(x0, y0 + d);
                self.b.line_to(x0 + w, y0 + d);
            }
        }
        if lines[1] {
            for d in steps(w, HATCH_STEP) {
                self.b.move_to(x0 + d, y0);
                self.b.line_to(x0 + d, y0 + h);
            }
        }
        let diag = HATCH_STEP * std::f64::consts::SQRT_2;
        for d in steps(w + h, diag) {
            let t = d - h;
            if lines[2] {
                self.b.move_to(x0 + t, y0);
                self.b.line_to(x0 + t + h, y0 + h);
            }
            if lines[3] {
                self.b.move_to(x0 + t, y0 + h);
                self.b.line_to(x0 + t + h, y0);
            }
        }
        self.b.paint(Paint::Stroke);
    }
}
