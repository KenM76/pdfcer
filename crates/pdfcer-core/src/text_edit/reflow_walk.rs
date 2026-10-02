//! The content-stream walk behind reflow-apply: locate a block's text
//! objects, record the font, text state and colours each block show
//! operator ran under, and refuse a region holding an operator a re-emitted
//! block cannot carry.

use std::collections::HashMap;

use crate::content::{ContentStream, ContentTokenKind, Operation};
use crate::object::Object;
use crate::span::ByteSpan;
use crate::text_edit::cause::UnsupportedCause;
use crate::text_state::{AmbientTextState, TextStateParam};

use super::reflow_apply::ReflowApplyError;

/// Graphics-state stack depth the walk tracks; deeper `q` drop the oldest.
const MAX_STACK: usize = 256;

/// The operators a reflowed region may hold. Everything else inside the
/// region is refused by name: re-emitting the block would drop it.
///
/// Positioning (`Td TD Tm T*`) is replaced by the new layout; text state,
/// fonts and colours are carried per glyph; `q`/`Q` are dropped when
/// balanced (the walk restores what they scoped); marked content between the
/// block's text objects is dropped, its enclosing wrapper kept.
const CARRIED: &[&[u8]] = &[
    b"BT", b"ET", b"Tf", b"Tc", b"Tw", b"Tz", b"TL", b"Ts", b"Tr", b"Td", b"TD", b"Tm", b"T*",
    b"Tj", b"TJ", b"'", b"\"", b"g", b"rg", b"k", b"G", b"RG", b"K", b"cs", b"CS", b"sc", b"scn",
    b"SC", b"SCN", b"BDC", b"BMC", b"EMC", b"MP", b"DP", b"q", b"Q",
];

/// One colour (fill or stroke) as the raw bytes of the operators that set
/// it: a colour-space operator and a colour operator (ISO 32000-1 §8.6.8,
/// Table 74). Empty bytes mean "never set in this stream".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Paint {
    space: Vec<u8>,
    value: Vec<u8>,
}

impl Paint {
    /// The bytes that put this colour in force. The never-set colour is the
    /// initial DeviceGray black (§8.4.1 Table 52): `0 g` / `0 G`.
    pub(super) fn set_bytes(&self, stroke: bool) -> Vec<u8> {
        if self.space.is_empty() && self.value.is_empty() {
            return if stroke {
                b"0 G\n".to_vec()
            } else {
                b"0 g\n".to_vec()
            };
        }
        let mut out = Vec::new();
        for part in [&self.space, &self.value] {
            if !part.is_empty() {
                out.extend_from_slice(part);
                out.push(b'\n');
            }
        }
        out
    }
}

/// The graphics state the walk models: the selected font, the §9.3 text
/// state and the two colours. All are graphics state, saved by `q` and
/// restored by `Q` (§8.4.2).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct WalkState {
    /// `Tf` resource name and size, once set.
    pub(super) font: Option<(Vec<u8>, f64)>,
    pub(super) ambient: AmbientTextState,
    pub(super) fill: Paint,
    pub(super) stroke: Paint,
}

impl WalkState {
    fn initial() -> Self {
        Self {
            font: None,
            ambient: AmbientTextState::initial(),
            fill: Paint::default(),
            stroke: Paint::default(),
        }
    }
}

/// The state one block show operator ran under.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SpanStyle {
    pub(super) font: Vec<u8>,
    pub(super) size: f64,
    pub(super) ambient: AmbientTextState,
    pub(super) fill: Paint,
    pub(super) stroke: Paint,
}

impl SpanStyle {
    /// Whether two styles render a glyph identically, so one show operator
    /// can carry both.
    pub(super) fn same_look(&self, other: &Self) -> bool {
        self.font == other.font
            && self.size == other.size
            && self.fill == other.fill
            && self.stroke == other.stroke
            && TextStateParam::ALL.iter().all(|&p| {
                p == TextStateParam::Leading
                    || self.ambient.get(p).value == other.ambient.get(p).value
            })
    }
}

/// The byte region to replace and the states around it.
///
/// `entry` is the state just before `start`: what stays in force for
/// anything the new body does not set, because the operators inside the
/// region are gone. `exit` is the state just after `end`: what the new body
/// must leave in force, or the reflow changed content it did not touch.
pub(super) struct BlockRegion {
    /// Start byte of the first block text object's `BT`.
    pub(super) start: usize,
    /// End byte of the last block text object's `ET`.
    pub(super) end: usize,
    /// Each block show operator's state, keyed by its operator span start.
    pub(super) styles: HashMap<usize, SpanStyle>,
    pub(super) entry: WalkState,
    pub(super) exit: WalkState,
}

/// One text object (`BT … ET`) seen in the walk, with the state at both of
/// its boundaries (the region bounds are only known after the walk).
struct TextObj {
    bt_start: usize,
    et_end: usize,
    show_spans: Vec<ByteSpan>,
    at_bt: WalkState,
    at_et: WalkState,
}

struct Walk<'s> {
    stream: &'s ContentStream,
    block_spans: &'s [ByteSpan],
    gs: WalkState,
    stack: Vec<WalkState>,
    objs: Vec<TextObj>,
    cur: Option<TextObj>,
    styles: HashMap<usize, SpanStyle>,
}

/// Walk the content stream, collect its text objects, and compute the byte
/// region spanning exactly the block's text objects.
///
/// # Errors
///
/// Refuses by name a block that shares a text object with other content, is
/// non-contiguous, shows outside `BT … ET`, or whose region holds an
/// operator outside [`CARRIED`] or unbalanced `q`/`Q`.
pub(super) fn locate_block_region(
    stream: &ContentStream,
    block_spans: &[ByteSpan],
) -> Result<BlockRegion, ReflowApplyError> {
    let mut walk = Walk {
        stream,
        block_spans,
        gs: WalkState::initial(),
        stack: Vec::new(),
        objs: Vec::new(),
        cur: None,
        styles: HashMap::new(),
    };
    for op in stream.operations() {
        walk.step(&op)?;
    }
    let region = walk.into_region()?;
    check_region_operators(stream, region.start, region.end)?;
    Ok(region)
}

impl Walk<'_> {
    fn is_block(&self, sp: ByteSpan) -> bool {
        self.block_spans
            .iter()
            .any(|b| b.start == sp.start && b.len == sp.len)
    }

    fn step(&mut self, op: &Operation<'_>) -> Result<(), ReflowApplyError> {
        let buf = &self.stream.buf;
        let Some(name) = op.operator_name(buf) else {
            return Ok(());
        };
        let (s, e) = text_op_span(op);
        let raw = buf.get(s..e).unwrap_or_default().to_vec();
        match name {
            b"q" => {
                self.stack.push(self.gs.clone());
                if self.stack.len() > MAX_STACK {
                    self.stack.remove(0);
                }
            }
            b"Q" => {
                if let Some(prev) = self.stack.pop() {
                    self.gs = prev;
                }
            }
            b"Tf" => {
                let font = op.operands.first().and_then(|t| match &t.kind {
                    ContentTokenKind::Operand(Object::Name(n)) => Some(n.as_bytes().to_vec()),
                    _ => None,
                });
                if let (Some(font), Some(size)) = (font, last_number(op)) {
                    self.gs.font = Some((font, size));
                }
            }
            // `TD` also sets `TL` (§9.4.2 Table 108).
            b"TD" => {
                if let [_, ty] = operand_numbers(op).as_slice() {
                    self.gs
                        .ambient
                        .set_indirect(TextStateParam::Leading, -*ty, "TD");
                }
            }
            b"BT" => self.begin_text(op),
            b"ET" => self.end_text(op),
            b"Tj" | b"TJ" | b"'" | b"\"" => {
                if name == b"\"" {
                    self.gs
                        .ambient
                        .apply_operator(name, &operand_numbers(op), &raw);
                }
                self.show(op.operator.span)?;
            }
            _ => self.colour_or_state(name, op, raw),
        }
        Ok(())
    }

    /// Colour operators (§8.6.8 Table 74) and the six §9.3 text-state
    /// operators. A colour-space operator resets the colour to that space's
    /// initial value, so it clears the recorded colour bytes.
    fn colour_or_state(&mut self, name: &[u8], op: &Operation<'_>, raw: Vec<u8>) {
        // `0 g` / `0 G` is the initial colour (§8.4.1), so it is stored as
        // the never-set value and compares equal to it.
        let initial = matches!(name, b"g" | b"G") && operand_numbers(op) == [0.0];
        match name {
            b"g" | b"G" if initial => {
                let paint = if name == b"g" {
                    &mut self.gs.fill
                } else {
                    &mut self.gs.stroke
                };
                *paint = Paint::default();
            }
            b"g" | b"rg" | b"k" => {
                self.gs.fill = Paint {
                    space: Vec::new(),
                    value: raw,
                }
            }
            b"G" | b"RG" | b"K" => {
                self.gs.stroke = Paint {
                    space: Vec::new(),
                    value: raw,
                }
            }
            b"cs" => {
                self.gs.fill = Paint {
                    space: raw,
                    value: Vec::new(),
                }
            }
            b"CS" => {
                self.gs.stroke = Paint {
                    space: raw,
                    value: Vec::new(),
                }
            }
            b"sc" | b"scn" => self.gs.fill.value = raw,
            b"SC" | b"SCN" => self.gs.stroke.value = raw,
            _ => {
                self.gs
                    .ambient
                    .apply_operator(name, &operand_numbers(op), &raw);
            }
        }
    }

    fn begin_text(&mut self, op: &Operation<'_>) {
        self.cur = Some(TextObj {
            bt_start: op.operator.span.start,
            et_end: op.operator.span.end(),
            show_spans: Vec::new(),
            at_bt: self.gs.clone(),
            at_et: self.gs.clone(),
        });
    }

    fn end_text(&mut self, op: &Operation<'_>) {
        if let Some(mut obj) = self.cur.take() {
            obj.et_end = op.operator.span.end();
            obj.at_et = self.gs.clone();
            self.objs.push(obj);
        }
    }

    fn show(&mut self, span: ByteSpan) -> Result<(), ReflowApplyError> {
        let block = self.is_block(span);
        if block {
            let (font, size) = self.gs.font.clone().ok_or(ReflowApplyError::Unsupported(
                UnsupportedCause::ShowWithoutFont,
            ))?;
            self.styles.insert(
                span.start,
                SpanStyle {
                    font,
                    size,
                    ambient: self.gs.ambient.clone(),
                    fill: self.gs.fill.clone(),
                    stroke: self.gs.stroke.clone(),
                },
            );
        }
        match self.cur.as_mut() {
            Some(obj) => obj.show_spans.push(span),
            None if block => {
                return Err(ReflowApplyError::Unsupported(
                    UnsupportedCause::ShowOutsideTextObject,
                ));
            }
            None => {}
        }
        Ok(())
    }

    /// The region over the text objects holding block show operators. Each
    /// must hold ONLY block operators, and they must be contiguous.
    fn into_region(self) -> Result<BlockRegion, ReflowApplyError> {
        let mut first: Option<usize> = None;
        let mut last: Option<usize> = None;
        for (i, obj) in self.objs.iter().enumerate() {
            if !obj.show_spans.iter().any(|&s| self.is_block(s)) {
                continue;
            }
            if obj.show_spans.iter().any(|&s| !self.is_block(s)) {
                return Err(ReflowApplyError::Unsupported(
                    UnsupportedCause::SharedTextObject,
                ));
            }
            if last.is_some_and(|l| i != l + 1) {
                return Err(ReflowApplyError::Unsupported(
                    UnsupportedCause::NonContiguousTextObjects,
                ));
            }
            first.get_or_insert(i);
            last = Some(i);
        }
        let (Some(fi), Some(la)) = (first, last) else {
            return Err(ReflowApplyError::Unsupported(
                UnsupportedCause::ShowOperatorsNotFound,
            ));
        };
        let (Some(f), Some(l)) = (self.objs.get(fi), self.objs.get(la)) else {
            return Err(ReflowApplyError::Unsupported(
                UnsupportedCause::ShowOperatorsNotFound,
            ));
        };
        Ok(BlockRegion {
            start: f.bt_start,
            end: l.et_end,
            entry: f.at_bt.clone(),
            exit: l.at_et.clone(),
            styles: self.styles,
        })
    }
}

/// Refuse a region holding an operator outside [`CARRIED`] (an inline image
/// included), or `q`/`Q` or marked-content operators that do not net to zero
/// within it: the region is replaced whole, so an unmatched `BDC`/`BMC` or
/// `EMC` inside it would unbalance the stream (ISO 32000-2 §14.6, which
/// requires sequences to nest).
fn check_region_operators(
    stream: &ContentStream,
    start: usize,
    end: usize,
) -> Result<(), ReflowApplyError> {
    let mut depth = 0_i64;
    let mut marked = 0_i64;
    for op in stream.operations() {
        let at = op.operator.span.start;
        if at < start || at >= end {
            continue;
        }
        let name = op.operator_name(&stream.buf).unwrap_or(b"BI");
        match name {
            b"q" => depth += 1,
            b"Q" => depth -= 1,
            b"BDC" | b"BMC" => marked += 1,
            b"EMC" => marked -= 1,
            n if CARRIED.contains(&n) => {}
            other => {
                return Err(ReflowApplyError::Unsupported(
                    UnsupportedCause::OperatorInBlock {
                        operator: String::from_utf8_lossy(other).into_owned(),
                    },
                ));
            }
        }
    }
    if depth != 0 {
        return Err(ReflowApplyError::Unsupported(
            UnsupportedCause::OperatorInBlock {
                operator: "q/Q (unbalanced)".to_owned(),
            },
        ));
    }
    if marked != 0 {
        return Err(ReflowApplyError::Unsupported(
            UnsupportedCause::OperatorInBlock {
                operator: "BDC/EMC (unbalanced)".to_owned(),
            },
        ));
    }
    Ok(())
}

/// The byte span of an operator including its operands — the raw sequence a
/// restore re-emits (see [`crate::text_state`]).
pub(super) fn text_op_span(op: &Operation<'_>) -> (usize, usize) {
    let start = op
        .operands
        .first()
        .map_or(op.operator.span.start, |t| t.span.start);
    (start, op.operator.span.end())
}

/// The last numeric operand of an operation (`Tf`'s size).
fn last_number(op: &Operation<'_>) -> Option<f64> {
    operand_numbers(op).last().copied()
}

/// Every numeric operand of an operation, in order.
pub(super) fn operand_numbers(op: &Operation<'_>) -> Vec<f64> {
    op.operands
        .iter()
        .filter_map(|t| match &t.kind {
            ContentTokenKind::Operand(o) => o.as_number(),
            _ => None,
        })
        .collect()
}
