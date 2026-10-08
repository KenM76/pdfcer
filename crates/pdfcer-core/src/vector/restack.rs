//! Changing an object's place in paint order (pdfcer-gui request G157):
//! bring to front, send to back, raise one step, lower one step.
//!
//! An object moves by cutting its bytes out and splicing them in elsewhere,
//! so the graphics state it was painted under has to travel with it. At the
//! destination the object is wrapped `q [cm] <state> … Q`, rebuilding its
//! CTM, colours, line and text parameters and any `gs` applied since the
//! history the two positions share. At the old position its bytes are
//! replaced by the state changes they made, so everything painted after is
//! untouched.
//!
//! `gs` operations the destination has and the object's history lacks are
//! undone by one new `/ExtGState` ([`super::restack_reset`]).
//!
//! What a wrapper cannot rebuild limits where an object may go: a different
//! clip, a different marked-content section, or an `/ExtGState` parameter
//! whose earlier value has no name (`SM`, `HTO`). Such an object
//! goes to the nearest position that can hold it, or stays, and the outcome
//! says so by index ([`RestackLimit`]). It is never moved to a place where
//! it would render differently.

use std::collections::{HashMap, HashSet};

use super::decompose::{ImageSource, VectorObject};
use super::edit::{PlannedEdit, VectorEditError};
use super::geometry::Matrix;
use super::restack_reset::{GsParams, Restore, params};
use super::restack_state::{Fingerprint, KEYS, Key, OpRef, Paint, State, Walk, walk};
use crate::content::ContentStream;
use crate::object::Dict;
use crate::writer::content::emit_number;

/// Where [`crate::edit::EditSession::restack_objects`] moves each selected object in paint order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StackMove {
    /// Above every unselected object.
    Front,
    /// Below every unselected object.
    Back,
    /// Just above the nearest unselected object above it whose bounding box
    /// overlaps its own; unchanged when none does.
    Forward,
    /// Just below the nearest unselected object below it whose bounding box
    /// overlaps its own; unchanged when none does.
    Backward,
}

/// Why an object did not land exactly where [`StackMove`] asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RestackLimitReason {
    /// The requested position is under a different clip, a different
    /// marked-content section, or graphics state a wrapper cannot rebuild;
    /// the object went to the nearest position that can hold it, or stayed.
    Scope,
    /// The object's own bytes change state the old position cannot keep
    /// without them (a clip, marked content, an unbalanced `q`/`Q`), so it
    /// was not moved.
    Entangled,
}

/// One object that did not land where it was asked to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct RestackLimit {
    /// The object's index before the move.
    pub object: usize,
    /// Why.
    pub reason: RestackLimitReason,
}

/// What [`crate::edit::EditSession::restack_objects`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RestackOutcome {
    /// The new index of each requested object, in request order (a repeated
    /// index repeats its answer). Equals the input for an object not moved.
    pub indices: Vec<usize>,
    /// The objects that moved, by their index before the move, ascending.
    pub moved: Vec<usize>,
    /// The objects that did not land where asked, ascending.
    pub limited: Vec<RestackLimit>,
}

/// How many destination candidates are fully checked per object, and per
/// gap: compatible-looking boundaries usually all pass or all fail alike.
const MAX_CANDIDATES: usize = 12;
const PER_GAP: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Path,
    Text,
    Image,
    Form,
}

fn kind_of(o: &VectorObject) -> Kind {
    match o {
        VectorObject::Path(_) => Kind::Path,
        VectorObject::Text(_) => Kind::Text,
        VectorObject::Image(i) if i.source == ImageSource::Form => Kind::Form,
        VectorObject::Image(_) => Kind::Image,
    }
}

/// The page's `/ExtGState` lookup and the free names new ones may take.
pub(crate) struct GsResources<'a> {
    /// The dictionary a `gs` operand names, resolved.
    pub ext_gstate: &'a dyn Fn(&[u8]) -> Option<Dict>,
    /// Unused `/ExtGState` names, at least one per selected object.
    pub free_names: &'a [Vec<u8>],
}

/// A planned restack: the edit, its outcome, and the `/ExtGState`s to bind.
pub(crate) struct Restack {
    pub edit: PlannedEdit,
    pub outcome: RestackOutcome,
    pub bind: Vec<(Vec<u8>, Dict)>,
}

/// Move `selected` objects of a page (its decomposition `objects`, in paint
/// order, over `content`) per `how`; an object asked for twice moves once.
///
/// # Errors
///
/// [`VectorEditError::ObjectOutOfRange`] for an index past `objects`, and
/// [`VectorEditError::OverlappingObjectSpans`] when two objects' bytes
/// overlap, which a decomposition of `content` never produces.
pub(crate) fn plan_restack(
    content: &ContentStream,
    objects: &[VectorObject],
    selected: &[usize],
    how: StackMove,
    res: &GsResources<'_>,
) -> Result<Restack, VectorEditError> {
    let spans = validate(objects, selected)?;
    let movers: Vec<usize> = {
        let mut m = selected.to_vec();
        m.sort_unstable();
        m.dedup();
        m
    };
    let chosen: HashSet<usize> = movers.iter().copied().collect();
    let unselected: Vec<usize> = (0..objects.len()).filter(|i| !chosen.contains(i)).collect();
    let mut bounds: Vec<(usize, Option<Fingerprint>)> = Vec::new();
    let w = walk(content, |off, w| bounds.push((off, w.fingerprint())));
    let gs: Vec<GsParams> = w
        .gs_names
        .iter()
        .map(|n| (res.ext_gstate)(n).map(|d| params(&d)))
        .collect();
    let ctx = Ctx {
        gs: &gs,
        free_names: res.free_names,
        buf: &content.buf,
        objects,
        spans: &spans,
        unselected: &unselected,
        bounds: &bounds,
        walk: &w,
    };
    let mut needed: HashSet<usize> = HashSet::new();
    let plans: Vec<Plan> = movers
        .iter()
        .map(|&a| ctx.plan_one(a, how, &mut needed))
        .collect();
    let full = full_states(content, &needed);
    finish(&ctx, &plans, &full, selected)
}

/// One mover's old position and destination candidates, before the full
/// states are known.
struct Plan {
    object: usize,
    /// The boundaries just before and just after the object's bytes.
    start: usize,
    end: usize,
    /// `(gap, boundary offset)` in preference order.
    candidates: Vec<(usize, usize)>,
    /// The gap that would satisfy the request exactly; `None`: no move.
    ideal: Option<usize>,
}

struct Ctx<'a> {
    gs: &'a [GsParams],
    free_names: &'a [Vec<u8>],
    buf: &'a [u8],
    objects: &'a [VectorObject],
    spans: &'a [(usize, usize)],
    unselected: &'a [usize],
    bounds: &'a [(usize, Option<Fingerprint>)],
    walk: &'a Walk,
}

impl Ctx<'_> {
    fn plan_one(&self, a: usize, how: StackMove, needed: &mut HashSet<usize>) -> Plan {
        let (lo, hi) = self.spans.get(a).copied().unwrap_or_default();
        // The last boundary at or before the object's first byte.
        let start = self
            .bounds
            .partition_point(|b| b.0 <= lo)
            .checked_sub(1)
            .and_then(|i| self.bounds.get(i))
            .map_or(0, |b| b.0);
        needed.insert(start);
        needed.insert(hi);
        let (ideal, gaps) = self.search(a, how);
        let mut candidates = Vec::new();
        if let Some(fp) = self.fingerprint_at(start) {
            for gap in gaps {
                let before = candidates.len();
                for off in self.gap_boundaries(gap, how) {
                    if candidates.len() >= MAX_CANDIDATES || candidates.len() - before >= PER_GAP {
                        break;
                    }
                    if self.compatible(fp, off) {
                        candidates.push((gap, off));
                        needed.insert(off);
                    }
                }
            }
        }
        Plan {
            object: a,
            start,
            end: hi,
            candidates,
            ideal,
        }
    }

    fn fingerprint_at(&self, off: usize) -> Option<Fingerprint> {
        let i = self.bounds.partition_point(|b| b.0 < off);
        self.bounds.get(i).filter(|b| b.0 == off)?.1
    }

    /// Unselected objects painted before gap `j` (gap `j` lies just before
    /// object `j`).
    fn below(&self, j: usize) -> usize {
        self.unselected.partition_point(|&u| u < j)
    }

    /// The ideal gap and the gaps to try, nearest-to-ideal first, never
    /// including one that leaves the object where it is relative to the
    /// unselected objects.
    fn search(&self, a: usize, how: StackMove) -> (Option<usize>, Vec<usize>) {
        let n = self.objects.len();
        let here = self.below(a);
        let overlaps = |u: &&usize| {
            let bb = |i: usize| self.objects.get(i).map(VectorObject::page_bbox);
            matches!((bb(a), bb(**u)), (Some(x), Some(y)) if x.intersects(y))
        };
        match how {
            StackMove::Front if self.below(n) > here => (
                Some(n),
                (0..=n)
                    .rev()
                    .take_while(|&j| self.below(j) > here)
                    .collect(),
            ),
            StackMove::Back if here > 0 => (
                Some(0),
                (0..=n).take_while(|&j| self.below(j) < here).collect(),
            ),
            StackMove::Forward => match self.unselected.iter().filter(|&&u| u > a).find(overlaps) {
                Some(&k) => (Some(k + 1), (k + 1..=n).collect()),
                None => (None, Vec::new()),
            },
            StackMove::Backward => {
                match self
                    .unselected
                    .iter()
                    .rev()
                    .filter(|&&u| u < a)
                    .find(overlaps)
                {
                    Some(&k) => (Some(k), (0..=k).rev().collect()),
                    None => (None, Vec::new()),
                }
            }
            _ => (None, Vec::new()),
        }
    }

    /// The walk boundaries inside gap `j`, toward the request's direction
    /// first.
    fn gap_boundaries(&self, j: usize, how: StackMove) -> Vec<usize> {
        let lo = j
            .checked_sub(1)
            .and_then(|p| self.spans.get(p))
            .map_or(0, |s| s.1);
        let hi = self.spans.get(j).map_or(self.buf.len(), |s| s.0);
        let from = self.bounds.partition_point(|b| b.0 < lo);
        let to = self.bounds.partition_point(|b| b.0 <= hi);
        let mut offs: Vec<usize> = self
            .bounds
            .get(from..to)
            .unwrap_or_default()
            .iter()
            .filter(|b| b.1.is_some())
            .map(|b| b.0)
            .collect();
        if matches!(how, StackMove::Front | StackMove::Forward) {
            offs.reverse();
        }
        offs
    }

    /// The cheap half of the destination check: same clip and same marked
    /// content.
    fn compatible(&self, s: Fingerprint, off: usize) -> bool {
        self.fingerprint_at(off)
            .is_some_and(|d| d.opaque == s.opaque && d.mc == s.mc)
    }
}

/// The full state at each offset in `needed`.
fn full_states(content: &ContentStream, needed: &HashSet<usize>) -> HashMap<usize, State> {
    let mut out = HashMap::new();
    if needed.is_empty() {
        return out;
    }
    walk(content, |off, w| {
        if needed.contains(&off) {
            out.insert(off, w.state());
        }
    });
    out
}

/// Choose each mover's destination, splice, and report.
fn finish(
    ctx: &Ctx<'_>,
    plans: &[Plan],
    full: &HashMap<usize, State>,
    selected: &[usize],
) -> Result<Restack, VectorEditError> {
    let mut bind = Vec::new();
    let mut edits: Vec<(usize, u8, usize, usize, Vec<u8>)> = Vec::new();
    let mut moved_to: HashMap<usize, usize> = HashMap::new();
    let mut limited = Vec::new();
    for (n, p) in plans.iter().enumerate() {
        let Some(ideal) = p.ideal else { continue };
        let name = ctx.free_names.get(n).map_or(&b""[..], Vec::as_slice);
        let s = full.get(&p.start);
        let removal = s
            .zip(full.get(&p.end))
            .and_then(|(s, e)| removal_delta(ctx.walk, ctx.buf, s, e));
        let (Some(removal), Some(s)) = (removal, s) else {
            limited.push(limit(p.object, RestackLimitReason::Entangled));
            continue;
        };
        let kind = ctx.objects.get(p.object).map_or(Kind::Form, kind_of);
        let hit = p.candidates.iter().find_map(|&(gap, off)| {
            let d = full.get(&off)?;
            let span = ctx.spans.get(p.object)?;
            wrapper(ctx, s, d, kind, span, name).map(|w| (gap, off, w))
        });
        let Some((gap, off, (wrap, reset))) = hit else {
            limited.push(limit(p.object, RestackLimitReason::Scope));
            continue;
        };
        if ctx.below(gap) != ctx.below(ideal) {
            limited.push(limit(p.object, RestackLimitReason::Scope));
        }
        let (start, end) = ctx.spans.get(p.object).copied().unwrap_or_default();
        edits.push((start, 1, p.object, end, removal));
        edits.push((off, 0, p.object, off, wrap));
        moved_to.insert(p.object, off);
        if let Some(reset) = reset {
            bind.push((name.to_vec(), reset));
        }
    }
    edits.sort_by_key(|e| (e.0, e.1, e.2));
    let mut out = Vec::with_capacity(ctx.buf.len() + edits.len() * 32);
    let mut cursor = 0;
    for (start, _, _, end, bytes) in &edits {
        out.extend_from_slice(ctx.buf.get(cursor..*start).unwrap_or_default());
        out.extend_from_slice(bytes);
        cursor = *end;
    }
    out.extend_from_slice(ctx.buf.get(cursor..).unwrap_or_default());

    let new_index = new_order(ctx.spans, &moved_to);
    let mut moved: Vec<usize> = moved_to.keys().copied().collect();
    moved.sort_unstable();
    limited.sort_by_key(|l| l.object);
    let outcome = RestackOutcome {
        indices: selected
            .iter()
            .map(|&i| new_index.get(i).copied().unwrap_or(i))
            .collect(),
        moved,
        limited,
    };
    let edit = PlannedEdit {
        content: out,
        operators_touched: outcome.moved.len(),
        disclosures: Vec::new(),
    };
    Ok(Restack {
        edit,
        outcome,
        bind,
    })
}

const fn limit(object: usize, reason: RestackLimitReason) -> RestackLimit {
    RestackLimit { object, reason }
}

/// Each object's index after the move: unmoved ones by their position,
/// moved ones by their insertion offset, an insertion painting before an
/// object starting at the same offset.
fn new_order(spans: &[(usize, usize)], moved_to: &HashMap<usize, usize>) -> Vec<usize> {
    let mut keys: Vec<(usize, u8, usize)> = spans
        .iter()
        .enumerate()
        .map(|(i, s)| match moved_to.get(&i) {
            Some(&off) => (off, 0, i),
            None => (s.0, 1, i),
        })
        .collect();
    keys.sort_unstable();
    let mut out = vec![0; spans.len()];
    for (pos, (_, _, i)) in keys.into_iter().enumerate() {
        if let Some(slot) = out.get_mut(i) {
            *slot = pos;
        }
    }
    out
}

fn validate(
    objects: &[VectorObject],
    selected: &[usize],
) -> Result<Vec<(usize, usize)>, VectorEditError> {
    let count = objects.len();
    if let Some(&index) = selected.iter().find(|&&i| i >= count) {
        return Err(VectorEditError::ObjectOutOfRange { index, count });
    }
    let spans: Vec<(usize, usize)> = objects
        .iter()
        .map(|o| (o.bytes().start, o.bytes().end()))
        .collect();
    for pair in spans.windows(2) {
        if let [prev, next] = pair
            && next.0 < prev.1
        {
            return Err(VectorEditError::OverlappingObjectSpans {
                start: next.0,
                end: next.1,
            });
        }
    }
    Ok(spans)
}

/// Replayable operators, ordered by when the original stream ran them.
#[derive(Default)]
struct Replay {
    ops: Vec<(u32, Vec<u8>)>,
}

impl Replay {
    fn op(&mut self, buf: &[u8], r: OpRef) {
        if self.ops.iter().all(|(seq, _)| *seq != r.seq || r.seq == 0) {
            let bytes = buf.get(r.start..r.end).unwrap_or_default().to_vec();
            self.ops.push((r.seq, bytes));
        }
    }

    fn lit(&mut self, bytes: &[u8]) {
        self.ops.push((0, bytes.to_vec()));
    }

    /// Re-establish `want` over a state whose paint is `have`.
    fn paint(&mut self, buf: &[u8], want: Paint, have: Paint, fill: bool) {
        if want == have {
            return;
        }
        match (want.space, want.value) {
            (None, None) => self.lit(if fill { b"0 g" } else { b"0 G" }),
            (None, Some(v)) => {
                self.lit(if fill {
                    b"/DeviceGray cs"
                } else {
                    b"/DeviceGray CS"
                });
                self.op(buf, v);
            }
            (Some(sp), v) => {
                self.op(buf, sp);
                if let Some(v) = v {
                    self.op(buf, v);
                }
            }
        }
    }

    fn into_bytes(mut self) -> Vec<u8> {
        self.ops.sort_by_key(|o| o.0);
        let mut out = Vec::new();
        for (_, op) in self.ops {
            out.extend_from_slice(&op);
            out.push(b' ');
        }
        out
    }
}

const fn relevant(key: Key, kind: Kind) -> bool {
    let text_only = matches!(
        key,
        Key::Font
            | Key::CharSpacing
            | Key::WordSpacing
            | Key::HScale
            | Key::Leading
            | Key::Rise
            | Key::Render
    );
    match kind {
        Kind::Text | Kind::Form => true,
        Kind::Path => !text_only,
        Kind::Image => matches!(key, Key::Intent),
    }
}

/// `q [cm] <state> <object> Q`, painting the object at destination state
/// `d` exactly as at its own start state `s`, with the `/ExtGState` to bind
/// under `reset_name` if one is needed; `None` when that cannot be done.
fn wrapper(
    ctx: &Ctx<'_>,
    s: &State,
    d: &State,
    kind: Kind,
    span: &(usize, usize),
    reset_name: &[u8],
) -> Option<(Vec<u8>, Option<Dict>)> {
    let (sf, df, buf) = (&s.frame, &d.frame, ctx.buf);
    let restore = Restore::new(ctx.walk, ctx.gs, sf, df)?;
    let mut replay = Replay::default();
    for (op, ctm) in restore.own_extras()? {
        if ctm != sf.ctm {
            return None; // a soft mask would land in a different space
        }
        replay.op(buf, op);
    }
    let restored = restore.run(|k| relevant(k, kind))?;
    let reset = if restored.reset.is_empty() {
        None
    } else if reset_name.is_empty() {
        return None;
    } else {
        let mut lit = b"/".to_vec();
        lit.extend_from_slice(reset_name);
        lit.extend_from_slice(b" gs");
        replay.lit(&lit);
        Some(restored.reset)
    };
    for lit in restored.lits {
        replay.lit(lit);
    }
    for op in restored.ops {
        replay.op(buf, op);
    }
    for ((key, want), have) in KEYS.iter().zip(sf.keys).zip(df.keys) {
        if want == have || key.gs_key().is_some() || !relevant(*key, kind) {
            continue;
        }
        match want {
            Some(op) => replay.op(buf, op),
            None => replay.lit(key.reset()?),
        }
    }
    replay.paint(buf, sf.fill, df.fill, true);
    if kind != Kind::Image {
        replay.paint(buf, sf.stroke, df.stroke, false);
    }
    let mut out = b"\nq ".to_vec();
    if sf.ctm != df.ctm {
        let m = sf.ctm.post_concat(df.ctm.inverse()?);
        emit_matrix(&mut out, m);
        out.extend_from_slice(b" cm ");
    }
    out.extend_from_slice(&replay.into_bytes());
    out.push(b'\n');
    out.extend_from_slice(buf.get(span.0..span.1)?);
    out.extend_from_slice(b"\nQ\n");
    Some((out, reset))
}

/// The operators taking state `s` to `e` without painting, to stand in for
/// an object's bytes at its old position; `None` when its bytes change
/// state that cannot be replayed.
fn removal_delta(walk: &Walk, buf: &[u8], s: &State, e: &State) -> Option<Vec<u8>> {
    let (sf, ef) = (&s.frame, &e.frame);
    if !s.insertable
        || !e.insertable
        || sf.opaque != ef.opaque
        || s.mc != e.mc
        || sf.level != ef.level
        || sf.ctm != ef.ctm
    {
        return None;
    }
    let mut replay = Replay::default();
    for (op, _) in walk.gs_since(ef.gs, sf.gs)? {
        replay.op(buf, op);
    }
    for (want, have) in ef.keys.iter().zip(sf.keys) {
        if *want != have {
            replay.op(buf, (*want)?);
        }
    }
    replay.paint(buf, ef.fill, sf.fill, true);
    replay.paint(buf, ef.stroke, sf.stroke, false);
    let bytes = replay.into_bytes();
    Some(if bytes.is_empty() {
        Vec::new()
    } else {
        let mut out = bytes;
        out.pop();
        out
    })
}

fn emit_matrix(out: &mut Vec<u8>, m: Matrix) {
    for (i, v) in [m.a, m.b, m.c, m.d, m.e, m.f].into_iter().enumerate() {
        if i > 0 {
            out.push(b' ');
        }
        emit_number(out, v);
    }
}
