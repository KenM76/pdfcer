//! Writing a materialized XFA form's fields as AcroForm fields and widget
//! annotations ([`crate::xfa::FieldOutput::Fillable`]; ISO 32000-1 §12.7.3,
//! widgets §12.5.6.19, appearances §12.5.5).

use std::collections::{BTreeMap, HashMap, HashSet};

use super::{EditError, EditSession, ObjectWrite, encode_text_string};
use crate::object::{Dict, Name, ObjId, Object};
use crate::xfa::{FillField, FillKind, FillValue, PaintedWidget};

/// A written page, as its widgets' appearances need it.
pub(super) struct WrittenPage {
    pub(super) id: ObjId,
    /// The page's `/Resources`, for every appearance on it.
    pub(super) resources: Object,
    /// The page's `/Font` resources, for `/DR`.
    pub(super) fonts: Dict,
}

/// The widget annotation id each `(page, widget)` a field uses gets.
pub(super) type WidgetIds = HashMap<(usize, usize), ObjId>;

fn name(s: &str) -> Object {
    Object::Name(Name::from(s.as_bytes()))
}

fn int(n: usize) -> Object {
    Object::Integer(i64::try_from(n).unwrap_or(i64::MAX))
}

fn text(s: &str) -> Object {
    Object::String(encode_text_string(s))
}

/// A check state's appearance name: `Off` is reserved for off (§12.7.4.2.3).
fn state_name(on: &str) -> &str {
    if on.is_empty() || on == "Off" {
        "On"
    } else {
        on
    }
}

/// `/DA` for a widget's text, its font under its `/DR` name.
fn da(page: usize, w: &PaintedWidget) -> Option<Object> {
    let (font, size, [r, g, b]) = w.text.as_ref()?;
    let da = format!("/P{page}_{font} {size} Tf {r} {g} {b} rg");
    Some(Object::String(da.into_bytes()))
}

impl EditSession {
    /// One annotation id per widget slot, by page.
    pub(super) fn widget_ids(
        &mut self,
        fields: &[FillField],
        pages: usize,
    ) -> Result<(WidgetIds, Vec<Vec<Object>>), EditError> {
        let mut ids = WidgetIds::new();
        let mut annots = vec![Vec::new(); pages];
        for (page, i, _) in fields.iter().flat_map(|f| &f.widgets) {
            let id = ObjId::new(self.alloc_number()?, 0);
            ids.insert((*page, *i), id);
            if let Some(list) = annots.get_mut(*page) {
                list.push(Object::Reference(id));
            }
        }
        Ok((ids, annots))
    }

    /// Write `fields` and their widgets; the top-level `/Fields` entries
    /// and the `/DR` font dictionary.
    pub(super) fn write_fields(
        &mut self,
        fields: &[FillField],
        widgets: &[Vec<PaintedWidget>],
        pages: &[WrittenPage],
        ids: &WidgetIds,
        objects: &mut Vec<ObjectWrite>,
    ) -> Result<(Vec<Object>, Dict), EditError> {
        let mut tree = NameTree::default();
        for field in fields {
            let names = tree.unique(&field.name);
            let parent = tree.parent_of(self, &names)?;
            let id = ObjId::new(self.alloc_number()?, 0);
            tree.attach(parent, id);
            let mut kids = Vec::new();
            let mut first_da = None;
            for (page, i, on) in &field.widgets {
                let (Some(w), Some(p), Some(&wid)) = (
                    widgets.get(*page).and_then(|l| l.get(*i)),
                    pages.get(*page),
                    ids.get(&(*page, *i)),
                ) else {
                    continue;
                };
                let da = da(*page, w);
                first_da = first_da.or_else(|| da.clone());
                self.write_widget(field, w, on.as_deref(), p, (wid, id), da, objects)?;
                kids.push(Object::Reference(wid));
            }
            let mut d = field_dict(field, names.last().map_or("", String::as_str));
            if let Some(da) = first_da {
                d.insert(Name::from(b"DA"), da);
            }
            if let Some(parent) = parent {
                d.insert(Name::from(b"Parent"), Object::Reference(parent));
            }
            d.insert(Name::from(b"Kids"), Object::Array(kids));
            objects.push(new_object(id, Object::Dict(d)));
        }
        let top = tree.write(objects);
        let mut dr = Dict::new();
        for (i, p) in pages.iter().enumerate() {
            for (k, v) in p.fonts.iter() {
                let mut key = format!("P{i}_").into_bytes();
                key.extend_from_slice(&k.0);
                dr.insert(Name::from(key.as_slice()), v.clone());
            }
        }
        Ok((top, dr))
    }

    /// One widget annotation, its appearance drawn in page space.
    #[allow(clippy::too_many_arguments)] // each is a distinct input to one dictionary
    fn write_widget(
        &mut self,
        field: &FillField,
        w: &PaintedWidget,
        on: Option<&str>,
        page: &WrittenPage,
        (id, parent): (ObjId, ObjId),
        da: Option<Object>,
        objects: &mut Vec<ObjectWrite>,
    ) -> Result<(), EditError> {
        let rect = Object::Array(w.rect.iter().copied().map(Object::Real).collect());
        let mut d = Dict::new();
        d.insert(Name::from(b"Type"), name("Annot"));
        d.insert(Name::from(b"Subtype"), name("Widget"));
        d.insert(Name::from(b"Rect"), rect.clone());
        d.insert(Name::from(b"P"), Object::Reference(page.id));
        d.insert(Name::from(b"Parent"), Object::Reference(parent));
        d.insert(
            Name::from(b"F"),
            Object::Integer(if w.hidden { 2 } else { 4 }),
        );
        if let Some(da) = da {
            d.insert(Name::from(b"DA"), da);
        }
        if !w.hidden {
            let mut form = Dict::new();
            form.insert(Name::from(b"Type"), name("XObject"));
            form.insert(Name::from(b"Subtype"), name("Form"));
            form.insert(Name::from(b"BBox"), rect);
            form.insert(Name::from(b"Resources"), page.resources.clone());
            let normal = self.new_stream(form.clone(), &w.normal, objects)?;
            let n = match (on, &w.off) {
                (Some(on), Some(off)) => {
                    let off = self.new_stream(form, off, objects)?;
                    let mut states = Dict::new();
                    states.insert(
                        Name::from(state_name(on).as_bytes()),
                        Object::Reference(normal),
                    );
                    states.insert(Name::from(b"Off"), Object::Reference(off));
                    Object::Dict(states)
                }
                _ => Object::Reference(normal),
            };
            let mut ap = Dict::new();
            ap.insert(Name::from(b"N"), n);
            d.insert(Name::from(b"AP"), Object::Dict(ap));
        }
        if let Some(on) = on {
            let selected = matches!(&field.value, FillValue::Name(v) if v == on);
            d.insert(
                Name::from(b"AS"),
                name(if selected { state_name(on) } else { "Off" }),
            );
        }
        objects.push(new_object(id, Object::Dict(d)));
        Ok(())
    }
}

fn new_object(id: ObjId, after: Object) -> ObjectWrite {
    ObjectWrite {
        id,
        before: None,
        after: Some(after),
    }
}

/// A terminal field's own entries (`/Kids`, `/Parent`, `/DA` added by the
/// caller).
fn field_dict(field: &FillField, partial: &str) -> Dict {
    let mut d = Dict::new();
    let ft = match field.kind {
        FillKind::Text => "Tx",
        FillKind::Check | FillKind::Radio => "Btn",
        FillKind::Choice => "Ch",
        FillKind::Signature => "Sig",
    };
    d.insert(Name::from(b"FT"), name(ft));
    d.insert(Name::from(b"T"), text(partial));
    if field.flags != 0 {
        d.insert(Name::from(b"Ff"), Object::Integer(i64::from(field.flags)));
    }
    if let Some(n) = field.max_len {
        d.insert(Name::from(b"MaxLen"), int(n));
    }
    if !field.options.is_empty() {
        let opt = field.options.iter().map(|(display, save)| {
            if display == save {
                text(display)
            } else {
                Object::Array(vec![text(save), text(display)])
            }
        });
        d.insert(Name::from(b"Opt"), Object::Array(opt.collect()));
    }
    let value = match &field.value {
        FillValue::None => None,
        FillValue::Text(s) => Some(text(s)),
        FillValue::Name(s) if s == "Off" => Some(name("Off")),
        FillValue::Name(s) => Some(name(state_name(s))),
        FillValue::List(v) => Some(Object::Array(v.iter().map(|s| text(s)).collect())),
    };
    if let Some(v) = value {
        d.insert(Name::from(b"V"), v);
    }
    d
}

/// The nonterminal fields above the terminal ones (§12.7.3.1).
#[derive(Default)]
struct NameTree {
    nodes: BTreeMap<Vec<String>, (ObjId, Vec<Object>)>,
    top: Vec<Object>,
    used: HashSet<Vec<String>>,
}

impl NameTree {
    /// `names`, its last part suffixed when another field took it.
    fn unique(&mut self, names: &[String]) -> Vec<String> {
        let mut out = names.to_vec();
        let mut n = 1;
        while self.used.contains(&out) || self.nodes.contains_key(&out) {
            n += 1;
            if let (Some(last), Some(base)) = (out.last_mut(), names.last()) {
                *last = format!("{base}~{n}");
            }
        }
        self.used.insert(out.clone());
        out
    }

    /// The nonterminal holding `names`' last part, created down from the
    /// top as needed.
    fn parent_of(
        &mut self,
        session: &mut EditSession,
        names: &[String],
    ) -> Result<Option<ObjId>, EditError> {
        let mut parent = None;
        for depth in 1..names.len() {
            let key = names.get(..depth).unwrap_or_default().to_vec();
            let id = if let Some((id, _)) = self.nodes.get(&key) {
                *id
            } else {
                let id = ObjId::new(session.alloc_number()?, 0);
                self.nodes.insert(key, (id, Vec::new()));
                self.attach(parent, id);
                id
            };
            parent = Some(id);
        }
        Ok(parent)
    }

    fn attach(&mut self, parent: Option<ObjId>, id: ObjId) {
        let kid = Object::Reference(id);
        match parent.and_then(|p| self.nodes.values_mut().find(|(i, _)| *i == p)) {
            Some((_, kids)) => kids.push(kid),
            None => self.top.push(kid),
        }
    }

    /// Write every nonterminal; the top-level fields.
    fn write(self, objects: &mut Vec<ObjectWrite>) -> Vec<Object> {
        let parents: HashMap<ObjId, ObjId> = self
            .nodes
            .values()
            .flat_map(|(id, kids)| {
                kids.iter().filter_map(move |k| match k {
                    Object::Reference(k) => Some((*k, *id)),
                    _ => None,
                })
            })
            .collect();
        for (names, (id, kids)) in self.nodes {
            let mut d = Dict::new();
            d.insert(
                Name::from(b"T"),
                text(names.last().map_or("", String::as_str)),
            );
            if let Some(p) = parents.get(&id) {
                d.insert(Name::from(b"Parent"), Object::Reference(*p));
            }
            d.insert(Name::from(b"Kids"), Object::Array(kids));
            objects.push(new_object(id, Object::Dict(d)));
        }
        self.top
    }
}
