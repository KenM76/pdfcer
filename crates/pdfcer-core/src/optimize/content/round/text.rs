//! Text line starts rounded within the tolerance (ISO 32000-2 §9.4.2
//! Table 108): a `Tm`'s translation `e f`, and each `Td`'s offset.
//!
//! A `Td` is relative to the line before it, so its rounding error would
//! add up down a page. Each one is rounded toward where the line truly
//! starts (its own offset plus the error the lines before it left), and the
//! check is on that absolute position: through `Tlm`'s linear part, then
//! the CTM. Glyph advances only translate `Tm`, so the error a line start
//! carries is the error of every glyph on the line.
//!
//! `TD` is left as written: its `ty` also sets the leading every later `T*`,
//! `'` and `"` use. `q`, `Q` or `cm` inside a text object (§9.4.1 forbids
//! them) un-rounds the object.

use super::{Linear, MAX_DECIMALS, Rounded, candidate};
use crate::content::ContentToken;
use crate::optimize::content::rewrite::number;

/// One text object's rounding so far.
pub(super) struct Text {
    /// Linear part of `Tlm`.
    tlm: Linear,
    /// Where the last `Tm` put the origin minus where it was written, before
    /// the CTM; the same sense as `carry`.
    tm_err: (f64, f64),
    /// The original `Td` offsets since that `Tm` minus the written ones, in
    /// text space.
    carry: (f64, f64),
    /// Tokens rounded in this object.
    slots: Vec<usize>,
    /// The farthest a line start moved, in default user space.
    max: f64,
}

/// An operation `(op, (buf, first, operands))`.
type Op<'a> = (&'a [u8], (&'a [u8], usize, &'a [ContentToken]));

/// Track `op` in the current text object, rounding what it positions.
pub(super) fn step(
    text: &mut Option<Text>,
    out: &mut Rounded,
    (op, at): Op,
    ctm: Linear,
    tol: f64,
) {
    match op {
        b"BT" => {
            end(text.take(), out);
            *text = Some(Text {
                tlm: Linear::IDENTITY,
                tm_err: (0.0, 0.0),
                carry: (0.0, 0.0),
                slots: Vec::new(),
                max: 0.0,
            });
        }
        b"ET" => end(text.take(), out),
        b"q" | b"Q" | b"cm" => abandon(text.take(), out),
        b"Tm" | b"Td" => {
            let done = text.as_mut().map(|t| match op {
                b"Tm" => t.tm(out, at, ctm, tol),
                _ => t.td(out, at, ctm, tol),
            });
            if done == Some(false) {
                abandon(text.take(), out);
            }
        }
        _ => {}
    }
}

/// Close a text object: its farthest movement joins the report.
pub(super) fn end(text: Option<Text>, out: &mut Rounded) {
    if let Some(t) = text {
        out.max = out.max.max(t.max);
    }
}

/// Undo a text object's roundings.
fn abandon(text: Option<Text>, out: &mut Rounded) {
    for i in text.map(|t| t.slots).unwrap_or_default() {
        if let Some(slot) = out.text.get_mut(i) {
            *slot = None;
        }
    }
}

impl Text {
    /// How far a line start is from where it belongs, after `carry`.
    fn error(&self, carry: (f64, f64), ctm: Linear) -> f64 {
        let (x, y) = self.tlm.apply(carry.0, carry.1);
        ctm.length(self.tm_err.0 + x, self.tm_err.1 + y)
    }

    /// `a b c d e f Tm`: round `e f`; `false` when the operands are not six
    /// numbers.
    fn tm(
        &mut self,
        out: &mut Rounded,
        (buf, first, operands): (&[u8], usize, &[ContentToken]),
        ctm: Linear,
        tol: f64,
    ) -> bool {
        let Some([a, b, c, d, e, f]) = numbers(operands) else {
            return false;
        };
        self.tlm = Linear { a, b, c, d };
        self.carry = (0.0, 0.0);
        self.tm_err = (0.0, 0.0);
        let (se, sf) = (source(buf, operands, 4), source(buf, operands, 5));
        for decimals in 0..=MAX_DECIMALS {
            let chosen = [(e, se), (f, sf)].map(|(x, s)| candidate(x, x, s, decimals));
            let [ce, cf] = &chosen;
            if ce.is_none() && cf.is_none() {
                continue;
            }
            let moved = |c: &Option<(f64, String)>, x: f64| c.as_ref().map_or(0.0, |(r, _)| x - r);
            let delta = (moved(ce, e), moved(cf, f));
            let err = ctm.length(delta.0, delta.1);
            if err <= tol {
                self.tm_err = delta;
                self.write(out, first + 4, chosen, err);
                return true;
            }
        }
        true
    }

    /// `tx ty Td`: round toward the true line start; `false` when the
    /// operands are not two numbers.
    fn td(
        &mut self,
        out: &mut Rounded,
        (buf, first, operands): (&[u8], usize, &[ContentToken]),
        ctm: Linear,
        tol: f64,
    ) -> bool {
        let Some([tx, ty]) = numbers(operands) else {
            return false;
        };
        let (sx, sy) = (source(buf, operands, 0), source(buf, operands, 1));
        let (cx, cy) = self.carry;
        for decimals in 0..=MAX_DECIMALS {
            let chosen =
                [(tx, cx, sx), (ty, cy, sy)].map(|(x, c, s)| candidate(x + c, x, s, decimals));
            let [rx, ry] = &chosen;
            if rx.is_none() && ry.is_none() {
                continue;
            }
            let left = |r: &Option<(f64, String)>, x: f64, c: f64| {
                c + r.as_ref().map_or(0.0, |(r, _)| x - r)
            };
            let carry = (left(rx, tx, cx), left(ry, ty, cy));
            let err = self.error(carry, ctm);
            if err <= tol {
                self.carry = carry;
                self.write(out, first, chosen, err);
                return true;
            }
        }
        true
    }

    /// Write the chosen spellings of two operands starting at token `at`.
    fn write(
        &mut self,
        out: &mut Rounded,
        at: usize,
        chosen: [Option<(f64, String)>; 2],
        err: f64,
    ) {
        for (k, c) in chosen.into_iter().enumerate() {
            if let (Some((_, text)), Some(slot)) = (c, out.text.get_mut(at + k)) {
                *slot = Some(text.into_bytes());
                self.slots.push(at + k);
            }
        }
        self.max = self.max.max(err);
    }
}

/// Exactly `N` number operands.
fn numbers<const N: usize>(operands: &[ContentToken]) -> Option<[f64; N]> {
    let v: Vec<f64> = operands.iter().map(number).collect::<Option<_>>()?;
    v.try_into().ok()
}

fn source<'a>(buf: &'a [u8], operands: &[ContentToken], k: usize) -> &'a [u8] {
    operands
        .get(k)
        .and_then(|t| t.span.slice(buf))
        .unwrap_or_default()
}
