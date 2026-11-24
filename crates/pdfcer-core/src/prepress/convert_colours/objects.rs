//! The planner, and the sampled side of convert colours: image samples
//! (ISO 32000-2 §8.9.5), `Indexed` lookup tables (§8.6.6.3) and axial and
//! radial shadings (§8.7.4.5.3, §8.7.4.5.4).

use std::collections::{BTreeMap, BTreeSet};

use crate::color::RenderingIntent;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object, Stream};
use crate::optimize::objects::is_image;
use crate::view::DocumentView;

use super::super::inks::Planned;
use super::super::rewrite::{
    SHADING_SEGMENTS, convert_samples, current, decode_ranges, image_samples, int, keep,
    lookup_table, name, reals, shading_domain, shading_functions, stitch, to_byte,
};
use super::embed::{EmbedPlan, describe, embed_plan};
use super::scope::{self, Reach};
use super::sources::{Destination, Registry, output_intent_profile};
use super::{
    ColourConversion, ColourConverter, ColourDestination, ConvertColoursReport, ConvertScope,
    Counted, ImageConversion, IntentChoice,
};

/// Nesting followed inside one object.
const MAX_NESTING: usize = 24;

/// A conversion worked out against a view, before it is committed.
pub(crate) struct ConvertPlan {
    pub(crate) writes: BTreeMap<ObjId, Planned>,
    pub(crate) report: ConvertColoursReport,
    /// The output intent to embed; it needs a new object number.
    pub(crate) embed: Option<EmbedPlan>,
}

/// Why a conversion cannot start.
pub(crate) enum PlanError {
    /// No destination, or no source selected.
    Refused(String),
    /// A page index past the end.
    Page { index: usize, count: usize },
}

/// Work out converting `conversion`'s sources in `scope`.
pub(crate) fn plan_convert_colours(
    view: &DocumentView<'_>,
    scope: &ConvertScope,
    conversion: &ColourConversion,
    converter: &dyn ColourConverter,
) -> Result<ConvertPlan, PlanError> {
    if !conversion.selects_any() {
        return Err(PlanError::Refused(
            "no source colour space is selected".to_owned(),
        ));
    }
    let intent_profile = output_intent_profile(view);
    let profile = match &conversion.destination {
        ColourDestination::Profile(p) => p.clone(),
        ColourDestination::OutputIntent => intent_profile.clone().ok_or_else(|| {
            PlanError::Refused(
                "the document has no output intent with a readable profile".to_owned(),
            )
        })?,
    };
    let dest = Destination::from_profile(profile).map_err(PlanError::Refused)?;
    let pages = crate::page_tree::pages_in(view.graph())
        .map_err(|e| PlanError::Refused(format!("the page tree cannot be read ({e})")))?;
    let (chosen, reach) = scoped(view, scope, &pages)?;
    let mut reg = Registry::new(view, conversion, converter, dest);
    let mut writes = BTreeMap::new();
    super::content::rewrite_pages(&mut reg, &chosen, &reach.ids, &mut writes);
    for id in &reach.ids {
        object(&mut reg, *id, &mut writes);
    }
    let embed = conversion
        .embed_output_intent
        .as_deref()
        .and_then(|subtype| {
            embed_plan(
                &reg.dest,
                subtype,
                intent_profile.as_deref(),
                &mut reg.report,
            )
        });
    let mut report = reg.report;
    disclose(
        &mut report,
        &reach,
        intent_profile.as_deref(),
        &reg.dest,
        conversion,
    );
    shared(view, scope, &pages, &writes, &mut report);
    Ok(ConvertPlan {
        writes,
        report,
        embed,
    })
}

/// The pages a scope walks and the objects it reaches.
fn scoped<'p>(
    view: &DocumentView<'_>,
    scope: &ConvertScope,
    pages: &'p [crate::page_tree::Page],
) -> Result<(Vec<&'p crate::page_tree::Page>, Reach), PlanError> {
    match scope {
        ConvertScope::All => Ok((pages.iter().collect(), scope::whole(view))),
        ConvertScope::Pages(list) => {
            let mut chosen = Vec::new();
            for index in list {
                let page = pages.get(*index).ok_or(PlanError::Page {
                    index: *index,
                    count: pages.len(),
                })?;
                if !chosen
                    .iter()
                    .any(|p: &&crate::page_tree::Page| p.id == page.id)
                {
                    chosen.push(page);
                }
            }
            let reach = scope::pages(view, &chosen);
            Ok((chosen, reach))
        }
    }
}

/// Convert one in-scope object: an image's samples, or the lookup tables
/// and shadings inside it.
fn object(reg: &mut Registry<'_, '_>, id: ObjId, writes: &mut BTreeMap<ObjId, Planned>) {
    let Some(mut obj) = current(reg.view, writes, id) else {
        return;
    };
    if let Object::Stream(s) = &obj
        && is_image(&s.dict)
        && let Some(planned) = image(reg, id, s)
    {
        writes.insert(id, planned);
        return;
    }
    if nested(reg, &mut obj, Some(id), 0) {
        keep(writes, id, obj);
    }
}

/// Convert an image in a source space to 8-bit destination samples, or
/// `None` when its space does not convert or it is left as it is.
fn image(reg: &mut Registry<'_, '_>, id: ObjId, s: &Stream) -> Option<Planned> {
    let view = reg.view;
    if matches!(
        s.dict.get(b"ImageMask").map(|o| view.resolve(o)),
        Some(Object::Boolean(true))
    ) {
        return None;
    }
    let src = reg.lookup(s.dict.get(b"ColorSpace")?, None, Some(id))?;
    let source = reg.sources.get(src)?;
    let (family, n, ranges) = (source.family, source.ranges.len(), source.ranges.clone());
    let leave = |reg: &mut Registry<'_, '_>, why: &str| {
        reg.report.unconvert(Some(id), format!("the image {why}"));
        None
    };
    if has_matte(view, &s.dict) {
        return leave(
            reg,
            "has a soft mask with /Matte, which is premultiplied in its space",
        );
    }
    let img = match image_samples(view, s, reg.dest.n) {
        Ok(img) => img,
        Err(why) => return leave(reg, &why),
    };
    let own = s
        .dict
        .get(b"Intent")
        .and_then(|o| view.resolve(o).as_name())
        .map_or(RenderingIntent::RelativeColorimetric, |n| {
            RenderingIntent::from_name(n.as_bytes())
        });
    let intent = reg.intent(own);
    let decode = decode_ranges(view, &s.dict, &ranges, img.bpc);
    let mut oog = false;
    let mut map = |t: &[f64]| {
        let (values, out) = reg.convert(src, t, intent, Some(id))?;
        oog |= out;
        Some(values.into_iter().map(to_byte).collect())
    };
    let Some(out) = convert_samples(&img, n, &decode, &mut map) else {
        return leave(
            reg,
            "data is shorter than its size, or its colours cannot be converted",
        );
    };
    let data = crate::filters::flate::encode(&out);
    reg.report.count(family, Counted::Image, oog);
    reg.report.images.push(ImageConversion {
        object: id,
        source: family,
        bits_before: u8::try_from(img.bpc).unwrap_or(8),
        filter_before: first_filter(view, &s.dict),
        bytes_before: s.data_span.len,
        bytes_after: data.len(),
    });
    let mut dict = s.dict.clone();
    dict.insert(Name::from(b"ColorSpace"), name(reg.dest.space()));
    dict.insert(Name::from(b"BitsPerComponent"), Object::Integer(8));
    dict.insert(Name::from(b"Filter"), name(b"FlateDecode"));
    dict.remove(b"Decode");
    dict.remove(b"DecodeParms");
    Some(Planned::Stream { dict, data })
}

/// The image's `/SMask` carries `/Matte` (§11.6.5.3).
fn has_matte(view: &DocumentView<'_>, d: &Dict) -> bool {
    match d.get(b"SMask").map(|o| view.resolve(o)) {
        Some(Object::Stream(m)) => m.dict.contains_key(b"Matte"),
        _ => false,
    }
}

fn first_filter(view: &DocumentView<'_>, d: &Dict) -> Option<String> {
    let f = view.resolve(d.get(b"Filter")?);
    let n = match f {
        Object::Name(n) => n,
        Object::Array(a) => view.resolve(a.first()?).as_name()?,
        _ => return None,
    };
    Some(String::from_utf8_lossy(n.as_bytes()).into_owned())
}

/// Rewrite `Indexed` spaces and shadings inside `obj`; true when it changed.
fn nested(reg: &mut Registry<'_, '_>, obj: &mut Object, at: Option<ObjId>, depth: usize) -> bool {
    if depth > MAX_NESTING {
        return false;
    }
    match obj {
        Object::Array(items) => {
            if let Some(converted) = indexed(reg, items, at) {
                *obj = converted;
                return true;
            }
            let mut changed = false;
            for item in items.iter_mut() {
                changed |= nested(reg, item, at, depth + 1);
            }
            changed
        }
        Object::Dict(d) => entries(reg, d, at, depth),
        Object::Stream(s) => entries(reg, &mut s.dict, at, depth),
        _ => false,
    }
}

fn entries(reg: &mut Registry<'_, '_>, d: &mut Dict, at: Option<ObjId>, depth: usize) -> bool {
    if d.contains_key(b"ShadingType") {
        return shading(reg, d, at);
    }
    let mut changed = false;
    for (key, value) in &mut d.0 {
        if matches!(key.as_bytes(), b"SMask" | b"Mask" | b"Thumb") {
            continue;
        }
        changed |= nested(reg, value, at, depth + 1);
    }
    changed
}

/// `[/Indexed base hival lookup]` over a source base, as
/// `[/Indexed dest hival lookup']`; `None` for any other array. Each table
/// byte maps linearly onto its component's range (§8.6.6.3).
fn indexed(reg: &mut Registry<'_, '_>, items: &[Object], at: Option<ObjId>) -> Option<Object> {
    let view = reg.view;
    let head = items.first().and_then(|o| view.resolve(o).as_name())?;
    if head.as_bytes() != b"Indexed" {
        return None;
    }
    let src = reg.lookup(items.get(1)?, None, at)?;
    let source = reg.sources.get(src)?;
    let (family, ranges) = (source.family, source.ranges.clone());
    let n = ranges.len();
    let hival = usize::try_from(view.resolve(items.get(2)?).as_int()?)
        .ok()?
        .min(255);
    let table = lookup_table(view, items.get(3)?)?;
    let intent = reg.intent(RenderingIntent::RelativeColorimetric);
    let mut out = Vec::with_capacity((hival + 1) * reg.dest.n);
    let mut oog = false;
    for entry in 0..=hival {
        let input: Option<Vec<f64>> = table.get(entry * n..(entry + 1) * n).map(|b| {
            b.iter()
                .zip(&ranges)
                .map(|(v, (lo, hi))| lo + f64::from(*v) / 255.0 * (hi - lo))
                .collect()
        });
        let Some((values, o)) = input.and_then(|i| reg.convert(src, &i, intent, at)) else {
            let why = "its indexed lookup table is short or cannot be converted".to_owned();
            reg.report.unconvert(at, why);
            return None;
        };
        oog |= o;
        out.extend(values.into_iter().map(to_byte));
    }
    reg.report.count(family, Counted::Lookup, oog);
    Some(Object::Array(vec![
        name(b"Indexed"),
        name(reg.dest.space()),
        Object::Integer(i64::try_from(hival).unwrap_or(255)),
        Object::String(out),
    ]))
}

/// Convert an axial or radial shading in place; true when it changed.
fn shading(reg: &mut Registry<'_, '_>, d: &mut Dict, at: Option<ObjId>) -> bool {
    let view = reg.view;
    let Some(src) = d
        .get(b"ColorSpace")
        .cloned()
        .and_then(|cs| reg.lookup(&cs, None, at))
    else {
        return false;
    };
    let Some(source) = reg.sources.get(src) else {
        return false;
    };
    let (family, n) = (source.family, source.ranges.len());
    let kind = int(view, d, b"ShadingType").unwrap_or(0);
    if !matches!(kind, 2 | 3) {
        let why = format!(
            "a type {kind} shading is in {}; only axial and radial shadings are converted",
            family.name()
        );
        reg.report.unconvert(at, why);
        return false;
    }
    let functions = shading_functions(view, d);
    let Some(domain) = shading_domain(view, d) else {
        reg.report
            .unconvert(at, "a shading's domain cannot be read".to_owned());
        return false;
    };
    let intent = reg.intent(RenderingIntent::RelativeColorimetric);
    let mut oog = false;
    let mut map = |t: &[f64]| {
        let (values, o) = reg.convert(src, t, intent, at)?;
        oog |= o;
        Some(values)
    };
    let Some(stitched) = stitch(&functions, n, domain, &mut map) else {
        reg.report.unconvert(
            at,
            "a shading's function cannot be evaluated or converted".to_owned(),
        );
        return false;
    };
    if let Some(bg) = d
        .get(b"Background")
        .and_then(|o| view.resolve(o).as_array())
    {
        let input: Vec<f64> = bg.iter().filter_map(Object::as_number).collect();
        match reg.convert(src, &input, intent, at) {
            Some((values, _)) => d.insert(Name::from(b"Background"), reals(&values)),
            None => {
                d.remove(b"Background");
            }
        }
    }
    d.insert(Name::from(b"ColorSpace"), name(reg.dest.space()));
    d.insert(Name::from(b"Function"), stitched);
    reg.report.count(family, Counted::Shading, oog);
    true
}

/// The report's standing disclosures.
fn disclose(
    report: &mut ConvertColoursReport,
    reach: &Reach,
    intent_profile: Option<&[u8]>,
    dest: &Destination,
    conversion: &ColourConversion,
) {
    if report.by_source.iter().any(|c| c.shadings > 0) {
        report.disclose(format!(
            "Converted shadings are sampled at {} points along their axis and joined linearly.",
            SHADING_SEGMENTS + 1
        ));
    }
    if reach.cut > 0 {
        report.disclose(format!(
            "{} soft masks and thumbnails were left as they are; they are not printed colours.",
            reach.cut
        ));
    }
    if !report.images.is_empty() {
        report.disclose("Converted images are written as 8-bit FlateDecode samples.".to_owned());
    }
    if conversion.intent == IntentChoice::ObjectsOwn
        && report.by_source.iter().any(|c| c.lookups + c.shadings > 0)
    {
        report.disclose(
            "Indexed lookup tables and shadings carry no rendering intent; they used RelativeColorimetric."
                .to_owned(),
        );
    }
    if intent_profile.is_some_and(|p| p != dest.profile) {
        let name = describe(&dest.profile).unwrap_or_default();
        report.disclose(format!(
            "The destination profile {name} is not the document's output intent profile."
        ));
    }
}

/// Disclose changed objects that pages outside the scope also draw.
fn shared(
    view: &DocumentView<'_>,
    scope: &ConvertScope,
    pages: &[crate::page_tree::Page],
    writes: &BTreeMap<ObjId, Planned>,
    report: &mut ConvertColoursReport,
) {
    let ConvertScope::Pages(list) = scope else {
        return;
    };
    let others: Vec<&crate::page_tree::Page> = pages
        .iter()
        .enumerate()
        .filter(|(i, _)| !list.contains(i))
        .map(|(_, p)| p)
        .collect();
    if others.is_empty() || writes.is_empty() {
        return;
    }
    let outside: BTreeSet<ObjId> = scope::pages(view, &others).ids;
    let both = writes.keys().filter(|id| outside.contains(id)).count();
    if both > 0 {
        report.disclose(format!(
            "{both} changed objects are also drawn on pages outside the scope, which change too."
        ));
    }
}
