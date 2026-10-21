//! The content walk behind [`super::enumerate`]: text-showing operators
//! collect raw codes per font dictionary; `Do`, pattern paint, soft-mask
//! `gs` and Type 3 glyph procedures recurse into the streams they draw.

use std::collections::BTreeSet;

use super::{CodeWidth, GlyphUsage};
use crate::content::{ContentStream, ContentTokenKind, MAX_FORM_DEPTH, Operation};
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::view::DocumentView;

/// Deepest `q` nesting followed; deeper is a fault, not a guess.
const MAX_SAVE_DEPTH: usize = 256;
/// Operators interpreted across the whole walk before it gives up.
const MAX_OPERATIONS: u64 = 50_000_000;

/// The font a show draws with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Font {
    /// No `Tf` in force: a show here cannot be attributed.
    Unset,
    /// A direct font dictionary, or one this walk does not track.
    Untracked,
    /// The indirect font dictionary and how its codes split.
    Tracked(ObjId, CodeWidth),
}

/// One walk over a document's drawn content.
pub(super) struct Walker<'a, 'v> {
    pub(super) view: &'a DocumentView<'v>,
    pub(super) usage: GlyphUsage,
    /// Streams on the current invocation path (cycle guard).
    stack: Vec<ObjId>,
    /// `(stream, inherited font)` pairs already walked.
    seen: BTreeSet<(ObjId, Font)>,
    operations: u64,
}

impl<'a, 'v> Walker<'a, 'v> {
    /// A walk over `view` that has seen nothing yet.
    pub(super) fn new(view: &'a DocumentView<'v>) -> Self {
        Self {
            view,
            usage: GlyphUsage::default(),
            stack: Vec::new(),
            seen: BTreeSet::new(),
            operations: 0,
        }
    }

    /// Walk the stream `id` against its own `/Resources` (or `fallback`
    /// when it has none, §7.8.3), starting with `font` in force.
    pub(super) fn stream(&mut self, id: ObjId, fallback: &Dict, font: Font) {
        if self.stack.contains(&id) || !self.seen.insert((id, font)) {
            return;
        }
        if self.stack.len() >= MAX_FORM_DEPTH {
            self.usage.fault(format!(
                "drawing nests deeper than {MAX_FORM_DEPTH} at {id}"
            ));
            return;
        }
        self.usage.holders.insert(id);
        let view = self.view;
        let own = view
            .value(id)
            .and_then(Object::as_dict)
            .and_then(|d| d.get(b"Resources"));
        let resources = match own {
            Some(r) => {
                self.usage.note_resources(view, r);
                view.resolve(r).as_dict().cloned().unwrap_or_default()
            }
            None => fallback.clone(),
        };
        match ContentStream::from_form(view, id) {
            Ok(content) => {
                self.stack.push(id);
                self.content(&content, &resources, font);
                self.stack.pop();
            }
            Err(e) => self
                .usage
                .fault(format!("the content of {id} cannot be read: {e}")),
        }
    }

    /// Interpret `content` against `resources`.
    pub(super) fn content(&mut self, content: &ContentStream, resources: &Dict, font: Font) {
        let mut saved: Vec<Font> = Vec::new();
        let mut font = font;
        for op in content.operations() {
            self.operations += 1;
            if self.operations > MAX_OPERATIONS {
                self.usage
                    .fault(format!("more than {MAX_OPERATIONS} operators"));
                return;
            }
            let Some(name) = op.operator_name(&content.buf) else {
                continue;
            };
            match name {
                b"q" if saved.len() >= MAX_SAVE_DEPTH => {
                    self.usage
                        .fault(format!("q nests deeper than {MAX_SAVE_DEPTH}"));
                    return;
                }
                b"q" => saved.push(font),
                b"Q" => font = saved.pop().unwrap_or(font),
                b"Tf" => font = self.select(&op, resources),
                b"Tj" | b"'" | b"\"" | b"TJ" => self.show(&op, font),
                b"Do" => self.invoke(&op, resources, font),
                b"scn" | b"SCN" => self.pattern(&op, resources),
                b"gs" => self.soft_mask(&op, resources),
                _ => {}
            }
        }
    }

    /// The font `Tf` selects; a Type 3 font's glyph procedures are walked.
    fn select(&mut self, op: &Operation<'_>, resources: &Dict) -> Font {
        let Some(name) = name_operand(op, 0) else {
            return Font::Unset;
        };
        let Some(entry) = sub_entry(self.view, resources, b"Font", name) else {
            self.usage
                .fault("a Tf names a font the resources lack".to_owned());
            return Font::Unset;
        };
        let Some(id) = entry.as_reference() else {
            return Font::Untracked;
        };
        let Some(dict) = self.view.value(id).and_then(Object::as_dict).cloned() else {
            return Font::Untracked;
        };
        match name_of(&dict, b"Subtype") {
            Some(b"Type3") => {
                self.type3(id, &dict, resources);
                Font::Untracked
            }
            Some(b"Type0") => Font::Tracked(id, type0_width(self.view, &dict)),
            _ => Font::Tracked(id, CodeWidth::One),
        }
    }

    fn show(&mut self, op: &Operation<'_>, font: Font) {
        let strings = op.operands.iter().filter_map(|t| match &t.kind {
            ContentTokenKind::Operand(o) => Some(o),
            _ => None,
        });
        let mut bytes: Vec<&[u8]> = Vec::new();
        for o in strings {
            match o {
                Object::String(s) => bytes.push(s),
                Object::Array(items) => bytes.extend(items.iter().filter_map(|i| match i {
                    Object::String(s) => Some(s.as_slice()),
                    _ => None,
                })),
                _ => {}
            }
        }
        if bytes.iter().all(|b| b.is_empty()) {
            return;
        }
        match font {
            Font::Unset => self
                .usage
                .fault("text is shown with no font selected".to_owned()),
            Font::Untracked => {}
            Font::Tracked(id, width) => self.usage.record(id, width, &bytes),
        }
    }

    fn invoke(&mut self, op: &Operation<'_>, resources: &Dict, font: Font) {
        let Some(name) = name_operand(op, 0) else {
            return;
        };
        let Some(id) =
            sub_entry(self.view, resources, b"XObject", name).and_then(Object::as_reference)
        else {
            return;
        };
        let is_form = self
            .view
            .value(id)
            .and_then(Object::as_dict)
            .is_some_and(|d| name_of(d, b"Subtype") == Some(b"Form"));
        if is_form {
            self.stream(id, resources, font);
        }
    }

    fn pattern(&mut self, op: &Operation<'_>, resources: &Dict) {
        let Some(name) = op.operands.iter().rev().find_map(|t| match &t.kind {
            ContentTokenKind::Operand(Object::Name(n)) => Some(n.as_bytes()),
            _ => None,
        }) else {
            return;
        };
        let Some(id) =
            sub_entry(self.view, resources, b"Pattern", name).and_then(Object::as_reference)
        else {
            return;
        };
        if matches!(self.view.value(id), Some(Object::Stream(_))) {
            self.stream(id, resources, Font::Unset);
        }
    }

    fn soft_mask(&mut self, op: &Operation<'_>, resources: &Dict) {
        let Some(name) = name_operand(op, 0) else {
            return;
        };
        let view = self.view;
        let Some(gs) = sub_entry(view, resources, b"ExtGState", name)
            .map(|o| view.resolve(o))
            .and_then(Object::as_dict)
        else {
            return;
        };
        let group = gs
            .get(b"SMask")
            .map(|o| view.resolve(o))
            .and_then(Object::as_dict)
            .and_then(|m| m.get(b"G"))
            .and_then(Object::as_reference);
        if let Some(id) = group {
            self.stream(id, resources, Font::Unset);
        }
    }

    /// Every glyph procedure of the Type 3 font `id`, against its own
    /// `/Resources` or, lacking them, `resources` (§9.6.5, Table 112).
    fn type3(&mut self, id: ObjId, dict: &Dict, resources: &Dict) {
        self.usage.holders.insert(id);
        let view = self.view;
        let own = dict.get(b"Resources");
        if let Some(r) = own {
            self.usage.note_resources(view, r);
        }
        let res = own
            .and_then(|r| view.resolve(r).as_dict().cloned())
            .unwrap_or_else(|| resources.clone());
        let procs: Vec<ObjId> = dict
            .get(b"CharProcs")
            .map(|o| view.resolve(o))
            .and_then(Object::as_dict)
            .map(|d| d.iter().filter_map(|(_, v)| v.as_reference()).collect())
            .unwrap_or_default();
        for p in procs {
            self.stream(p, &res, Font::Unset);
        }
    }
}

fn name_operand<'o>(op: &Operation<'o>, index: usize) -> Option<&'o [u8]> {
    match &op.operands.get(index)?.kind {
        ContentTokenKind::Operand(Object::Name(n)) => Some(n.as_bytes()),
        _ => None,
    }
}

fn name_of<'d>(dict: &'d Dict, key: &[u8]) -> Option<&'d [u8]> {
    dict.get(key)
        .and_then(Object::as_name)
        .map(|n| n.as_bytes())
}

/// `resources[category][name]`, unresolved.
fn sub_entry<'r>(
    view: &'r DocumentView<'_>,
    resources: &'r Dict,
    category: &[u8],
    name: &[u8],
) -> Option<&'r Object> {
    view.resolve(resources.get(category)?).as_dict()?.get(name)
}

/// How a Type 0 font's codes split: two bytes under `Identity-H`/`-V`
/// (§9.7.5.2, Table 116); any other CMap is not followed.
fn type0_width(view: &DocumentView<'_>, dict: &Dict) -> CodeWidth {
    match dict.get(b"Encoding").map(|o| view.resolve(o)) {
        Some(Object::Name(n)) if matches!(n.as_bytes(), b"Identity-H" | b"Identity-V") => {
            CodeWidth::Two
        }
        _ => CodeWidth::Unknown,
    }
}
