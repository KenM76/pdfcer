//! [`metadata_inventory`]: one walk over every object reachable from the
//! trailer, plus the readers that already know attachments, layers and form
//! fields.

use std::collections::{BTreeSet, HashMap, HashSet};

use super::{
    JsSlot, MetadataInventory, MetadataItem, MetadataItemId, MetadataKind, PREVIEW_CHARS, Target,
    escape,
};
use crate::attachments::{AttachmentKind, list_attachments};
use crate::forms::{FieldValue, MAX_ACTION_CHAIN_DEPTH, parse_acroform};
use crate::graph::ObjectGraph;
use crate::layers::list_layers;
use crate::object::{Dict, ObjId, Object};
use crate::textstring::decode_text_string;
use crate::view::DocumentView;
use crate::writer::encoder::IdentityEncoder;
use crate::writer::serialize::write_object;

/// Annotation subtypes that are not comments: links, widgets, pop-ups
/// (removed with their parent), and the media, 3D and print-production
/// kinds. Every other subtype is a markup annotation (§12.5.6.2).
const NOT_COMMENTS: &[&[u8]] = &[
    b"Link",
    b"Widget",
    b"Popup",
    b"Screen",
    b"Movie",
    b"3D",
    b"RichMedia",
    b"PrinterMark",
    b"TrapNet",
    b"Watermark",
];

/// Where objects sit, for [`MetadataItem::location`].
struct Places {
    catalog: Option<ObjId>,
    page_of: HashMap<ObjId, usize>,
    annot_page: HashMap<ObjId, usize>,
}

/// Every carrier of metadata or hidden information in `view`, in the order
/// [`MetadataInventory::items`] states. `file_bytes` is the file as opened;
/// its `%%EOF` markers give the earlier revisions.
///
/// Read-only. The object walk is bounded by
/// [`MAX_REACHABLE_OBJECTS`](crate::edit::MAX_REACHABLE_OBJECTS) and a
/// script's `/Next` chain by [`MAX_ACTION_CHAIN_DEPTH`].
#[must_use]
pub fn metadata_inventory(view: &DocumentView<'_>, file_bytes: &[u8]) -> MetadataInventory {
    let places = places(view);
    let mut roots: Vec<ObjId> = places.catalog.into_iter().collect();
    roots.extend(view.trailer_entry(b"Info").and_then(Object::as_reference));
    let live = crate::edit::reachable(view, &roots, &HashSet::new());
    let truncated = live.len() >= crate::edit::MAX_REACHABLE_OBJECTS;
    let live: BTreeSet<ObjId> = live.into_iter().collect();

    let mut items = info_items(view);
    let mut per_object: [Vec<MetadataItem>; 4] = Default::default();
    for &id in &live {
        if let Some(dict) = view.value(id).and_then(Object::as_dict) {
            object_items(view, &places, id, dict, &mut per_object);
        }
    }
    for group in per_object {
        items.extend(group);
    }
    items.extend(names_script_item(view, places.catalog));
    items.extend(attachment_items(view));
    items.extend(comment_items(view, &places));
    items.extend(hidden_layer_items(view));
    items.extend(form_data_item(view));
    items.extend(revisions_item(file_bytes));
    items.extend(document_id_item(view));
    MetadataInventory { items, truncated }
}

fn places(view: &DocumentView<'_>) -> Places {
    let pages = crate::page_tree::pages_in(view).unwrap_or_default();
    let mut annot_page = HashMap::new();
    for (index, page) in pages.iter().enumerate() {
        let annots = view
            .value(page.id)
            .and_then(Object::as_dict)
            .and_then(|d| d.get(b"Annots"))
            .map(|a| view.resolve(a))
            .and_then(Object::as_array)
            .unwrap_or_default();
        for annot in annots.iter().filter_map(Object::as_reference) {
            annot_page.insert(annot, index);
        }
    }
    Places {
        catalog: view.trailer_entry(b"Root").and_then(Object::as_reference),
        page_of: pages.iter().enumerate().map(|(i, p)| (p.id, i)).collect(),
        annot_page,
    }
}

fn item(
    target: &Target,
    kind: MetadataKind,
    location: String,
    preview: String,
    bytes: u64,
) -> MetadataItem {
    MetadataItem {
        id: MetadataItemId::of(target),
        kind,
        location,
        preview: clip(&preview),
        bytes,
    }
}

/// `text` with whitespace runs collapsed, cut to [`PREVIEW_CHARS`].
fn clip(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    let joined = words.join(" ");
    if joined.chars().count() <= PREVIEW_CHARS {
        return joined;
    }
    let mut out: String = joined.chars().take(PREVIEW_CHARS - 1).collect();
    out.push('…');
    out
}

/// What `obj` occupies when written: a stream's dictionary plus its encoded
/// data, else the serialised value.
fn value_bytes(obj: &Object) -> u64 {
    let mut out = Vec::new();
    match obj {
        Object::Stream(s) => {
            write_object(
                &mut out,
                &Object::Dict(s.dict.clone()),
                ObjId::new(0, 0),
                &[],
                &IdentityEncoder,
            );
            out.len() as u64 + s.data_span.len as u64
        }
        other => {
            write_object(&mut out, other, ObjId::new(0, 0), &[], &IdentityEncoder);
            out.len() as u64
        }
    }
}

/// `id`'s stream data, decoded through its filters.
fn stream_data(view: &DocumentView<'_>, obj: &Object) -> Option<Vec<u8>> {
    let Object::Stream(stream) = obj else {
        return None;
    };
    let raw = view.slice(stream.data_span)?;
    crate::filters::decode_stream(&stream.dict, raw).ok()
}

fn info_items(view: &DocumentView<'_>) -> Vec<MetadataItem> {
    let Some(info) = view
        .trailer_entry(b"Info")
        .map(|i| view.resolve(i))
        .and_then(Object::as_dict)
    else {
        return Vec::new();
    };
    info.iter()
        .map(|(key, value)| {
            let value = view.resolve(value);
            let preview = match value {
                Object::String(s) => decode_text_string(s).text,
                Object::Name(n) => format!("/{}", escape(n.as_bytes())),
                other => format!("{other:?}"),
            };
            item(
                &Target::Info(key.as_bytes().to_vec()),
                MetadataKind::InfoEntry,
                "document information".to_owned(),
                preview,
                key.as_bytes().len() as u64 + 2 + value_bytes(value),
            )
        })
        .collect()
}

/// `id` in words: `catalog`, `page 3`, `annotation 30 0 on page 2`,
/// `image object 12 0`.
fn location(places: &Places, id: ObjId, dict: &Dict) -> String {
    if places.catalog == Some(id) {
        return "catalog".to_owned();
    }
    if let Some(page) = places.page_of.get(&id) {
        return format!("page {}", page + 1);
    }
    if let Some(page) = places.annot_page.get(&id) {
        return format!("annotation {id} on page {}", page + 1);
    }
    let kind = [b"Subtype".as_slice(), b"Type"]
        .iter()
        .find_map(|k| dict.get(k).and_then(Object::as_name))
        .map(|n| String::from_utf8_lossy(n.as_bytes()).to_lowercase());
    match kind {
        Some(k) => format!("{k} object {id}"),
        None => format!("object {id}"),
    }
}

/// The XMP, piece-info, thumbnail and script items on one object, pushed
/// into `out[0..4]` in that order.
fn object_items(
    view: &DocumentView<'_>,
    places: &Places,
    id: ObjId,
    dict: &Dict,
    out: &mut [Vec<MetadataItem>; 4],
) {
    let [xmp, piece, thumb, js] = out;
    let at = || location(places, id, dict);
    if let Some(stream) = dict.get(b"Metadata").map(|m| view.resolve(m))
        && matches!(stream, Object::Stream(_))
    {
        let kind = if places.catalog == Some(id) {
            MetadataKind::DocumentXmp
        } else {
            MetadataKind::ObjectXmp
        };
        let text = stream_data(view, stream)
            .map(|d| xml_text(&d))
            .unwrap_or_default();
        xmp.push(item(
            &Target::Xmp(id),
            kind,
            at(),
            text,
            value_bytes(stream),
        ));
    }
    if let Some(info) = dict.get(b"PieceInfo").map(|p| view.resolve(p))
        && let Some(apps) = info.as_dict()
    {
        let names: Vec<String> = apps.iter().map(|(k, _)| escape(k.as_bytes())).collect();
        let preview = format!("private data of {}", names.join(", "));
        piece.push(item(
            &Target::PieceInfo(id),
            MetadataKind::PieceInfo,
            at(),
            preview,
            value_bytes(info),
        ));
    }
    if let Some(image) = dict.get(b"Thumb").map(|t| view.resolve(t))
        && let Some(d) = image.as_dict()
    {
        let dim = |k: &[u8]| d.get(k).and_then(Object::as_int).unwrap_or(0);
        let preview = format!("{} x {} image", dim(b"Width"), dim(b"Height"));
        thumb.push(item(
            &Target::Thumb(id),
            MetadataKind::Thumbnail,
            at(),
            preview,
            value_bytes(image),
        ));
    }
    script_items(view, id, dict, &at, js);
}

fn script_items(
    view: &DocumentView<'_>,
    id: ObjId,
    dict: &Dict,
    at: &dyn Fn() -> String,
    out: &mut Vec<MetadataItem>,
) {
    let mut slots: Vec<(JsSlot, &Object)> = Vec::new();
    if let Some(a) = dict.get(b"OpenAction") {
        slots.push((JsSlot::OpenAction, a));
    }
    if let Some(a) = dict.get(b"A") {
        slots.push((JsSlot::Action, a));
    }
    if let Some(aa) = dict
        .get(b"AA")
        .map(|a| view.resolve(a))
        .and_then(Object::as_dict)
    {
        for (trigger, a) in aa.iter() {
            slots.push((JsSlot::Additional(trigger.as_bytes().to_vec()), a));
        }
    }
    for (slot, action) in slots {
        if let Some((script, bytes)) = javascript_in(view, action) {
            let target = Target::Js(id, slot);
            out.push(item(&target, MetadataKind::JavaScript, at(), script, bytes));
        }
    }
}

/// The first script in `action`'s `/Next` chain and the chain's size, when
/// any action in it is JavaScript.
fn javascript_in(view: &DocumentView<'_>, action: &Object) -> Option<(String, u64)> {
    let mut queue = vec![(view.resolve(action), 0usize)];
    let mut found: Option<String> = None;
    let mut bytes = 0;
    while let Some((node, depth)) = queue.pop() {
        if depth > MAX_ACTION_CHAIN_DEPTH {
            continue;
        }
        if let Some(list) = node.as_array() {
            queue.extend(list.iter().map(|a| (view.resolve(a), depth + 1)));
            continue;
        }
        let Some(d) = node.as_dict() else { continue };
        bytes += value_bytes(node);
        if d.get(b"S")
            .and_then(Object::as_name)
            .is_some_and(|n| n.as_bytes() == b"JavaScript")
        {
            let js = d.get(b"JS").map(|j| view.resolve(j));
            let text = match js {
                Some(Object::String(s)) => Some(decode_text_string(s).text),
                Some(stream) => stream_data(view, stream).map(|b| decode_text_string(&b).text),
                None => None,
            };
            found.get_or_insert(text.unwrap_or_default());
        }
        if let Some(next) = d.get(b"Next") {
            queue.push((view.resolve(next), depth + 1));
        }
    }
    found.map(|text| (text, bytes))
}

/// XMP's character data with the markup removed, for a preview.
fn xml_text(xml: &[u8]) -> String {
    let text = String::from_utf8_lossy(xml);
    let mut out = String::new();
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => {
                in_tag = true;
                out.push(' ');
            }
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
        if out.len() > PREVIEW_CHARS * 8 {
            break;
        }
    }
    out
}

fn names_script_item(view: &DocumentView<'_>, catalog: Option<ObjId>) -> Option<MetadataItem> {
    let tree = view
        .value(catalog?)
        .and_then(Object::as_dict)?
        .get(b"Names")
        .map(|n| view.resolve(n))
        .and_then(Object::as_dict)?
        .get(b"JavaScript")
        .map(|j| view.resolve(j))?;
    Some(item(
        &Target::JsNames,
        MetadataKind::JavaScript,
        "document scripts".to_owned(),
        "the document-level JavaScript name tree".to_owned(),
        value_bytes(tree),
    ))
}

fn attachment_items(view: &DocumentView<'_>) -> Vec<MetadataItem> {
    list_attachments(view)
        .into_iter()
        .filter_map(|a| {
            let AttachmentKind::DocumentLevel { tree_key, .. } = &a.kind else {
                return None;
            };
            let raw = a
                .stream_id
                .and_then(|id| view.value(id))
                .map_or(0, value_bytes);
            Some(item(
                &Target::Attachment(tree_key.clone()),
                MetadataKind::Attachment,
                "document attachments".to_owned(),
                a.name.clone(),
                a.declared_size.unwrap_or(raw),
            ))
        })
        .collect()
}

fn comment_items(view: &DocumentView<'_>, places: &Places) -> Vec<MetadataItem> {
    let mut annots: Vec<(&ObjId, &usize)> = places.annot_page.iter().collect();
    annots.sort_by_key(|(id, page)| (**page, **id));
    annots
        .into_iter()
        .filter_map(|(&id, &page)| {
            let obj = view.value(id)?;
            let dict = obj.as_dict()?;
            let subtype = dict.get(b"Subtype").and_then(Object::as_name)?.as_bytes();
            if NOT_COMMENTS.contains(&subtype) {
                return None;
            }
            let contents = match dict.get(b"Contents").map(|c| view.resolve(c)) {
                Some(Object::String(s)) => decode_text_string(s).text,
                _ => String::new(),
            };
            let preview = format!("{}: {contents}", String::from_utf8_lossy(subtype));
            Some(item(
                &Target::Comment(id),
                MetadataKind::Comment,
                format!("annotation {id} on page {}", page + 1),
                preview,
                value_bytes(obj),
            ))
        })
        .collect()
}

fn hidden_layer_items(view: &DocumentView<'_>) -> Vec<MetadataItem> {
    list_layers(view)
        .into_iter()
        .filter(|l| !l.visible_by_default)
        .map(|l| {
            let bytes = view.value(l.id).map_or(0, value_bytes);
            item(
                &Target::Layer(l.id),
                MetadataKind::HiddenLayer,
                format!("layer object {}", l.id),
                l.name,
                bytes,
            )
        })
        .collect()
}

fn form_data_item(view: &DocumentView<'_>) -> Option<MetadataItem> {
    let form = parse_acroform(view)?;
    let mut filled = 0usize;
    let mut bytes = 0u64;
    for field in &form.fields {
        let size = match &field.value {
            FieldValue::Text(t) | FieldValue::Name(t) => t.len(),
            FieldValue::Choice(c) => c.iter().map(Vec::len).sum(),
            _ => continue,
        };
        filled += 1;
        bytes += size as u64;
    }
    (filled > 0).then(|| {
        item(
            &Target::FormData,
            MetadataKind::FormData,
            "form fields".to_owned(),
            format!("{filled} filled field(s)"),
            bytes,
        )
    })
}

fn revisions_item(file_bytes: &[u8]) -> Option<MetadataItem> {
    let ends = crate::password_history::revision_ends(file_bytes);
    let earlier = ends.len().checked_sub(1).filter(|n| *n > 0)?;
    let bytes = ends.get(earlier - 1).copied().unwrap_or(0) as u64;
    Some(item(
        &Target::Revisions,
        MetadataKind::EarlierRevisions,
        "file".to_owned(),
        format!("{earlier} earlier revision(s)"),
        bytes,
    ))
}

fn document_id_item(view: &DocumentView<'_>) -> Option<MetadataItem> {
    let id = view.trailer_entry(b"ID")?;
    let first = id.as_array()?.first().map(|f| view.resolve(f));
    let preview = match first {
        Some(Object::String(s)) => s.iter().map(|b| format!("{b:02x}")).collect(),
        _ => String::new(),
    };
    Some(item(
        &Target::DocumentId,
        MetadataKind::DocumentId,
        "trailer".to_owned(),
        preview,
        value_bytes(id),
    ))
}
