//! The token-level rewrite behind `content(...)`: which operators go, how
//! numbers are spelled, and where separators fall.

use super::{ContentSkip, Numbers, ResolvedContent, Whitespace, check, round};
use crate::content::{ContentStream, ContentToken, ContentTokenKind};
use crate::lexer::{Lexer, TokenKind, is_regular};
use crate::object::Object;
use crate::optimize::SkipReason;

/// A rewritten stream and what changed in it.
pub(super) struct Compacted {
    pub(super) bytes: Vec<u8>,
    pub(super) removed: u64,
    pub(super) shortened: u64,
    /// Its `text` is emptied; `count` is what was written.
    pub(super) rounded: round::Rounded,
}

/// Rewrite decoded content `buf` under `settings`, checked by re-reading.
pub(super) fn compact(buf: &[u8], settings: ResolvedContent) -> Result<Compacted, SkipReason> {
    let content =
        ContentStream::parse(buf.to_vec()).map_err(|e| SkipReason::Undecodable(e.to_string()))?;
    let mut keep = vec![true; content.tokens.len()];
    let removed = if settings.redundant {
        mark_redundant(&content, &mut keep)
    } else {
        0
    };
    let mut rounded = round::plan(&content, settings.round, settings.start);
    let (bytes, shortened, written) = emit(&content, (&keep, &rounded.text), settings);
    if !check::same(&content, (&keep, &rounded.text), &bytes) {
        return Err(SkipReason::Content(ContentSkip::Mismatch));
    }
    rounded.text = Vec::new();
    rounded.count = written;
    Ok(Compacted {
        bytes,
        removed,
        shortened,
        rounded,
    })
}

/// Operators that change none of the state [`State`] tracks.
const NEUTRAL: &[&[u8]] = &[
    b"m", b"l", b"c", b"v", b"y", b"h", b"re", b"S", b"s", b"f", b"F", b"f*", b"B", b"B*", b"b",
    b"b*", b"n", b"W", b"W*", b"BT", b"ET", b"Td", b"TD", b"Tm", b"T*", b"Tj", b"TJ", b"'", b"BMC",
    b"BDC", b"EMC", b"MP", b"DP", b"Do", b"sh", b"BX", b"EX", b"cm",
];

/// Setters that hold one value each (ISO 32000-2 Tables 56 and 103).
const SETTERS: &[&[u8]] = &[
    b"w", b"J", b"j", b"M", b"d", b"ri", b"i", b"Tc", b"Tw", b"Tz", b"TL", b"Ts", b"Tr", b"Tf",
];
const FILL: &[&[u8]] = &[b"g", b"rg", b"k", b"cs", b"sc", b"scn"];
const STROKE: &[&[u8]] = &[b"G", b"RG", b"K", b"CS", b"SC", b"SCN"];

/// What each tracked slot is known to hold: the operator that last set it
/// and that operator's operands. A slot absent is unknown.
#[derive(Clone, Default)]
struct State<'a> {
    slots: Vec<(&'a [u8], &'a [u8], &'a [ContentToken])>,
}

impl<'a> State<'a> {
    fn holds(&self, slot: &[u8], op: &[u8], operands: &[ContentToken]) -> bool {
        self.slots
            .iter()
            .any(|(s, o, a)| *s == slot && *o == op && same_operands(a, operands))
    }

    fn set(&mut self, slot: &'a [u8], op: &'a [u8], operands: &'a [ContentToken]) {
        self.slots.retain(|(s, _, _)| *s != slot);
        self.slots.push((slot, op, operands));
    }
}

/// The slot `op` sets, if it is a tracked setter.
fn slot_of(op: &[u8]) -> Option<&'static [u8]> {
    if FILL.contains(&op) {
        Some(b"fill")
    } else if STROKE.contains(&op) {
        Some(b"stroke")
    } else {
        SETTERS.iter().copied().find(|s| *s == op)
    }
}

/// Mark the redundant operators (and their operands) in `keep`; the count.
fn mark_redundant(content: &ContentStream, keep: &mut [bool]) -> u64 {
    let buf = content.buf.as_slice();
    let mut state = State::default();
    let mut stack: Vec<State<'_>> = Vec::new();
    // Bare `q`s with nothing kept after them yet.
    let mut open: Vec<usize> = Vec::new();
    let mut removed = 0;
    for (first, i, operands, op) in operations(content) {
        let Some(op) = op.and_then(|t| t.span.slice(buf)) else {
            open.clear();
            continue;
        };
        let discard = match op {
            b"q" => {
                // A `q` with operands still pushes, or a later `Q` would pop
                // the wrong level.
                stack.push(state.clone());
                if operands.is_empty() {
                    open.push(i);
                } else {
                    open.clear();
                }
                false
            }
            b"Q" => {
                state = stack.pop().unwrap_or_default();
                match open.pop().filter(|_| operands.is_empty()) {
                    Some(q) => {
                        unkeep(keep, q..q + 1);
                        removed += 1;
                        true
                    }
                    None => {
                        open.clear();
                        false
                    }
                }
            }
            b"cm" if is_identity(operands) => true,
            _ => {
                let repeat = match slot_of(op) {
                    Some(slot) if state.holds(slot, op, operands) => true,
                    Some(slot) => {
                        state.set(slot, op, operands);
                        false
                    }
                    None => {
                        if !NEUTRAL.contains(&op) {
                            state = State::default();
                        }
                        false
                    }
                };
                if !repeat {
                    open.clear();
                }
                repeat
            }
        };
        if discard {
            unkeep(keep, first..i + 1);
            removed += 1;
        }
    }
    removed
}

/// Each operation: its first token index, its operator's index, its
/// operands, and the operator token (`None` for an inline image).
pub(super) fn operations(
    content: &ContentStream,
) -> impl Iterator<Item = (usize, usize, &[ContentToken], Option<&ContentToken>)> {
    let mut start = 0;
    content
        .tokens
        .iter()
        .enumerate()
        .filter_map(move |(i, tok)| {
            if matches!(tok.kind, ContentTokenKind::Operand(_)) {
                return None;
            }
            let first = start;
            start = i + 1;
            let operands = content.tokens.get(first..i).unwrap_or_default();
            let op = matches!(tok.kind, ContentTokenKind::Operator).then_some(tok);
            Some((first, i, operands, op))
        })
}

fn unkeep(keep: &mut [bool], range: std::ops::Range<usize>) {
    if let Some(slots) = keep.get_mut(range) {
        slots.iter_mut().for_each(|k| *k = false);
    }
}

fn is_identity(operands: &[ContentToken]) -> bool {
    let want = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    operands.len() == 6
        && operands
            .iter()
            .zip(want)
            .all(|(t, w)| number(t).is_some_and(|v| v == w))
}

/// The value of a numeric operand token.
pub(super) fn number(t: &ContentToken) -> Option<f64> {
    match &t.kind {
        ContentTokenKind::Operand(o) => o.as_number(),
        _ => None,
    }
}

/// Operand lists equal by value: numbers compare as numbers.
fn same_operands(a: &[ContentToken], b: &[ContentToken]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| match (&x.kind, &y.kind) {
            (ContentTokenKind::Operand(p), ContentTokenKind::Operand(q)) => same_object(p, q),
            _ => false,
        })
}

/// Objects equal by value: numbers as numbers (`1` is `1.0`), arrays
/// element-wise, anything else byte-equal.
pub(super) fn same_object(a: &Object, b: &Object) -> bool {
    match (a, b) {
        (Object::Array(p), Object::Array(q)) => {
            p.len() == q.len() && p.iter().zip(q).all(|(x, y)| same_object(x, y))
        }
        _ => match (a.as_number(), b.as_number()) {
            (Some(x), Some(y)) => x == y,
            _ => a == b,
        },
    }
}

/// The kept tokens written out, with `rounded` spellings where set; how
/// many numbers got shorter and how many were rounded.
fn emit(
    content: &ContentStream,
    (keep, rounded): (&[bool], &[Option<Vec<u8>>]),
    s: ResolvedContent,
) -> (Vec<u8>, u64, u64) {
    let buf = content.buf.as_slice();
    let minimal = s.whitespace == Whitespace::Minimal;
    let mut out = Vec::with_capacity(buf.len());
    let mut shortened = 0;
    let mut written = 0;
    // End of the previous token, kept or not: a removed token's gap goes
    // with it.
    let mut prev_end = 0;
    let mut after_op = false;
    if minimal && content.tokens.first().is_some_and(|t| t.span.start > 0) {
        out.push(b'\n');
    }
    for (i, (tok, &kept)) in content.tokens.iter().zip(keep).enumerate() {
        let gap = buf.get(prev_end..tok.span.start).unwrap_or_default();
        prev_end = tok.span.end();
        if !kept {
            continue;
        }
        let source = tok.span.slice(buf).unwrap_or_default();
        let round = rounded.get(i).and_then(Option::as_deref);
        let short = match &tok.kind {
            ContentTokenKind::Operand(o) if round.is_none() => respell(o, source, s),
            _ => None,
        };
        let text = round
            .or(short.as_ref().map(|(t, _)| t.as_slice()))
            .unwrap_or(source);
        shortened += short.as_ref().map_or(0, |(_, n)| *n);
        written += u64::from(round.is_some());
        if !minimal {
            out.extend_from_slice(gap);
        }
        let needs = out
            .last()
            .zip(text.first())
            .is_some_and(|(&a, &b)| is_regular(a) && is_regular(b));
        if needs {
            out.push(if after_op { b'\n' } else { b' ' });
        }
        out.extend_from_slice(text);
        after_op = !matches!(tok.kind, ContentTokenKind::Operand(_));
        if minimal && matches!(tok.kind, ContentTokenKind::InlineImage { .. }) {
            // Readers find `EI` by the white-space after it.
            out.push(b'\n');
        }
    }
    let tail = buf.get(prev_end..).unwrap_or_default();
    if !minimal {
        out.extend_from_slice(tail);
    } else if !tail.is_empty() && out.last() != Some(&b'\n') {
        out.push(b'\n');
    }
    (out, shortened, written)
}

/// A shorter spelling of operand `o` under `s`, and how many numbers in it
/// got shorter.
fn respell(o: &Object, source: &[u8], s: ResolvedContent) -> Option<(Vec<u8>, u64)> {
    match o {
        Object::Array(_) | Object::Dict(_) => composite(source, s),
        _ if s.numbers == Numbers::Shortest => shortest(o, source).map(|t| (t, 1)),
        _ => None,
    }
}

/// An array or dictionary re-spelled token by token: numbers shortest,
/// separators minimal, anything else byte for byte. `None` when not shorter
/// or not lexable.
fn composite(source: &[u8], s: ResolvedContent) -> Option<(Vec<u8>, u64)> {
    let minimal = s.whitespace == Whitespace::Minimal;
    let mut lexer = Lexer::new(source);
    let mut out = Vec::with_capacity(source.len());
    let mut count = 0;
    let mut prev_end = 0;
    while let Some(tok) = lexer.next_token().ok()? {
        let lexeme = tok.lexeme(source)?;
        let number = match tok.kind {
            TokenKind::Integer(i) => Some(Object::Integer(i)),
            TokenKind::Real(r) => Some(Object::Real(r)),
            _ => None,
        };
        let short = number
            .filter(|_| s.numbers == Numbers::Shortest)
            .and_then(|n| shortest(&n, lexeme));
        count += u64::from(short.is_some());
        let text = short.as_deref().unwrap_or(lexeme);
        if minimal {
            let needs = out
                .last()
                .zip(text.first())
                .is_some_and(|(&a, &b)| is_regular(a) && is_regular(b));
            if needs {
                out.push(b' ');
            }
        } else {
            out.extend_from_slice(source.get(prev_end..tok.span.start)?);
        }
        prev_end = tok.span.end();
        out.extend_from_slice(text);
    }
    if !minimal {
        out.extend_from_slice(source.get(prev_end..)?);
    }
    (out.len() < source.len()).then_some((out, count))
}

/// The shortest spelling of a number operand, when shorter than `source`
/// and read as the same value at `f32` precision too.
fn shortest(o: &Object, source: &[u8]) -> Option<Vec<u8>> {
    let text = match o {
        Object::Integer(i) => i.to_string(),
        Object::Real(r) if r.is_finite() => {
            let t = real(*r)?;
            let same = |x: &str| x.parse::<f32>().ok();
            let src = std::str::from_utf8(source).ok()?;
            (same(&t)? == same(src)?).then_some(t)?
        }
        _ => return None,
    };
    (text.len() < source.len()).then(|| text.into_bytes())
}

/// `r` with no exponent, no trailing `.0` and no leading zero.
pub(super) fn real(r: f64) -> Option<String> {
    let t = format!("{r:?}");
    if t.contains(['e', 'E']) {
        return None;
    }
    let t = t.strip_suffix(".0").map_or(t.clone(), str::to_owned);
    let t = if t == "-0" { "0".to_owned() } else { t };
    Some(if let Some(rest) = t.strip_prefix("0.") {
        format!(".{rest}")
    } else if let Some(rest) = t.strip_prefix("-0.") {
        format!("-.{rest}")
    } else {
        t
    })
}
