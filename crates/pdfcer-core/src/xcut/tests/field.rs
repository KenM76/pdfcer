//! The form-field drivers: `copy_field` / `cut_field`, the clip's byte
//! format, and a `NewField` paste, for the merged widget and the field with
//! `/Kids`.

use std::collections::BTreeSet;

use super::fixture::{self, SENTINEL, SENTINEL_KEY};
use super::runner::{annots, disclosed, dump, offset};
use crate::document::Document;
use crate::edit::EditSession;
use crate::formclip::{FieldClip, FieldPasteOutcome, FieldPastePolicy, PasteTooltip};
use crate::object::{ObjId, Object};
use crate::page_tree::Rect;
use crate::writer::SaveOptions;
use crate::xcut::dump::{Canon, PropertyDump, compare};
use crate::xcut::{Change, ObjectKind, Op};

/// The name the paste plants the copy under (free in every fixture).
const PASTED: &str = "XcutPasted";

fn source_name(kind: ObjectKind) -> &'static str {
    if kind == ObjectKind::Field {
        "fld"
    } else {
        "Ken"
    }
}

/// The fixture's first widget `/Rect` (`[100 100 200 150]` for both
/// kinds), moved by `(dx, dy)`.
fn moved_rect((dx, dy): (f64, f64)) -> Rect {
    Rect {
        llx: 100.0 + dx,
        lly: 100.0 + dy,
        urx: 200.0 + dx,
        ury: 150.0 + dy,
    }
}

fn paste(
    s: &mut EditSession,
    page: usize,
    clip: &FieldClip,
    rect: Rect,
) -> Result<FieldPasteOutcome, String> {
    let policy = FieldPastePolicy::NewField {
        name: PASTED.to_owned(),
        tooltip: PasteTooltip::Carry,
        copy_value: true,
        copy_actions: true,
    };
    s.paste_field(clip, page, rect, &policy)
        .map_err(|e| format!("paste: {e}"))
}

fn through_bytes(clip: &FieldClip) -> Result<FieldClip, String> {
    FieldClip::from_bytes(&clip.to_bytes()).map_err(|e| format!("clip bytes: {e}"))
}

/// Run a carry op; the session the paste landed in and its outcome.
fn carry(
    op: Op,
    kind: ObjectKind,
) -> Result<(PropertyDump, EditSession, FieldPasteOutcome), String> {
    let (mut s, id) = fixture::build(kind);
    let before = dump(&s, id);
    let name = source_name(kind);
    let rect = moved_rect(offset(op));
    let copied = || s.copy_field(name).map_err(|e| format!("copy: {e}"));
    let (session, outcome) = match op {
        Op::Copy => {
            let clip = through_bytes(&copied()?)?;
            let mut dst = fixture::destination();
            let outcome = paste(&mut dst, 0, &clip, rect)?;
            (dst, outcome)
        }
        Op::Paste | Op::Duplicate => {
            let clip = through_bytes(&copied()?)?;
            let page = usize::from(op == Op::Paste);
            let outcome = paste(&mut s, page, &clip, rect)?;
            (s, outcome)
        }
        Op::Cut => {
            let cut = s.cut_field(name).map_err(|e| format!("cut: {e}"))?;
            if s.copy_field(name).is_ok() {
                return Err("cut left the source field in place".to_owned());
            }
            let clip = through_bytes(&cut.clip)?;
            let outcome = paste(&mut s, 0, &clip, rect)?;
            (s, outcome)
        }
        _ => return Err(format!("{op} is not a carry")),
    };
    Ok((before, session, outcome))
}

/// `dump` without the paths under `key` (compared on their own).
fn without(dump: &PropertyDump, key: &str) -> PropertyDump {
    let prefix = format!("/{key}");
    PropertyDump(
        dump.0
            .iter()
            .filter(|(p, _)| {
                !(p.starts_with(&prefix)
                    && p[prefix.len()..]
                        .chars()
                        .next()
                        .is_none_or(|c| "/[#".contains(c)))
            })
            .map(|(p, c)| (p.clone(), c.clone()))
            .collect(),
    )
}

fn sentinel_survived(after: &PropertyDump, what: &str) -> Option<String> {
    let key = format!("/{}", String::from_utf8_lossy(SENTINEL_KEY));
    (after.0.get(&key) != Some(&Canon::Str(SENTINEL.as_bytes().to_vec())))
        .then(|| format!("the sentinel did not survive on the {what}"))
}

/// A kid widget's dump without walking its `/Parent`: the field is compared
/// on its own, and walking it first would turn the kid's own entries into
/// back-references to its sibling's.
fn widget_dump(s: &EditSession, id: ObjId) -> PropertyDump {
    let mut dict = match s.value(id) {
        Some(Object::Dict(d)) => d.clone(),
        _ => return PropertyDump::default(),
    };
    dict.remove(b"Parent");
    PropertyDump::of(s, &Object::Dict(dict))
}

/// The widgets of field `id`, by `/Kids` order.
fn kids(s: &EditSession, id: ObjId) -> Vec<ObjId> {
    match s.value(id) {
        Some(Object::Dict(d)) => match d.get(b"Kids") {
            Some(Object::Array(a)) => a.iter().filter_map(Object::as_reference).collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// Faults for a `Preserve` carry cell of [`ObjectKind::Widget`] or
/// [`ObjectKind::Field`].
pub(super) fn preserve_carry(op: Op, kind: ObjectKind, changes: &[Change]) -> Vec<String> {
    let (before, s, outcome) = match carry(op, kind) {
        Ok(t) => t,
        Err(e) => return vec![e],
    };
    let at = offset(op);
    let after = dump(&s, outcome.field_id);
    let mut faults = Vec::new();
    if kind == ObjectKind::Widget {
        faults.extend(compare(&before, &after, changes, at));
        faults.extend(sentinel_survived(&after, "widget"));
    } else {
        faults.extend(compare(
            &without(&before, "Kids"),
            &without(&after, "Kids"),
            changes,
            at,
        ));
        faults.extend(sentinel_survived(&after, "field"));
        let (src, _) = fixture::build(kind);
        let (old, new) = (
            kids(&src, ObjId::new(fixture::TARGET, 0)),
            kids(&s, outcome.field_id),
        );
        if old.len() != new.len() {
            faults.push(format!("{} widgets became {}", old.len(), new.len()));
        }
        for (i, (o, n)) in old.iter().zip(&new).enumerate() {
            let (b, a) = (widget_dump(&src, *o), widget_dump(&s, *n));
            faults.extend(
                compare(&b, &a, changes, at)
                    .into_iter()
                    .map(|f| format!("kid {i}: {f}")),
            );
            faults.extend(sentinel_survived(&a, "widget"));
            if !matches!(s.value(*n), Some(Object::Dict(d)) if d.get(b"Parent") == Some(&Object::Reference(outcome.field_id)))
            {
                faults.push(format!("kid {i}: /Parent does not name the pasted field"));
            }
        }
    }
    faults.extend(disclosed(&before, &outcome.dropped_keys, 0, changes));
    let page = usize::from(op == Op::Paste);
    let names: Vec<Vec<u8>> = annots(&s, page)
        .iter()
        .filter_map(|id| match s.value(*id) {
            Some(Object::Dict(d)) => match d.get(b"NM") {
                Some(Object::String(nm)) => Some(nm.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    if names.iter().collect::<BTreeSet<_>>().len() != names.len() {
        faults.push(format!("/NM not unique on the page: {names:?}"));
    }
    faults
}

/// Faults for the field's reload cell: full rewrite, reopen, find it by name.
pub(super) fn preserve_reload(changes: &[Change]) -> Vec<String> {
    let (s, id) = fixture::build(ObjectKind::Field);
    let before = dump(&s, id);
    let doc = match s.to_full_bytes(&SaveOptions::default()) {
        Ok((bytes, _)) => Document::from_bytes(bytes),
        Err(e) => return vec![format!("save: {e}")],
    };
    let doc = match doc {
        Ok(d) => d,
        Err(e) => return vec![format!("reopen: {e}")],
    };
    let view = doc.view();
    let Some(after_id) =
        crate::forms::parse_acroform(&view).and_then(|f| f.field_by_name("fld").map(|f| f.id))
    else {
        return vec!["the field did not survive the reload".to_owned()];
    };
    compare(&before, &dump(&view, after_id), changes, (0.0, 0.0))
}
