//! `/LE` line endings (ISO 32000-2 §12.5.6.7 Table 179; 32000-1 Table 176):
//! the ten styles, read and written by name, and painted by the one builder
//! every caller shares (`/Line`, `/PolyLine`, the `/FreeText` callout).
//!
//! Table 179 states no size for any style (spec RAG `ARR-A1`). pdfcer's
//! arrowhead is [`arrow_length`] long and ±22.5° wide; the square, circle,
//! diamond, butt and slash span half that length either side of the end.
//! `OpenArrow`/`ClosedArrow` point outward, away from the shaft; the `R`
//! forms are their mirror about the endpoint.

use super::emit_ellipse;
use crate::object::{Name, Object};
use crate::writer::content::{ContentBuilder, Paint};

/// A line-ending style (`/LE`, ISO 32000-2 Table 179; 32000-1 Table 176).
/// The closed shapes (`Square`, `Circle`, `Diamond`, `ClosedArrow`,
/// `RClosedArrow`) are filled with the annotation's `/IC` when it has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    /// `/None` — no ending shape.
    None,
    /// `/Square` — a square centred on the endpoint.
    Square,
    /// `/Circle` — a circle centred on the endpoint.
    Circle,
    /// `/Diamond` — a square turned 45°, centred on the endpoint.
    Diamond,
    /// `/OpenArrow` — two short lines meeting at the endpoint. Acrobat's
    /// default at both ends (`markup__shape_and_line_annotations.md`).
    OpenArrow,
    /// `/ClosedArrow` — the `OpenArrow` closed into a triangle.
    ClosedArrow,
    /// `/Butt` (PDF 1.5) — a short line across the endpoint, perpendicular
    /// to the line.
    Butt,
    /// `/ROpenArrow` (PDF 1.5) — `OpenArrow` reversed: its barbs reach past
    /// the endpoint.
    ROpenArrow,
    /// `/RClosedArrow` (PDF 1.5) — `ClosedArrow` reversed.
    RClosedArrow,
    /// `/Slash` (PDF 1.6) — a short line across the endpoint, 30° clockwise
    /// from the perpendicular.
    Slash,
}

impl LineEnding {
    /// Every style, in Table 179's order.
    pub const ALL: [Self; 10] = [
        Self::Square,
        Self::Circle,
        Self::Diamond,
        Self::OpenArrow,
        Self::ClosedArrow,
        Self::None,
        Self::Butt,
        Self::ROpenArrow,
        Self::RClosedArrow,
        Self::Slash,
    ];

    /// The `/LE` name, e.g. `b"ROpenArrow"`.
    #[must_use]
    pub const fn name(self) -> &'static [u8] {
        match self {
            Self::None => b"None",
            Self::Square => b"Square",
            Self::Circle => b"Circle",
            Self::Diamond => b"Diamond",
            Self::OpenArrow => b"OpenArrow",
            Self::ClosedArrow => b"ClosedArrow",
            Self::Butt => b"Butt",
            Self::ROpenArrow => b"ROpenArrow",
            Self::RClosedArrow => b"RClosedArrow",
            Self::Slash => b"Slash",
        }
    }

    /// The style a `/LE` name names; `None` for a name outside Table 179.
    /// Names are case-sensitive, as PDF names are (§7.3.5).
    ///
    /// ```
    /// use pdfcer_core::annot_author::LineEnding;
    /// assert_eq!(LineEnding::from_name(b"Slash"), Some(LineEnding::Slash));
    /// assert_eq!(LineEnding::from_name(b"slash"), None);
    /// ```
    #[must_use]
    pub fn from_name(name: &[u8]) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.name() == name)
    }

    const fn is_closed(self) -> bool {
        matches!(
            self,
            Self::Square | Self::Circle | Self::Diamond | Self::ClosedArrow | Self::RClosedArrow
        )
    }

    const fn is_reversed(self) -> bool {
        matches!(self, Self::ROpenArrow | Self::RClosedArrow)
    }
}

/// One `/LE` name; anything outside Table 179 reads as `None`.
pub(super) fn line_ending_of(obj: Option<&Object>) -> LineEnding {
    match obj {
        Some(Object::Name(n)) => LineEnding::from_name(n.as_bytes()).unwrap_or(LineEnding::None),
        _ => LineEnding::None,
    }
}

/// A two-name `/LE` array (Line, PolyLine); default `[/None /None]`.
pub(super) fn endings_of(obj: Option<&Object>) -> (LineEnding, LineEnding) {
    match obj {
        Some(Object::Array(a)) if a.len() == 2 => {
            (line_ending_of(a.first()), line_ending_of(a.get(1)))
        }
        _ => (LineEnding::None, LineEnding::None),
    }
}

/// `endings` as the two-name `/LE` array `endings_of` reads back.
pub(super) fn endings_object(endings: (LineEnding, LineEnding)) -> Object {
    Object::Array(vec![
        Object::Name(Name::from(endings.0.name())),
        Object::Name(Name::from(endings.1.name())),
    ])
}

/// The arrowhead length pdfcer uses for a given line width (pdfcer's own
/// choice — the exact Acrobat scaling is undocumented). Scales with width
/// and has a floor so a thin line still gets a visible head.
fn arrow_length(width: f64) -> f64 {
    (width * 4.0).max(8.0)
}

/// How far past its endpoints a line with `endings` paints. A reversed
/// arrow's barbs reach `a·sec(22.5°)` ≈ 1.08·a past the end, plus their
/// miter; every other style stays within `a` (square corner 0.71·a plus
/// a stroke of at most a/4).
pub(super) fn ending_margin(width: f64, endings: (LineEnding, LineEnding)) -> f64 {
    let a = arrow_length(width);
    if endings.0.is_reversed() || endings.1.is_reversed() {
        a * 1.1 + width
    } else {
        (width / 2.0).max(a)
    }
}

/// tan(22.5°): the arrowhead's half-width per unit of length.
const TAN_22_5: f64 = 0.414_213_562_373_095;
/// cos 30° and sin 30°, for `/Slash`.
const COS_30: f64 = 0.866_025_403_784_438_6;
const SIN_30: f64 = 0.5;

/// An endpoint's frame: `u` points from the shaft out through the tip, `p`
/// is `u` turned 90° counter-clockwise.
struct EndFrame {
    tip: (f64, f64),
    u: (f64, f64),
    p: (f64, f64),
}

impl EndFrame {
    fn new(tip: (f64, f64), other: (f64, f64)) -> Option<Self> {
        let (dx, dy) = (tip.0 - other.0, tip.1 - other.1);
        let len = dx.hypot(dy);
        if len < f64::EPSILON {
            return None; // zero-length line: no direction
        }
        let u = (dx / len, dy / len);
        Some(Self {
            tip,
            u,
            p: (-u.1, u.0),
        })
    }

    /// The point `along` the line outward and `across` it.
    fn at(&self, along: f64, across: f64) -> (f64, f64) {
        (
            self.tip.0 + self.u.0 * along + self.p.0 * across,
            self.tip.1 + self.u.1 * along + self.p.1 * across,
        )
    }
}

fn polygon(b: &mut ContentBuilder, points: &[(f64, f64)], paint: Paint) {
    if let Some((first, rest)) = points.split_first() {
        b.move_to(first.0, first.1);
        for p in rest {
            b.line_to(p.0, p.1);
        }
        b.close_subpath();
        b.paint(paint);
    }
}

fn segment(b: &mut ContentBuilder, from: (f64, f64), to: (f64, f64)) {
    b.move_to(from.0, from.1);
    b.line_to(to.0, to.1);
    b.paint(Paint::Stroke);
}

/// An arrowhead with its tip at the endpoint; `dir` 1 points it outward
/// (`OpenArrow`), −1 inward (`ROpenArrow`).
fn arrow(b: &mut ContentBuilder, f: &EndFrame, a: f64, dir: f64, closed: Option<Paint>) {
    let half = a * TAN_22_5;
    let barb1 = f.at(-dir * a, half);
    let barb2 = f.at(-dir * a, -half);
    match closed {
        Some(paint) => polygon(b, &[f.tip, barb1, barb2], paint),
        None => {
            b.move_to(barb1.0, barb1.1);
            b.line_to(f.tip.0, f.tip.1);
            b.line_to(barb2.0, barb2.1);
            b.paint(Paint::Stroke);
        }
    }
}

/// Emit one line ending at `tip`, the shaft arriving from `other`. The
/// closed styles are filled in the current fill colour when `filled` (the
/// caller set `/IC`), stroked only otherwise; the rest are stroked.
pub(super) fn emit_line_ending(
    b: &mut ContentBuilder,
    tip: (f64, f64),
    other: (f64, f64),
    ending: LineEnding,
    width: f64,
    filled: bool,
) {
    let Some(f) = EndFrame::new(tip, other) else {
        return;
    };
    let a = arrow_length(width);
    let h = a / 2.0;
    let paint = if filled && ending.is_closed() {
        Paint::FillStroke
    } else {
        Paint::Stroke
    };
    match ending {
        LineEnding::None => {}
        LineEnding::OpenArrow => arrow(b, &f, a, 1.0, None),
        LineEnding::ClosedArrow => arrow(b, &f, a, 1.0, Some(paint)),
        LineEnding::ROpenArrow => arrow(b, &f, a, -1.0, None),
        LineEnding::RClosedArrow => arrow(b, &f, a, -1.0, Some(paint)),
        LineEnding::Square => polygon(
            b,
            &[f.at(-h, -h), f.at(h, -h), f.at(h, h), f.at(-h, h)],
            paint,
        ),
        LineEnding::Diamond => polygon(
            b,
            &[f.at(h, 0.0), f.at(0.0, h), f.at(-h, 0.0), f.at(0.0, -h)],
            paint,
        ),
        LineEnding::Circle => {
            emit_ellipse(b, tip.0, tip.1, h, h);
            b.paint(paint);
        }
        LineEnding::Butt => segment(b, f.at(0.0, h), f.at(0.0, -h)),
        LineEnding::Slash => {
            // `p` turned 30° clockwise.
            let v = (
                f.p.0 * COS_30 + f.p.1 * SIN_30,
                -f.p.0 * SIN_30 + f.p.1 * COS_30,
            );
            segment(
                b,
                (tip.0 - v.0 * h, tip.1 - v.1 * h),
                (tip.0 + v.0 * h, tip.1 + v.1 * h),
            );
        }
    }
}
