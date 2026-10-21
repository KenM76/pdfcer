#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::pipeline_tests::{AT_100, doc, grey, plan, skip};
use crate::edit::EditSession;
use crate::object::{ObjId, Object};
use crate::optimize::{ImageChange, OptimizePlan, SkipReason};
use crate::space_audit::tests::stream;

fn change(p: &OptimizePlan, n: u32) -> Option<&ImageChange> {
    p.changes
        .iter()
        .find(|c| c.id.num == n)
        .and_then(|c| c.image.as_ref())
}

fn size_of(s: &EditSession, n: u32) -> (Option<Object>, Option<Object>) {
    let Some(Object::Stream(st)) = s.value(ObjId::new(n, 0)) else {
        panic!("object {n} is not a stream")
    };
    (
        st.dict.get(b"Width").cloned(),
        st.dict.get(b"Height").cloned(),
    )
}

#[test]
fn a_matte_mask_is_resampled_to_its_parents_new_size() {
    let pair = [grey("/SMask 6 0 R"), grey("/Matte [0]")];
    let (mut s, p) = plan(doc(AT_100, &pair), &[("image(ppi=72)", None)]);
    let (parent, mask) = (change(&p, 5).unwrap(), change(&p, 6).unwrap());
    assert_eq!(parent.size_after, (100, 100));
    assert_eq!(mask.size_after, parent.size_after);
    assert_eq!(mask.settings.method, parent.settings.method);
    assert_eq!(mask.resampled_with, Some(ObjId::new(5, 0)));
    assert_eq!(parent.resampled_with, None);
    s.apply_optimize(&p).unwrap();
    let hundred = (Some(Object::Integer(100)), Some(Object::Integer(100)));
    assert_eq!(size_of(&s, 5), hundred);
    assert_eq!(size_of(&s, 6), hundred);
    let Some(Object::Stream(m)) = s.value(ObjId::new(6, 0)) else {
        panic!()
    };
    assert!(m.dict.contains_key(b"Matte"));
}

#[test]
fn the_mask_follows_its_parent_even_when_its_own_settings_do_not_resample() {
    let pair = [grey("/SMask 6 0 R"), grey("/Matte [0]")];
    let (_, p) = plan(
        doc(AT_100, &pair),
        &[("image(ppi=72)", None), ("image(ppi=600)", Some(6))],
    );
    assert_eq!(change(&p, 6).unwrap().size_after, (100, 100));
}

#[test]
fn a_parent_kept_at_its_size_leaves_the_mask_to_its_own_settings() {
    let pair = [grey("/SMask 6 0 R"), grey("/Matte [0]")];
    let (_, p) = plan(doc(AT_100, &pair), &[("image(ppi=600)", None)]);
    assert!(change(&p, 5).is_none());
    assert!(change(&p, 6).is_none());
    assert!(skip(&p, 5).is_none() && skip(&p, 6).is_none());
}

#[test]
fn a_matte_mask_shared_by_two_parents_keeps_both_at_their_size() {
    let images = [
        grey("/SMask 7 0 R"),
        grey("/SMask 7 0 R"),
        grey("/Matte [0]"),
    ];
    let content = "q 100 0 0 100 0 0 cm /Im5 Do /Im6 Do Q";
    let (_, p) = plan(doc(content, &images), &[("image(ppi=72)", None)]);
    for n in [5, 6, 7] {
        assert!(
            matches!(skip(&p, n), Some(SkipReason::ImageUnsupported(w)) if w.contains("Matte")),
            "object {n}: {:?}",
            skip(&p, n)
        );
    }
}

#[test]
fn a_mask_that_cannot_follow_withdraws_its_parents_rewrite() {
    let broken = stream(
        "/Type /XObject /Subtype /Image /Width 300 /Height 300 /ColorSpace /DeviceGray \
         /BitsPerComponent 8 /Filter /FlateDecode /Matte [0]",
        b"not flate data",
    );
    let pair = [grey("/SMask 6 0 R"), broken];
    let (_, p) = plan(doc(AT_100, &pair), &[("image(ppi=72)", None)]);
    assert!(p.changes.is_empty(), "{:?}", p.changes);
    assert!(p.writes.is_empty());
    assert!(
        matches!(skip(&p, 5), Some(SkipReason::ImageUnsupported(w)) if w.contains("6 0 R")),
        "{:?}",
        skip(&p, 5)
    );
    assert!(matches!(skip(&p, 6), Some(SkipReason::Undecodable(_))));
}

#[test]
fn one_half_of_a_pair_is_never_written_alone() {
    let pair = [grey("/SMask 6 0 R"), grey("/Matte [0]")];
    let (mut s, mut p) = plan(doc(AT_100, &pair), &[("image(ppi=72)", None)]);
    assert_eq!(
        p.writes.iter().find(|w| w.id.num == 5).unwrap().partner,
        Some(ObjId::new(6, 0))
    );
    p.writes.retain(|w| w.id.num != 6);
    s.apply_optimize(&p).unwrap();
    let full = (Some(Object::Integer(300)), Some(Object::Integer(300)));
    assert_eq!(size_of(&s, 5), full);
}
