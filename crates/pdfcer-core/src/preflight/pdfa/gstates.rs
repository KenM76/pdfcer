//! PDF/A rows on graphics state parameter dictionaries: transparency at 1b
//! (ISO 19005-1 §6.4), blend modes (-2 §6.2.10), transfer functions and
//! halftones (-1 §6.2.8, -2 §6.2.5). The dictionaries are the values of
//! every `/ExtGState` resource dictionary in a live object.

use std::collections::BTreeSet;

use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::preflight::checks::Sink;
use crate::preflight::cos::{each_dict, name};
use crate::view::DocumentView;

use super::Ctx;

/// The blend modes of ISO 32000-1 §11.3.5, plus the deprecated
/// `Compatible`.
const BLEND_MODES: [&[u8]; 17] = [
    b"Normal",
    b"Compatible",
    b"Multiply",
    b"Screen",
    b"Overlay",
    b"Darken",
    b"Lighten",
    b"ColorDodge",
    b"ColorBurn",
    b"HardLight",
    b"SoftLight",
    b"Difference",
    b"Exclusion",
    b"Hue",
    b"Saturation",
    b"Color",
    b"Luminosity",
];

/// Run the row if it is one of this module's; `false` when it is not.
pub(super) fn check(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) -> bool {
    let rows = [
        "PAI-TG-1",
        "PAI-TG-4",
        "PAI-TG-5",
        "PAI-TG-6",
        "PAI-TG-7",
        "PAI-TG-10",
        "PAI-TG-11",
        "PAI-TG-12",
        "PAI-TG-13",
    ];
    if !rows.contains(&ctx.row.id) {
        return false;
    }
    for (id, gs) in collect(view) {
        if let Some(what) = test(view, ctx.row.id, &gs) {
            sink.push(ctx.doc(&format!("Graphics state in object {id} {what}.")));
        }
    }
    true
}

/// Each graphics state dictionary with the object it is (when indirect)
/// or sits in.
fn collect(view: &DocumentView<'_>) -> Vec<(ObjId, Dict)> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    each_dict(view, &mut |id, dict, _| {
        let Some(Object::Dict(table)) = dict.get(b"ExtGState").map(|o| view.resolve(o)) else {
            return;
        };
        for (_, value) in table.iter() {
            let own = value.as_reference();
            if own.is_some_and(|o| !seen.insert(o)) {
                continue;
            }
            if let Some(gs) = view.resolve(value).as_dict() {
                out.push((own.unwrap_or(id), gs.clone()));
            }
        }
    });
    out
}

fn test(view: &DocumentView<'_>, row: &str, gs: &Dict) -> Option<String> {
    let get = |k: &[u8]| gs.get(k).map(|o| view.resolve(o));
    let key = |k: &[u8]| gs.contains_key(k);
    match row {
        "PAI-TG-1" => {
            let none = match get(b"SMask") {
                None => true,
                Some(Object::Name(n)) => n.as_bytes() == b"None",
                Some(_) => false,
            };
            (!none).then(|| "has a soft mask (/SMask)".into())
        }
        "PAI-TG-4" | "PAI-TG-7" => blend(view, row, get(b"BM")?),
        "PAI-TG-5" | "PAI-TG-6" => {
            let k: &[u8] = if row == "PAI-TG-5" { b"CA" } else { b"ca" };
            let v = get(k).and_then(Object::as_number)?;
            #[allow(clippy::float_cmp)] // the requirement is exactly 1.0
            let opaque = v == 1.0;
            (!opaque).then(|| format!("has /{} {v}", String::from_utf8_lossy(k)))
        }
        "PAI-TG-10" => key(b"TR").then(|| "has a transfer function (/TR)".into()),
        "PAI-TG-11" => (key(b"TR2") && name(view, gs, b"TR2") != Some(b"Default"))
            .then(|| "has a /TR2 other than /Default".into()),
        "PAI-TG-12" => key(b"HTP").then(|| "has /HTP".into()),
        _ => halftone(view, get(b"HT")?),
    }
}

/// TG-4 (1b: Normal or Compatible) and TG-7 (2b/3b: a standard mode). An
/// array is judged by every element.
fn blend(view: &DocumentView<'_>, row: &str, bm: &Object) -> Option<String> {
    let names: Vec<&[u8]> = match bm {
        Object::Name(n) => vec![n.as_bytes()],
        Object::Array(a) => a
            .iter()
            .filter_map(|o| view.resolve(o).as_name())
            .map(|n| n.as_bytes())
            .collect(),
        _ => return Some("has a /BM that is not a name".into()),
    };
    let allowed: &[&[u8]] = if row == "PAI-TG-4" {
        &BLEND_MODES[..2]
    } else {
        &BLEND_MODES
    };
    names
        .into_iter()
        .find(|n| !allowed.contains(n))
        .map(|n| format!("uses the blend mode /{}", String::from_utf8_lossy(n)))
}

/// TG-13: a halftone dictionary or stream is type 1 or 5 with no
/// `/HalftoneName`. The name `/Default` is not a halftone dictionary.
fn halftone(view: &DocumentView<'_>, ht: &Object) -> Option<String> {
    let d = ht.as_dict()?;
    let kind = d
        .get(b"HalftoneType")
        .map(|o| view.resolve(o))
        .and_then(Object::as_int);
    if !matches!(kind, Some(1 | 5)) {
        let shown = kind.map_or("none".into(), |k| k.to_string());
        return Some(format!("has a halftone of type {shown}"));
    }
    d.contains_key(b"HalftoneName")
        .then(|| "has a halftone with /HalftoneName".into())
}
