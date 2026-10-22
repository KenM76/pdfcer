//! The scale a form XObject or tiling pattern starts at, from the streams
//! that invoke it: a form's `/Matrix` maps form space into the space in
//! force at its `Do` (ISO 32000-2 §8.10.1), and a pattern's `/Matrix` maps
//! pattern space into the default space of the stream whose resources
//! name it (§8.7.3.1).

use super::{Linear, State};
use crate::content::{ContentStream, ContentTokenKind};
use crate::object::Object;
use crate::optimize::content::Scale;
use crate::optimize::content::rewrite::operations;

/// The linear map from a stream's own space to default user space.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Base(Linear);

impl Base {
    /// Page content: its space is default user space.
    pub(crate) const PAGE: Self = Self(Linear::IDENTITY);

    /// The base of a stream whose `/Matrix` is `m` drawn under `self`; an
    /// absent `/Matrix` is the identity, one that is not six numbers gives
    /// `None`.
    pub(crate) fn under(self, m: Option<&Object>) -> Option<Self> {
        let Some(m) = m else {
            return Some(self);
        };
        let Object::Array(items) = m else {
            return None;
        };
        let v: Vec<f64> = items.iter().map(Object::as_number).collect::<Option<_>>()?;
        let [a, b, c, d, _, _] = v.as_slice() else {
            return None;
        };
        Some(Self(
            Linear {
                a: *a,
                b: *b,
                c: *c,
                d: *d,
            }
            .then(self.0),
        ))
    }

    /// The most a move in the stream's space stretches in default space.
    pub(crate) fn scale(self) -> Option<Scale> {
        Scale::new(self.0.stretch())
    }

    /// A hashable key, so a stream reached twice under one base is walked
    /// once.
    pub(crate) fn key(self) -> [u64; 4] {
        [self.0.a, self.0.b, self.0.c, self.0.d].map(f64::to_bits)
    }
}

/// What one run of streams invokes.
#[derive(Debug, Default)]
pub(crate) struct Invoked {
    /// Each `Do`'s name and the base in force there; `None` after a `cm`
    /// or `Q` whose effect is unknown.
    pub(crate) forms: Vec<(Vec<u8>, Option<Base>)>,
    /// `false` when a stream did not decode, so it may invoke names not
    /// listed.
    pub(crate) complete: bool,
}

/// The `Do`s in `streams`, run in order from `base` as if concatenated
/// (page content, §7.7.3.3 Table 30; a form is one stream).
pub(crate) fn invoked(base: Base, streams: &[Option<&ContentStream>]) -> Invoked {
    let mut out = Invoked {
        forms: Vec::new(),
        complete: true,
    };
    let mut state = Some(State::new());
    for content in streams {
        let Some(content) = content else {
            out.complete = false;
            state = None;
            continue;
        };
        let buf = content.buf.as_slice();
        for (_, _, operands, op) in operations(content) {
            let op = op.and_then(|t| t.span.slice(buf)).unwrap_or_default();
            if op == b"Do" {
                if let [tok] = operands
                    && let ContentTokenKind::Operand(Object::Name(n)) = &tok.kind
                {
                    let at = state.as_ref().map(|s| Base(s.ctm.then(base.0)));
                    out.forms.push((n.as_bytes().to_vec(), at));
                } else {
                    out.complete = false;
                }
            } else if state.as_mut().is_some_and(|s| !s.step(op, operands)) {
                state = None;
            }
        }
    }
    out
}
