//! Discarding form fields, multimedia and 3D models during an optimize run
//! (Pass 616.3). This deletes them, unlike flattening: what they showed on
//! the page goes too.
//!
//! Carriers (ISO 32000-2):
//! - a page's `/Annots` (§12.5.2) loses each widget (§12.5.6.19), screen,
//!   movie, sound or rich-media annotation (§12.5.6.18, 12.5.6.17,
//!   12.5.6.16, §13.7) and 3D annotation (§13.6), with every pop-up whose
//!   `/Parent` is one of them (Table 183);
//! - a structure element's `/K` loses its object references to them
//!   (§14.7.5.3), which would otherwise keep them in the file;
//! - the catalog loses `/AcroForm` (§12.7.2), and `/Names /Renditions`
//!   (Table 32) with the multimedia. A certified file is never reached:
//!   the whole discard is refused on one (§12.8.2.2).
//! - the actions that drive them (rendition, movie, sound, rich-media
//!   execute, go-to-3D-view) are removed by [`super::actions`].
//!
//! The list removals are [`ListDrop`]s, applied against the list as it is
//! when the run reaches them, so a comment discard in the same run that
//! has already shortened `/Annots` does not stale them.

use std::collections::{BTreeMap, BTreeSet};

use super::objects::{DiscardObject, ObjectDiscard, Strip};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::view::DocumentView;

/// The references to remove from the list under `key` of `owner`: an
/// item that is one of `gone`, or an object reference dictionary whose
/// `/Obj` is. A list left empty removes the key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ListDrop {
    pub(crate) owner: ObjId,
    pub(crate) key: &'static [u8],
    pub(crate) gone: BTreeSet<ObjId>,
}

/// The class an annotation of `subtype` belongs to.
fn class_of(subtype: &[u8]) -> Option<DiscardObject> {
    match subtype {
        b"Widget" => Some(DiscardObject::FormFields),
        b"Screen" | b"Movie" | b"Sound" | b"RichMedia" => Some(DiscardObject::Multimedia),
        b"3D" => Some(DiscardObject::ThreeDModels),
        _ => None,
    }
}

fn subtype(dict: &Dict) -> Option<&[u8]> {
    dict.get(b"Subtype")
        .and_then(Object::as_name)
        .map(Name::as_bytes)
}

/// The class of the annotation at `id`, or of its parent for a pop-up.
fn annot_class(view: &DocumentView<'_>, id: ObjId) -> Option<DiscardObject> {
    let Object::Dict(dict) = view.value(id)? else {
        return None;
    };
    match subtype(dict)? {
        b"Popup" => {
            let parent = view.resolve(dict.get(b"Parent")?).as_dict()?;
            class_of(subtype(parent)?)
        }
        s => class_of(s),
    }
}

/// The annotation class of a target annotation, for an action that names
/// one (a go-to-3D-view action may target a rich-media annotation).
pub(crate) fn target_class(view: &DocumentView<'_>, target: &Object) -> Option<DiscardObject> {
    view.resolve(target)
        .as_dict()
        .and_then(subtype)
        .and_then(class_of)
}

/// The new value of a list after `drop`: `None` when unchanged,
/// `Some(None)` when nothing is left. `resolve` reads an indirect list or
/// object reference dictionary.
pub(crate) fn filtered<'a>(
    value: &'a Object,
    gone: &BTreeSet<ObjId>,
    resolve: impl Fn(ObjId) -> Option<&'a Object>,
) -> Option<Option<Object>> {
    let resolved = |o: &'a Object| match o {
        Object::Reference(r) => resolve(*r),
        other => Some(other),
    };
    let goes = |item: &'a Object| {
        if item.as_reference().is_some_and(|r| gone.contains(&r)) {
            return true;
        }
        let objr = resolved(item).and_then(Object::as_dict).filter(|d| {
            d.get(b"Type")
                .and_then(Object::as_name)
                .is_some_and(|t| t.as_bytes() == b"OBJR")
        });
        objr.and_then(|d| d.get(b"Obj"))
            .and_then(Object::as_reference)
            .is_some_and(|r| gone.contains(&r))
    };
    let Some(Object::Array(items)) = resolved(value) else {
        return goes(value).then_some(None);
    };
    let kept: Vec<Object> = items.iter().filter(|i| !goes(i)).cloned().collect();
    if kept.len() == items.len() {
        return None;
    }
    Some((!kept.is_empty()).then_some(Object::Array(kept)))
}

/// Every annotation, catalog entry and structure reference of a class
/// `wanted` selects, as one [`ObjectDiscard`] per object (`bytes` zero;
/// the session measures it).
pub(crate) fn annotation_carriers(
    view: &DocumentView<'_>,
    live: &BTreeSet<ObjId>,
    catalog: ObjId,
    cat: &Dict,
    wanted: impl Fn(DiscardObject) -> bool,
) -> Vec<ObjectDiscard> {
    let gone: BTreeMap<ObjId, DiscardObject> = live
        .iter()
        .filter_map(|&id| Some((id, annot_class(view, id).filter(|&c| wanted(c))?)))
        .collect();
    let mut out = Vec::new();
    let pages = crate::page_tree::pages_in(view).unwrap_or_default();
    for (index, page) in pages.iter().enumerate() {
        out.extend(page_item(view, page.id, index, &gone));
    }
    out.extend(catalog_items(view, catalog, cat, &wanted));
    if cat.get(b"StructTreeRoot").is_some() && !wanted(DiscardObject::Tags) {
        for &id in live {
            out.extend(structure_item(view, id, &gone));
        }
    }
    out
}

fn item(object: DiscardObject, owner: ObjId, location: String) -> ObjectDiscard {
    ObjectDiscard {
        object,
        owner,
        location,
        bytes: 0,
        strips: Vec::new(),
        rewrites: Vec::new(),
        drops: Vec::new(),
    }
}

/// The ids of `list` that `gone` holds, counted by class.
fn counted(
    list: &[Object],
    gone: &BTreeMap<ObjId, DiscardObject>,
) -> (BTreeSet<ObjId>, BTreeMap<DiscardObject, usize>) {
    let mut ids = BTreeSet::new();
    let mut by_class = BTreeMap::new();
    for id in list.iter().filter_map(Object::as_reference) {
        if let Some(&class) = gone.get(&id) {
            ids.insert(id);
            *by_class.entry(class).or_insert(0) += 1;
        }
    }
    (ids, by_class)
}

fn page_item(
    view: &DocumentView<'_>,
    page: ObjId,
    index: usize,
    gone: &BTreeMap<ObjId, DiscardObject>,
) -> Option<ObjectDiscard> {
    let annots = view.value(page)?.as_dict()?.get(b"Annots")?;
    let list = view.resolve(annots).as_array()?;
    let (ids, by_class) = counted(list, gone);
    let &object = by_class.keys().next()?;
    let what: Vec<String> = by_class
        .iter()
        .map(|(class, n)| format!("{class} ({n})"))
        .collect();
    let mut out = item(
        object,
        page,
        format!("page {}: {}", index + 1, what.join(", ")),
    );
    out.drops.push(ListDrop {
        owner: page,
        key: b"Annots",
        gone: ids,
    });
    Some(out)
}

fn structure_item(
    view: &DocumentView<'_>,
    id: ObjId,
    gone: &BTreeMap<ObjId, DiscardObject>,
) -> Option<ObjectDiscard> {
    let dict = view.value(id)?.as_dict()?;
    dict.get(b"S")?;
    let kids = dict.get(b"K")?;
    let targets: Vec<Object> = match view.resolve(kids) {
        Object::Array(list) => list.clone(),
        other => vec![other.clone()],
    };
    let named: Vec<Object> = targets
        .iter()
        .filter_map(|k| view.resolve(k).as_dict()?.get(b"Obj").cloned())
        .collect();
    let (ids, by_class) = counted(&named, gone);
    let &object = by_class.keys().next()?;
    let mut out = item(
        object,
        id,
        format!("structure element {} {}", id.num, id.generation),
    );
    out.drops.push(ListDrop {
        owner: id,
        key: b"K",
        gone: ids,
    });
    Some(out)
}

fn catalog_items(
    view: &DocumentView<'_>,
    catalog: ObjId,
    cat: &Dict,
    wanted: impl Fn(DiscardObject) -> bool,
) -> Vec<ObjectDiscard> {
    let at = |path: Vec<&'static [u8]>| Strip {
        owner: catalog,
        path,
    };
    let mut forms = Vec::new();
    if cat.get(b"AcroForm").is_some() {
        forms.push(at(vec![b"AcroForm"]));
    }
    let mut media = Vec::new();
    let names = cat.get(b"Names").map(|v| view.resolve(v));
    if names
        .and_then(Object::as_dict)
        .is_some_and(|d| d.get(b"Renditions").is_some())
    {
        media.push(at(vec![b"Names", b"Renditions"]));
    }
    let mut out = Vec::new();
    for (object, strips) in [
        (DiscardObject::FormFields, forms),
        (DiscardObject::Multimedia, media),
    ] {
        if wanted(object) && !strips.is_empty() {
            let mut one = item(object, catalog, "catalog".into());
            one.strips = strips;
            out.push(one);
        }
    }
    out
}
