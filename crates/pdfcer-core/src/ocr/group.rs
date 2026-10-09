//! An OCR layer on an optional-content group (ISO 32000-1 §8.11.3.2).
//!
//! The group's `/OC /name BDC … EMC` section is written inside the
//! `/pdfc_OCR` section, so the marker stays the stream's outermost sequence
//! ([`super::marker`]) and the layer is still found and removed whole.

use std::collections::BTreeSet;

use crate::content::{ContentStream, ContentTokenKind};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::page_tree::{self, Page};
use crate::view::DocumentView;

/// Form XObjects nested deeper than this are counted as using the group.
const MAX_FORM_DEPTH: usize = 12;

/// Whether `group` is a dictionary listed in `/OCProperties /OCGs`
/// (§8.11.4.2 Table 100).
pub(crate) fn is_registered(view: &DocumentView<'_>, group: ObjId) -> bool {
    let Some(catalog) = view.catalog_id() else {
        return false;
    };
    let ocgs = view
        .resolved(catalog)
        .as_dict()
        .and_then(|c| c.get(b"OCProperties"))
        .and_then(|o| view.resolve(o).as_dict())
        .and_then(|o| o.get(b"OCGs"))
        .map(|a| view.resolve(a));
    let listed = matches!(ocgs, Some(Object::Array(a))
        if a.iter().any(|o| o.as_reference() == Some(group)));
    listed && view.resolved(group).as_dict().is_some()
}

/// The `/Properties` names page `page` binds to each of `groups`, and
/// whether each is new (the page has no name for the group yet, so the
/// caller must bind it). New names are distinct from the page's and from
/// each other; a group listed twice gets one name, new at most once.
pub(crate) fn property_names(
    view: &DocumentView<'_>,
    page: &Page,
    groups: &[ObjId],
) -> Vec<(Name, bool)> {
    let props = properties(view, &page.resources);
    let mut chosen: Vec<(ObjId, Name)> = Vec::new();
    let mut out = Vec::with_capacity(groups.len());
    for &group in groups {
        if let Some((_, name)) = chosen.iter().find(|(g, _)| *g == group) {
            out.push((name.clone(), false));
            continue;
        }
        let existing = props
            .0
            .iter()
            .find(|(_, v)| v.as_reference() == Some(group))
            .map(|(n, _)| n.clone());
        let new = existing.is_none();
        let name = existing.unwrap_or_else(|| {
            (1..)
                .map(|i| Name(format!("OC{i}").into_bytes()))
                .find(|n| props.get(&n.0).is_none() && chosen.iter().all(|(_, c)| c != n))
                .unwrap_or_else(|| Name(Vec::new()))
        });
        chosen.push((group, name.clone()));
        out.push((name, new));
    }
    out
}

/// `resources`' `/Properties` subdictionary, resolved; empty when absent.
pub(crate) fn properties(view: &DocumentView<'_>, resources: &Dict) -> Dict {
    resources
        .get(b"Properties")
        .and_then(|o| view.resolve(o).as_dict())
        .cloned()
        .unwrap_or_default()
}

/// The groups a layer stream's `/OC` sections are on, through `page`'s
/// `/Properties`: the whole layer's (a section opened before the layer's
/// `q`, [`OcrLayerOptions::on_layer`](super::layer::OcrLayerOptions::on_layer))
/// and the region groups' (opened after it,
/// [`OcrLayerOptions::on_region_layer`](super::layer::OcrLayerOptions::on_region_layer)),
/// distinct and in stream order, the whole layer's group excluded.
pub(crate) fn layer_groups(
    view: &DocumentView<'_>,
    page: &Page,
    layer: ObjId,
) -> (Option<ObjId>, Vec<ObjId>) {
    let mut whole = None;
    let mut regions = Vec::new();
    let Some(cs) = super::refold::decoded(view, layer).and_then(|d| ContentStream::parse(d).ok())
    else {
        return (whole, regions);
    };
    let props = properties(view, &page.resources);
    let b = cs.buf.as_slice();
    let mut after_q = false;
    for op in cs.operations() {
        if op.operator_name(b) == Some(&b"q"[..]) {
            after_q = true;
            continue;
        }
        let Some(group) =
            oc_section_name(&op, b).and_then(|n| props.get(n.as_bytes())?.as_reference())
        else {
            continue;
        };
        if !after_q {
            whole.get_or_insert(group);
        } else if whole != Some(group) && !regions.contains(&group) {
            regions.push(group);
        }
    }
    (whole, regions)
}

/// The property-list name of `op` when it is `/OC /name BDC`.
fn oc_section_name<'a>(op: &crate::content::Operation<'a>, b: &'a [u8]) -> Option<&'a Name> {
    if op.operator_name(b)? != b"BDC" {
        return None;
    }
    let operand = |i: usize| match op.operands.get(i).map(|t| &t.kind) {
        Some(ContentTokenKind::Operand(o)) => o.as_name(),
        _ => None,
    };
    (operand(0)?.as_bytes() == b"OC").then_some(())?;
    operand(1)
}

/// Whether anything in the document still draws on `group`: page content in
/// an `/OC` section bound to it, an annotation or XObject whose `/OC` names
/// it (directly or through a membership dictionary, §8.11.2.2), or a form
/// XObject that binds it. Content that cannot be decoded counts as using it:
/// a wrong "still used" costs an offer to delete, a wrong "empty" costs a
/// layer.
///
/// Public as `pdfcer_core::layers::group_in_use`.
#[must_use]
pub fn group_in_use(view: &DocumentView<'_>, group: ObjId) -> bool {
    let Ok(pages) = page_tree::pages_in(view) else {
        return true;
    };
    pages.iter().any(|page| page_uses(view, page, group))
}

fn page_uses(view: &DocumentView<'_>, page: &Page, group: ObjId) -> bool {
    let annots = view
        .resolved(page.id)
        .as_dict()
        .and_then(|d| d.get(b"Annots"))
        .map(|a| view.resolve(a));
    if let Some(Object::Array(annots)) = annots
        && annots.iter().any(|a| {
            let oc = view.resolve(a).as_dict().and_then(|d| d.get(b"OC"));
            oc.is_some_and(|oc| names_group(view, oc, group))
        })
    {
        return true;
    }
    let mut seen = BTreeSet::new();
    if xobjects_use(view, &page.resources, group, 0, &mut seen) {
        return true;
    }
    let bound: Vec<Name> = properties(view, &page.resources)
        .0
        .into_iter()
        .filter(|(_, v)| names_group(view, v, group))
        .map(|(k, _)| k)
        .collect();
    !bound.is_empty() && content_uses(view, page, &bound)
}

/// Whether the page content opens an `/OC` section with one of `names`.
fn content_uses(view: &DocumentView<'_>, page: &Page, names: &[Name]) -> bool {
    let mut buf = Vec::new();
    for id in super::marker::content_stream_ids(view, page) {
        let Some(data) = super::refold::decoded(view, id) else {
            return true;
        };
        buf.extend_from_slice(&data);
        buf.push(b'\n');
    }
    let Ok(cs) = ContentStream::parse(buf) else {
        return true;
    };
    let b = cs.buf.as_slice();
    cs.operations()
        .any(|op| oc_section_name(&op, b).is_some_and(|n| names.contains(n)))
}

/// Whether an XObject in `resources` (or in a form's own resources) is on
/// `group`, or a form binds it in its `/Properties`.
fn xobjects_use(
    view: &DocumentView<'_>,
    resources: &Dict,
    group: ObjId,
    depth: usize,
    seen: &mut BTreeSet<ObjId>,
) -> bool {
    let Some(xobjects) = resources
        .get(b"XObject")
        .and_then(|o| view.resolve(o).as_dict())
    else {
        return false;
    };
    for (_, entry) in &xobjects.0 {
        if let Some(id) = entry.as_reference()
            && !seen.insert(id)
        {
            continue;
        }
        let Object::Stream(stream) = view.resolve(entry) else {
            continue;
        };
        if stream
            .dict
            .get(b"OC")
            .is_some_and(|oc| names_group(view, oc, group))
        {
            return true;
        }
        let Some(inner) = stream
            .dict
            .get(b"Resources")
            .and_then(|o| view.resolve(o).as_dict())
        else {
            continue;
        };
        if depth >= MAX_FORM_DEPTH
            || properties(view, inner)
                .0
                .iter()
                .any(|(_, v)| names_group(view, v, group))
            || xobjects_use(view, inner, group, depth + 1, seen)
        {
            return true;
        }
    }
    false
}

/// Whether `oc` is `group` or a membership dictionary listing it.
fn names_group(view: &DocumentView<'_>, oc: &Object, group: ObjId) -> bool {
    if oc.as_reference() == Some(group) {
        return true;
    }
    let Some(ocgs) = view.resolve(oc).as_dict().and_then(|d| d.get(b"OCGs")) else {
        return false;
    };
    match view.resolve(ocgs) {
        Object::Array(a) => a.iter().any(|o| o.as_reference() == Some(group)),
        _ => ocgs.as_reference() == Some(group),
    }
}
