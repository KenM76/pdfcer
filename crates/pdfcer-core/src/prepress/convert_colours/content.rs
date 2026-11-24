//! The content side of convert colours: rewrite `g`/`rg`/`G`/`RG`,
//! `cs`/`CS` and `sc`/`scn`/`SC`/`SCN` under a source space as the
//! destination's own operator (ISO 32000-2 §8.6.8, Table 73), through page
//! content, forms, tiling patterns and annotation appearances.
//!
//! `cs` is replaced by the destination operator with the converted initial
//! colour, so the colour in force is the same before any `sc` follows. The
//! rendering intent in force is tracked from `ri` and `/RI` (§8.6.5.8) for
//! [`super::IntentChoice::ObjectsOwn`].

use std::collections::{BTreeMap, BTreeSet};

use crate::color::RenderingIntent;
use crate::content::{ContentStream, ContentTokenKind, Operation};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::page_tree::Page;
use crate::preflight::walk::first_operand;

use super::super::inks::Planned;
use super::super::rewrite::{
    Edit, appearances, apply, colour_text, edit, flate_writes, numbers, resource,
};
use super::Counted;
use super::sources::Registry;

/// Forms, patterns and appearances nest at most this deep.
const MAX_DEPTH: usize = 32;
/// Streams walked in all, counting a form once per time it is drawn.
const MAX_VISITS: usize = 4096;

/// The graphics state this rewrite tracks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct GState {
    /// The source fill space, by index into `Registry::sources`.
    fill: Option<usize>,
    /// The source stroke space.
    stroke: Option<usize>,
    /// The rendering intent in force.
    intent: RenderingIntent,
}

struct Walker<'r, 'a, 'd> {
    reg: &'r mut Registry<'a, 'd>,
    scope: &'r BTreeSet<ObjId>,
    active: Vec<ObjId>,
    visits: usize,
    /// Each in-scope stream's new decoded bytes, from the first time it was
    /// drawn; `None` when it needed no change.
    out: BTreeMap<ObjId, Option<Vec<u8>>>,
}

/// Rewrite the content and annotation appearances of `pages`, recording
/// streams in `scope` into `writes`.
pub(super) fn rewrite_pages(
    reg: &mut Registry<'_, '_>,
    pages: &[&Page],
    scope: &BTreeSet<ObjId>,
    writes: &mut BTreeMap<ObjId, Planned>,
) {
    let view = reg.view;
    let mut w = Walker {
        reg,
        scope,
        active: Vec::new(),
        visits: 0,
        out: BTreeMap::new(),
    };
    for page in pages {
        w.page(&page.contents, &page.resources);
        for ap in appearances(view, page.id) {
            w.form(ap, &Dict::default(), GState::default(), 0);
        }
    }
    flate_writes(view, w.out, writes);
}

impl Walker<'_, '_, '_> {
    /// A page's content streams, which share one graphics state.
    fn page(&mut self, contents: &[ObjId], resources: &Dict) {
        let mut stack = vec![GState::default()];
        let mut results = Vec::new();
        for id in contents {
            match ContentStream::from_form(self.reg.view, *id) {
                Ok(stream) => {
                    let edits = self.walk(*id, &stream, resources, &mut stack, 0);
                    results.push((*id, apply(&stream.buf, &edits)));
                }
                Err(e) => {
                    let why =
                        format!("page content cannot be read ({e}); none of it was converted");
                    self.reg.report.unconvert(Some(*id), why);
                    return;
                }
            }
        }
        for (id, bytes) in results {
            self.record(id, bytes);
        }
    }

    /// Keep an in-scope stream's first rewrite; report a later, different
    /// one.
    fn record(&mut self, id: ObjId, bytes: Option<Vec<u8>>) {
        if !self.scope.contains(&id) {
            return;
        }
        match self.out.get(&id) {
            None => {
                self.out.insert(id, bytes);
            }
            Some(first) if *first != bytes => {
                let why =
                    "it is drawn under different colour states; it was rewritten for the first"
                        .to_owned();
                self.reg.report.unconvert(Some(id), why);
            }
            Some(_) => {}
        }
    }

    /// A form, tiling pattern or appearance, drawn with `caller`'s state.
    fn form(&mut self, id: ObjId, outer: &Dict, caller: GState, depth: usize) {
        if depth >= MAX_DEPTH || self.active.contains(&id) {
            return;
        }
        self.visits += 1;
        if self.visits > MAX_VISITS {
            if self.visits == MAX_VISITS + 1 {
                self.reg.report.disclose(format!(
                    "The document draws more than {MAX_VISITS} nested streams; the rest were not converted."
                ));
            }
            return;
        }
        let view = self.reg.view;
        let Some(Object::Stream(s)) = view.graph().value(id) else {
            return;
        };
        let own = s
            .dict
            .get(b"Resources")
            .and_then(|o| view.resolve(o).as_dict())
            .cloned();
        match ContentStream::from_form(view, id) {
            Ok(stream) => {
                self.active.push(id);
                let mut stack = vec![caller];
                let res = own.as_ref().unwrap_or(outer);
                let edits = self.walk(id, &stream, res, &mut stack, depth);
                self.active.pop();
                let bytes = apply(&stream.buf, &edits);
                self.record(id, bytes);
            }
            Err(e) => {
                let why = format!("its content cannot be read ({e})");
                self.reg.report.unconvert(Some(id), why);
            }
        }
    }

    /// Walk one stream, returning its edits.
    fn walk(
        &mut self,
        id: ObjId,
        stream: &ContentStream,
        resources: &Dict,
        stack: &mut Vec<GState>,
        depth: usize,
    ) -> Vec<Edit> {
        let mut edits = Vec::new();
        for op in stream.operations() {
            if let ContentTokenKind::InlineImage { params, .. } = &op.operator.kind {
                self.inline_image(id, params, resources);
                continue;
            }
            let name = op.operator_name(&stream.buf).unwrap_or_default();
            if stack.is_empty() {
                stack.push(GState::default());
            }
            match name {
                b"q" => {
                    let top = stack.last().copied().unwrap_or_default();
                    stack.push(top);
                }
                b"Q" if stack.len() > 1 => {
                    stack.pop();
                }
                _ => {
                    let mut next = stack.last().copied().unwrap_or_default();
                    let ctx = Ctx {
                        id,
                        resources,
                        depth,
                    };
                    if let Some(text) = self.operator(&ctx, name, &op, &mut next) {
                        edits.push(edit(&op, text));
                    }
                    if let Some(top) = stack.last_mut() {
                        *top = next;
                    }
                }
            }
        }
        edits
    }

    /// One operator other than `q`/`Q`: update `state`, and return the
    /// replacement text when the operator is rewritten.
    fn operator(
        &mut self,
        ctx: &Ctx<'_>,
        name: &[u8],
        op: &Operation<'_>,
        state: &mut GState,
    ) -> Option<String> {
        let stroke = name.first().is_some_and(u8::is_ascii_uppercase);
        match name {
            b"cs" | b"CS" => {
                let hit = self.set_space(ctx, op);
                *slot(state, stroke) = hit;
                self.initial(ctx.id, hit?, state.intent, stroke)
            }
            b"sc" | b"scn" | b"SC" | b"SCN" => match *slot(state, stroke) {
                Some(src) => self.set_colour(ctx.id, op, src, state.intent, stroke),
                None if name.ends_with(b"n") || name.ends_with(b"N") => {
                    self.pattern(op, ctx.resources, ctx.depth);
                    None
                }
                None => None,
            },
            b"g" | b"G" | b"rg" | b"RG" => {
                *slot(state, stroke) = None;
                let device: &[u8] = if name.len() == 1 {
                    b"DeviceGray"
                } else {
                    b"DeviceRGB"
                };
                let device = Object::Name(Name::from(device));
                let src = self
                    .reg
                    .lookup(&device, Some(ctx.resources), Some(ctx.id))?;
                self.set_colour(ctx.id, op, src, state.intent, stroke)
            }
            b"k" | b"K" => {
                *slot(state, stroke) = None;
                None
            }
            b"ri" => {
                if let Some(Object::Name(n)) = first_operand(op) {
                    state.intent = RenderingIntent::from_name(n.as_bytes());
                }
                None
            }
            b"gs" => {
                self.ext_gstate(op, ctx.resources, state);
                None
            }
            b"Do" => {
                self.xobject(op, ctx.resources, *state, ctx.depth);
                None
            }
            _ => None,
        }
    }

    /// `cs`/`CS`: the source space it selects, if any.
    fn set_space(&mut self, ctx: &Ctx<'_>, op: &Operation<'_>) -> Option<usize> {
        let Some(Object::Name(n)) = first_operand(op) else {
            return None;
        };
        let view = self.reg.view;
        let object = match n.as_bytes() {
            b"DeviceRGB" | b"DeviceGray" | b"DeviceCMYK" | b"Pattern" => Object::Name(n.clone()),
            other => resource(view, ctx.resources, b"ColorSpace", other)?,
        };
        if let Some(items) = view.resolve(&object).as_array()
            && items
                .first()
                .and_then(Object::as_name)
                .is_some_and(|h| h.as_bytes() == b"Pattern")
        {
            if let Some(base) = items.get(1)
                && self
                    .reg
                    .lookup(base, Some(ctx.resources), Some(ctx.id))
                    .is_some()
            {
                let why = "an uncoloured pattern paints in a source space".to_owned();
                self.reg.report.unconvert(Some(ctx.id), why);
            }
            return None;
        }
        self.reg.lookup(&object, Some(ctx.resources), Some(ctx.id))
    }

    /// The destination operator for source `src`'s initial colour.
    fn initial(
        &mut self,
        id: ObjId,
        src: usize,
        intent: RenderingIntent,
        stroke: bool,
    ) -> Option<String> {
        let start = self.reg.sources.get(src)?.initial();
        let intent = self.reg.intent(intent);
        let (values, _) = self.reg.convert(src, &start, intent, Some(id))?;
        Some(colour_text(&values, self.reg.dest.operator(stroke)))
    }

    /// A colour operator under source `src`.
    fn set_colour(
        &mut self,
        id: ObjId,
        op: &Operation<'_>,
        src: usize,
        intent: RenderingIntent,
        stroke: bool,
    ) -> Option<String> {
        let family = self.reg.sources.get(src)?.family;
        let intent = self.reg.intent(intent);
        let converted = numbers(op).and_then(|v| self.reg.convert(src, &v, intent, Some(id)));
        let Some((values, oog)) = converted else {
            let why = format!(
                "a {} colour operator has operands it cannot use; it was set to the space's initial colour",
                family.name()
            );
            self.reg.report.unconvert(Some(id), why);
            let start = self.reg.sources.get(src)?.initial();
            let (values, _) = self.reg.convert(src, &start, intent, Some(id))?;
            return Some(colour_text(&values, self.reg.dest.operator(stroke)));
        };
        self.reg.report.count(family, Counted::Paint, oog);
        Some(colour_text(&values, self.reg.dest.operator(stroke)))
    }

    /// `gs`: track `/RI`.
    fn ext_gstate(&self, op: &Operation<'_>, resources: &Dict, state: &mut GState) {
        let view = self.reg.view;
        let Some(Object::Name(n)) = first_operand(op) else {
            return;
        };
        let entry = resource(view, resources, b"ExtGState", n.as_bytes());
        if let Some(ri) = entry
            .as_ref()
            .and_then(|e| view.resolve(e).as_dict())
            .and_then(|d| d.get(b"RI"))
            .and_then(|o| view.resolve(o).as_name())
        {
            state.intent = RenderingIntent::from_name(ri.as_bytes());
        }
    }

    fn xobject(&mut self, op: &Operation<'_>, resources: &Dict, state: GState, depth: usize) {
        let view = self.reg.view;
        let Some(Object::Name(n)) = first_operand(op) else {
            return;
        };
        let Some(Object::Reference(x)) = resource(view, resources, b"XObject", n.as_bytes()) else {
            return;
        };
        if let Some(Object::Stream(s)) = view.graph().value(x)
            && s.dict
                .get(b"Subtype")
                .and_then(Object::as_name)
                .is_some_and(|n| n.as_bytes() == b"Form")
        {
            self.form(x, resources, state, depth + 1);
        }
    }

    /// `scn` naming a tiling pattern: walk its cell.
    fn pattern(&mut self, op: &Operation<'_>, resources: &Dict, depth: usize) {
        let view = self.reg.view;
        let Some(Object::Name(n)) = op.operands.iter().rev().find_map(|t| match &t.kind {
            ContentTokenKind::Operand(o) => Some(o.clone()),
            _ => None,
        }) else {
            return;
        };
        if let Some(Object::Reference(p)) = resource(view, resources, b"Pattern", n.as_bytes())
            && matches!(view.graph().value(p), Some(Object::Stream(_)))
        {
            self.form(p, resources, GState::default(), depth + 1);
        }
    }

    /// An inline image in a source space is left as it is: its samples are
    /// part of the content stream (§8.9.7).
    fn inline_image(&mut self, id: ObjId, params: &Dict, resources: &Dict) {
        let view = self.reg.view;
        let Some(cs) = params.get(b"CS").or(params.get(b"ColorSpace")) else {
            return;
        };
        let object = match cs {
            Object::Name(n) => match n.as_bytes() {
                b"G" | b"RGB" | b"DeviceGray" | b"DeviceRGB" => cs.clone(),
                other => match resource(view, resources, b"ColorSpace", other) {
                    Some(o) => o,
                    None => return,
                },
            },
            other => other.clone(),
        };
        let base = match view.resolve(&object).as_array() {
            Some(items)
                if items
                    .first()
                    .and_then(Object::as_name)
                    .is_some_and(|h| matches!(h.as_bytes(), b"I" | b"Indexed")) =>
            {
                items.get(1).cloned()
            }
            _ => Some(object),
        };
        if let Some(base) = base
            && self.reg.lookup(&base, Some(resources), Some(id)).is_some()
        {
            let why = "an inline image paints in a source space; its samples were not converted"
                .to_owned();
            self.reg.report.unconvert(Some(id), why);
        }
    }
}

/// The fill or stroke space slot.
fn slot(state: &mut GState, stroke: bool) -> &mut Option<usize> {
    if stroke {
        &mut state.stroke
    } else {
        &mut state.fill
    }
}

/// Where an operator is.
struct Ctx<'a> {
    id: ObjId,
    resources: &'a Dict,
    depth: usize,
}
