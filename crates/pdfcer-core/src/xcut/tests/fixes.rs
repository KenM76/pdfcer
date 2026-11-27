//! Regressions for the defects the 781.0 gate found in the raw carry.

use super::fixture;
use super::runner::{annots, copy, paste};
use crate::object::Object;
use crate::xcut::ObjectKind;

/// §12.6.4.2: only a `/GoTo` names a target in the source document. A
/// `/URI` action is not document-bound and must survive a cross-document
/// paste.
#[test]
#[allow(clippy::unwrap_used)]
fn a_uri_link_keeps_its_action_across_documents() {
    let (s, id) = fixture::build(ObjectKind::Link);
    let clip = copy(&s, id).unwrap();
    let mut dst = fixture::destination();
    let landed = paste(&mut dst, 0, &clip, (0.0, 0.0)).unwrap();
    assert_eq!(
        landed.after.0.get("/A/URI"),
        Some(&crate::xcut::dump::Canon::Str(
            b"https://example.org/".to_vec()
        ))
    );
    assert!(
        !landed
            .outcome
            .disclosures
            .iter()
            .any(|d| d.contains("destination")),
        "{:?}",
        landed.outcome.disclosures
    );
}

/// §12.5.2 Table 164: `/NM` is unique on the page, so a duplicate gets a
/// fresh one rather than losing it.
#[test]
#[allow(clippy::unwrap_used)]
fn a_duplicate_gets_a_fresh_unique_name() {
    let (mut s, id) = fixture::build(ObjectKind::Stamp);
    let clip = copy(&s, id).unwrap();
    paste(&mut s, 0, &clip, (10.0, 0.0)).unwrap();
    paste(&mut s, 0, &clip, (20.0, 0.0)).unwrap();
    let names: Vec<Vec<u8>> = annots(&s, 0)
        .iter()
        .filter_map(|a| match s.value(*a) {
            Some(Object::Dict(d)) => match d.get(b"NM") {
                Some(Object::String(n)) => Some(n.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    // target, note, and two pasted copies.
    assert_eq!(names.len(), 4, "{names:?}");
    let unique: std::collections::BTreeSet<_> = names.iter().collect();
    assert_eq!(unique.len(), names.len(), "{names:?}");
}

/// The keys a paste drops are counted and disclosed, not silent.
#[test]
#[allow(clippy::unwrap_used)]
fn dropped_source_bound_keys_are_disclosed() {
    let (s, id) = fixture::build(ObjectKind::Stamp);
    let clip = copy(&s, id).unwrap();
    let landed = paste(&mut fixture::destination(), 0, &clip, (0.0, 0.0)).unwrap();
    for key in ["StructParent", "Popup", "IRT"] {
        assert_eq!(landed.outcome.dropped_keys.get(key), Some(&1), "{key}");
    }
    assert!(
        landed
            .outcome
            .disclosures
            .iter()
            .any(|d| d.contains("structure-tree") && d.contains("pop-up")),
        "{:?}",
        landed.outcome.disclosures
    );
}

/// A self-reference (a Screen's rendition `/AN`) binds to the pasted copy
/// and does not drag the source page tree into the clip.
#[test]
#[allow(clippy::unwrap_used)]
fn a_self_playing_screen_points_at_its_pasted_copy() {
    let (s, id) = fixture::self_playing_screen();
    let clip = copy(&s, id).unwrap();
    let carried = clip.annotations.first().unwrap();
    let crate::vector::ClipAnnotation::Raw(raw) = carried else {
        panic!("not raw: {carried:?}");
    };
    assert!(
        raw.objects.len() <= 1,
        "page tree leaked into the clip: {:?}",
        raw.objects
    );
    let mut dst = fixture::destination();
    paste(&mut dst, 0, &clip, (0.0, 0.0)).unwrap();
    let pasted = *annots(&dst, 0).last().unwrap();
    let Some(Object::Dict(d)) = dst.value(pasted) else {
        panic!("no pasted annotation");
    };
    let Some(Object::Dict(action)) = d.get(b"A") else {
        panic!("action lost: {d:?}");
    };
    assert_eq!(action.get(b"AN"), Some(&Object::Reference(pasted)));
}

/// 781.2: a rotated markup paste redraws `/AP` from the geometry and keeps
/// every key the redraw does not own, the sentinel included.
#[test]
#[allow(clippy::unwrap_used)]
fn a_rotated_markup_paste_rebakes_and_keeps_the_sentinel() {
    let (mut s, id) = fixture::build(ObjectKind::Square);
    let before = super::runner::dump(&s, id);
    let clip = copy(&s, id).unwrap();
    let depth = s.undo_depth();
    let quarter = crate::vector::Matrix::new(0.0, 1.0, -1.0, 0.0, 400.0, 0.0);
    let outcome = s.paste_objects(0, &clip, quarter).unwrap();
    let pasted = *annots(&s, 0).last().unwrap();
    let after = super::runner::dump(&s, pasted);
    for key in ["/XXpdfcerSentinel", "/XXpdfcerSentinelStream"] {
        let mine: Vec<_> = before
            .0
            .iter()
            .filter(|(k, _)| k.starts_with(key))
            .collect();
        let theirs: Vec<_> = after.0.iter().filter(|(k, _)| k.starts_with(key)).collect();
        assert!(!mine.is_empty(), "the fixture plants {key}");
        assert_eq!(mine, theirs, "{key} survives the redraw");
    }
    let ap = |d: &crate::xcut::dump::PropertyDump| {
        d.0.iter()
            .filter(|(k, _)| k.starts_with("/AP"))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<Vec<_>>()
    };
    assert_ne!(ap(&before), ap(&after), "the appearance is redrawn");
    assert!(
        outcome.disclosures.iter().any(|d| d.contains("REDREW")),
        "{:?}",
        outcome.disclosures
    );
    assert_eq!(s.undo_depth(), depth + 1, "one paste, one undo step");
}

/// The top-level keys of `id`'s dictionary, sorted.
fn key_set(s: &crate::edit::EditSession, id: crate::object::ObjId) -> Vec<String> {
    let Some(Object::Dict(d)) = s.value(id) else {
        return Vec::new();
    };
    let mut keys: Vec<String> = d
        .iter()
        .map(|(k, _)| String::from_utf8_lossy(k.as_bytes()).into_owned())
        .collect();
    keys.sort();
    keys
}

/// 781.2.a: a rotated or scaled markup paste keeps exactly the source's key
/// set, less the declared drops. The re-bake writes back only what changed
/// (ISO 32000-1 §12.5.4: an absent `/BS` already means its default; ISO
/// 32000-2 §12.5.2 Table 166: `/BM` is optional and the baked `/AP` selects
/// its own blend mode).
#[test]
#[allow(clippy::unwrap_used)]
fn a_transformed_markup_paste_adds_no_key() {
    use crate::xcut::policy::clipboard::MARKUP;
    use crate::xcut::{Change, Expect, Op, expect};
    let quarter = crate::vector::Matrix::new(0.0, 1.0, -1.0, 0.0, 400.0, 0.0);
    let turn_and_grow = crate::vector::Matrix::new(0.0, 2.0, -2.0, 0.0, 500.0, 0.0);
    let dropped: Vec<&str> = MARKUP
        .iter()
        .filter_map(|c| match c {
            Change::Dropped { key, .. } => Some(*key),
            _ => None,
        })
        .collect();
    let mut faults = Vec::new();
    let markup = fixture::kinds()
        .filter(|k| matches!(expect(Op::Paste, *k), Expect::Preserve(c) if c == MARKUP));
    for kind in markup {
        for (label, at) in [("rotated", quarter), ("rotated and scaled", turn_and_grow)] {
            let (mut s, id) = fixture::build(kind);
            let mut want = key_set(&s, id);
            want.retain(|k| !dropped.contains(&k.as_str()));
            let clip = copy(&s, id).unwrap();
            let outcome = s.paste_objects(0, &clip, at).unwrap();
            let pasted = *annots(&s, 0).last().unwrap();
            if !outcome.disclosures.iter().any(|d| d.contains("REDREW")) {
                faults.push(format!("{kind:?} {label}: not redrawn"));
            }
            if kind == ObjectKind::Ink && label == "rotated and scaled" {
                let dump = super::runner::dump(&s, pasted);
                let width = dump.0.get("/Border[2]");
                let want_width = crate::xcut::dump::Canon::Num(4.0);
                if width != Some(&want_width) {
                    faults.push(format!("Ink {label}: /Border width {width:?}, want 4"));
                }
            }
            let got = key_set(&s, pasted);
            if got != want {
                faults.push(format!("{kind:?} {label}: keys {got:?}, want {want:?}"));
            }
        }
    }
    assert!(faults.is_empty(), "{faults:#?}");
}
