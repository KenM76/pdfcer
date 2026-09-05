//! `Pass 251.1` — `delete_pages` must decrement `/Count` on EVERY ancestor of a
//! removed page, not only its immediate parent (pdfcer-gui bug, 2026-09-05,
//! against a real nested SolidWorks drawing: a reader that trusts the ROOT
//! `/Count` — Acrobat does — showed the removed pages as trailing blanks).
//!
//! The defect was invisible on a FLAT one-level tree, where the immediate
//! parent IS the root, so this fixture is THREE levels deep: it can tell "no
//! upward walk at all" from "a walk that stops one short".

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::writer::SaveOptions;
use std::path::Path;

fn nested() -> Document {
    Document::load(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/synthetic/pageops/nested-tree-3level.pdf"),
    )
    .expect("load nested-tree-3level.pdf")
}

fn root_pages_id(g: &dyn ObjectGraph) -> ObjId {
    let root = g.trailer_entry(b"Root").expect("trailer /Root");
    let catalog = g.resolve(root).as_dict().expect("catalog dict");
    // NOTE: read the raw /Pages entry WITHOUT resolve() — resolve dereferences
    // a reference to its target, and here we want the reference's id itself.
    match catalog.get(b"Pages") {
        Some(Object::Reference(id)) => *id,
        other => panic!("catalog /Pages is not a reference: {other:?}"),
    }
}

/// Leaf `/Page` count under `id`, asserting every `/Pages` node's `/Count`
/// equals its true leaf-descendant tally.
fn check(g: &dyn ObjectGraph, id: ObjId) -> usize {
    let dict = g
        .value(id)
        .and_then(Object::as_dict)
        .unwrap_or_else(|| panic!("object {id:?} missing or not a dict"));
    let ty = dict
        .get(b"Type")
        .map(|o| g.resolve(o))
        .and_then(Object::as_name)
        .map(|n| n.as_bytes().to_vec());
    match ty.as_deref() {
        Some(b"Page") => 1,
        Some(b"Pages") => {
            let kids = dict
                .get(b"Kids")
                .map(|o| g.resolve(o))
                .and_then(Object::as_array)
                .expect("/Pages node has /Kids")
                .to_vec();
            let mut leaves = 0;
            for kid in &kids {
                // Raw reference, not resolve() — we need the child's id to recurse.
                if let Object::Reference(kid_id) = kid {
                    leaves += check(g, *kid_id);
                }
            }
            let declared = match dict.get(b"Count").map(|o| g.resolve(o)) {
                Some(Object::Integer(n)) => usize::try_from(*n).unwrap_or(0),
                other => panic!("/Pages node {id:?} has no integer /Count: {other:?}"),
            };
            assert_eq!(
                declared, leaves,
                "/Count on /Pages node {id:?} must equal its {leaves} leaf descendants, not {declared} \
                 (a stale ANCESTOR count is the nested-tree bug)"
            );
            leaves
        }
        other => panic!("object {id:?} has unexpected /Type {other:?}"),
    }
}

fn assert_tree_consistent(bytes: &[u8], expected_pages: usize) {
    let doc = Document::from_bytes(bytes.to_vec()).expect("saved output reloads");
    let g = doc.view();
    let leaves = check(&g, root_pages_id(&g));
    assert_eq!(
        leaves, expected_pages,
        "the whole tree must hold exactly {expected_pages} leaves after the delete"
    );
    // And the reachable-page walk agrees with the tree the counts describe.
    assert_eq!(
        pdfcer_core::page_tree::pages(&doc)
            .expect("page tree walks")
            .len(),
        expected_pages,
    );
}

#[test]
fn deleting_a_page_deep_in_a_nested_tree_fixes_every_ancestor_count() {
    // Delete page index 1 (the 2nd leaf, under A1) — three levels below root.
    // A1: 3->2, A: 6->5, root: 12->11 must ALL drop.
    let mut s = EditSession::new(nested());
    let out = s.delete_pages(&[1]).expect("delete page 1");
    assert_eq!(out.pages_removed, 1);

    // Both save modes must write a consistent tree. The operator hit this via
    // the CLI's default incremental save; the full rewrite must be right too.
    let (inc, _) = s
        .to_incremental_bytes(&SaveOptions::default())
        .expect("incremental");
    assert_tree_consistent(&inc, 11);
    let (full, _) = s.to_full_bytes(&SaveOptions::default()).expect("full");
    assert_tree_consistent(&full, 11);
}

#[test]
fn deleting_two_pages_from_different_subtrees_keeps_every_count_true() {
    // p2 (under A1) and p10 (under B2) — two different branches, so more than
    // one intermediate node loses leaves and the root loses two.
    let mut s = EditSession::new(nested());
    s.delete_pages(&[1, 9]).expect("delete pages 1 and 9");
    let (full, _) = s.to_full_bytes(&SaveOptions::default()).expect("full");
    assert_tree_consistent(&full, 10);
}
