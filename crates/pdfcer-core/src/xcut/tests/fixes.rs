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
