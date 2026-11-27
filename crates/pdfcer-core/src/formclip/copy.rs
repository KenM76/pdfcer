//! Building a [`FieldClip`]: the field and widget dictionaries travel whole by
//! clone-then-patch (decision 228), minus the keys that name the source
//! document, and a field leaving its parent carries the attributes it
//! inherited (ISO 32000-1 §12.7.3.1 Table 220).

use std::collections::{BTreeMap, BTreeSet};

use super::{Closure, FIELD_CLIP_VERSION, FieldClip, FieldClipWidget, MAX_CLIP_WIDGETS};
use crate::forms::{self, FieldType};
use crate::forms_author::{FIELD_ONLY_KEYS, WIDGET_KEYS_TO_MOVE};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, Object};
use crate::page_tree::Rect;
use crate::vector::clip::ClipError;
use crate::view::DocumentView;
use crate::xcut::carry::{Strip, clone_then_patch};

/// The field's name and its place in the source's field tree; the paste
/// supplies all four.
const FIELD_IDENTITY: &[(&[u8], Strip)] = &[
    (b"T", Strip::Silent),
    (b"Parent", Strip::Silent),
    (b"Kids", Strip::Silent),
    (b"Type", Strip::Silent),
];

/// Widget keys that name something in the source document only.
///
/// `/P` and `/Parent` are re-supplied by the paste; `/T` and `/Kids` belong
/// to the field (R101). The marked four travel as `null` (`xcut::carry`):
/// `/NM` is regenerated unique on the destination page (§12.5.2 Table 164);
/// `/StructParent` indexes the source's parent tree (§14.7.4.4), and
/// `/Popup` and `/IRT` name source annotations, so the paste drops and
/// counts them.
const WIDGET_SOURCE_BOUND: &[(&[u8], Strip)] = &[
    (b"P", Strip::Silent),
    (b"Parent", Strip::Silent),
    (b"Type", Strip::Silent),
    (b"Subtype", Strip::Silent),
    (b"T", Strip::Silent),
    (b"Kids", Strip::Silent),
    (b"StructParent", Strip::Marked),
    (b"NM", Strip::Marked),
    (b"Popup", Strip::Marked),
    (b"IRT", Strip::Marked),
];

/// Field-level keys a merged dictionary's widget half leaves to its field
/// half, beyond [`FIELD_ONLY_KEYS`]. A leftover would win when the paste
/// folds the widget half back over the field (undoing a `/DA` font rename).
const MERGED_FIELD_KEYS: &[&[u8]] = &[b"DA", b"TU", b"Lock", b"SV"];

/// §12.7.3.1 Table 220's inheritable field attributes (`/MaxLen` per Table
/// 229). A button's `/Opt` is inheritable too (§12.7.4.2.1), a choice
/// field's is not.
const INHERITABLE: &[&[u8]] = &[b"FT", b"Ff", b"V", b"DV", b"DA", b"Q", b"MaxLen"];

/// How many `/Parent` links the inheritance walk follows.
const MAX_PARENT_DEPTH: usize = 32;

/// Build a clip from a parsed field and the document it lives in, interning
/// its objects into `closure` (the returned clip's own `objects` is empty, so
/// several fields can share one closure).
///
/// `field` must be a terminal field from `form`; `view` must be the same
/// document's view (a session's [`view`](crate::edit::EditSession::view), so
/// authored appearance spans resolve into the staging buffer).
///
/// # Errors
///
/// [`ClipError::ClipTooLarge`] when the field's closure exceeds
/// [`MAX_CLIP_OBJECTS`](super::MAX_CLIP_OBJECTS), or when it carries more
/// than [`MAX_CLIP_WIDGETS`] widgets.
pub(crate) fn build_field_clip(
    closure: &mut Closure<'_>,
    view: &DocumentView<'_>,
    form: &forms::AcroForm,
    field: &forms::Field,
) -> Result<FieldClip, ClipError> {
    let graph = view.graph();
    if field.widgets.len() > MAX_CLIP_WIDGETS {
        return Err(ClipError::ClipTooLarge {
            found: field.widgets.len(),
            limit: MAX_CLIP_WIDGETS,
        });
    }
    let merged = matches!(field.widgets.as_slice(), [w] if w.id == field.id);
    let raw = graph
        .resolved(field.id)
        .as_dict()
        .cloned()
        .unwrap_or_default();
    let (field_dict, inherited) = field_half(closure, graph, &raw, field, merged)?;
    let mut widgets = Vec::with_capacity(field.widgets.len());
    for widget in &field.widgets {
        let raw = graph
            .resolved(widget.id)
            .as_dict()
            .cloned()
            .unwrap_or_default();
        widgets.push(FieldClipWidget {
            dict: widget_half(closure, &raw, merged)?,
            rect: widget.rect.unwrap_or(Rect {
                llx: 0.0,
                lly: 0.0,
                urx: 0.0,
                ury: 0.0,
            }),
        });
    }
    let da_font = da_font(closure, graph, form, &field_dict)?;
    Ok(FieldClip {
        version: FIELD_CLIP_VERSION,
        field: field_dict,
        widgets,
        objects: BTreeMap::new(),
        da_font,
        inherited,
        source_name: field.fully_qualified_name.clone(),
        field_type: field.field_type,
        button_kind: field.button_kind,
    })
}

/// The field half: the raw dictionary minus its identity (and, for a merged
/// dictionary, minus its widget keys), plus every inherited attribute it
/// does not set itself. Returns the names of the inherited keys.
fn field_half<G: ObjectGraph + ?Sized>(
    closure: &mut Closure<'_>,
    graph: &G,
    raw: &Dict,
    field: &forms::Field,
    merged: bool,
) -> Result<(Dict, Vec<String>), ClipError> {
    let mut strip: Vec<(&[u8], Strip)> = FIELD_IDENTITY.to_vec();
    if merged {
        strip.extend(
            WIDGET_KEYS_TO_MOVE
                .iter()
                .chain([b"Popup".as_slice(), b"IRT".as_slice()].iter())
                .map(|k| (*k, Strip::Silent)),
        );
    }
    let mut patch = inherited_attributes(closure, graph, raw, field)?;
    // A field whose `/FT` came from an ancestor that could not be read is
    // still typed by the read projection; the paste cannot proceed without it.
    if set_value(raw, b"FT").is_none()
        && !patch.iter().any(|(k, _)| k.as_bytes() == b"FT")
        && let Some(ft) = field.field_type
    {
        patch.push((Name::from(b"FT"), Object::Name(Name::from(ft.as_ft_name()))));
    }
    let names = patch
        .iter()
        .map(|(k, _)| String::from_utf8_lossy(k.as_bytes()).into_owned())
        .collect();
    Ok((clone_then_patch(raw, closure, &strip, patch)?, names))
}

/// The widget half: the raw dictionary minus [`WIDGET_SOURCE_BOUND`] and,
/// for a merged dictionary, minus the keys its field half carries.
fn widget_half(closure: &mut Closure<'_>, raw: &Dict, merged: bool) -> Result<Dict, ClipError> {
    let mut strip: Vec<(&[u8], Strip)> = WIDGET_SOURCE_BOUND.to_vec();
    if merged {
        strip.extend(
            FIELD_ONLY_KEYS
                .iter()
                .chain(MERGED_FIELD_KEYS)
                .map(|k| (*k, Strip::Silent)),
        );
    }
    clone_then_patch(raw, closure, &strip, Vec::new())
}

/// The inheritable attributes (§12.7.3.1) `raw` does not set, read up its
/// `/Parent` chain, then the `/AcroForm` `/DA` and `/Q` defaults (Table 218)
/// for a variable-text field. Values are interned into `closure`.
fn inherited_attributes<G: ObjectGraph + ?Sized>(
    closure: &mut Closure<'_>,
    graph: &G,
    raw: &Dict,
    field: &forms::Field,
) -> Result<Vec<(Name, Object)>, ClipError> {
    let mut wanted: Vec<&[u8]> = INHERITABLE.to_vec();
    if field.field_type == Some(FieldType::Button) {
        wanted.push(b"Opt");
    }
    wanted.retain(|k| set_value(raw, k).is_none());
    let mut found: Vec<(Name, Object)> = Vec::new();
    let mut take_from = |dict: &Dict, wanted: &mut Vec<&[u8]>| {
        wanted.retain(|key| match set_value(dict, key) {
            Some(value) => {
                found.push((Name::from(*key), value.clone()));
                false
            }
            None => true,
        });
    };
    let mut seen = BTreeSet::new();
    let mut next = raw.get(b"Parent").and_then(Object::as_reference);
    while let Some(id) = next {
        if seen.len() >= MAX_PARENT_DEPTH || !seen.insert(id) {
            break;
        }
        let Some(parent) = graph.resolved(id).as_dict() else {
            break;
        };
        take_from(parent, &mut wanted);
        next = parent.get(b"Parent").and_then(Object::as_reference);
    }
    if matches!(field.field_type, Some(FieldType::Text | FieldType::Choice))
        && let Some(acroform) = graph
            .catalog_dict()
            .and_then(|c| c.get(b"AcroForm"))
            .and_then(|a| graph.resolve(a).as_dict())
    {
        let mut defaults: Vec<&[u8]> = wanted
            .iter()
            .copied()
            .filter(|k| matches!(*k, b"DA" | b"Q"))
            .collect();
        take_from(acroform, &mut defaults);
    }
    found
        .into_iter()
        .map(|(k, v)| Ok((k, closure.take(&v, 0)?)))
        .collect()
}

/// `dict[key]`, treating a `null` value as absent (§7.3.7).
fn set_value<'d>(dict: &'d Dict, key: &[u8]) -> Option<&'d Object> {
    dict.get(key).filter(|v| !matches!(v, Object::Null))
}

/// The `/AcroForm /DR /Font` entry the carried `/DA` names, interned as an
/// indirect clip object so the paste installs it by reference (§12.7.3.3:
/// the `/DA` font must resolve in the destination's `/DR`).
fn da_font<G: ObjectGraph + ?Sized>(
    closure: &mut Closure<'_>,
    graph: &G,
    form: &forms::AcroForm,
    field: &Dict,
) -> Result<Option<(Vec<u8>, u32)>, ClipError> {
    let entry = field
        .get(b"DA")
        .and_then(|da| match da {
            Object::String(bytes) => Some(bytes.clone()),
            Object::Reference(id) => match closure.objects.get(&id.num).map(|o| &o.value) {
                Some(Object::String(bytes)) => Some(bytes.clone()),
                _ => None,
            },
            _ => None,
        })
        .and_then(|da| crate::vartext::parse_default_appearance(&da).ok())
        .map(|parsed| parsed.font_name)
        .and_then(|name| Some((dr_font_entry(graph, form, &name)?, name)));
    let Some((value, name)) = entry else {
        return Ok(None);
    };
    let number = match closure.take(&value, 0)? {
        Object::Reference(id) => id.num,
        direct => closure.adopt(direct)?,
    };
    Ok(Some((name, number)))
}

/// The `/AcroForm /DR /Font /<name>` value, unresolved, so an indirect entry
/// stays indirect and the closure walker sees a reference to intern.
fn dr_font_entry<G: ObjectGraph + ?Sized>(
    graph: &G,
    form: &forms::AcroForm,
    name: &[u8],
) -> Option<Object> {
    if !form.has_default_resources {
        return None;
    }
    let catalog = graph.catalog_dict()?;
    let acroform = graph.resolve(catalog.get(b"AcroForm")?).as_dict()?;
    let dr = graph.resolve(acroform.get(b"DR")?).as_dict()?;
    let fonts = graph.resolve(dr.get(b"Font")?).as_dict()?;
    fonts.get(name).cloned()
}
