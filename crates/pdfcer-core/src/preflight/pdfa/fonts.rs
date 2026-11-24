//! PDF/A font rows (ISO 19005-1 §6.3, -2/-3 §6.2.11): the font dictionary's
//! keys, embedding, subset sets and TrueType encodings. Fonts are found as
//! the values of every `/Font` resource dictionary in a live object, and
//! the descendants of each Type 0 font among them.

use std::collections::BTreeSet;

use crate::fontinfo::is_standard_14;
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::preflight::FindingTarget;
use crate::preflight::checks::{self, Sink};
use crate::preflight::cos::{each_dict, name};
use crate::view::DocumentView;

use super::Ctx;

/// `/Subtype` values a font dictionary may have (ISO 32000-1 Table 110).
const SUBTYPES: [&[u8]; 7] = [
    b"Type1",
    b"MMType1",
    b"TrueType",
    b"Type3",
    b"Type0",
    b"CIDFontType0",
    b"CIDFontType2",
];

/// FontDescriptor `/Flags` bit 3, Symbolic (ISO 32000-1 Table 123).
const SYMBOLIC: i64 = 4;

/// One font met: its object id when indirect, and its dictionary.
struct Font {
    id: Option<ObjId>,
    dict: Dict,
}

/// Run the row if it is one of this module's; `false` when it is not.
pub(super) fn check(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) -> bool {
    if !ctx.row.id.starts_with("PAI-FT-") {
        return false;
    }
    if ctx.row.id == "PAI-FT-11" {
        ctx.relay(ctx.rule, sink, |r, s| checks::fonts(view, r, s));
        return true;
    }
    for font in collect(view) {
        if let Some(what) = test(view, ctx, &font) {
            let base = name(view, &font.dict, b"BaseFont").map_or("unnamed".to_owned(), |n| {
                String::from_utf8_lossy(n).into_owned()
            });
            let (target, label) = match font.id {
                Some(id) => (FindingTarget::Font(id), format!("Font {id} ({base})")),
                None => (FindingTarget::Document, format!("A direct font ({base})")),
            };
            sink.push(ctx.found(None, target, &format!("{label} {what}.")));
        }
    }
    true
}

/// Every font dictionary in a `/Font` resource, then their descendants,
/// each indirect one once.
fn collect(view: &DocumentView<'_>) -> Vec<Font> {
    let mut stack = Vec::new();
    each_dict(view, &mut |_, dict, _| {
        if let Some(Object::Dict(fonts)) = dict.get(b"Font").map(|o| view.resolve(o)) {
            stack.extend(fonts.iter().map(|(_, v)| v.clone()));
        }
    });
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    while let Some(obj) = stack.pop() {
        let id = obj.as_reference();
        if id.is_some_and(|id| !seen.insert(id)) {
            continue;
        }
        let Some(dict) = view.resolve(&obj).as_dict() else {
            continue;
        };
        if let Some(kids) = dict
            .get(b"DescendantFonts")
            .map(|o| view.resolve(o))
            .and_then(Object::as_array)
        {
            stack.extend(kids.iter().cloned());
        }
        out.push(Font {
            id,
            dict: dict.clone(),
        });
    }
    out
}

fn descriptor<'v>(view: &'v DocumentView<'_>, font: &'v Font) -> Option<&'v Dict> {
    font.dict
        .get(b"FontDescriptor")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)
}

/// A `ABCDEF+` subset tag (ISO 32000-1 §9.6.4).
fn is_subset(base: Option<&[u8]>) -> bool {
    base.is_some_and(|b| {
        b.len() > 7
            && b.get(6) == Some(&b'+')
            && b.get(..6)
                .is_some_and(|t| t.iter().all(u8::is_ascii_uppercase))
    })
}

/// What `font` breaks for the row, as a predicate phrase.
fn test(view: &DocumentView<'_>, ctx: &Ctx<'_>, font: &Font) -> Option<String> {
    let dict = &font.dict;
    let subtype = name(view, dict, b"Subtype");
    let base = name(view, dict, b"BaseFont");
    let desc = descriptor(view, font);
    let part1 = ctx.level.part() == 1;
    match ctx.row.id {
        "PAI-FT-1" => {
            (name(view, dict, b"Type") != Some(b"Font")).then(|| "has no /Type /Font".into())
        }
        "PAI-FT-2" => (!subtype.is_some_and(|s| SUBTYPES.contains(&s)))
            .then(|| "has no standard /Subtype".into()),
        "PAI-FT-3" => {
            (base.is_none() && subtype != Some(b"Type3")).then(|| "has no /BaseFont".into())
        }
        "PAI-FT-4" => {
            let simple = matches!(subtype, Some(b"Type1" | b"MMType1" | b"TrueType"));
            let standard = base.is_some_and(|b| is_standard_14(&String::from_utf8_lossy(b)));
            ((simple && !standard) || subtype == Some(b"Type3"))
                .then(|| widths(view, dict))
                .flatten()
        }
        "PAI-FT-5" => {
            let ff3 = desc
                .and_then(|d| d.get(b"FontFile3"))
                .map(|o| view.resolve(o));
            let Some(Object::Stream(s)) = ff3 else {
                return None;
            };
            let st = name(view, &s.dict, b"Subtype");
            let ok = matches!(st, Some(b"Type1C" | b"CIDFontType0C"))
                || (!part1 && st == Some(b"OpenType"));
            (!ok).then(|| "has a FontFile3 of a /Subtype this part does not allow".into())
        }
        "PAI-FT-7" => {
            let embedded =
                desc.is_some_and(|d| d.contains_key(b"FontFile2") || d.contains_key(b"FontFile3"));
            (subtype == Some(b"CIDFontType2")
                && (part1 || embedded)
                && !dict.contains_key(b"CIDToGIDMap"))
            .then(|| "is a CIDFontType2 with no /CIDToGIDMap".into())
        }
        "PAI-FT-13" | "PAI-FT-14" => subset_set(ctx, subtype, base, desc),
        _ => truetype(view, ctx, subtype, dict, desc),
    }
}

/// FT-4: `FirstChar`, `LastChar` and a `Widths` of matching length.
fn widths(view: &DocumentView<'_>, dict: &Dict) -> Option<String> {
    let int = |k: &[u8]| {
        dict.get(k)
            .map(|o| view.resolve(o))
            .and_then(Object::as_int)
    };
    let (Some(first), Some(last)) = (int(b"FirstChar"), int(b"LastChar")) else {
        return Some("has no /FirstChar or /LastChar".into());
    };
    let n = dict
        .get(b"Widths")
        .map(|o| view.resolve(o))
        .and_then(Object::as_array)
        .map(<[Object]>::len);
    let want = usize::try_from(last.saturating_sub(first).saturating_add(1)).unwrap_or(0);
    match n {
        None => Some("has no /Widths".into()),
        Some(n) if n != want => Some(format!(
            "has {n} /Widths for FirstChar {first} to LastChar {last}"
        )),
        _ => None,
    }
}

/// FT-13 (Type 1 subset, `/CharSet`) and FT-14 (CIDFont subset, `/CIDSet`):
/// required at 1b. At 2b/3b only a present set's completeness is
/// constrained, which the row's note discloses as not checked.
fn subset_set(
    ctx: &Ctx<'_>,
    subtype: Option<&[u8]>,
    base: Option<&[u8]>,
    desc: Option<&Dict>,
) -> Option<String> {
    let (types, key): (&[&[u8]], &[u8]) = if ctx.row.id == "PAI-FT-13" {
        (&[b"Type1", b"MMType1"], b"CharSet")
    } else {
        (&[b"CIDFontType0", b"CIDFontType2"], b"CIDSet")
    };
    let applies =
        ctx.level.part() == 1 && is_subset(base) && subtype.is_some_and(|s| types.contains(&s));
    (applies && desc.is_some_and(|d| !d.contains_key(key))).then(|| {
        format!(
            "is a subset with no /{} in its descriptor",
            String::from_utf8_lossy(key)
        )
    })
}

/// FT-16 (1b: a non-symbolic TrueType font's encoding) and FT-18 (a
/// symbolic TrueType font has no `/Encoding`).
fn truetype(
    view: &DocumentView<'_>,
    ctx: &Ctx<'_>,
    subtype: Option<&[u8]>,
    dict: &Dict,
    desc: Option<&Dict>,
) -> Option<String> {
    if subtype != Some(b"TrueType") {
        return None;
    }
    let flags = desc
        .and_then(|d| d.get(b"Flags"))
        .map(|o| view.resolve(o))
        .and_then(Object::as_int)
        .unwrap_or(0);
    let symbolic = flags & SYMBOLIC != 0;
    let encoding = dict.get(b"Encoding").map(|o| view.resolve(o));
    if ctx.row.id == "PAI-FT-18" {
        return (symbolic && encoding.is_some()).then(|| "is symbolic and has an /Encoding".into());
    }
    if ctx.row.id != "PAI-FT-16" || symbolic {
        return None;
    }
    let standard = |n: Option<&[u8]>| matches!(n, Some(b"MacRomanEncoding" | b"WinAnsiEncoding"));
    let ok = match encoding {
        Some(Object::Name(n)) => standard(Some(n.as_bytes())),
        Some(Object::Dict(d)) => {
            !d.contains_key(b"Differences") && standard(name(view, d, b"BaseEncoding"))
        }
        _ => false,
    };
    (!ok).then(|| {
        "is a non-symbolic TrueType font whose /Encoding is not MacRoman or WinAnsi without \
         /Differences"
            .into()
    })
}
