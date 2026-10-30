//! 3-D edge strokes as grey bevels (XFA 3.3 edge `stroke` pp.688-689 give
//! the intent only; the bands and greys are the PDFium witness's, gap
//! G-CB11, slice `adobe_ext__xfa_checkbutton_exclgroup_paint.md` §3).

use crate::writer::content::{LineCap, Paint};

use super::{LayoutRect, Painter};

/// A 3-D `stroke` value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ThreeD {
    Lowered,
    Raised,
    Etched,
    Embossed,
}

impl ThreeD {
    /// The 3-D kind an edge's `stroke` names, or `None` for any other.
    pub(super) fn from_stroke(s: &str) -> Option<Self> {
        match s {
            "lowered" => Some(Self::Lowered),
            "raised" => Some(Self::Raised),
            "etched" => Some(Self::Etched),
            "embossed" => Some(Self::Embossed),
            _ => None,
        }
    }
}

const BLACK: [f64; 3] = [0.0; 3];
const WHITE: [f64; 3] = [1.0; 3];
const DARK: [f64; 3] = [0.25; 3];
const GREY: [f64; 3] = [128.0 / 255.0; 3];
const LIGHT: [f64; 3] = [192.0 / 255.0; 3];

impl Painter<'_> {
    /// `kind` of thickness `t` drawn inside `r`: an outer step (a black ring
    /// or a full-width bevel) and an inner half-width bevel.
    pub(super) fn bevel_rect(&mut self, r: LayoutRect, kind: ThreeD, t: f64) {
        let h = t / 2.0;
        let inner = deflate(r, h);
        match kind {
            ThreeD::Lowered | ThreeD::Raised => self.ring(r, h),
            ThreeD::Etched => self.bands(r, t, GREY, WHITE),
            ThreeD::Embossed => self.bands(r, t, GREY, BLACK),
        }
        let (tl, br) = match kind {
            ThreeD::Lowered => (GREY, LIGHT),
            ThreeD::Raised | ThreeD::Etched => (WHITE, GREY),
            ThreeD::Embossed => (BLACK, GREY),
        };
        self.bands(inner, h, tl, br);
    }

    /// A black band `w` wide just inside `r`.
    fn ring(&mut self, r: LayoutRect, w: f64) {
        let b = deflate(r, w / 2.0);
        self.b.set_line_width(w);
        self.b.set_stroke_rgb(0.0, 0.0, 0.0);
        self.b.rect(b.x, self.height - b.y - b.h, b.w, b.h);
        self.b.paint(Paint::Stroke);
    }

    /// Bands `w` wide inside `r`: `tl` over the top and left, `br` over the
    /// bottom and right, split on the top-right and bottom-left diagonals.
    fn bands(&mut self, r: LayoutRect, w: f64, tl: [f64; 3], br: [f64; 3]) {
        if r.w <= 0.0 || r.h <= 0.0 {
            return;
        }
        let w = w.min(r.w / 2.0).min(r.h / 2.0);
        let (x0, x1) = (r.x, r.x + r.w);
        let (top, bottom) = (self.height - r.y, self.height - r.y - r.h);
        let tl_pts = [
            (x0, top),
            (x1, top),
            (x1 - w, top - w),
            (x0 + w, top - w),
            (x0 + w, bottom + w),
            (x0, bottom),
        ];
        let br_pts = [
            (x1, top),
            (x1, bottom),
            (x0, bottom),
            (x0 + w, bottom + w),
            (x1 - w, bottom + w),
            (x1 - w, top - w),
        ];
        self.polygon(&tl_pts, tl);
        self.polygon(&br_pts, br);
    }

    fn polygon(&mut self, pts: &[(f64, f64)], rgb: [f64; 3]) {
        let Some((&(x, y), rest)) = pts.split_first() else {
            return;
        };
        self.b.set_fill_rgb(rgb[0], rgb[1], rgb[2]);
        self.b.move_to(x, y);
        for &(x, y) in rest {
            self.b.line_to(x, y);
        }
        self.b.close_subpath();
        self.b.paint(Paint::Fill);
    }

    /// A round box's 3-D outline, every kind drawn as `lowered` (the
    /// witness): two `t/2` rings inside radius `r` about `(cx, cy)`, each a
    /// darker upper-left and a lighter lower-right half.
    pub(super) fn bevel_circle(&mut self, cx: f64, cy: f64, r: f64, t: f64) {
        let h = t / 2.0;
        self.b.set_line_width(h);
        self.b.set_line_cap(LineCap::Butt);
        for (radius, upper, lower) in [(r - h / 2.0, GREY, WHITE), (r - 1.5 * h, DARK, LIGHT)] {
            if radius <= 0.0 {
                continue;
            }
            self.half_arc(cx, cy, radius, 45.0, upper);
            self.half_arc(cx, cy, radius, 225.0, lower);
        }
    }

    /// The half circle from `start` degrees anticlockwise, stroked in `rgb`.
    fn half_arc(&mut self, cx: f64, cy: f64, r: f64, start: f64, rgb: [f64; 3]) {
        self.b.set_stroke_rgb(rgb[0], rgb[1], rgb[2]);
        let k = r * 0.552_284_749_8;
        let at = |deg: f64| {
            let a = deg.to_radians();
            (a.cos(), a.sin())
        };
        let (c0, s0) = at(start);
        self.b.move_to(cx + r * c0, cy + r * s0);
        for q in 0..2 {
            let (ca, sa) = at(start + 90.0 * f64::from(q));
            let (cb, sb) = at(start + 90.0 * f64::from(q + 1));
            self.b.curve_to(
                cx + r * ca - k * sa,
                cy + r * sa + k * ca,
                cx + r * cb + k * sb,
                cy + r * sb - k * cb,
                cx + r * cb,
                cy + r * sb,
            );
        }
        self.b.paint(Paint::Stroke);
    }
}

fn deflate(r: LayoutRect, d: f64) -> LayoutRect {
    LayoutRect {
        x: r.x + d,
        y: r.y + d,
        w: r.w - 2.0 * d,
        h: r.h - 2.0 * d,
    }
}
