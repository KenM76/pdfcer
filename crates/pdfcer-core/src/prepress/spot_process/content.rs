//! The content side of spot to process: rewrite `cs`/`CS` and
//! `sc`/`scn`/`SC`/`SCN` under a converted space as `k`/`K` (ISO 32000-2
//! §8.6.8, Table 73), through page content, forms, tiling patterns and
//! annotation appearances, and report paints whose overprint does not
//! survive (§8.6.7).
//!
//! After conversion a paint is `DeviceCMYK`, and overprint mode 1 keeps a
//! zero component from knocking out what is beneath, which is what the spot
//! did. Under mode 0, and for images and shadings, to which mode 1 never
//! applies, the converted paint knocks out; those streams are reported.

use std::collections::BTreeMap;

use crate::content::{ContentStream, ContentTokenKind, Operation};
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::preflight::walk::first_operand;

use super::super::inks::InkPlan;
use super::super::rewrite::{Edit, appearances, apply, colour_text, edit, flate_writes, resource};
use super::Spaces;

/// Forms, patterns and appearances nest at most this deep.
const MAX_DEPTH: usize = 32;
/// Streams walked in all, counting a form once per time it is drawn.
const MAX_VISITS: usize = 4096;

/// The graphics state this rewrite tracks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct GState {
    /// The converted fill space, by index into `Spaces::conversions`.
    fill: Option<usize>,
    /// The converted stroke space.
    stroke: Option<usize>,
    /// `/OP`.
    op_stroke: bool,
    /// `/op`, or `/OP` when `/op` is absent (§8.6.7).
    op_fill: bool,
    /// `/OPM` is 1.
    opm1: bool,
}

struct Rewriter<'a, 'v, 'd> {
    spaces: &'a mut Spaces<'v, 'd>,
    plan: &'a mut InkPlan,
    active: Vec<ObjId>,
    visits: usize,
    /// Each rewritten stream's new decoded bytes, from the first time it
    /// was drawn; `None` when it needed no change.
    out: BTreeMap<ObjId, Option<Vec<u8>>>,
}

/// Rewrite every page's content and annotation appearances.
pub(super) fn rewrite_pages(spaces: &mut Spaces<'_, '_>, plan: &mut InkPlan) {
    let view = spaces.view;
    let Ok(pages) = crate::page_tree::pages_in(view.graph()) else {
        plan.report
            .disclose("The page tree could not be read; no page content was converted.".to_owned());
        return;
    };
    let mut r = Rewriter {
        spaces,
        plan,
        active: Vec::new(),
        visits: 0,
        out: BTreeMap::new(),
    };
    for page in &pages {
        r.page(&page.contents, &page.resources);
        for ap in appearances(view, page.id) {
            r.form(ap, &Dict::default(), GState::default(), 0);
        }
    }
    let Rewriter { out, plan, .. } = r;
    flate_writes(view, out, &mut plan.writes);
}

impl Rewriter<'_, '_, '_> {
    /// A page's content streams, which share one graphics state.
    fn page(&mut self, contents: &[ObjId], resources: &Dict) {
        let mut stack = vec![GState::default()];
        let mut results = Vec::new();
        for id in contents {
            match ContentStream::from_form(self.spaces.view, *id) {
                Ok(stream) => {
                    let edits = self.walk(*id, &stream, resources, &mut stack, 0);
                    results.push((*id, apply(&stream.buf, &edits)));
                }
                Err(e) => {
                    let why =
                        format!("the page content cannot be read ({e}); none of it was converted");
                    self.plan.report.refuse(Some(*id), b"", why);
                    return;
                }
            }
        }
        for (id, bytes) in results {
            self.record(id, bytes);
        }
    }

    /// Keep a stream's first rewrite; report a later, different one.
    fn record(&mut self, id: ObjId, bytes: Option<Vec<u8>>) {
        match self.out.get(&id) {
            None => {
                self.out.insert(id, bytes);
            }
            Some(first) if *first != bytes => {
                let why =
                    "it is drawn under different colour states; it was rewritten for the first"
                        .to_owned();
                self.plan.report.refuse(Some(id), b"", why);
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
                self.plan.report.disclose(format!(
                    "The document draws more than {MAX_VISITS} nested streams; the rest were not converted."
                ));
            }
            return;
        }
        let view = self.spaces.view;
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
                let edits = self.walk(
                    id,
                    &stream,
                    own.as_ref().unwrap_or(outer),
                    &mut stack,
                    depth,
                );
                self.active.pop();
                let bytes = apply(&stream.buf, &edits);
                self.record(id, bytes);
            }
            Err(e) => {
                let why = format!("its content cannot be read ({e})");
                self.plan.report.refuse(Some(id), b"", why);
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
                    let state = stack.last_mut().map_or(GState::default(), |s| *s);
                    let mut next = state;
                    if let Some(text) = self.operator(id, name, &op, resources, &mut next, depth) {
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
        id: ObjId,
        name: &[u8],
        op: &Operation<'_>,
        resources: &Dict,
        state: &mut GState,
        depth: usize,
    ) -> Option<String> {
        match name {
            b"cs" | b"CS" => {
                let stroke = name == b"CS";
                let hit = self.set_space(id, op, resources);
                *if stroke {
                    &mut state.stroke
                } else {
                    &mut state.fill
                } = hit;
                let cmyk = self.spaces.conversions.get(hit?)?.initial()?;
                self.plan.report.paints += 1;
                Some(cmyk_text(cmyk, stroke))
            }
            b"sc" | b"scn" | b"SC" | b"SCN" => {
                let stroke = name.first() == Some(&b'S');
                let conv = if stroke { state.stroke } else { state.fill };
                match conv {
                    Some(c) => self.set_colour(id, op, c, stroke),
                    None if name == b"scn" || name == b"SCN" => {
                        self.pattern(op, resources, depth);
                        None
                    }
                    None => None,
                }
            }
            b"g" | b"rg" | b"k" => {
                state.fill = None;
                None
            }
            b"G" | b"RG" | b"K" => {
                state.stroke = None;
                None
            }
            b"gs" => {
                self.ext_gstate(op, resources, state);
                None
            }
            b"Do" => {
                self.xobject(id, op, resources, *state, depth);
                None
            }
            b"sh" => {
                self.shading(id, op, resources, *state);
                None
            }
            _ => {
                self.paint(id, name, *state);
                None
            }
        }
    }

    /// `cs`/`CS`: the converted space it selects, if any.
    fn set_space(&mut self, id: ObjId, op: &Operation<'_>, resources: &Dict) -> Option<usize> {
        let Some(Object::Name(n)) = first_operand(op) else {
            return None;
        };
        let object = resource(self.spaces.view, resources, b"ColorSpace", n.as_bytes())?;
        let view = self.spaces.view;
        if let Some(items) = view.resolve(&object).as_array()
            && items
                .first()
                .and_then(Object::as_name)
                .is_some_and(|h| h.as_bytes() == b"Pattern")
        {
            if let Some(base) = items.get(1)
                && let Some(c) = self.spaces.lookup(base, Some(id), self.plan)
            {
                let ink = self
                    .spaces
                    .conversions
                    .get(c)
                    .map(|v| v.ink().to_vec())
                    .unwrap_or_default();
                let why = "an uncoloured pattern paints in it".to_owned();
                self.plan.report.refuse(Some(id), &ink, why);
            }
            return None;
        }
        self.spaces.lookup(&object, Some(id), self.plan)
    }

    /// `sc`/`scn` under converted space `c`.
    fn set_colour(
        &mut self,
        id: ObjId,
        op: &Operation<'_>,
        c: usize,
        stroke: bool,
    ) -> Option<String> {
        let conv = self.spaces.conversions.get(c)?;
        let tints: Vec<f64> = op
            .operands
            .iter()
            .filter_map(|t| match &t.kind {
                ContentTokenKind::Operand(o) => o.as_number(),
                _ => None,
            })
            .collect();
        let Some(cmyk) = conv
            .cmyk(&tints)
            .filter(|_| tints.len() == op.operands.len())
        else {
            let ink = conv.ink().to_vec();
            let why = format!(
                "a colour operator has {} operands it cannot use",
                op.operands.len()
            );
            self.plan.report.refuse(Some(id), &ink, why);
            return Some(cmyk_text(conv.initial()?, stroke));
        };
        self.plan.report.paints += 1;
        Some(cmyk_text(cmyk, stroke))
    }

    fn ext_gstate(&self, op: &Operation<'_>, resources: &Dict, state: &mut GState) {
        let view = self.spaces.view;
        let Some(Object::Name(n)) = first_operand(op) else {
            return;
        };
        let Some(entry) = resource(view, resources, b"ExtGState", n.as_bytes()) else {
            return;
        };
        let Some(d) = view.resolve(&entry).as_dict() else {
            return;
        };
        let flag = |k: &[u8]| match d.get(k).map(|o| view.resolve(o)) {
            Some(Object::Boolean(b)) => Some(*b),
            _ => None,
        };
        if let Some(op_) = flag(b"OP") {
            state.op_stroke = op_;
            state.op_fill = op_;
        }
        if let Some(fill) = flag(b"op") {
            state.op_fill = fill;
        }
        if let Some(m) = d.get(b"OPM").and_then(|o| view.resolve(o).as_number()) {
            state.opm1 = m == 1.0;
        }
    }

    /// Report a fill or stroke that loses its overprint.
    fn paint(&mut self, id: ObjId, name: &[u8], state: GState) {
        let fills = matches!(
            name,
            b"f" | b"F" | b"f*" | b"b" | b"b*" | b"B" | b"B*" | b"Tj" | b"TJ" | b"'" | b"\""
        );
        let strokes = matches!(name, b"S" | b"s" | b"b" | b"b*" | b"B" | b"B*");
        let lost = !state.opm1
            && ((fills && state.fill.is_some() && state.op_fill)
                || (strokes && state.stroke.is_some() && state.op_stroke));
        if lost {
            self.lost(id);
        }
    }

    fn lost(&mut self, id: ObjId) {
        if !self.plan.report.overprint_lost.contains(&id) {
            self.plan.report.overprint_lost.push(id);
        }
    }

    fn xobject(
        &mut self,
        id: ObjId,
        op: &Operation<'_>,
        resources: &Dict,
        state: GState,
        depth: usize,
    ) {
        let view = self.spaces.view;
        let Some(Object::Name(n)) = first_operand(op) else {
            return;
        };
        let Some(Object::Reference(x)) = resource(view, resources, b"XObject", n.as_bytes()) else {
            return;
        };
        let Some(Object::Stream(s)) = view.graph().value(x) else {
            return;
        };
        match s
            .dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .map(|n| n.as_bytes())
        {
            Some(b"Form") => self.form(x, resources, state, depth + 1),
            Some(b"Image") => {
                let cs = s.dict.get(b"ColorSpace").cloned();
                if let Some(cs) = cs
                    && self.spaces.lookup(&cs, Some(x), self.plan).is_some()
                    && state.op_fill
                {
                    self.lost(id);
                }
            }
            _ => {}
        }
    }

    fn shading(&mut self, id: ObjId, op: &Operation<'_>, resources: &Dict, state: GState) {
        let view = self.spaces.view;
        let Some(Object::Name(n)) = first_operand(op) else {
            return;
        };
        let cs = resource(view, resources, b"Shading", n.as_bytes()).and_then(|s| {
            view.resolve(&s)
                .as_dict()
                .and_then(|d| d.get(b"ColorSpace"))
                .cloned()
        });
        if let Some(cs) = cs
            && self.spaces.lookup(&cs, None, self.plan).is_some()
            && state.op_fill
        {
            self.lost(id);
        }
    }

    /// `scn` naming a tiling pattern: walk its cell.
    fn pattern(&mut self, op: &Operation<'_>, resources: &Dict, depth: usize) {
        let view = self.spaces.view;
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

    /// An inline image in a converted space is refused: its samples are
    /// part of the content stream.
    fn inline_image(&mut self, id: ObjId, params: &Dict, resources: &Dict) {
        let Some(Object::Name(n)) = params.get(b"CS").or(params.get(b"ColorSpace")) else {
            return;
        };
        let Some(object) = resource(self.spaces.view, resources, b"ColorSpace", n.as_bytes())
        else {
            return;
        };
        if let Some(c) = self.spaces.lookup(&object, Some(id), self.plan) {
            let ink = self
                .spaces
                .conversions
                .get(c)
                .map(|v| v.ink().to_vec())
                .unwrap_or_default();
            let why = "an inline image paints in it; its samples were not converted".to_owned();
            self.plan.report.refuse(Some(id), &ink, why);
        }
    }
}

/// `c m y k k`, or `K` for a stroke.
fn cmyk_text(cmyk: [f64; 4], stroke: bool) -> String {
    colour_text(&cmyk, if stroke { "K" } else { "k" })
}
