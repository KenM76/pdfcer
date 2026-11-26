//! The content walk: every content item is tagged or an artifact, the two
//! never nest inside each other, tagged text has a language, and no
//! reference XObject is drawn (ISO 14289-1 §7.1 ¶1–3, §7.2 ¶3, §7.20;
//! ISO 14289-2 §8.2.2; veraPDF PDFUA-1 7.1-1…3, 7.2-34, 7.20-1).
//!
//! A marked-content sequence is *tagged* when its `/MCID` is claimed by a
//! structure element: on the page by an integer kid or an MCR without
//! `/Stm`, inside a form XObject by an MCR whose `/Stm` is that form
//! (ISO 32000-1 §14.7.4.2, Table 324). Inside a form a page claim on the same id is
//! also accepted, for producers that leave `/Stm` out. A form with a
//! `/StructParent` is tagged as a whole.

use super::fonts::Used;
use super::structure::Tree;
use super::{FindingTarget, Out, name_of, string_of};
use crate::content::{ContentStream, ContentTokenKind, Operation};
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::page_tree::Page;
use crate::preflight::walk::{first_operand, last_operand, resource};
use crate::view::DocumentView;

/// Forms nest at most this deep.
const MAX_DEPTH: usize = 32;

/// What encloses the operator being read.
#[derive(Debug, Clone, Copy, Default)]
struct Ctx {
    artifact: bool,
    tagged: bool,
    lang: bool,
}

/// One open marked-content sequence.
#[derive(Debug, Clone, Copy)]
struct Mark {
    artifact: bool,
    tagged: bool,
    lang: bool,
}

#[derive(Debug, Default)]
struct Counts {
    artifact_in_tagged: usize,
    tagged_in_artifact: usize,
    untagged: usize,
    no_lang: usize,
    reference_forms: Vec<ObjId>,
    unread: Vec<String>,
}

struct Walker<'v, 'd> {
    view: &'v DocumentView<'d>,
    tree: &'v Tree,
    page: ObjId,
    /// The form XObject being read, `None` on the page itself.
    stream: Option<ObjId>,
    used: &'v mut Used,
    counts: Counts,
    active: Vec<u32>,
}

/// Walk one page's content and report what it found.
pub(super) fn check(
    view: &DocumentView<'_>,
    tree: &Tree,
    page: &Page,
    number: usize,
    used: &mut Used,
    out: &mut Out,
) {
    let mut w = Walker {
        view,
        tree,
        page: page.id,
        stream: None,
        used,
        counts: Counts::default(),
        active: Vec::new(),
    };
    match ContentStream::from_page(view, page) {
        Ok(stream) => w.walk(&stream, &page.resources, Ctx::default(), 0),
        Err(e) => w.counts.unread.push(format!("the page content: {e}")),
    }
    report(&w.counts, number, out);
}

fn report(c: &Counts, page: usize, out: &mut Out) {
    let at = Some(page);
    let t = FindingTarget::Page;
    for what in &c.unread {
        let message = format!("{what} could not be read, so its tagging was not checked.");
        out.incomplete("PUI-01-005", "PUI2-8.2.2-1", at, t, message);
    }
    if c.artifact_in_tagged > 0 {
        let n = c.artifact_in_tagged;
        let message = format!("{n} artifact sequence(s) sit inside tagged content.");
        out.violation("PUI-01-003", "", at, t, message);
    }
    if c.tagged_in_artifact > 0 {
        let n = c.tagged_in_artifact;
        let message = format!("{n} tagged sequence(s) sit inside an artifact.");
        out.violation("PUI-01-004", "", at, t, message);
    }
    if c.untagged > 0 {
        let n = c.untagged;
        let message = format!("{n} content item(s) are neither tagged nor marked as artifacts.");
        out.violation("PUI-01-005", "PUI2-8.2.2-1", at, t, message);
    }
    if c.no_lang > 0 {
        let n = c.no_lang;
        let message = format!("{n} tagged text item(s) have no determinable language.");
        out.violation("PUI-11-001", "", at, t, message);
    }
    for id in &c.reference_forms {
        let message = format!("The form XObject {id} is a reference XObject (/Ref).");
        out.violation("PUI-30-001", "", at, t, message);
    }
}

impl Walker<'_, '_> {
    fn walk(&mut self, stream: &ContentStream, resources: &Dict, base: Ctx, depth: usize) {
        let mut marks: Vec<Mark> = Vec::new();
        let mut fonts: Vec<(Option<ObjId>, bool)> = vec![(None, true)];
        for op in stream.operations() {
            let ctx = enclosing(base, &marks);
            if let ContentTokenKind::InlineImage { .. } = &op.operator.kind {
                self.item(ctx, false);
                continue;
            }
            let name = op.operator_name(&stream.buf).unwrap_or_default();
            match name {
                b"BMC" | b"BDC" => {
                    let mark = self.open(&op, name == b"BDC", resources, ctx);
                    marks.push(mark);
                }
                b"EMC" => {
                    marks.pop();
                }
                b"q" => fonts.push(*fonts.last().unwrap_or(&(None, true))),
                b"Q" if fonts.len() > 1 => {
                    fonts.pop();
                }
                b"Tf" => self.set_font(&op, resources, &mut fonts),
                b"Tr" => set_render_mode(&op, &mut fonts),
                b"Tj" | b"TJ" | b"'" | b"\"" => {
                    if let Some((Some(id), visible)) = fonts.last().copied() {
                        self.used.record(id, visible);
                    }
                    self.item(ctx, true);
                }
                b"S" | b"s" | b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" | b"sh" => {
                    self.item(ctx, false);
                }
                b"Do" => self.xobject(&op, resources, ctx, depth),
                _ => {}
            }
        }
    }

    /// One content item drawn in `ctx` (veraPDF 7.1-3, 7.2-34).
    fn item(&mut self, ctx: Ctx, text: bool) {
        if !ctx.artifact && !ctx.tagged {
            self.counts.untagged += 1;
        }
        if text && ctx.tagged && !ctx.artifact && !ctx.lang && self.tree.catalog_lang.is_none() {
            self.counts.no_lang += 1;
        }
    }

    /// A `BMC` or `BDC`: classify the sequence it opens (veraPDF 7.1-1, 7.1-2).
    fn open(&mut self, op: &Operation<'_>, bdc: bool, resources: &Dict, ctx: Ctx) -> Mark {
        let tag = first_operand(op);
        let artifact = matches!(&tag, Some(Object::Name(n)) if n.as_bytes() == b"Artifact");
        let props = match last_operand(op).filter(|_| bdc) {
            Some(Object::Dict(d)) => Some(d),
            Some(Object::Name(n)) => resource(self.view, resources, b"Properties", n.as_bytes())
                .map(|o| self.view.resolve(o))
                .and_then(Object::as_dict)
                .cloned(),
            _ => None,
        };
        let props = props.unwrap_or_default();
        let mut lang = string_of(self.view, props.get(b"Lang")).is_some();
        let mut tagged = false;
        if let Some(Object::Integer(mcid)) = props.get(b"MCID") {
            let elem = self
                .stream
                .and_then(|f| self.tree.stream_mcids.get(&(f, *mcid)))
                .or_else(|| self.tree.mcids.get(&(self.page, *mcid)));
            if let Some(&elem) = elem {
                tagged = true;
                lang |= self.tree.has_lang(elem);
            }
        }
        if artifact && ctx.tagged {
            self.counts.artifact_in_tagged += 1;
        }
        if tagged && ctx.artifact {
            self.counts.tagged_in_artifact += 1;
        }
        Mark {
            artifact,
            tagged,
            lang,
        }
    }

    fn set_font(&self, op: &Operation<'_>, resources: &Dict, fonts: &mut [(Option<ObjId>, bool)]) {
        let Some(Object::Name(n)) = first_operand(op) else {
            return;
        };
        let id =
            resource(self.view, resources, b"Font", n.as_bytes()).and_then(Object::as_reference);
        if let Some(top) = fonts.last_mut() {
            top.0 = id;
        }
    }

    fn xobject(&mut self, op: &Operation<'_>, resources: &Dict, ctx: Ctx, depth: usize) {
        let Some(Object::Name(n)) = first_operand(op) else {
            return;
        };
        let Some(Object::Reference(id)) = resource(self.view, resources, b"XObject", n.as_bytes())
        else {
            return;
        };
        let id = *id;
        let Some(Object::Stream(x)) = self.view.graph().value(id) else {
            return;
        };
        match name_of(self.view, x.dict.get(b"Subtype")) {
            Some(b"Image") => {
                let mut own = ctx;
                own.tagged |= x.dict.contains_key(b"StructParent");
                self.item(own, false);
            }
            Some(b"Form") => {
                if x.dict.contains_key(b"Ref") {
                    self.counts.reference_forms.push(id);
                }
                let mut inner = ctx;
                inner.tagged |= x.dict.contains_key(b"StructParent");
                self.form(id, resources, inner, depth);
            }
            _ => {}
        }
    }

    fn form(&mut self, id: ObjId, outer: &Dict, ctx: Ctx, depth: usize) {
        if self.active.contains(&id.num) {
            return;
        }
        if depth >= MAX_DEPTH {
            let why = format!("the form XObject {id}, nested more than {MAX_DEPTH} forms deep,");
            self.counts.unread.push(why);
            return;
        }
        let Some(Object::Stream(s)) = self.view.graph().value(id) else {
            return;
        };
        let own = s
            .dict
            .get(b"Resources")
            .map(|o| self.view.resolve(o))
            .and_then(Object::as_dict)
            .cloned();
        match ContentStream::from_form(self.view, id) {
            Ok(inner) => {
                self.active.push(id.num);
                let outer_stream = self.stream.replace(id);
                self.walk(&inner, own.as_ref().unwrap_or(outer), ctx, depth + 1);
                self.stream = outer_stream;
                self.active.pop();
            }
            Err(e) => self
                .counts
                .unread
                .push(format!("the form XObject {id}: {e}")),
        }
    }
}

fn enclosing(base: Ctx, marks: &[Mark]) -> Ctx {
    marks.iter().fold(base, |c, m| Ctx {
        artifact: c.artifact || m.artifact,
        tagged: c.tagged || m.tagged,
        lang: c.lang || m.lang,
    })
}

/// `Tr 3` draws invisible text (ISO 32000-1 §9.3.6).
fn set_render_mode(op: &Operation<'_>, fonts: &mut [(Option<ObjId>, bool)]) {
    if let (Some(Object::Integer(mode)), Some(top)) = (first_operand(op), fonts.last_mut()) {
        top.1 = mode != 3;
    }
}
