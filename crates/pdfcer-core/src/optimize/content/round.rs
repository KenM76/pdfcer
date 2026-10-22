//! `round`: path coordinates rounded within a tolerance measured in default
//! user space, through the `cm` matrices in force (ISO 32000-2 §8.4.4
//! Table 56: `cm` concatenates a matrix onto the CTM; §8.3.2.3).

use super::rewrite::{number, operations, real};
use super::{Tolerance, Unrounded};
use crate::content::{ContentStream, ContentToken};

/// Annex C: a reader keeps about five fractional digits of a real.
const MAX_DECIMALS: i32 = 5;

/// The rounded spelling of each token (`None` keeps it), and the report.
pub(super) struct Rounded {
    pub(super) text: Vec<Option<Vec<u8>>>,
    pub(super) count: u64,
    pub(super) max: f64,
    pub(super) unrounded: Option<Unrounded>,
}

/// The linear part `[a b c d]` of a transformation matrix; the translation
/// moves every point alike, so a displacement never sees it.
#[derive(Debug, Clone, Copy)]
struct Linear {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
}

impl Linear {
    const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
    };

    /// `self` then `t`, for row vectors (`p × self × t`).
    fn then(self, t: Self) -> Self {
        Self {
            a: self.a * t.a + self.b * t.c,
            b: self.a * t.b + self.b * t.d,
            c: self.c * t.a + self.d * t.c,
            d: self.c * t.b + self.d * t.d,
        }
    }

    /// How far a move of `(dx, dy)` goes after the transformation.
    fn length(self, dx: f64, dy: f64) -> f64 {
        (self.a * dx + self.c * dy).hypot(self.b * dx + self.d * dy)
    }
}

/// The operands a path-construction operator takes.
#[derive(Debug, Clone, Copy)]
enum Shape {
    /// `n` points, two numbers each.
    Points(usize),
    /// `x y width height`.
    Rect,
}

fn shape(op: &[u8]) -> Option<Shape> {
    Some(match op {
        b"m" | b"l" => Shape::Points(1),
        b"v" | b"y" => Shape::Points(2),
        b"c" => Shape::Points(3),
        b"re" => Shape::Rect,
        _ => return None,
    })
}

/// Plan the rounding of `content` within `tol`; `page_start` says it begins
/// in default user space.
pub(super) fn plan(content: &ContentStream, tol: Option<Tolerance>, page_start: bool) -> Rounded {
    let mut out = Rounded {
        text: vec![None; content.tokens.len()],
        count: 0,
        max: 0.0,
        unrounded: None,
    };
    let Some(tol) = tol else {
        return out;
    };
    if !page_start {
        out.unrounded = Some(Unrounded::NotPageStart);
        return out;
    }
    let buf = content.buf.as_slice();
    let mut ctm = Linear::IDENTITY;
    let mut stack = Vec::new();
    for (first, _, operands, op) in operations(content) {
        let Some(op) = op.and_then(|t| t.span.slice(buf)) else {
            continue;
        };
        let known = match op {
            b"q" => {
                stack.push(ctm);
                true
            }
            b"Q" => stack.pop().map(|m| ctm = m).is_some(),
            b"cm" => matrix(operands).map(|m| ctm = m.then(ctm)).is_some(),
            _ => {
                if let Some(shape) = shape(op) {
                    round_op(&mut out, (buf, first, operands), shape, ctm, tol.as_units());
                }
                true
            }
        };
        if !known {
            out.unrounded = Some(Unrounded::UnknownMatrix);
            break;
        }
    }
    out
}

/// The linear part of a `cm`'s six number operands.
fn matrix(operands: &[ContentToken]) -> Option<Linear> {
    let v: Vec<f64> = operands.iter().map(number).collect::<Option<_>>()?;
    match v.as_slice() {
        [a, b, c, d, _, _] => Some(Linear {
            a: *a,
            b: *b,
            c: *c,
            d: *d,
        }),
        _ => None,
    }
}

/// Round one operation's coordinates to the fewest decimals that keep every
/// point (every corner of a `re`) within `tol`.
fn round_op(
    out: &mut Rounded,
    (buf, first, operands): (&[u8], usize, &[ContentToken]),
    shape: Shape,
    ctm: Linear,
    tol: f64,
) {
    let want = match shape {
        Shape::Points(n) => 2 * n,
        Shape::Rect => 4,
    };
    let Some(values) = operands.iter().map(number).collect::<Option<Vec<f64>>>() else {
        return;
    };
    if values.len() != want {
        return;
    }
    for decimals in 0..=MAX_DECIMALS {
        let chosen: Vec<Option<(f64, String)>> = values
            .iter()
            .zip(operands)
            .map(|(&v, t)| candidate(v, t.span.slice(buf).unwrap_or_default(), decimals))
            .collect();
        let delta: Vec<f64> = values
            .iter()
            .zip(&chosen)
            .map(|(v, c)| c.as_ref().map_or(0.0, |(r, _)| r - v))
            .collect();
        let moved = displacement(shape, &delta, ctm);
        if moved <= tol {
            for (k, c) in chosen.into_iter().enumerate() {
                if let (Some((_, text)), Some(slot)) = (c, out.text.get_mut(first + k)) {
                    *slot = Some(text.into_bytes());
                }
            }
            out.max = out.max.max(moved);
            return;
        }
    }
}

/// `v` rounded to `decimals`, when that is another value with a shorter
/// spelling than `source`.
fn candidate(v: f64, source: &[u8], decimals: i32) -> Option<(f64, String)> {
    let p = 10f64.powi(decimals);
    let r = (v * p).round() / p;
    if !r.is_finite() || r == v {
        return None;
    }
    let text = real(r)?;
    let value = text.parse::<f64>().ok()?;
    (text.len() < source.len() && value != v).then_some((value, text))
}

/// The farthest point an operation's coordinate changes `delta` move.
fn displacement(shape: Shape, delta: &[f64], ctm: Linear) -> f64 {
    match (shape, delta) {
        (Shape::Rect, [dx, dy, dw, dh]) => [(0.0, 0.0), (*dw, 0.0), (0.0, *dh), (*dw, *dh)]
            .iter()
            .map(|(w, h)| ctm.length(dx + w, dy + h))
            .fold(0.0, f64::max),
        _ => delta
            .chunks(2)
            .map(|p| match p {
                [dx, dy] => ctm.length(*dx, *dy),
                _ => 0.0,
            })
            .fold(0.0, f64::max),
    }
}
