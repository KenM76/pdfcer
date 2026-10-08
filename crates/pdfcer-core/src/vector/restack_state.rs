//! The graphics state in force between the operations of a content stream,
//! split into the part a `q … Q` wrapper can re-establish byte-exactly (the
//! CTM, colours, line and text parameters, `gs` applied on top of a shared
//! history) and the part it cannot (the clip, marked-content nesting).
//!
//! [`super::restack`] reads it to move an object's bytes elsewhere in paint
//! order without changing how the object, or anything else, renders.

use super::geometry::Matrix;
use crate::content::{ContentStream, ContentTokenKind, Operation};

/// One operation's verbatim bytes (operands through operator) and its
/// position in the stream, used to order replays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct OpRef {
    pub start: usize,
    pub end: usize,
    pub seq: u32,
}

/// A single-operator graphics- or text-state parameter (ISO 32000-2 Tables
/// 51 and 103), in [`KEY_OPS`] order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Key {
    Width,
    Dash,
    Cap,
    Join,
    Miter,
    Intent,
    Flatness,
    Font,
    CharSpacing,
    WordSpacing,
    HScale,
    Leading,
    Rise,
    Render,
}

pub(super) const NKEYS: usize = 14;

pub(super) const KEYS: [Key; NKEYS] = [
    Key::Width,
    Key::Dash,
    Key::Cap,
    Key::Join,
    Key::Miter,
    Key::Intent,
    Key::Flatness,
    Key::Font,
    Key::CharSpacing,
    Key::WordSpacing,
    Key::HScale,
    Key::Leading,
    Key::Rise,
    Key::Render,
];

const KEY_OPS: [&[u8]; NKEYS] = [
    b"w", b"d", b"J", b"j", b"M", b"ri", b"i", b"Tf", b"Tc", b"Tw", b"Tz", b"TL", b"Ts", b"Tr",
];

impl Key {
    /// The operator that restores the parameter's initial value (§8.4.1
    /// Table 51, §9.3.1 Table 103), or `None` where none exists (`Tf`) or the
    /// initial value is device-dependent (flatness).
    pub(super) const fn reset(self) -> Option<&'static [u8]> {
        match self {
            Self::Width => Some(b"1 w"),
            Self::Dash => Some(b"[] 0 d"),
            Self::Cap => Some(b"0 J"),
            Self::Join => Some(b"0 j"),
            Self::Miter => Some(b"10 M"),
            Self::Intent => Some(b"/RelativeColorimetric ri"),
            Self::Flatness | Self::Font => None,
            Self::CharSpacing => Some(b"0 Tc"),
            Self::WordSpacing => Some(b"0 Tw"),
            Self::HScale => Some(b"100 Tz"),
            Self::Leading => Some(b"0 TL"),
            Self::Rise => Some(b"0 Ts"),
            Self::Render => Some(b"0 Tr"),
        }
    }

    /// The `/ExtGState` key that also sets the parameter (§8.4.5 Table 57).
    pub(super) const fn gs_key(self) -> Option<&'static [u8]> {
        match self {
            Self::Width => Some(b"LW"),
            Self::Dash => Some(b"D"),
            Self::Cap => Some(b"LC"),
            Self::Join => Some(b"LJ"),
            Self::Miter => Some(b"ML"),
            Self::Intent => Some(b"RI"),
            Self::Flatness => Some(b"FL"),
            Self::Font => Some(b"Font"),
            _ => None,
        }
    }
}

/// A colour selection: the operator that chose the space (`cs`/`CS`, or a
/// device operator such as `rg`, which sets space and value at once; `None`
/// for the initial `DeviceGray`) and the one that set the value (`None` for
/// the space's initial colour).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct Paint {
    pub space: Option<OpRef>,
    pub value: Option<OpRef>,
}

/// The state `q` saves and `Q` restores.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Frame {
    pub ctm: Matrix,
    pub keys: [Option<OpRef>; NKEYS],
    pub fill: Paint,
    pub stroke: Paint,
    /// Node in [`Walk::gs`] for the newest `gs` applied (0: none).
    pub gs: u32,
    /// Identity of the clip and other state no wrapper reproduces; changes on
    /// `W`, `W*` and `"`.
    pub opaque: u32,
    /// Identity of this `q` level, so an unbalanced `Q` is detectable.
    pub level: u32,
}

/// The full state at one boundary between operations.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct State {
    pub frame: Frame,
    /// Node in the marked-content arena (0: outside every section).
    pub mc: u32,
    /// Neither inside `BT … ET` nor between path construction and painting,
    /// so a `q … Q` may be inserted here.
    pub insertable: bool,
}

/// The parts of a [`State`] a wrapper cannot change, compared cheaply
/// (`None` from [`Walk::fingerprint`] where nothing may be inserted).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Fingerprint {
    pub opaque: u32,
    pub mc: u32,
    pub gs: u32,
}

/// The walk's running state; `gs` outlives it as the `gs` history arena.
pub(super) struct Walk {
    frames: Vec<Frame>,
    mc: u32,
    mc_parent: Vec<u32>,
    /// `(parent, op, CTM when applied)` per `gs` node; node 0 is the empty
    /// history. The CTM matters because a soft mask is fixed in the space
    /// current at its `gs` (§11.6.5.2).
    pub gs: Vec<(u32, OpRef, Matrix)>,
    /// The resource name each `gs` node applied (empty for node 0).
    pub gs_names: Vec<Vec<u8>>,
    next_id: u32,
    in_bt: bool,
    path_open: bool,
}

impl Walk {
    fn new() -> Self {
        Self {
            frames: vec![root_frame()],
            mc: 0,
            mc_parent: vec![0],
            gs: vec![(
                0,
                OpRef {
                    start: 0,
                    end: 0,
                    seq: 0,
                },
                Matrix::IDENTITY,
            )],
            gs_names: vec![Vec::new()],
            next_id: 1,
            in_bt: false,
            path_open: false,
        }
    }

    /// The state in force at the current position.
    pub(super) fn state(&self) -> State {
        State {
            frame: self.frames.last().cloned().unwrap_or_else(root_frame),
            mc: self.mc,
            insertable: !self.in_bt && !self.path_open,
        }
    }

    /// The current position's clip, marked-content and `gs` history; `None`
    /// inside a text object or an unpainted path.
    pub(super) fn fingerprint(&self) -> Option<Fingerprint> {
        let f = self.frames.last()?;
        (!self.in_bt && !self.path_open).then_some(Fingerprint {
            opaque: f.opaque,
            mc: self.mc,
            gs: f.gs,
        })
    }

    fn fresh(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// The `gs` operations on the path from `from` up to (not including) its
    /// ancestor `to`, oldest first, each with the CTM it was applied under;
    /// `None` when `to` is not an ancestor.
    pub(super) fn gs_since(&self, from: u32, to: u32) -> Option<Vec<(OpRef, Matrix)>> {
        self.gs_nodes_since(from, to)?
            .into_iter()
            .map(|n| self.gs.get(n as usize).map(|&(_, op, ctm)| (op, ctm)))
            .collect()
    }

    /// The `gs` nodes from `from` up to (not including) its ancestor `to`,
    /// oldest first; `None` when `to` is not an ancestor.
    pub(super) fn gs_nodes_since(&self, from: u32, to: u32) -> Option<Vec<u32>> {
        let mut out = Vec::new();
        let mut node = from;
        while node != to {
            if node == 0 {
                return None;
            }
            out.push(node);
            node = self.gs_parent(node);
        }
        out.reverse();
        Some(out)
    }

    /// The `gs` node `node` was applied on top of (0 for the root).
    pub(super) fn gs_parent(&self, node: u32) -> u32 {
        self.gs.get(node as usize).map_or(0, |g| g.0)
    }

    /// The CTM `node`'s `gs` was applied under.
    pub(super) fn gs_ctm(&self, node: u32) -> Matrix {
        self.gs.get(node as usize).map_or(Matrix::IDENTITY, |g| g.2)
    }

    /// The newest `gs` node in both histories (0: none shared).
    pub(super) fn lca(&self, a: u32, b: u32) -> u32 {
        let mut seen = std::collections::HashSet::new();
        let mut node = a;
        while node != 0 && seen.insert(node) {
            node = self.gs_parent(node);
        }
        let mut node = b;
        while node != 0 && !seen.contains(&node) {
            node = self.gs_parent(node);
        }
        node
    }

    /// The seq of the newest `gs` in a history (0 for none).
    pub(super) fn gs_seq(&self, node: u32) -> u32 {
        self.gs.get(node as usize).map_or(0, |(_, op, _)| op.seq)
    }

    fn apply(&mut self, buf: &[u8], op: &Operation<'_>, seq: u32) {
        let Some(name) = op.operator_name(buf) else {
            return; // an inline image paints; it sets no state
        };
        let start = op
            .operands
            .first()
            .map_or(op.operator.span.start, |t| t.span.start);
        let r = OpRef {
            start,
            end: op.operator.span.end(),
            seq,
        };
        match name {
            b"q" => self.push(),
            b"Q" => {
                if self.frames.len() > 1 {
                    self.frames.pop();
                }
            }
            b"BT" => self.in_bt = true,
            b"ET" => {
                self.in_bt = false;
                if self.text_clips(buf) {
                    let id = self.fresh();
                    if let Some(f) = self.frames.last_mut() {
                        f.opaque = id;
                    }
                }
            }
            b"BMC" | b"BDC" => {
                self.mc_parent.push(self.mc);
                self.mc = u32::try_from(self.mc_parent.len() - 1).unwrap_or(u32::MAX);
            }
            b"EMC" => self.mc = self.mc_parent.get(self.mc as usize).copied().unwrap_or(0),
            b"m" | b"l" | b"c" | b"v" | b"y" | b"h" | b"re" => self.path_open = true,
            b"S" | b"s" | b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" | b"n" => {
                self.path_open = false;
            }
            _ => self.apply_state(name, op, r),
        }
    }

    /// Whether the text rendering mode in force adds to the clip at `ET`
    /// (modes 4–7, §9.3.6).
    fn text_clips(&self, buf: &[u8]) -> bool {
        let Some(Some(tr)) = self.frames.last().and_then(|f| f.keys.get(13).copied()) else {
            return false;
        };
        let digits: Vec<u8> = buf
            .get(tr.start..tr.end)
            .unwrap_or_default()
            .iter()
            .copied()
            .take_while(u8::is_ascii_digit)
            .collect();
        std::str::from_utf8(&digits)
            .ok()
            .and_then(|d| d.parse::<u32>().ok())
            .is_some_and(|mode| mode >= 4)
    }

    fn push(&mut self) {
        let level = self.fresh();
        if let Some(top) = self.frames.last() {
            let mut f = top.clone();
            f.level = level;
            self.frames.push(f);
        }
    }

    fn apply_state(&mut self, name: &[u8], op: &Operation<'_>, r: OpRef) {
        let fresh = if matches!(name, b"W" | b"W*" | b"\"") {
            self.fresh()
        } else {
            0
        };
        if name == b"gs"
            && let Some(f) = self.frames.last()
        {
            self.gs.push((f.gs, r, f.ctm));
            let name = op.operands.first().and_then(|t| match &t.kind {
                ContentTokenKind::Operand(o) => o.as_name().map(|n| n.as_bytes().to_vec()),
                _ => None,
            });
            self.gs_names.push(name.unwrap_or_default());
        }
        let gs_node = u32::try_from(self.gs.len() - 1).unwrap_or(u32::MAX);
        let Some(f) = self.frames.last_mut() else {
            return;
        };
        match name {
            b"cm" => {
                if let Some(m) = matrix_operands(op) {
                    f.ctm = m.post_concat(f.ctm);
                }
            }
            b"g" | b"rg" | b"k" => f.fill = paint(Some(r), Some(r)),
            b"G" | b"RG" | b"K" => f.stroke = paint(Some(r), Some(r)),
            b"cs" => f.fill = paint(Some(r), None),
            b"CS" => f.stroke = paint(Some(r), None),
            b"sc" | b"scn" => f.fill.value = Some(r),
            b"SC" | b"SCN" => f.stroke.value = Some(r),
            b"gs" => f.gs = gs_node,
            b"W" | b"W*" | b"\"" => f.opaque = fresh,
            _ => {
                if let Some(i) = KEY_OPS.iter().position(|k| *k == name)
                    && let Some(slot) = f.keys.get_mut(i)
                {
                    *slot = Some(r);
                }
            }
        }
    }
}

const fn root_frame() -> Frame {
    Frame {
        ctm: Matrix::IDENTITY,
        keys: [None; NKEYS],
        fill: Paint {
            space: None,
            value: None,
        },
        stroke: Paint {
            space: None,
            value: None,
        },
        gs: 0,
        opaque: 0,
        level: 0,
    }
}

const fn paint(space: Option<OpRef>, value: Option<OpRef>) -> Paint {
    Paint { space, value }
}

fn matrix_operands(op: &Operation<'_>) -> Option<Matrix> {
    let mut v = [0.0f64; 6];
    if op.operands.len() != 6 {
        return None;
    }
    for (slot, tok) in v.iter_mut().zip(op.operands) {
        let ContentTokenKind::Operand(o) = &tok.kind else {
            return None;
        };
        *slot = o.as_number()?;
    }
    let [a, b, c, d, e, f] = v;
    Some(Matrix::new(a, b, c, d, e, f))
}

/// Walk `content`, calling `visit(offset, walk)` at offset 0 and after every
/// operation with the offset just past it. Returns the finished walk, whose
/// `gs` arena node numbers stay valid for every [`State`] it produced.
pub(super) fn walk(content: &ContentStream, mut visit: impl FnMut(usize, &Walk)) -> Walk {
    let mut w = Walk::new();
    visit(0, &w);
    for (i, op) in content.operations().enumerate() {
        let seq = u32::try_from(i + 1).unwrap_or(u32::MAX);
        w.apply(&content.buf, &op, seq);
        visit(op.operator.span.end(), &w);
    }
    w
}
