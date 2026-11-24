//! PDF/A rows answered from colour-space arrays and from every value of
//! every live object: ICC profiles (ISO 19005-1 §6.2.3.2, -2 §6.2.4.2),
//! DeviceN colourants and Separation consistency (-2 §6.2.4.4), and the
//! implementation limits (-1 §6.1.12, -2 §6.1.13).

use std::collections::BTreeMap;

use crate::graph::ObjectGraph;
use crate::object::{ObjId, Object};
use crate::preflight::checks::Sink;
use crate::preflight::cos::decoded;
use crate::prepress::{each_array, live_ids};
use crate::view::DocumentView;

use super::Ctx;
use super::icc::Header;

/// Nesting followed inside one object for the value limits.
const MAX_DEPTH: usize = 24;

/// Process colourants of a DeviceN space, which need no `/Colorants` entry.
const PROCESS: [&[u8]; 5] = [b"Cyan", b"Magenta", b"Yellow", b"Black", b"None"];

/// What one array or value contributes to a row.
enum Hit {
    Found(String),
    Unread(String),
}

/// Run the row if it is one of this module's; `false` when it is not.
pub(super) fn check(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) -> bool {
    match ctx.row.id {
        "PAI-FS-30" | "PAI-FS-31" | "PAI-FS-32" | "PAI-FS-33" | "PAI-FS-34" | "PAI-FS-35"
        | "PAI-FS-36" | "PAI-FS-37" | "PAI-FS-38" => limits(view, ctx, sink),
        "PAI-FS-41" | "PAI-FS-42" => {
            let max = if ctx.row.id == "PAI-FS-41" { 8 } else { 32 };
            arrays(view, ctx, sink, &mut |items| device_n_count(items, max));
        }
        "PAI-CS-1" | "PAI-CS-2" | "PAI-CS-3" => {
            arrays(view, ctx, sink, &mut |items| icc(view, ctx, items));
        }
        "PAI-CS-8" => arrays(view, ctx, sink, &mut |items| colorants(view, items)),
        "PAI-CS-9" => separations(view, ctx, sink),
        _ => return false,
    }
    true
}

/// Each live object's first hit from `test` over its arrays.
fn arrays(
    view: &DocumentView<'_>,
    ctx: &Ctx<'_>,
    sink: &mut Sink,
    test: &mut dyn FnMut(&[Object]) -> Option<Hit>,
) {
    for id in live_ids(view) {
        let Some(value) = view.graph().value(id) else {
            continue;
        };
        let mut hit = None;
        each_array(value, 0, &mut |items| {
            if hit.is_none() {
                hit = test(items);
            }
        });
        push(ctx, sink, id, hit);
    }
}

fn push(ctx: &Ctx<'_>, sink: &mut Sink, id: ObjId, hit: Option<Hit>) {
    match hit {
        Some(Hit::Found(what)) => sink.push(ctx.doc(&format!("Object {id} {what}."))),
        Some(Hit::Unread(what)) => sink.push(ctx.unread(None, &format!("object {id} {what}"))),
        None => {}
    }
}

/// The space family an array names (`ICCBased`, `DeviceN`, ...).
fn family(items: &[Object]) -> Option<&[u8]> {
    items
        .first()
        .and_then(Object::as_name)
        .map(|n| n.as_bytes())
}

fn device_n_count(items: &[Object], max: usize) -> Option<Hit> {
    let n = items
        .get(1)
        .and_then(Object::as_array)
        .map(<[Object]>::len)?;
    (family(items) == Some(b"DeviceN") && n > max).then(|| {
        Hit::Found(format!(
            "has a DeviceN space with {n} colourants; the limit is {max}"
        ))
    })
}

/// CS-1/CS-2 (profile version, class and space) and CS-3 (`/N`).
fn icc(view: &DocumentView<'_>, ctx: &Ctx<'_>, items: &[Object]) -> Option<Hit> {
    if family(items) != Some(b"ICCBased") {
        return None;
    }
    let Some(Object::Stream(stream)) = items.get(1).map(|o| view.resolve(o)) else {
        return Some(Hit::Found(
            "has an ICCBased space with no profile stream".into(),
        ));
    };
    let Some(header) = Header::of(view, stream) else {
        return Some(Hit::Unread(
            "has an ICC profile that could not be decoded".into(),
        ));
    };
    if ctx.row.id == "PAI-CS-3" {
        let n = stream
            .dict
            .get(b"N")
            .map(|o| view.resolve(o))
            .and_then(Object::as_int);
        let want = header.channels();
        return (want.is_some() && n != want).then(|| {
            Hit::Found(format!(
                "has an ICCBased /N of {} for a {} profile",
                n.map_or("none".into(), |n| n.to_string()),
                header.describe()
            ))
        });
    }
    let max = if ctx.row.id == "PAI-CS-1" { 2 } else { 4 };
    let class = matches!(&header.class, b"prtr" | b"mntr" | b"scnr" | b"spac");
    let ok = class && header.channels().is_some() && header.major <= max;
    (!ok).then(|| {
        Hit::Found(format!(
            "has an ICC profile ({}) outside v{max} prtr, mntr, scnr or spac Gray, RGB, CMYK or Lab",
            header.describe()
        ))
    })
}

/// CS-8: a DeviceN space naming a spot colourant has an attributes
/// dictionary whose `/Colorants` names it.
fn colorants(view: &DocumentView<'_>, items: &[Object]) -> Option<Hit> {
    if family(items) != Some(b"DeviceN") {
        return None;
    }
    let names = items
        .get(1)
        .map(|o| view.resolve(o))
        .and_then(Object::as_array)?;
    let table = items
        .get(4)
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)
        .and_then(|a| a.get(b"Colorants"))
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict);
    names
        .iter()
        .filter_map(|o| view.resolve(o).as_name())
        .map(|n| n.as_bytes())
        .find(|n| !PROCESS.contains(n) && !table.is_some_and(|t| t.contains_key(n)))
        .map(|spot| {
            Hit::Found(format!(
                "has a DeviceN space whose /Colorants does not name its spot colourant {}",
                String::from_utf8_lossy(spot)
            ))
        })
}

/// CS-9: every Separation space of one colourant name has the same
/// alternate space and tint transform.
fn separations(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) {
    let mut first: BTreeMap<Vec<u8>, (ObjId, Object, Object)> = BTreeMap::new();
    for id in live_ids(view) {
        let Some(value) = view.graph().value(id) else {
            continue;
        };
        let mut hit = None;
        each_array(value, 0, &mut |items| {
            let (Some(b"Separation"), Some(Object::Name(n)), Some(alt), Some(tint)) =
                (family(items), items.get(1), items.get(2), items.get(3))
            else {
                return;
            };
            let entry = first
                .entry(n.as_bytes().to_vec())
                .or_insert_with(|| (id, alt.clone(), tint.clone()));
            let agree = same(view, &entry.1, alt) && same(view, &entry.2, tint);
            if !agree && hit.is_none() {
                hit = Some(Hit::Found(format!(
                    "has a Separation space for {} whose alternate or tint transform differs \
                     from the one in object {}",
                    String::from_utf8_lossy(n.as_bytes()),
                    entry.0
                )));
            }
        });
        push(ctx, sink, id, hit);
    }
}

/// Whether two values are the same object, or equal once resolved (a
/// stream by its dictionary and decoded data).
fn same(view: &DocumentView<'_>, a: &Object, b: &Object) -> bool {
    if a == b {
        return true;
    }
    match (view.resolve(a), view.resolve(b)) {
        (Object::Stream(x), Object::Stream(y)) => {
            x.dict == y.dict && decoded(view, x).is_some_and(|d| Some(d) == decoded(view, y))
        }
        (x, y) => x == y,
    }
}

/// FS-30..FS-38: the first value in each live object beyond the row's limit.
fn limits(view: &DocumentView<'_>, ctx: &Ctx<'_>, sink: &mut Sink) {
    let row = ctx.row.id;
    for id in live_ids(view) {
        if let Some(value) = view.graph().value(id) {
            let hit = beyond(row, value, 0).map(Hit::Found);
            push(ctx, sink, id, hit);
        }
    }
}

fn beyond(row: &str, obj: &Object, depth: usize) -> Option<String> {
    if depth > MAX_DEPTH {
        return None;
    }
    if let Some(what) = over(row, obj) {
        return Some(what);
    }
    match obj {
        Object::Array(a) => a.iter().find_map(|o| beyond(row, o, depth + 1)),
        Object::Dict(d) => entries(row, d, depth),
        Object::Stream(s) => entries(row, &s.dict, depth),
        _ => None,
    }
}

fn entries(row: &str, d: &crate::object::Dict, depth: usize) -> Option<String> {
    d.iter().find_map(|(k, v)| {
        over(row, &Object::Name(k.clone())).or_else(|| beyond(row, v, depth + 1))
    })
}

/// The limit `obj` itself breaks for `row`.
fn over(row: &str, obj: &Object) -> Option<String> {
    let bad = match (row, obj) {
        ("PAI-FS-30", Object::Integer(i)) => i32::try_from(*i).is_err(),
        ("PAI-FS-31", Object::Real(r)) => r.abs() > 32767.0,
        ("PAI-FS-32", Object::Real(r)) => r.abs() > 3.403e38,
        ("PAI-FS-33", Object::Real(r)) => *r != 0.0 && r.abs() < 1.175e-38,
        ("PAI-FS-34", Object::String(s)) => s.len() > 65535,
        ("PAI-FS-35", Object::String(s)) => s.len() > 32767,
        ("PAI-FS-36", Object::Name(n)) => n.as_bytes().len() > 127,
        ("PAI-FS-37", Object::Array(a)) => a.len() > 8191,
        ("PAI-FS-38", Object::Dict(d)) => d.len() > 4095,
        ("PAI-FS-38", Object::Stream(s)) => s.dict.len() > 4095,
        _ => false,
    };
    bad.then(|| match obj {
        Object::Integer(i) => format!("holds the integer {i}"),
        Object::Real(r) => format!("holds the real {r:e}"),
        Object::String(s) => format!("holds a string of {} bytes", s.len()),
        Object::Name(n) => format!("holds a name of {} bytes", n.as_bytes().len()),
        Object::Array(a) => format!("holds an array of {} elements", a.len()),
        Object::Dict(d) => format!("holds a dictionary of {} entries", d.len()),
        Object::Stream(s) => format!("holds a stream dictionary of {} entries", s.dict.len()),
        _ => "breaks a limit".into(),
    })
}
