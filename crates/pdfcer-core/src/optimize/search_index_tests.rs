#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test code: a failed lookup should fail the test loudly

use super::tests::{build, dict, id, only, saved_text, session};
use super::*;
use crate::doc_metadata::MetadataKind;
use crate::graph::ObjectGraph;
use crate::object::Object;

/// A catalog whose `/PieceInfo` is `piece` (object 1 refers to it as
/// written), with a search index (5) whose private data is a stream (6).
fn sample(piece: &str, extra: &str) -> Vec<u8> {
    let b = |s: &str| s.as_bytes().to_vec();
    let mut bodies = vec![
        b(&format!(
            "<< /Type /Catalog /Pages 2 0 R /PieceInfo {piece} >>"
        )),
        b("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        b("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R >>"),
        b("<< /Length 0 >>\nstream\n\nendstream"),
        b("<< /LastModified (D:20260101000000Z) /Private 6 0 R >>"),
        b("<< /Length 10 >>\nstream\nINDEX-DATA\nendstream"),
    ];
    if !extra.is_empty() {
        bodies.push(b(extra));
    }
    build(&bodies)
}

fn index() -> DiscardOptions {
    DiscardOptions::none().with_object(DiscardObject::SearchIndex)
}

#[test]
fn only_the_index_goes_and_the_other_applications_data_stays() {
    let mut s = session(sample(
        "<< /SearchIndex 5 0 R /Other << /LastModified (D:20260101) /Private (KEEP-ME) >> >>",
        "",
    ));
    let plan = s.plan_optimize(&only(index())).unwrap();
    let [item] = plan.discard_objects.as_slice() else {
        panic!("{:?}", plan.discard_objects);
    };
    assert_eq!(
        (item.object, item.location.as_str()),
        (DiscardObject::SearchIndex, "catalog")
    );
    assert!(item.bytes > 0);
    assert!(plan.notes.iter().any(|n| n.contains("search index goes")));
    s.apply_optimize(&plan).unwrap();
    let cat = dict(&s, 1);
    let piece = cat.get(b"PieceInfo").and_then(Object::as_dict).unwrap();
    assert!(piece.get(b"SearchIndex").is_none() && piece.get(b"Other").is_some());
    assert!(s.view().value(id(6)).is_none(), "the index data survived");
    let text = saved_text(&mut s, &plan);
    assert!(!text.contains("INDEX-DATA") && text.contains("KEEP-ME"));
    s.undo();
    assert!(s.view().value(id(6)).is_some());
}

#[test]
fn an_indirect_piece_info_left_empty_goes() {
    let mut s = session(sample("7 0 R", "<< /SearchIndex 5 0 R >>"));
    let plan = s.plan_optimize(&only(index())).unwrap();
    s.apply_optimize(&plan).unwrap();
    assert!(dict(&s, 1).get(b"PieceInfo").is_none());
    for gone in [5, 6, 7] {
        assert!(s.view().value(id(gone)).is_none(), "object {gone} survived");
    }
    assert!(!saved_text(&mut s, &plan).contains("INDEX-DATA"));
}

#[test]
fn an_indirect_piece_info_shared_with_others_keeps_them() {
    let mut s = session(sample(
        "7 0 R",
        "<< /SearchIndex 5 0 R /Other << /Private (KEEP-ME) >> >>",
    ));
    let plan = s.plan_optimize(&only(index())).unwrap();
    s.apply_optimize(&plan).unwrap();
    let piece = dict(&s, 7);
    assert!(piece.get(b"SearchIndex").is_none() && piece.get(b"Other").is_some());
}

#[test]
fn nothing_to_list_without_an_index_or_after_piece_info_goes() {
    let s = session(sample("<< /Other << /Private (KEEP-ME) >> >>", ""));
    assert!(
        s.plan_optimize(&only(index()))
            .unwrap()
            .discard_objects
            .is_empty()
    );

    let mut s = session(sample("<< /SearchIndex 5 0 R >>", ""));
    let both = index().with(MetadataKind::PieceInfo);
    let plan = s.plan_optimize(&only(both)).unwrap();
    s.apply_optimize(&plan).unwrap();
    assert!(dict(&s, 1).get(b"PieceInfo").is_none());
}
