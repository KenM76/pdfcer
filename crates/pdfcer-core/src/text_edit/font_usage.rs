//! Where a simple font's codes are shown: the proof decision 172 needs that a
//! code is unused and that a font object is private to the pages edited.

use std::collections::BTreeSet;

use crate::content::ContentStream;
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::text_edit::edit::{carried_codes, walk_records};
use crate::text_edit::forms::scan_page_forms;
use crate::view::DocumentView;

/// Upper bound on objects visited proving a resource graph does not reach the font.
const REACH_BUDGET: usize = 20_000;

/// Every code shown by font object `font` on any page, any form a page
/// invokes, or any annotation appearance.
///
/// `Err` when some surface that could show it cannot be read — then "unused"
/// cannot be proven, which is a decision 172 guard.
pub(crate) fn codes_shown(doc: &DocumentView<'_>, font: ObjId) -> Result<BTreeSet<u32>, String> {
    let unproven = |what: &str| format!("{what}, so the new code cannot be proven unused");
    let pages =
        crate::page_tree::pages_in(doc).map_err(|_| unproven("the page tree is unreadable"))?;
    let mut shown = BTreeSet::new();
    for page in &pages {
        ensure_plain(doc, &page.resources, font).map_err(&unproven)?;
        let stream = ContentStream::from_page(doc, page)
            .map_err(|_| unproven("a page's content cannot be parsed"))?;
        collect(doc, &page.resources, &stream, font, &mut shown);
        let scan = scan_page_forms(doc, page);
        if scan.unresolved > 0 || scan.depth_overflows > 0 {
            return Err(unproven("a form XObject cannot be read"));
        }
        for form in &scan.forms {
            ensure_plain(doc, &form.resources, font).map_err(&unproven)?;
            if font_names(doc, &form.resources, font).is_empty() {
                continue;
            }
            let stream = decode_form(doc, form.id)
                .ok_or_else(|| unproven("a form XObject's content cannot be parsed"))?;
            collect(doc, &form.resources, &stream, font, &mut shown);
        }
        appearances(doc, page.id, font, &mut shown).map_err(&unproven)?;
    }
    Ok(shown)
}

/// `Ok` when no surface reaches `/ToUnicode` stream `map` except through font
/// `font`: page resources (forms included, transitively), annotation
/// appearances and the interactive form's `/DR`. Rewriting a shared map would
/// change another font's text.
pub(crate) fn map_private(doc: &DocumentView<'_>, font: ObjId, map: ObjId) -> Result<(), String> {
    let shared = || "the /ToUnicode map may be shared with another font".to_owned();
    let pages = crate::page_tree::pages_in(doc).map_err(|_| shared())?;
    let reached = |o: &Object| reaches_avoiding(doc, o, map, Some(font));
    for page in &pages {
        if reached(&Object::Dict(page.resources.clone())) {
            return Err(shared());
        }
        let annots = doc
            .resolved(page.id)
            .as_dict()
            .and_then(|p| p.get(b"Annots"))
            .map(|a| doc.resolve(a))
            .and_then(Object::as_array)
            .unwrap_or(&[]);
        let ap = |a: &Object| doc.resolve(a).as_dict().and_then(|d| d.get(b"AP")).cloned();
        if annots.iter().filter_map(ap).any(|a| reached(&a)) {
            return Err(shared());
        }
    }
    let dr = doc
        .catalog_dict()
        .and_then(|c| c.get(b"AcroForm"))
        .map(|a| doc.resolve(a))
        .and_then(Object::as_dict)
        .and_then(|a| a.get(b"DR"));
    match dr {
        Some(dr) if reached(dr) => Err(shared()),
        _ => Ok(()),
    }
}

fn collect(
    doc: &DocumentView<'_>,
    resources: &Dict,
    stream: &ContentStream,
    font: ObjId,
    shown: &mut BTreeSet<u32>,
) {
    let names = font_names(doc, resources, font);
    if names.is_empty() {
        return;
    }
    let recs = walk_records(doc, resources, stream);
    for name in names {
        shown.extend(carried_codes(&recs, &name));
    }
}

/// The `/Font` resource names in `resources` that reference `font`.
fn font_names(doc: &DocumentView<'_>, resources: &Dict, font: ObjId) -> Vec<Vec<u8>> {
    doc.resolve(resources.get(b"Font").unwrap_or(&Object::Null))
        .as_dict()
        .map(|fonts| {
            fonts
                .iter()
                .filter(|(_, v)| v.as_reference() == Some(font))
                .map(|(k, _)| k.as_bytes().to_vec())
                .collect()
        })
        .unwrap_or_default()
}

/// Refuse a surface whose patterns, graphics states or Type 3 fonts reach
/// `font`: their content is not walked here.
fn ensure_plain(doc: &DocumentView<'_>, resources: &Dict, font: ObjId) -> Result<(), &'static str> {
    for key in [b"Pattern".as_slice(), b"ExtGState"] {
        if let Some(v) = resources.get(key)
            && reaches(doc, v, font)
        {
            return Err("a pattern or soft mask may show the font");
        }
    }
    let fonts = doc.resolve(resources.get(b"Font").unwrap_or(&Object::Null));
    for (_, f) in fonts.as_dict().map(Dict::iter).into_iter().flatten() {
        if f.as_reference() == Some(font) {
            continue;
        }
        if let Some(res) = doc.resolve(f).as_dict().and_then(|d| d.get(b"Resources"))
            && reaches(doc, res, font)
        {
            return Err("a Type 3 font may show the font");
        }
    }
    Ok(())
}

/// Annotation appearance streams on `page` that name `font` directly are
/// walked; one that reaches it any other way is unproven.
fn appearances(
    doc: &DocumentView<'_>,
    page: ObjId,
    font: ObjId,
    shown: &mut BTreeSet<u32>,
) -> Result<(), &'static str> {
    let Some(annots) = doc
        .resolved(page)
        .as_dict()
        .and_then(|p| p.get(b"Annots"))
        .map(|a| doc.resolve(a))
        .and_then(Object::as_array)
    else {
        return Ok(());
    };
    for annot in annots {
        let Some(ap) = doc
            .resolve(annot)
            .as_dict()
            .and_then(|a| a.get(b"AP"))
            .map(|o| doc.resolve(o))
            .and_then(Object::as_dict)
        else {
            continue;
        };
        for (_, entry) in ap.iter() {
            let streams: Vec<&Object> = match doc.resolve(entry) {
                Object::Dict(states) => states.iter().map(|(_, s)| s).collect(),
                _ => vec![entry],
            };
            for s in streams {
                let Object::Stream(st) = doc.resolve(s) else {
                    continue;
                };
                let Some(res) = st.dict.get(b"Resources") else {
                    continue;
                };
                if !reaches(doc, res, font) {
                    continue;
                }
                let Some(res) = doc.resolve(res).as_dict() else {
                    return Err("an annotation appearance may show the font");
                };
                let plain = ["XObject", "Pattern", "ExtGState"]
                    .iter()
                    .all(|k| res.get(k.as_bytes()).is_none_or(|v| !reaches(doc, v, font)));
                let stream = doc
                    .slice(st.data_span)
                    .and_then(|raw| crate::filters::decode_stream(&st.dict, raw).ok())
                    .and_then(|d| ContentStream::parse(d).ok());
                match (plain, stream) {
                    (true, Some(stream)) => collect(doc, res, &stream, font, shown),
                    _ => return Err("an annotation appearance may show the font"),
                }
            }
        }
    }
    Ok(())
}

fn decode_form(doc: &DocumentView<'_>, id: ObjId) -> Option<ContentStream> {
    let Some(Object::Stream(form)) = doc.graph().value(id) else {
        return None;
    };
    let raw = doc.slice(form.data_span)?;
    let decoded = crate::filters::decode_stream(&form.dict, raw).ok()?;
    ContentStream::parse(decoded).ok()
}

/// Whether the object graph under `start` references `font`. Exhausting the
/// budget answers `true`: an unproven "no" is a "maybe".
fn reaches(doc: &DocumentView<'_>, start: &Object, font: ObjId) -> bool {
    reaches_avoiding(doc, start, font, None)
}

/// [`reaches`], not looking through object `skip`.
fn reaches_avoiding(
    doc: &DocumentView<'_>,
    start: &Object,
    font: ObjId,
    skip: Option<ObjId>,
) -> bool {
    let mut seen: BTreeSet<ObjId> = skip.into_iter().collect();
    let mut stack: Vec<&Object> = vec![start];
    let mut budget = REACH_BUDGET;
    while let Some(o) = stack.pop() {
        budget = match budget.checked_sub(1) {
            Some(b) => b,
            None => return true,
        };
        match o {
            Object::Reference(id) => {
                if *id == font {
                    return true;
                }
                if seen.insert(*id)
                    && let Some(v) = doc.graph().value(*id)
                {
                    stack.push(v);
                }
            }
            Object::Array(a) => stack.extend(a.iter()),
            Object::Dict(d) => stack.extend(d.iter().map(|(_, v)| v)),
            Object::Stream(s) => stack.extend(s.dict.iter().map(|(_, v)| v)),
            _ => {}
        }
    }
    false
}
