//! Embedding the destination profile as an output intent (ISO 32000-2
//! §14.11.5, Table 365), and reading an ICC profile's description
//! (ICC.1:2022 §9.2.41).

use std::collections::BTreeMap;

use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::view::DocumentView;

use super::super::inks::Planned;
use super::super::rewrite::{current, keep, name};
use super::sources::Destination;
use super::{ConvertColoursReport, OutputIntentChange};

/// An output intent waiting for its profile's object number.
pub(crate) struct EmbedPlan {
    subtype: String,
    description: Option<String>,
    n: usize,
    profile: Vec<u8>,
}

/// What to embed, or `None` when the destination already is the
/// document's output intent profile (disclosed).
pub(crate) fn embed_plan(
    dest: &Destination,
    subtype: &str,
    intent_profile: Option<&[u8]>,
    report: &mut ConvertColoursReport,
) -> Option<EmbedPlan> {
    if intent_profile == Some(dest.profile.as_slice()) {
        report.disclose(
            "The destination profile already is the document's output intent; none was embedded."
                .to_owned(),
        );
        return None;
    }
    Some(EmbedPlan {
        subtype: subtype.to_owned(),
        description: describe(&dest.profile),
        n: dest.n,
        profile: dest.profile.clone(),
    })
}

/// Add the profile as object `profile_id` and its output intent to the
/// catalog's `/OutputIntents`, replacing the entry of the same `/S`.
pub(crate) fn embed_writes(
    view: &DocumentView<'_>,
    plan: EmbedPlan,
    profile_id: ObjId,
    writes: &mut BTreeMap<ObjId, Planned>,
    report: &mut ConvertColoursReport,
) {
    let Some(catalog_id) = view.trailer_entry(b"Root").and_then(Object::as_reference) else {
        report.disclose("The document has no catalog; no output intent was embedded.".to_owned());
        return;
    };
    let Some(Object::Dict(mut catalog)) = current(view, writes, catalog_id) else {
        report.disclose("The catalog cannot be read; no output intent was embedded.".to_owned());
        return;
    };
    let mut stream = Dict::default();
    stream.insert(
        Name::from(b"N"),
        Object::Integer(i64::try_from(plan.n).unwrap_or(4)),
    );
    stream.insert(Name::from(b"Filter"), name(b"FlateDecode"));
    let data = crate::filters::flate::encode(&plan.profile);
    writes.insert(profile_id, Planned::Stream { dict: stream, data });
    let entry = Object::Dict(intent_dict(&plan, profile_id));
    match catalog.get(b"OutputIntents").cloned() {
        Some(Object::Reference(array_id)) => {
            let mut items = match current(view, writes, array_id) {
                Some(Object::Array(items)) => items,
                _ => Vec::new(),
            };
            report.output_intent = place(view, &mut items, entry, &plan.subtype);
            keep(writes, array_id, Object::Array(items));
        }
        other => {
            let mut items = match other {
                Some(Object::Array(items)) => items,
                _ => Vec::new(),
            };
            report.output_intent = place(view, &mut items, entry, &plan.subtype);
            catalog.insert(Name::from(b"OutputIntents"), Object::Array(items));
            keep(writes, catalog_id, Object::Dict(catalog));
        }
    }
    let label = plan.description.as_deref().unwrap_or("Custom");
    let line = match &report.output_intent {
        OutputIntentChange::Replaced { previous } => format!(
            "The {} output intent \"{previous}\" was replaced by the destination profile \"{label}\".",
            plan.subtype
        ),
        _ => format!(
            "The destination profile \"{label}\" was embedded as a {} output intent.",
            plan.subtype
        ),
    };
    report.disclose(line);
}

/// `<< /Type /OutputIntent /S .. /OutputConditionIdentifier .. /Info ..
/// /DestOutputProfile .. >>`.
fn intent_dict(plan: &EmbedPlan, profile_id: ObjId) -> Dict {
    let text = |s: &str| Object::String(crate::edit::encode_text_string(s));
    let mut d = Dict::default();
    d.insert(Name::from(b"Type"), name(b"OutputIntent"));
    d.insert(Name::from(b"S"), name(plan.subtype.as_bytes()));
    let label = plan.description.as_deref().unwrap_or("Custom");
    d.insert(Name::from(b"OutputConditionIdentifier"), text(label));
    if let Some(info) = &plan.description {
        d.insert(Name::from(b"Info"), text(info));
    }
    d.insert(
        Name::from(b"DestOutputProfile"),
        Object::Reference(profile_id),
    );
    d
}

/// Put `entry` in `items` in place of the intent of `subtype`, or at the
/// end.
fn place(
    view: &DocumentView<'_>,
    items: &mut Vec<Object>,
    entry: Object,
    subtype: &str,
) -> OutputIntentChange {
    let same = items.iter().position(|i| {
        view.resolve(i)
            .as_dict()
            .and_then(|d| d.get(b"S"))
            .and_then(|s| view.resolve(s).as_name())
            .is_some_and(|n| n.as_bytes() == subtype.as_bytes())
    });
    let Some(at) = same else {
        items.push(entry);
        return OutputIntentChange::Added;
    };
    let previous = items
        .get(at)
        .and_then(|i| view.resolve(i).as_dict())
        .and_then(|d| d.get(b"OutputConditionIdentifier"))
        .map(|o| match view.resolve(o) {
            Object::String(s) => crate::textstring::decode_text_string(s).text,
            _ => String::new(),
        })
        .unwrap_or_default();
    if let Some(slot) = items.get_mut(at) {
        *slot = entry;
    }
    OutputIntentChange::Replaced { previous }
}

/// A profile's `desc` tag text: `textDescriptionType`'s ASCII part
/// (ICC.1:1998-09 §6.5.16) or `multiLocalizedUnicodeType`'s first record
/// (ICC.1:2022 §10.15, Table 54).
pub(crate) fn describe(profile: &[u8]) -> Option<String> {
    let u32_at = |at: usize| -> Option<usize> {
        let b: [u8; 4] = profile.get(at..at + 4)?.try_into().ok()?;
        usize::try_from(u32::from_be_bytes(b)).ok()
    };
    let count = u32_at(128)?.min(256);
    let (offset, size) = (0..count).find_map(|i| {
        let at = 132 + i * 12;
        (profile.get(at..at + 4)? == b"desc").then(|| Some((u32_at(at + 4)?, u32_at(at + 8)?)))?
    })?;
    let tag = profile.get(offset..offset.checked_add(size)?)?;
    let text = match tag.get(0..4)? {
        b"desc" => {
            let len = u32_at(offset + 8)?;
            let ascii = tag.get(12..12 + len)?;
            String::from_utf8_lossy(ascii)
                .trim_end_matches('\0')
                .to_owned()
        }
        b"mluc" => {
            let len = u32_at(offset + 20)?;
            let at = u32_at(offset + 24)?;
            let units: Vec<u16> = tag
                .get(at..at + len)?
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_be_bytes(*c))
                .collect();
            String::from_utf16_lossy(&units)
                .trim_end_matches('\0')
                .to_owned()
        }
        _ => return None,
    };
    let text = text.trim().to_owned();
    (!text.is_empty()).then_some(text)
}
