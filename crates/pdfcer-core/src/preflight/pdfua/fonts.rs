//! Font checks over the fonts the page content actually shows text with:
//! embedding, Unicode mapping, `CIDToGIDMap` and TrueType encodings
//! (ISO 14289-1 §7.21.3.2, §7.21.4.1, §7.21.6, §7.21.7; ISO 14289-2
//! §8.4.5.3.2, §8.4.5.5.1, §8.4.5.7, §8.4.5.8).

use std::collections::BTreeMap;

use super::{FindingTarget, Out, name_of};
use crate::fontinfo::{Encoding, FontRecord, FontSubtype, Program};
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::view::DocumentView;

/// Fonts that showed text, and whether any of that text was visible
/// (render mode other than 3, ISO 32000-1 §9.3.6).
#[derive(Debug, Default)]
pub(super) struct Used {
    fonts: BTreeMap<ObjId, bool>,
}

impl Used {
    /// Note a font as used, and whether any use of it paints visibly.
    pub(super) fn record(&mut self, font: ObjId, visible: bool) {
        *self.fonts.entry(font).or_default() |= visible;
    }
}

/// The encodings whose glyph names map to Unicode without a `/ToUnicode`.
const STANDARD_ENCODINGS: &[&str] = &["MacRomanEncoding", "MacExpertEncoding", "WinAnsiEncoding"];
/// TrueType's permitted non-symbolic encodings (ISO 14289-1 §7.21.6).
const TRUETYPE_ENCODINGS: &[&str] = &["MacRomanEncoding", "WinAnsiEncoding"];
/// The Adobe character collections that need no `/ToUnicode`.
const KNOWN_COLLECTIONS: &[&[u8]] = &[b"GB1", b"CNS1", b"Japan1", b"Korea1"];

/// Check every used font.
pub(super) fn check(view: &DocumentView<'_>, used: &Used, out: &mut Out) {
    if used.fonts.is_empty() {
        return;
    }
    for font in crate::fontinfo::inventory(view).fonts {
        let Some(id) = font.id else { continue };
        let Some(&visible) = used.fonts.get(&id) else {
            continue;
        };
        let page = font.pages.first().map(|&p| p as usize);
        let dict = view.graph().value(id).and_then(Object::as_dict);
        if visible {
            embedding(&font, id, page, out);
        }
        to_unicode(view, &font, dict, id, page, out);
        if let Some(dict) = dict {
            cid_to_gid(view, &font, dict, id, page, out);
        }
        truetype_encoding(&font, id, page, out);
    }
}

fn name(font: &FontRecord, id: ObjId) -> String {
    match &font.base_font {
        Some(b) => format!("The font {b} (object {id})"),
        None => format!("The font object {id}"),
    }
}

/// ISO 14289-1 §7.21.4.1 (Matterhorn 31-009) / ISO 14289-2 §8.4.5.5.1.
fn embedding(font: &FontRecord, id: ObjId, page: Option<usize>, out: &mut Out) {
    if font.subtype == FontSubtype::Type3 {
        return;
    }
    let t = FindingTarget::Font(id);
    match &font.program {
        Program::Embedded(_) => {}
        Program::NotEmbedded => {
            let message = format!("{} shows visible text but is not embedded.", name(font, id));
            out.violation("PUI-31-009", "PUI2-8.4.5.5.1-1", page, t, message);
        }
        Program::Unreadable { why, .. } => {
            let message = format!(
                "{}'s embedded program cannot be read: {why}.",
                name(font, id)
            );
            out.incomplete("PUI-31-009", "PUI2-8.4.5.5.1-1", page, t, message);
        }
        _ => {
            let message = format!("{}'s embedding could not be classified.", name(font, id));
            out.incomplete("PUI-31-009", "PUI2-8.4.5.5.1-1", page, t, message);
        }
    }
}

/// ISO 14289-1 §7.21.7 (Matterhorn 31-027) / ISO 14289-2 §8.4.5.8.
fn to_unicode(
    view: &DocumentView<'_>,
    font: &FontRecord,
    dict: Option<&Dict>,
    id: ObjId,
    page: Option<usize>,
    out: &mut Out,
) {
    if font.has_to_unicode {
        return;
    }
    let standard = |e: &Option<String>| {
        e.as_deref()
            .is_some_and(|e| STANDARD_ENCODINGS.contains(&e))
    };
    let verdict = match (&font.subtype, &font.encoding) {
        (_, Encoding::Predefined(e)) if STANDARD_ENCODINGS.contains(&e.as_str()) => return,
        (
            _,
            Encoding::Dictionary {
                base,
                has_differences: false,
            },
        ) if standard(base) => return,
        (FontSubtype::Type0, _) => {
            if dict.is_some_and(|d| adobe_collection(view, d)) {
                return;
            }
            Some("it has no /ToUnicode and its character collection is not a known Adobe one")
        }
        (FontSubtype::TrueType, _) if font.symbolic == Some(false) => return,
        (FontSubtype::Type1 | FontSubtype::MmType1, Encoding::Absent) if font.standard_14 => return,
        (FontSubtype::Type1 | FontSubtype::MmType1 | FontSubtype::Type3, _) => None,
        _ => Some("it has no /ToUnicode and no encoding that maps to Unicode"),
    };
    let t = FindingTarget::Font(id);
    match verdict {
        Some(why) => {
            let message = format!(
                "{}'s text cannot be mapped to Unicode: {why}.",
                name(font, id)
            );
            out.violation("PUI-31-027", "PUI2-8.4.5.8-1", page, t, message);
        }
        None => {
            let message = format!(
                "{} has no /ToUnicode; its glyph names were not checked against the Adobe \
                 Glyph List.",
                name(font, id)
            );
            out.incomplete("PUI-31-027", "PUI2-8.4.5.8-1", page, t, message);
        }
    }
}

/// The Type 0 font's descendant CIDFont dictionary.
fn cid_font<'a>(view: &'a DocumentView<'_>, type0: &'a Dict) -> Option<&'a Dict> {
    let kids = type0
        .get(b"DescendantFonts")
        .map(|o| view.resolve(o))
        .and_then(Object::as_array)?;
    kids.first()
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)
}

fn adobe_collection(view: &DocumentView<'_>, type0: &Dict) -> bool {
    let Some(info) = cid_font(view, type0)
        .and_then(|d| d.get(b"CIDSystemInfo"))
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)
    else {
        return false;
    };
    let field = |k: &[u8]| match info.get(k).map(|o| view.resolve(o)) {
        Some(Object::String(s)) => Some(s.clone()),
        _ => None,
    };
    field(b"Registry").as_deref() == Some(b"Adobe")
        && field(b"Ordering").is_some_and(|o| KNOWN_COLLECTIONS.contains(&o.as_slice()))
}

/// ISO 14289-1 §7.21.3.2 (Matterhorn 31-004, 31-005) / ISO 14289-2
/// §8.4.5.3.2: an embedded CIDFontType2 carries `/CIDToGIDMap`, a stream or
/// `/Identity`.
fn cid_to_gid(
    view: &DocumentView<'_>,
    font: &FontRecord,
    dict: &Dict,
    id: ObjId,
    page: Option<usize>,
    out: &mut Out,
) {
    if font.descendant_subtype != Some(FontSubtype::CidFontType2)
        || !matches!(font.program, Program::Embedded(_))
    {
        return;
    }
    let Some(cid) = cid_font(view, dict) else {
        return;
    };
    let t = FindingTarget::Font(id);
    match cid.get(b"CIDToGIDMap") {
        None => {
            let message = format!("{}'s CIDFontType2 has no /CIDToGIDMap.", name(font, id));
            out.violation("PUI-31-005", "PUI2-8.4.5.3.2-1", page, t, message);
        }
        Some(o) => {
            let ok = matches!(view.resolve(o), Object::Stream(_))
                || name_of(view, Some(o)) == Some(b"Identity");
            if !ok {
                let message = format!(
                    "{}'s /CIDToGIDMap is neither a stream nor /Identity.",
                    name(font, id)
                );
                out.violation("PUI-31-004", "", page, t, message);
            }
        }
    }
}

/// ISO 14289-1 §7.21.6 (Matterhorn 31-019…022, 31-024) / ISO 14289-2
/// §8.4.5.7.
fn truetype_encoding(font: &FontRecord, id: ObjId, page: Option<usize>, out: &mut Out) {
    if font.subtype != FontSubtype::TrueType {
        return;
    }
    let t = FindingTarget::Font(id);
    let who = name(font, id);
    if font.symbolic == Some(true) {
        if font.encoding != Encoding::Absent {
            let message = format!("{who} is a symbolic TrueType font with an /Encoding.");
            out.violation("PUI-31-024", "PUI2-8.4.5.7-3", page, t, message);
        }
        return;
    }
    if font.symbolic.is_none() {
        return;
    }
    let ok_name = |e: &str| TRUETYPE_ENCODINGS.contains(&e);
    let (row, message) = match &font.encoding {
        Encoding::Predefined(e) if ok_name(e) => return,
        Encoding::Absent => (
            "PUI-31-019",
            format!("{who} is non-symbolic TrueType with no /Encoding."),
        ),
        Encoding::Dictionary { base: None, .. } => (
            "PUI-31-020",
            format!("{who}'s /Encoding dictionary has no /BaseEncoding."),
        ),
        Encoding::Dictionary {
            base: Some(b),
            has_differences,
        } if ok_name(b) => {
            if *has_differences {
                let message = format!(
                    "{who}'s /Differences names were not checked against the Adobe Glyph List."
                );
                out.incomplete("PUI-31-022", "PUI2-8.4.5.7-2", page, t, message);
            }
            return;
        }
        _ => (
            "PUI-31-021",
            format!("{who}'s encoding is neither MacRomanEncoding nor WinAnsiEncoding."),
        ),
    };
    out.violation(row, "PUI2-8.4.5.7-2", page, t, message);
}
