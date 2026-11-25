//! Planning `/Artifact` wraps for a page's untagged content (ISO 14289-1
//! §7.1 ¶2, ISO 14289-2 §8.2.2: content is tagged or an artifact).
//!
//! An artifact is `/Artifact BMC … EMC` (ISO 32000-1 §14.8.2.2.2). A wrap
//! must nest properly with the page's other marked-content sequences and
//! with `q`/`Q`, `BT`/`ET` and `BX`/`EX` (§14.6; ISO 32000-2 erratum 302),
//! and must never split a path object, so the content is first read into a
//! tree of those pairs and each wrap opens and closes inside one of them.
//!
//! Classification matches the validator (`content.rs`): a sequence is
//! tagged when the structure tree claims its `/MCID` for the page; inside
//! a form any `/MCID` is taken as tagged and a form with `/StructParent` is
//! tagged as a whole. A sequence whose `/MCID` no element claims is left
//! alone and counted: it was meant to be tagged, so calling it decorative
//! would hide real content.

use std::collections::HashSet;

use super::{PdfuaPart, name_of, structure};
use crate::content::{ContentStream, ContentTokenKind, Operation};
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::page_tree::Page;
use crate::preflight::walk::{first_operand, last_operand, resource};
use crate::view::DocumentView;

/// Forms nest at most this deep.
const MAX_DEPTH: usize = 32;
const OPEN: &[u8] = b"/Artifact BMC\n";
const CLOSE: &[u8] = b"\nEMC\n";

/// What [`plan_page`] would do to one page.
#[derive(Debug, Clone, Default)]
pub(crate) struct ArtifactPlan {
    /// The page's whole content with the wraps inserted; unchanged when
    /// `sequences` is 0.
    pub(crate) content: Vec<u8>,
    /// `/Artifact` sequences inserted.
    pub(crate) sequences: usize,
    /// Content items (painting operators, images, forms drawn) they cover.
    pub(crate) items: usize,
    /// Sequences with an `/MCID` the structure tree does not claim, left
    /// untouched.
    pub(crate) orphan_marks: usize,
    /// Forms holding tagged content beside untagged content; a wrap cannot
    /// reach inside them, so their untagged content is left.
    pub(crate) mixed_forms: Vec<ObjId>,
    /// Forms whose content, or a form they draw, could not be read in
    /// full; never wrapped, so untagged content may remain inside them.
    pub(crate) unread_forms: Vec<ObjId>,
}

/// Every `(page, MCID)` the structure tree claims, or `None` when the
/// catalog has no `/StructTreeRoot`.
pub(crate) fn claimed_mcids(view: &DocumentView<'_>) -> Option<HashSet<(ObjId, i64)>> {
    let tree = structure::read(view, PdfuaPart::Ua1);
    tree.present.then(|| tree.mcids.keys().copied().collect())
}

/// Plan the wraps that make every untagged content item on `page` an
/// artifact.
///
/// # Errors
///
/// A reason, when the content cannot be read or its `q`/`Q`, `BT`/`ET`,
/// `BX`/`EX` or marked-content operators do not pair up; nothing is planned.
pub(crate) fn plan_page(
    view: &DocumentView<'_>,
    page: &Page,
    claimed: &HashSet<(ObjId, i64)>,
) -> Result<ArtifactPlan, String> {
    let stream = ContentStream::from_page(view, page).map_err(|e| format!("its content: {e}"))?;
    let mut plan = ArtifactPlan::default();
    let mut reader = Reader {
        view,
        page: page.id,
        claimed,
        resources: &page.resources,
        plan: &mut plan,
    };
    let frames = reader.read(&stream)?;
    let mut splices: Vec<(usize, u8, &'static [u8])> = Vec::new();
    let root = frames.len() - 1;
    wrap_frame(&frames, root, &mut splices, &mut plan);
    splices.sort_by_key(|&(at, order, _)| (at, order));
    let mut content = Vec::with_capacity(stream.buf.len() + splices.len() * OPEN.len());
    let mut from = 0;
    for (at, _, bytes) in splices {
        content.extend_from_slice(stream.buf.get(from..at).unwrap_or_default());
        content.extend_from_slice(bytes);
        from = at;
    }
    content.extend_from_slice(stream.buf.get(from..).unwrap_or_default());
    plan.content = content;
    Ok(plan)
}

/// What a pair encloses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Root,
    Save,
    Text,
    Compat,
    Mark(MarkClass),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkClass {
    /// Claimed by the structure tree, or an `/Artifact`: already covered.
    Covered,
    /// An `/MCID` nothing claims.
    Orphan,
    /// Any other sequence (`/OC`, `/Span` without an MCID, …).
    Other,
}

/// One unit of a pair's body.
#[derive(Debug, Clone, Copy)]
enum Unit {
    Leaf {
        start: usize,
        end: usize,
        class: Leaf,
    },
    Frame(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Leaf {
    /// An untagged content item.
    Item,
    /// An operator that draws nothing.
    Neutral,
    /// Something a wrap must not enclose: tagged content.
    Barrier,
}

#[derive(Debug)]
struct Frame {
    kind: Kind,
    start: usize,
    end: usize,
    units: Vec<Unit>,
    /// Untagged items anywhere inside.
    items: usize,
    /// A covered or orphan sequence, or a tagged form, anywhere inside.
    barrier: bool,
}

impl Frame {
    const fn new(kind: Kind, start: usize) -> Self {
        Self {
            kind,
            start,
            end: start,
            units: Vec::new(),
            items: 0,
            barrier: matches!(kind, Kind::Mark(MarkClass::Covered | MarkClass::Orphan)),
        }
    }
}

struct Reader<'r, 'v, 'd> {
    view: &'v DocumentView<'d>,
    page: ObjId,
    claimed: &'r HashSet<(ObjId, i64)>,
    resources: &'r Dict,
    plan: &'r mut ArtifactPlan,
}

impl Reader<'_, '_, '_> {
    /// The pairs of `stream`, children before parents; the root is last.
    fn read(&mut self, stream: &ContentStream) -> Result<Vec<Frame>, String> {
        let mut done: Vec<Frame> = Vec::new();
        let mut open: Vec<Frame> = vec![Frame::new(Kind::Root, 0)];
        for op in stream.operations() {
            let (start, end) = span(&op);
            let name = match &op.operator.kind {
                ContentTokenKind::InlineImage { .. } => b"BI".as_slice(),
                _ => op.operator_name(&stream.buf).unwrap_or_default(),
            };
            let opens = match name {
                b"q" => Some(Kind::Save),
                b"BT" => Some(Kind::Text),
                b"BX" => Some(Kind::Compat),
                b"BMC" | b"BDC" => Some(Kind::Mark(self.mark_class(&op, name == b"BDC"))),
                _ => None,
            };
            if let Some(kind) = opens {
                open.push(Frame::new(kind, start));
                continue;
            }
            if let Some(closer) = closes(name) {
                let mut frame = open.pop().filter(|f| closer(f.kind)).ok_or_else(|| {
                    format!(
                        "its content operators do not pair up (an unmatched {})",
                        text(name)
                    )
                })?;
                frame.end = end;
                if frame.kind == Kind::Mark(MarkClass::Orphan) {
                    self.plan.orphan_marks += 1;
                }
                let parent = open.last_mut().ok_or("its content has no root")?;
                parent.items += frame.items;
                parent.barrier |= frame.barrier;
                parent.units.push(Unit::Frame(done.len()));
                done.push(frame);
                continue;
            }
            let class = self.leaf(&op, name);
            let top = open.last_mut().ok_or("its content has no root")?;
            top.items += usize::from(class == Leaf::Item);
            top.barrier |= class == Leaf::Barrier;
            top.units.push(Unit::Leaf { start, end, class });
        }
        if open.len() != 1 {
            return Err("its content leaves a q, BT, BX or marked-content sequence open".into());
        }
        done.extend(open.pop());
        Ok(done)
    }

    fn mark_class(&self, op: &Operation<'_>, bdc: bool) -> MarkClass {
        if matches!(first_operand(op), Some(Object::Name(n)) if n.as_bytes() == b"Artifact") {
            return MarkClass::Covered;
        }
        match props(self.view, self.resources, op, bdc).get(b"MCID") {
            Some(Object::Integer(mcid)) if self.claimed.contains(&(self.page, *mcid)) => {
                MarkClass::Covered
            }
            Some(_) => MarkClass::Orphan,
            None => MarkClass::Other,
        }
    }

    fn leaf(&mut self, op: &Operation<'_>, name: &[u8]) -> Leaf {
        match name {
            b"BI" | b"Tj" | b"TJ" | b"'" | b"\"" | b"S" | b"s" | b"f" | b"F" | b"f*" | b"B"
            | b"B*" | b"b" | b"b*" | b"sh" => Leaf::Item,
            b"Do" => self.xobject(op),
            _ => Leaf::Neutral,
        }
    }

    fn xobject(&mut self, op: &Operation<'_>) -> Leaf {
        let Some(id) = xobject_id(self.view, self.resources, op) else {
            return Leaf::Neutral;
        };
        let Some(Object::Stream(x)) = self.view.graph().value(id) else {
            return Leaf::Neutral;
        };
        match name_of(self.view, x.dict.get(b"Subtype")) {
            // An image or form with `/StructParent` is itself a structure
            // content item (ISO 32000-1 14.7.4.3, Tables 89 and 95).
            Some(b"Image" | b"Form") if x.dict.contains_key(b"StructParent") => Leaf::Barrier,
            Some(b"Image") => Leaf::Item,
            Some(b"Form") => {
                let mut census = FormCensus::default();
                census.walk(self.view, id, self.resources, 0, &mut Vec::new());
                if census.unread {
                    if !self.plan.unread_forms.contains(&id) {
                        self.plan.unread_forms.push(id);
                    }
                    return Leaf::Barrier;
                }
                if !census.tagged {
                    return if census.untagged > 0 {
                        Leaf::Item
                    } else {
                        Leaf::Neutral
                    };
                }
                if census.untagged > 0 && !self.plan.mixed_forms.contains(&id) {
                    self.plan.mixed_forms.push(id);
                }
                Leaf::Barrier
            }
            _ => Leaf::Neutral,
        }
    }
}

/// The test a frame on the stack must pass to be closed by `name`.
fn closes(name: &[u8]) -> Option<fn(Kind) -> bool> {
    match name {
        b"Q" => Some(|k| k == Kind::Save),
        b"ET" => Some(|k| k == Kind::Text),
        b"EX" => Some(|k| k == Kind::Compat),
        b"EMC" => Some(|k| matches!(k, Kind::Mark(_))),
        _ => None,
    }
}

/// Insert the wraps for `frames[index]`'s body, then for each child that
/// holds both tagged and untagged content.
fn wrap_frame(
    frames: &[Frame],
    index: usize,
    splices: &mut Vec<(usize, u8, &'static [u8])>,
    plan: &mut ArtifactPlan,
) {
    let Some(frame) = frames.get(index) else {
        return;
    };
    // A segment runs from just after one barrier to just before the next; its
    // wrap opens at its first unit (so no path object is split) and closes
    // after its last item.
    let mut segment_start: Option<usize> = None;
    let mut last_item: Option<(usize, usize)> = None;
    let mut items = 0;
    for unit in &frame.units {
        let (start, end, status, count) = status(frames, *unit);
        if status == Leaf::Barrier {
            flush(splices, plan, segment_start.take(), last_item.take(), items);
            items = 0;
            if let Unit::Frame(child) = unit {
                recurse(frames, *child, splices, plan);
            }
            continue;
        }
        segment_start.get_or_insert(start);
        if status == Leaf::Item {
            last_item = Some((start, end));
            items += count;
        }
    }
    flush(splices, plan, segment_start, last_item, items);
}

/// Record one wrap from `first` to the end of `last`, when there is an item.
fn flush(
    splices: &mut Vec<(usize, u8, &'static [u8])>,
    plan: &mut ArtifactPlan,
    first: Option<usize>,
    last: Option<(usize, usize)>,
    items: usize,
) {
    if let (Some(start), Some((_, end))) = (first, last) {
        splices.push((start, 1, OPEN));
        splices.push((end, 0, CLOSE));
        plan.sequences += 1;
        plan.items += items;
    }
}

/// Descend into a barrier child unless it is itself covered or orphaned.
fn recurse(
    frames: &[Frame],
    child: usize,
    splices: &mut Vec<(usize, u8, &'static [u8])>,
    plan: &mut ArtifactPlan,
) {
    let descend = frames
        .get(child)
        .is_some_and(|f| !matches!(f.kind, Kind::Mark(MarkClass::Covered | MarkClass::Orphan)));
    if descend {
        wrap_frame(frames, child, splices, plan);
    }
}

/// A unit's span, how a wrap treats it, and the items it holds.
fn status(frames: &[Frame], unit: Unit) -> (usize, usize, Leaf, usize) {
    match unit {
        Unit::Leaf { start, end, class } => (start, end, class, usize::from(class == Leaf::Item)),
        Unit::Frame(i) => {
            let Some(f) = frames.get(i) else {
                return (0, 0, Leaf::Barrier, 0);
            };
            let class = if f.barrier {
                Leaf::Barrier
            } else if f.items > 0 {
                Leaf::Item
            } else {
                Leaf::Neutral
            };
            (f.start, f.end, class, f.items)
        }
    }
}

/// A form's content, read as the validator reads it below the page.
#[derive(Debug, Default)]
struct FormCensus {
    /// It draws marked content with an `/MCID` or a tagged form.
    tagged: bool,
    /// Items neither inside an `/MCID` sequence nor an artifact.
    untagged: usize,
    /// Its content, or a form it draws, could not be read in full (past
    /// the depth cap, or malformed): never wrap what cannot be seen.
    unread: bool,
}

impl FormCensus {
    fn walk(
        &mut self,
        view: &DocumentView<'_>,
        id: ObjId,
        outer: &Dict,
        depth: usize,
        active: &mut Vec<ObjId>,
    ) {
        // A form already on the stack is being read by an outer call.
        if active.contains(&id) {
            return;
        }
        if depth >= MAX_DEPTH {
            self.unread = true;
            return;
        }
        let Some(Object::Stream(s)) = view.graph().value(id) else {
            return;
        };
        let own = s
            .dict
            .get(b"Resources")
            .map(|o| view.resolve(o))
            .and_then(Object::as_dict)
            .cloned();
        let resources = own.as_ref().unwrap_or(outer);
        let Ok(stream) = ContentStream::from_form(view, id) else {
            self.unread = true;
            return;
        };
        active.push(id);
        let mut covered: Vec<bool> = Vec::new();
        for op in stream.operations() {
            let inside = covered.iter().any(|c| *c);
            let name = match &op.operator.kind {
                ContentTokenKind::InlineImage { .. } => b"BI".as_slice(),
                _ => op.operator_name(&stream.buf).unwrap_or_default(),
            };
            match name {
                b"BMC" | b"BDC" => {
                    let artifact = matches!(first_operand(&op),
                        Some(Object::Name(n)) if n.as_bytes() == b"Artifact");
                    let mcid = props(view, resources, &op, name == b"BDC").contains_key(b"MCID");
                    self.tagged |= mcid;
                    covered.push(artifact || mcid);
                }
                b"EMC" => {
                    covered.pop();
                }
                b"Do" => self.nested(view, resources, &op, inside, depth, active),
                b"BI" | b"Tj" | b"TJ" | b"'" | b"\"" | b"S" | b"s" | b"f" | b"F" | b"f*" | b"B"
                | b"B*" | b"b" | b"b*" | b"sh" => {
                    self.untagged += usize::from(!inside);
                }
                _ => {}
            }
        }
        active.pop();
    }

    fn nested(
        &mut self,
        view: &DocumentView<'_>,
        resources: &Dict,
        op: &Operation<'_>,
        inside: bool,
        depth: usize,
        active: &mut Vec<ObjId>,
    ) {
        let Some(id) = xobject_id(view, resources, op) else {
            return;
        };
        let Some(Object::Stream(x)) = view.graph().value(id) else {
            return;
        };
        match name_of(view, x.dict.get(b"Subtype")) {
            Some(b"Image" | b"Form") if x.dict.contains_key(b"StructParent") => self.tagged = true,
            Some(b"Image") => self.untagged += usize::from(!inside),
            Some(b"Form") if !inside => self.walk(view, id, resources, depth + 1, active),
            Some(b"Form") => {
                let mut inner = Self::default();
                inner.walk(view, id, resources, depth + 1, active);
                self.tagged |= inner.tagged;
                self.unread |= inner.unread;
            }
            _ => {}
        }
    }
}

/// The byte range of one operation, operands included.
fn span(op: &Operation<'_>) -> (usize, usize) {
    let start = op
        .operands
        .first()
        .map_or(op.operator.span.start, |t| t.span.start);
    (start, op.operator.span.end())
}

/// A `BDC`'s property list, inline or through `/Properties`.
fn props(view: &DocumentView<'_>, resources: &Dict, op: &Operation<'_>, bdc: bool) -> Dict {
    let found = match last_operand(op).filter(|_| bdc) {
        Some(Object::Dict(d)) => Some(d),
        Some(Object::Name(n)) => resource(view, resources, b"Properties", n.as_bytes())
            .map(|o| view.resolve(o))
            .and_then(Object::as_dict)
            .cloned(),
        _ => None,
    };
    found.unwrap_or_default()
}

fn xobject_id(view: &DocumentView<'_>, resources: &Dict, op: &Operation<'_>) -> Option<ObjId> {
    let Some(Object::Name(n)) = first_operand(op) else {
        return None;
    };
    resource(view, resources, b"XObject", n.as_bytes()).and_then(Object::as_reference)
}

fn text(name: &[u8]) -> String {
    String::from_utf8_lossy(name).into_owned()
}
