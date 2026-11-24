//! The preflight content walk: every colour space and overprint request a
//! page draws with, through forms, patterns, shadings, images and the
//! annotations' normal appearances.
//!
//! It answers *which* spaces are used, not *where*: a use carries the object
//! that names the space (an image, an annotation, a colour-space array), and
//! no page rectangle. Type 3 glyph procedures and soft-mask groups are not
//! entered; a stream that cannot be decoded is reported, never skipped
//! silently.

use crate::content::{ContentStream, ContentTokenKind, Operation};
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::view::DocumentView;

use super::ColourFamily;

/// Forms, patterns and appearances nest at most this deep.
const MAX_DEPTH: usize = 32;
/// A page stops recording after this many events.
const MAX_EVENTS: usize = 4096;

/// What drew a use: the page's own content (and forms under it) or one
/// annotation's appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Origin {
    Page,
    Annotation(ObjId),
}

/// One colour space a page paints with.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ColourUse {
    pub family: ColourFamily,
    /// Spot colorants named by a `Separation` or `DeviceN` space, process
    /// names, `All` and `None` excluded.
    pub spots: Vec<String>,
    /// The indirect colour-space array, when the space is one.
    pub space: Option<ObjId>,
    /// The image XObject the space belongs to.
    pub image: Option<ObjId>,
}

/// One thing the walk saw.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Event {
    Colour(ColourUse),
    /// An `ExtGState` with `/OP` or `/op` true was set.
    Overprint(Option<ObjId>),
    /// A stream that could not be read, by sentence.
    Unwalked(String),
}

/// Every event on one page, in content order, each with its origin.
pub(super) fn page_events(
    view: &DocumentView<'_>,
    page: &crate::page_tree::Page,
) -> Vec<(Origin, Event)> {
    let mut w = Walker {
        view,
        out: Vec::new(),
        active: Vec::new(),
        origin: Origin::Page,
    };
    match ContentStream::from_page(view, page) {
        Ok(stream) => w.walk(&stream, &page.resources, 0),
        Err(e) => w.push(Event::Unwalked(format!("page content: {e}"))),
    }
    for annot in annotations(view, page.id) {
        w.origin = Origin::Annotation(annot);
        for ap in normal_appearances(view, annot) {
            w.form(ap, &Dict::new(), 0);
        }
    }
    w.out
}

fn annotations(view: &DocumentView<'_>, page: ObjId) -> Vec<ObjId> {
    view.graph()
        .value(page)
        .and_then(Object::as_dict)
        .and_then(|d| d.get(b"Annots"))
        .map(|o| view.resolve(o))
        .and_then(Object::as_array)
        .map(|a| a.iter().filter_map(Object::as_reference).collect())
        .unwrap_or_default()
}

/// `/AP /N`: one stream, or a dictionary of appearance states.
fn normal_appearances(view: &DocumentView<'_>, annot: ObjId) -> Vec<ObjId> {
    let n = view
        .graph()
        .value(annot)
        .and_then(Object::as_dict)
        .and_then(|d| d.get(b"AP"))
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)
        .and_then(|ap| ap.get(b"N"));
    match n {
        Some(Object::Reference(id))
            if matches!(view.graph().value(*id), Some(Object::Stream(_))) =>
        {
            vec![*id]
        }
        Some(o) => view
            .resolve(o)
            .as_dict()
            .map(|states| {
                states
                    .iter()
                    .filter_map(|(_, v)| v.as_reference())
                    .collect()
            })
            .unwrap_or_default(),
        None => Vec::new(),
    }
}

struct Walker<'v, 'd> {
    view: &'v DocumentView<'d>,
    out: Vec<(Origin, Event)>,
    active: Vec<u32>,
    origin: Origin,
}

impl Walker<'_, '_> {
    fn push(&mut self, event: Event) {
        if self.out.len() < MAX_EVENTS
            && !self
                .out
                .iter()
                .any(|(o, e)| *o == self.origin && *e == event)
        {
            self.out.push((self.origin, event));
        }
    }

    fn walk(&mut self, stream: &ContentStream, resources: &Dict, depth: usize) {
        let mut fill_pattern = false;
        let mut stroke_pattern = false;
        for op in stream.operations() {
            if let ContentTokenKind::InlineImage { params, .. } = &op.operator.kind {
                self.inline_image(params, resources);
                continue;
            }
            let name = op.operator_name(&stream.buf).unwrap_or_default();
            let device = match name {
                b"g" | b"G" => Some(ColourFamily::DeviceGray),
                b"rg" | b"RG" => Some(ColourFamily::DeviceRgb),
                b"k" | b"K" => Some(ColourFamily::DeviceCmyk),
                _ => None,
            };
            if let Some(family) = device {
                self.device(family);
                continue;
            }
            match name {
                b"cs" | b"CS" => {
                    let pattern = self.set_space(&op, resources);
                    if name == b"cs" {
                        fill_pattern = pattern;
                    } else {
                        stroke_pattern = pattern;
                    }
                }
                b"scn" if fill_pattern => self.pattern(&op, resources, depth),
                b"SCN" if stroke_pattern => self.pattern(&op, resources, depth),
                b"sh" => self.shading(&op, resources),
                b"gs" => self.ext_gstate(&op, resources),
                b"Do" => self.xobject(&op, resources, depth),
                _ => {}
            }
        }
    }

    fn device(&mut self, family: ColourFamily) {
        self.push(Event::Colour(ColourUse {
            family,
            spots: Vec::new(),
            space: None,
            image: None,
        }));
    }

    /// Record a `cs`/`CS` space; true when it is a pattern space.
    fn set_space(&mut self, op: &Operation<'_>, resources: &Dict) -> bool {
        let Some(Object::Name(n)) = first_operand(op) else {
            return false;
        };
        let object = resource(self.view, resources, b"ColorSpace", n.as_bytes())
            .cloned()
            .unwrap_or(Object::Name(n));
        let Some(mut use_) = classify(self.view, &object, 0) else {
            return false;
        };
        use_.space = object.as_reference().or(use_.space);
        let pattern = use_.family == ColourFamily::Pattern;
        self.push(Event::Colour(use_));
        pattern
    }

    fn pattern(&mut self, op: &Operation<'_>, resources: &Dict, depth: usize) {
        let Some(Object::Name(n)) = last_operand(op) else {
            return;
        };
        let Some(Object::Reference(id)) = resource(self.view, resources, b"Pattern", n.as_bytes())
        else {
            return;
        };
        let id = *id;
        match self.view.graph().value(id) {
            Some(Object::Stream(_)) => self.form(id, resources, depth),
            Some(Object::Dict(d)) => {
                let shading = d.get(b"Shading").map(|o| self.view.resolve(o)).cloned();
                if let Some(s) = shading {
                    self.shading_object(&s);
                }
            }
            _ => {}
        }
    }

    fn shading(&mut self, op: &Operation<'_>, resources: &Dict) {
        let Some(Object::Name(n)) = first_operand(op) else {
            return;
        };
        if let Some(s) = resource(self.view, resources, b"Shading", n.as_bytes()).cloned() {
            let s = self.view.resolve(&s).clone();
            self.shading_object(&s);
        }
    }

    fn shading_object(&mut self, shading: &Object) {
        let dict = match shading {
            Object::Dict(d) => Some(d),
            Object::Stream(s) => Some(&s.dict),
            _ => None,
        };
        if let Some(cs) = dict.and_then(|d| d.get(b"ColorSpace")).cloned()
            && let Some(mut use_) = classify(self.view, &cs, 0)
        {
            use_.space = cs.as_reference().or(use_.space);
            self.push(Event::Colour(use_));
        }
    }

    fn ext_gstate(&mut self, op: &Operation<'_>, resources: &Dict) {
        let Some(Object::Name(n)) = first_operand(op) else {
            return;
        };
        let Some(entry) = resource(self.view, resources, b"ExtGState", n.as_bytes()).cloned()
        else {
            return;
        };
        let asks = self.view.resolve(&entry).as_dict().is_some_and(|d| {
            [b"OP".as_slice(), b"op"].iter().any(|k| {
                matches!(
                    d.get(k).map(|o| self.view.resolve(o)),
                    Some(Object::Boolean(true))
                )
            })
        });
        if asks {
            self.push(Event::Overprint(entry.as_reference()));
        }
    }

    fn xobject(&mut self, op: &Operation<'_>, resources: &Dict, depth: usize) {
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
        match x
            .dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .map(|n| n.as_bytes())
        {
            Some(b"Image") => self.image(id, &x.dict.clone()),
            Some(b"Form") => self.form(id, resources, depth),
            _ => {}
        }
    }

    fn image(&mut self, id: ObjId, dict: &Dict) {
        let mask = matches!(
            dict.get(b"ImageMask").map(|o| self.view.resolve(o)),
            Some(Object::Boolean(true))
        );
        let Some(cs) = dict.get(b"ColorSpace").filter(|_| !mask).cloned() else {
            return;
        };
        if let Some(mut use_) = classify(self.view, &cs, 0) {
            use_.space = cs.as_reference().or(use_.space);
            use_.image = Some(id);
            self.push(Event::Colour(use_));
        }
    }

    /// Inline image `/CS` (or `/ColorSpace`), abbreviations included
    /// (ISO 32000-2 §8.9.7, Table 92).
    fn inline_image(&mut self, params: &Dict, resources: &Dict) {
        let mask = matches!(
            params.get(b"IM").or(params.get(b"ImageMask")),
            Some(Object::Boolean(true))
        );
        let Some(cs) = params
            .get(b"CS")
            .or(params.get(b"ColorSpace"))
            .filter(|_| !mask)
        else {
            return;
        };
        let cs = match cs {
            Object::Name(n) => match n.as_bytes() {
                b"G" | b"RGB" | b"CMYK" | b"I" => cs.clone(),
                other => resource(self.view, resources, b"ColorSpace", other)
                    .cloned()
                    .unwrap_or_else(|| cs.clone()),
            },
            _ => cs.clone(),
        };
        if let Some(use_) = classify(self.view, &cs, 0) {
            self.push(Event::Colour(use_));
        }
    }

    /// Walk a form, tiling pattern or appearance stream.
    fn form(&mut self, id: ObjId, outer: &Dict, depth: usize) {
        if depth >= MAX_DEPTH || self.active.contains(&id.num) {
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
                self.walk(&inner, own.as_ref().unwrap_or(outer), depth + 1);
                self.active.pop();
            }
            Err(e) => self.push(Event::Unwalked(format!("object {id}: {e}"))),
        }
    }
}

fn first_operand(op: &Operation<'_>) -> Option<Object> {
    op.operands.iter().find_map(|t| match &t.kind {
        ContentTokenKind::Operand(o) => Some(o.clone()),
        _ => None,
    })
}

fn last_operand(op: &Operation<'_>) -> Option<Object> {
    op.operands.iter().rev().find_map(|t| match &t.kind {
        ContentTokenKind::Operand(o) => Some(o.clone()),
        _ => None,
    })
}

/// `resources/<category>/<name>`, unresolved (so an indirect entry keeps
/// its id).
fn resource<'a>(
    view: &'a DocumentView<'_>,
    resources: &'a Dict,
    category: &[u8],
    name: &[u8],
) -> Option<&'a Object> {
    resources
        .get(category)
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)
        .and_then(|d| d.get(name))
}

/// Classify a colour-space object (ISO 32000-2 §8.6). `Indexed` reports its
/// base; a pattern space reports `Pattern`. `None` for an unknown space.
pub(super) fn classify(view: &DocumentView<'_>, cs: &Object, depth: usize) -> Option<ColourUse> {
    if depth > 4 {
        return None;
    }
    let plain = |family| {
        Some(ColourUse {
            family,
            spots: Vec::new(),
            space: None,
            image: None,
        })
    };
    match view.resolve(cs) {
        Object::Name(n) => match n.as_bytes() {
            b"DeviceGray" | b"G" => plain(ColourFamily::DeviceGray),
            b"DeviceRGB" | b"RGB" => plain(ColourFamily::DeviceRgb),
            b"DeviceCMYK" | b"CMYK" => plain(ColourFamily::DeviceCmyk),
            b"Pattern" => plain(ColourFamily::Pattern),
            _ => None,
        },
        Object::Array(items) => classify_array(view, items, depth),
        _ => None,
    }
}

fn classify_array(view: &DocumentView<'_>, items: &[Object], depth: usize) -> Option<ColourUse> {
    let head = items
        .first()
        .map(|o| view.resolve(o))
        .and_then(Object::as_name)?;
    let with = |family, spots| {
        Some(ColourUse {
            family,
            spots,
            space: None,
            image: None,
        })
    };
    match head.as_bytes() {
        b"CalGray" => with(ColourFamily::CalGray, Vec::new()),
        b"CalRGB" => with(ColourFamily::CalRgb, Vec::new()),
        b"Lab" => with(ColourFamily::Lab, Vec::new()),
        b"ICCBased" => {
            let n = items.get(1).and_then(|o| match view.resolve(o) {
                Object::Stream(s) => s
                    .dict
                    .get(b"N")
                    .map(|o| view.resolve(o))
                    .and_then(Object::as_int),
                _ => None,
            });
            let family = match n {
                Some(1) => ColourFamily::IccGray,
                Some(3) => ColourFamily::IccRgb,
                Some(4) => ColourFamily::IccCmyk,
                _ => ColourFamily::IccOther,
            };
            with(family, Vec::new())
        }
        b"Indexed" | b"I" => classify(view, items.get(1)?, depth + 1),
        b"Pattern" => with(ColourFamily::Pattern, Vec::new()),
        b"Separation" => {
            let name = items
                .get(1)
                .map(|o| view.resolve(o))
                .and_then(Object::as_name);
            with(
                ColourFamily::Separation,
                spot_names(name.into_iter().map(|n| n.as_bytes())),
            )
        }
        b"DeviceN" => {
            let names = items
                .get(1)
                .map(|o| view.resolve(o))
                .and_then(Object::as_array)
                .unwrap_or_default();
            let names = names
                .iter()
                .filter_map(|o| view.resolve(o).as_name())
                .map(|n| n.as_bytes());
            with(ColourFamily::DeviceN, spot_names(names))
        }
        _ => None,
    }
}

/// Colorant names that are spots: not `All` or `None` (§8.6.6.4), and not
/// `Cyan`, `Magenta`, `Yellow` or `Black`, which are always process
/// colorants (§8.6.6.5).
fn spot_names<'a>(names: impl Iterator<Item = &'a [u8]>) -> Vec<String> {
    const NOT_SPOT: [&[u8]; 6] = [b"All", b"None", b"Cyan", b"Magenta", b"Yellow", b"Black"];
    let mut out: Vec<String> = names
        .filter(|n| !NOT_SPOT.contains(n))
        .map(|n| String::from_utf8_lossy(n).into_owned())
        .collect();
    out.sort();
    out.dedup();
    out
}
