//! The gate: drive every driven cell of the matrix and print the metrics.

use std::collections::BTreeSet;

use super::fixture::{self, SENTINEL, SENTINEL_KEY};
use crate::document::Document;
use crate::edit::{EditError, EditSession, PasteOutcome};
use crate::graph::ObjectGraph;
use crate::object::{ObjId, Object};
use crate::vector::{ClipAnnotation, Matrix, ObjectClip};
use crate::writer::SaveOptions;
use crate::xcut::dump::{Canon, PropertyDump, compare};
use crate::xcut::{Change, Disclosure, Expect, ObjectKind, Op, classify_annot, expect};

/// Each page's object id, in page order.
pub(super) fn pages<G: ObjectGraph + ?Sized>(graph: &G) -> Vec<ObjId> {
    crate::page_tree::page_slots(graph)
        .map(|slots| slots.iter().map(|s| s.id).collect())
        .unwrap_or_default()
}

/// The annotation ids on page `page`, in `/Annots` order.
pub(super) fn annots<G: ObjectGraph + ?Sized>(graph: &G, page: usize) -> Vec<ObjId> {
    pages(graph)
        .get(page)
        .map(|&p| {
            crate::annot::page_annotations(graph, p)
                .iter()
                .filter_map(|a| a.id)
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn dump<G: ObjectGraph + ?Sized>(graph: &G, id: ObjId) -> PropertyDump {
    PropertyDump::of(graph, &Object::Reference(id))
}

fn index_of(s: &EditSession, id: ObjId) -> Result<usize, String> {
    annots(s, 0)
        .iter()
        .position(|a| *a == id)
        .ok_or_else(|| "target is not on page 1".to_owned())
}

/// The fixture's target, copied off page 1 through the clip's byte format.
pub(super) fn copy(s: &EditSession, id: ObjId) -> Result<ObjectClip, String> {
    let clip = s
        .copy_annotations(0, &[index_of(s, id)?])
        .map_err(|e| format!("copy: {e}"))?;
    ObjectClip::from_bytes(&clip.to_bytes()).map_err(|e| format!("clip bytes: {e}"))
}

/// A paste and where its annotation landed.
pub(super) struct Landed {
    pub(super) outcome: PasteOutcome,
    pub(super) after: PropertyDump,
    pub(super) page_names: Vec<Vec<u8>>,
}

/// Paste `clip` onto `page` of `s` at `(dx, dy)`; the new annotation is the
/// last on the page.
pub(super) fn paste(
    s: &mut EditSession,
    page: usize,
    clip: &ObjectClip,
    (dx, dy): (f64, f64),
) -> Result<Landed, String> {
    let outcome = s
        .paste_objects(page, clip, Matrix::translate(dx, dy))
        .map_err(|e| format!("paste: {e}"))?;
    let on_page = annots(s, page);
    let last = on_page.last().copied().ok_or("nothing on the page")?;
    let page_names = on_page
        .iter()
        .filter_map(|id| match s.value(*id) {
            Some(Object::Dict(d)) => match d.get(b"NM") {
                Some(Object::String(nm)) => Some(nm.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    Ok(Landed {
        outcome,
        after: dump(s, last),
        page_names,
    })
}

/// The `(dx, dy)` each carry op pastes at.
pub(super) fn offset(op: Op) -> (f64, f64) {
    match op {
        Op::Copy => (25.0, -10.0),
        Op::Paste => (30.0, 40.0),
        Op::Duplicate => (10.0, -10.0),
        Op::Cut => (5.0, 5.0),
        _ => (0.0, 0.0),
    }
}

/// Run a carry op; `Err` is a fault before any comparison could be made.
fn carry(op: Op, kind: ObjectKind) -> Result<(PropertyDump, Landed), String> {
    let (mut s, id) = fixture::build(kind);
    let before = dump(&s, id);
    let at = offset(op);
    let landed = match op {
        Op::Copy => paste(&mut fixture::destination(), 0, &copy(&s, id)?, at)?,
        Op::Paste => {
            let clip = copy(&s, id)?;
            paste(&mut s, 1, &clip, at)?
        }
        Op::Duplicate => {
            let clip = copy(&s, id)?;
            paste(&mut s, 0, &clip, at)?
        }
        Op::Cut => {
            let index = index_of(&s, id)?;
            let clip = s
                .cut_annotations(0, &[index])
                .map_err(|e| format!("cut: {e}"))?;
            if annots(&s, 0).contains(&id) {
                return Err("cut left the source on the page".to_owned());
            }
            let clip = ObjectClip::from_bytes(&clip.to_bytes()).map_err(|e| e.to_string())?;
            paste(&mut s, 0, &clip, at)?
        }
        _ => return Err(format!("{op} is not a carry")),
    };
    Ok((before, landed))
}

/// Faults for a `Preserve` carry cell.
pub(super) fn preserve_carry(op: Op, kind: ObjectKind, changes: &[Change]) -> Vec<String> {
    let (before, landed) = match carry(op, kind) {
        Ok(pair) => pair,
        Err(e) => return vec![e],
    };
    let mut faults = compare(&before, &landed.after, changes, offset(op));
    faults.extend(disclosed(
        &before,
        &landed.outcome.dropped_keys,
        landed.outcome.replies_unthreaded,
        changes,
    ));
    let unique: BTreeSet<&Vec<u8>> = landed.page_names.iter().collect();
    if unique.len() != landed.page_names.len() {
        faults.push(format!(
            "/NM not unique on the page: {:?}",
            landed.page_names
        ));
    }
    let sentinel = format!("/{}", String::from_utf8_lossy(SENTINEL_KEY));
    if landed.after.0.get(&sentinel) != Some(&Canon::Str(SENTINEL.as_bytes().to_vec())) {
        faults.push("the sentinel did not survive".to_owned());
    }
    faults
}

/// Every declared drop of a key the source held must be counted, in the
/// dropped-key map of whichever paste ran (`replies_unthreaded` is the
/// annotation paste's).
pub(super) fn disclosed(
    before: &PropertyDump,
    dropped_keys: &std::collections::BTreeMap<String, u64>,
    replies_unthreaded: u64,
    changes: &[Change],
) -> Vec<String> {
    let mut faults = Vec::new();
    for change in changes {
        let Change::Dropped {
            key, disclosed_by, ..
        } = change
        else {
            continue;
        };
        if !before.0.contains_key(&format!("/{key}")) {
            continue;
        }
        let counted = match disclosed_by {
            Disclosure::PasteDroppedKeys | Disclosure::FieldPasteDroppedKeys => {
                dropped_keys.get(*key).copied().unwrap_or(0)
            }
            Disclosure::PasteRepliesUnthreaded => replies_unthreaded,
        };
        if counted == 0 {
            faults.push(format!("/{key} dropped without being counted"));
        }
    }
    faults
}

/// Faults for a `Preserve` reload cell: full rewrite, reopen, dump again.
fn preserve_reload(kind: ObjectKind, changes: &[Change]) -> Vec<String> {
    let (s, id) = fixture::build(kind);
    let before = dump(&s, id);
    let Ok(index) = index_of(&s, id) else {
        return vec!["target is not on page 1".to_owned()];
    };
    let doc = match s.to_full_bytes(&SaveOptions::default()) {
        Ok((bytes, _)) => Document::from_bytes(bytes),
        Err(e) => return vec![format!("save: {e}")],
    };
    let doc = match doc {
        Ok(d) => d,
        Err(e) => return vec![format!("reopen: {e}")],
    };
    let view = doc.view();
    let Some(&after_id) = annots(&view, 0).get(index) else {
        return vec!["the annotation did not survive the reload".to_owned()];
    };
    compare(&before, &dump(&view, after_id), changes, (0.0, 0.0))
}

/// Faults for a `Refused` carry cell: a named error or a disclosure, and
/// nothing pasted.
fn refused(op: Op, kind: ObjectKind) -> Vec<String> {
    let (mut s, id) = fixture::build(kind);
    let Ok(index) = index_of(&s, id) else {
        return vec!["target is not on page 1".to_owned()];
    };
    if op == Op::Cut {
        return match s.cut_annotations(0, &[index]) {
            Err(EditError::CutWouldNotSurvive { .. }) if annots(&s, 0).contains(&id) => Vec::new(),
            other => vec![format!("cut was not refused: {:?}", other.map(|_| ()))],
        };
    }
    let clip = match copy(&s, id) {
        Ok(c) => c,
        Err(e) => return vec![e],
    };
    if !matches!(
        clip.annotations.as_slice(),
        [ClipAnnotation::Unsupported { .. }]
    ) {
        return vec!["copy carried a refused kind".to_owned()];
    }
    let mut dst = fixture::destination();
    let target = if op == Op::Copy { &mut dst } else { &mut s };
    match target.paste_objects(0, &clip, Matrix::translate(1.0, 1.0)) {
        Ok(o) if o.annotations_pasted == 0 && !o.disclosures.is_empty() => Vec::new(),
        Ok(o) => vec![format!("paste was silent or pasted: {o:?}")],
        Err(e) => vec![format!("paste errored instead of disclosing: {e}")],
    }
}

/// Faults for one driven cell.
fn drive(op: Op, kind: ObjectKind, cell: Expect) -> Vec<String> {
    let has_fixture = fixture::kinds().any(|k| k == kind);
    match cell {
        Expect::Preserve(_) | Expect::Refused(_) if !has_fixture => {
            vec!["no fixture builds this kind".to_owned()]
        }
        Expect::Preserve(changes) if op == Op::ReloadRoundTrip && kind == ObjectKind::Field => {
            super::field::preserve_reload(changes)
        }
        Expect::Preserve(changes) if op == Op::ReloadRoundTrip => preserve_reload(kind, changes),
        Expect::Preserve(changes)
            if matches!(op, Op::Copy | Op::Cut | Op::Paste | Op::Duplicate)
                && matches!(kind, ObjectKind::Widget | ObjectKind::Field) =>
        {
            super::field::preserve_carry(op, kind, changes)
        }
        Expect::Preserve(changes)
            if matches!(op, Op::Copy | Op::Cut | Op::Paste | Op::Duplicate) =>
        {
            preserve_carry(op, kind, changes)
        }
        Expect::Refused(_) if matches!(op, Op::Copy | Op::Cut | Op::Paste | Op::Duplicate) => {
            refused(op, kind)
        }
        Expect::Preserve(_)
        | Expect::Refused(_)
        | Expect::Remove(_)
        | Expect::Consume { .. }
        | Expect::Represent => vec![format!("no driver for {cell}")],
        Expect::NotApplicable(_) | Expect::Pending(_) => Vec::new(),
    }
}

#[derive(Default, Debug)]
struct Metrics {
    cells: usize,
    run: usize,
    preserve: usize,
    remove: usize,
    consume: usize,
    represent: usize,
    refused: usize,
    na: usize,
    pending: usize,
}

#[test]
fn every_driven_cell_holds() {
    let mut m = Metrics::default();
    let mut faults = Vec::new();
    for &op in Op::ALL {
        for &kind in ObjectKind::ALL {
            let cell = expect(op, kind);
            m.cells += 1;
            match cell {
                Expect::Preserve(_) => m.preserve += 1,
                Expect::Remove(_) => m.remove += 1,
                Expect::Consume { .. } => m.consume += 1,
                Expect::Represent => m.represent += 1,
                Expect::Refused(_) => m.refused += 1,
                Expect::NotApplicable(_) => m.na += 1,
                Expect::Pending(_) => m.pending += 1,
            }
            if !cell.is_driven() {
                continue;
            }
            m.run += 1;
            faults.extend(
                drive(op, kind, cell)
                    .into_iter()
                    .map(|f| format!("{op} x {kind} [{cell}]: {f}")),
            );
        }
    }
    println!(
        "xcut cells={} run={} preserve={} remove={} consume={} represent={} refused={} na={} pending={}",
        m.cells, m.run, m.preserve, m.remove, m.consume, m.represent, m.refused, m.na, m.pending
    );
    assert_eq!(m.cells, Op::ALL.len() * ObjectKind::ALL.len());
    assert_eq!(m.run, m.cells - m.na - m.pending);
    assert!(
        faults.is_empty(),
        "{} faults:\n{}",
        faults.len(),
        faults.join("\n")
    );
}

#[test]
fn every_fixture_classifies_as_its_kind() {
    for kind in fixture::kinds() {
        let (s, id) = fixture::build(kind);
        let Some(Object::Dict(dict)) = s.value(id) else {
            panic!("{kind}: no target");
        };
        if kind == ObjectKind::Field {
            let form = crate::forms::parse_acroform(&s.graph()).expect("form");
            assert_eq!(form.field_by_name("fld").map(|f| f.id), Some(id));
            assert!(dict.contains_key(b"Kids"), "{kind}: no kids");
        } else {
            assert_eq!(classify_annot(&s, dict), kind, "{kind}");
        }
        assert!(dict.contains_key(SENTINEL_KEY), "{kind}: no sentinel");
    }
}
