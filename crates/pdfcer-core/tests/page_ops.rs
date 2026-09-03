//! # Pass 3.2 acceptance tests — structural page operations
//!
//! End-to-end, through the same API `pdfce-gui` and `pdfcer` use:
//! [`EditSession`] for the in-place edits, [`pdfcer_core::pageops`] for the
//! document producers. Nothing here reaches into internals, because the
//! contract being tested is the one the front ends depend on.
//!
//! ## What each group is actually pinning
//!
//! - **Delete** — the page leaves the tree, the *surviving* pages'
//!   objects are re-emitted byte-identically (§5), and the freed objects
//!   get conforming §7.5.4 type-0 entries with an incremented generation
//!   and a well-formed linked list. Decision 007 **W9** is the reason
//!   the free-list assertions are byte-level rather than
//!   "does it reload": *"A malformed type-0 free chain produces files
//!   Acrobat tolerates and stricter readers reject — the worst failure
//!   shape, because the obvious test passes."*
//! - **Reorder** — the order changes, the tree's *shape* does not, and
//!   inherited attributes survive a move between ancestors.
//! - **Undo** — every structural command satisfies §11.1's contract:
//!   edit → undo → save is **byte-identical to the input**. This is the
//!   test Pass 3.1 shipped for value edits, extended to edits that add
//!   and remove cross-reference entries, which is a strictly harder case
//!   because a stale free entry would survive an undo that only restored
//!   values.
//! - **Producers** — extract/merge/split/insert output is a loadable,
//!   renderable, deterministic standalone document.

use std::collections::HashSet;

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{ObjId, Object, Provenance};
use pdfcer_core::pageops::{
    DocumentView, InsertPosition, SplitCriterion, extract, insert, merge, split,
};
use pdfcer_core::signature::{SaveMode, SignatureImpact};
use pdfcer_core::writer::SaveOptions;
use pdfcer_core::xref::XrefEntry;

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Build an offset-consistent classic PDF from `(number, body)` pairs.
fn build(objects: &[(u32, &str)]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    let max_num = objects.iter().map(|(n, _)| *n).max().unwrap_or(0);
    buf.extend_from_slice(format!("xref\n0 {}\n", max_num + 1).as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for num in 1..=max_num {
        match offsets.iter().find(|(n, _)| *n == num) {
            Some((_, off)) => buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes()),
            None => buf.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /ID [<0102> <0304>] >>\nstartxref\n{xref_at}\n%%EOF\n",
            max_num + 1
        )
        .as_bytes(),
    );
    buf
}

/// Three pages, each with its own content stream, all attributes
/// inherited from the root node.
fn three_page_doc() -> Vec<u8> {
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 \
             /MediaBox [0 0 200 100] /Resources << >> >>",
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Contents 6 0 R >>"),
        (4, "<< /Type /Page /Parent 2 0 R /Contents 7 0 R >>"),
        (5, "<< /Type /Page /Parent 2 0 R /Contents 8 0 R >>"),
        (6, "<< /Length 5 >>\nstream\npage1\nendstream"),
        (7, "<< /Length 5 >>\nstream\npage2\nendstream"),
        (8, "<< /Length 5 >>\nstream\npage3\nendstream"),
    ])
}

/// A nested tree: root → [branch(p1,p2), p3]. The branch sets
/// `/Rotate 90`, which p1 and p2 inherit and p3 does not — the shape that
/// catches a reorder that moves a page between ancestors without
/// materializing what it used to inherit.
fn nested_doc() -> Vec<u8> {
    build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R 6 0 R] /Count 3 \
             /MediaBox [0 0 200 100] /Resources << >> >>",
        ),
        (
            3,
            "<< /Type /Pages /Parent 2 0 R /Kids [4 0 R 5 0 R] /Count 2 /Rotate 90 >>",
        ),
        (4, "<< /Type /Page /Parent 3 0 R >>"),
        (5, "<< /Type /Page /Parent 3 0 R >>"),
        (6, "<< /Type /Page /Parent 2 0 R >>"),
    ])
}

fn session(bytes: &[u8]) -> EditSession {
    EditSession::new(Document::from_bytes(bytes.to_vec()).expect("fixture must load"))
}

fn save_incremental(session: &EditSession) -> Vec<u8> {
    session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save must succeed")
        .0
}

// ---------------------------------------------------------------------------
// Delete — the page tree
// ---------------------------------------------------------------------------

#[test]
fn deleting_a_page_removes_it_from_the_tree_and_from_the_saved_file() {
    let source = three_page_doc();
    let mut s = session(&source);
    let outcome = s.delete_pages(&[1]).unwrap();
    assert_eq!(outcome.pages_removed, 1);
    assert_eq!(s.pages().unwrap().len(), 2);

    // The save must survive a reload with the page genuinely gone —
    // not merely absent from the in-memory view.
    let saved = save_incremental(&s);
    let reloaded = Document::from_bytes(saved).unwrap();
    let pages = pdfcer_core::page_tree::pages(&reloaded).unwrap();
    assert_eq!(pages.len(), 2);
    assert!(
        reloaded.get(ObjId::new(4, 0)).is_none(),
        "the deleted page object must not resolve after the save"
    );
}

#[test]
fn a_deleted_pages_exclusive_content_stream_is_freed_and_shared_ones_are_not() {
    // Page 2 owns object 7 exclusively; if the sweep were "free
    // everything the page referenced" it would be right here by luck, so
    // the second half of the test is the one that matters.
    let source = three_page_doc();
    let mut s = session(&source);
    let outcome = s.delete_pages(&[1]).unwrap();
    // The page object plus its content stream.
    assert_eq!(outcome.objects_freed, 2);

    let reloaded = Document::from_bytes(save_incremental(&s)).unwrap();
    assert!(reloaded.get(ObjId::new(7, 0)).is_none(), "exclusive stream");
    assert!(
        reloaded.get(ObjId::new(6, 0)).is_some(),
        "another page's stream must survive"
    );
    assert!(reloaded.get(ObjId::new(8, 0)).is_some());
}

#[test]
fn an_object_shared_with_a_surviving_page_is_never_freed() {
    // Both pages point at ONE content stream. Deleting either must leave
    // it alone: the sweep is a liveness computation against the document
    // as it will be, not "did the removed page reference this".
    let source = build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 \
             /MediaBox [0 0 10 10] /Resources << >> >>",
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Contents 5 0 R >>"),
        (4, "<< /Type /Page /Parent 2 0 R /Contents 5 0 R >>"),
        (5, "<< /Length 6 >>\nstream\nshared\nendstream"),
    ]);
    let mut s = session(&source);
    let outcome = s.delete_pages(&[0]).unwrap();
    assert_eq!(outcome.objects_freed, 1, "only the page object itself");

    let reloaded = Document::from_bytes(save_incremental(&s)).unwrap();
    assert!(
        reloaded.get(ObjId::new(5, 0)).is_some(),
        "a shared stream must survive its co-owner being deleted"
    );
}

#[test]
fn deleting_every_page_of_a_branch_prunes_the_now_empty_node() {
    // Table 29 requires a Pages node to have `Kids`; an empty one is not
    // a legal node, so it goes with its last child.
    let source = nested_doc();
    let mut s = session(&source);
    let outcome = s.delete_pages(&[0, 1]).unwrap();
    assert_eq!(outcome.pages_removed, 2);
    assert_eq!(s.pages().unwrap().len(), 1);

    let reloaded = Document::from_bytes(save_incremental(&s)).unwrap();
    assert!(
        reloaded.get(ObjId::new(3, 0)).is_none(),
        "the emptied branch node must be freed too"
    );
    assert_eq!(pdfcer_core::page_tree::pages(&reloaded).unwrap().len(), 1);
}

#[test]
fn deleting_the_last_page_is_a_named_refusal() {
    // §7.7.3.3, and `core_ops__delete_pages.md`: "Cannot delete the only
    // remaining page."
    let source = three_page_doc();
    let mut s = session(&source);
    let err = s.delete_pages(&[0, 1, 2]).unwrap_err();
    assert!(matches!(
        err,
        EditError::WouldRemoveEveryPage {
            removing: 3,
            total: 3
        }
    ));
    assert!(!s.can_undo(), "a refused edit must leave no history");
    assert!(!s.is_modified());
}

#[test]
fn surviving_ancestors_get_a_recomputed_count_never_a_decremented_one() {
    // `page_tree` deliberately does not trust /Count, so a file whose
    // /Count was already wrong must come out RIGHT, not equally wrong.
    let source = build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 99 \
             /MediaBox [0 0 10 10] /Resources << >> >>",
        ),
        (3, "<< /Type /Page /Parent 2 0 R >>"),
        (4, "<< /Type /Page /Parent 2 0 R >>"),
    ]);
    let mut s = session(&source);
    s.delete_pages(&[0]).unwrap();
    let reloaded = Document::from_bytes(save_incremental(&s)).unwrap();
    let root = reloaded
        .resolved(ObjId::new(2, 0))
        .as_dict()
        .unwrap()
        .get(b"Count")
        .and_then(Object::as_int);
    assert_eq!(root, Some(1), "count is derived from the walk, not patched");
}

// ---------------------------------------------------------------------------
// Delete — the free list (decision 007 W9)
// ---------------------------------------------------------------------------

/// Read the saved file's newest cross-reference section by reloading it,
/// which is the same view a conforming reader gets.
fn xref_of(bytes: &[u8]) -> Vec<(u32, XrefEntry)> {
    let doc = Document::from_bytes(bytes.to_vec()).expect("saved file must reload");
    doc.xref().iter().collect()
}

#[test]
fn a_freed_object_gets_a_type_zero_entry_with_an_incremented_generation() {
    // §7.5.4: a free entry's generation is "the generation number to be
    // used if the object is ever reused". Emitting the OLD generation
    // would let a stale reference resolve to a resurrected object.
    let source = three_page_doc();
    let mut s = session(&source);
    s.delete_pages(&[1]).unwrap();
    let saved = save_incremental(&s);

    let entries = xref_of(&saved);
    let page = entries
        .iter()
        .find(|(num, _)| *num == 4)
        .map(|(_, entry)| *entry)
        .expect("object 4 must have an entry");
    match page {
        XrefEntry::Free { generation, .. } => assert_eq!(generation, 1),
        other => panic!("deleted object must be free, got {other:?}"),
    }
}

#[test]
fn the_free_list_is_a_well_formed_linked_list_headed_at_object_zero() {
    // W9's failure shape exactly: a chain that is nearly right passes
    // "does it reload" in every tolerant reader and is rejected by
    // strict ones. Walk it and prove it terminates on every freed object.
    let source = three_page_doc();
    let mut s = session(&source);
    s.delete_pages(&[0, 1]).unwrap();
    let saved = save_incremental(&s);
    let entries = xref_of(&saved);

    let free: std::collections::HashMap<u32, (u32, u16)> = entries
        .iter()
        .filter_map(|(num, entry)| match entry {
            XrefEntry::Free {
                next_free,
                generation,
            } => Some((*num, (*next_free, *generation))),
            _ => None,
        })
        .collect();
    let head = free.get(&0).expect("object 0 is always free (§7.5.4)");
    assert_eq!(head.1, 65_535, "the head's generation is 65535");

    // Walk from the head; every freed object must appear exactly once and
    // the walk must terminate at 0.
    let mut walked: HashSet<u32> = HashSet::new();
    let mut current = head.0;
    while current != 0 {
        assert!(walked.insert(current), "the free list must not cycle");
        let (next, _) = *free
            .get(&current)
            .unwrap_or_else(|| panic!("free list points at object {current}, which is not free"));
        current = next;
        assert!(walked.len() <= free.len(), "free list ran away");
    }
    // Four objects leave: two pages plus their two exclusive streams.
    assert_eq!(walked.len(), 4, "every freed object is on the list");
}

#[test]
fn deleting_leaves_surviving_objects_byte_identical() {
    // §5's invariant, on the operation most likely to break it: an
    // untouched page's definition bytes must be exactly what they were.
    //
    // "Untouched" is doing real work in that sentence, and it acquired an
    // exception after this test was written. A surviving page that is a
    // member of a preseparated set (§14.11.4) which just LOST a member is
    // not untouched: its `/SeparationInfo /Pages` array names an object
    // that no longer exists, so re-emitting it is what preserves the
    // document rather than what damages it. That case is pinned in
    // `tests/separation_sets.rs`, which also asserts the converse — a set
    // that lost nothing is byte-identical, exactly as here. This fixture
    // is not preseparated, so the unqualified invariant holds for it.
    let source = three_page_doc();
    let before = Document::from_bytes(source.clone()).unwrap();
    let mut s = session(&source);
    s.delete_pages(&[1]).unwrap();
    let after = Document::from_bytes(save_incremental(&s)).unwrap();

    for id in [ObjId::new(3, 0), ObjId::new(5, 0), ObjId::new(6, 0)] {
        let Provenance::File(span) = before.get(id).unwrap().provenance else {
            panic!("fixture objects are file-level");
        };
        let want = span.slice(before.bytes()).unwrap();
        let got = after
            .get(id)
            .and_then(|io| io.file_span())
            .and_then(|s| s.slice(after.bytes()));
        assert_eq!(got, Some(want), "object {id} lost its verbatim bytes");
    }
}

#[test]
fn an_incremental_delete_appends_and_never_rewrites_prior_bytes() {
    // §7.5.6: "changes shall be appended to the end of the file, leaving
    // its original contents intact" — which is also what keeps a
    // signature's byte range intact (§12.8.1 NOTE 1).
    let source = three_page_doc();
    let mut s = session(&source);
    s.delete_pages(&[1]).unwrap();
    let saved = save_incremental(&s);
    assert!(
        saved.starts_with(&source),
        "an incremental save must not touch a single prior byte"
    );
}

// ---------------------------------------------------------------------------
// Undo — §11.1's contract, extended to structural edits
// ---------------------------------------------------------------------------

#[test]
fn delete_then_undo_saves_a_byte_identical_file() {
    // The Pass 3.1 headline contract, on an edit that adds and removes
    // cross-reference entries. Strictly harder than a value edit: a
    // deletion tracked outside the save-time diff would leave a free
    // entry behind that undo never removed.
    let source = three_page_doc();
    let mut s = session(&source);
    s.delete_pages(&[0, 2]).unwrap();
    assert!(s.is_modified());
    s.undo();
    assert!(!s.is_modified(), "an undone delete is not a change");
    assert!(s.dirty_set().is_empty());
    assert_eq!(save_incremental(&s), source);
    assert_eq!(s.pages().unwrap().len(), 3);
}

#[test]
fn reorder_then_undo_saves_a_byte_identical_file() {
    let source = three_page_doc();
    let mut s = session(&source);
    s.reorder_pages(&[2, 0, 1]).unwrap();
    assert!(s.is_modified());
    s.undo();
    assert_eq!(save_incremental(&s), source);
}

#[test]
fn batch_rotate_then_undo_saves_a_byte_identical_file() {
    let source = three_page_doc();
    let mut s = session(&source);
    assert_eq!(s.rotate_pages(&[0, 1, 2], 90).unwrap(), 3);
    s.undo();
    assert_eq!(save_incremental(&s), source);
}

#[test]
fn a_delete_is_one_undo_entry_however_many_pages_it_removed() {
    // §11.3: one operator gesture is one undo entry. A per-page command
    // stack would make "undo my delete" take three clicks.
    let source = three_page_doc();
    let mut s = session(&source);
    s.delete_pages(&[0, 1]).unwrap();
    assert_eq!(s.undo_depth(), 1);
    s.undo();
    assert_eq!(s.pages().unwrap().len(), 3, "one undo restores both pages");
}

#[test]
fn redo_reapplies_a_structural_edit_including_its_deletions() {
    let source = three_page_doc();
    let mut s = session(&source);
    s.delete_pages(&[1]).unwrap();
    s.undo();
    assert!(s.redo().is_some());
    assert_eq!(s.pages().unwrap().len(), 2);
    // ...and the free entries came back with it.
    let reloaded = Document::from_bytes(save_incremental(&s)).unwrap();
    assert!(reloaded.get(ObjId::new(4, 0)).is_none());
}

// ---------------------------------------------------------------------------
// Reorder
// ---------------------------------------------------------------------------

#[test]
fn reorder_changes_the_order_without_touching_page_content() {
    let source = three_page_doc();
    let mut s = session(&source);
    let before: Vec<ObjId> = s.pages().unwrap().iter().map(|p| p.id).collect();
    s.reorder_pages(&[2, 0, 1]).unwrap();
    let after: Vec<ObjId> = s.pages().unwrap().iter().map(|p| p.id).collect();
    assert_eq!(after, vec![before[2], before[0], before[1]]);

    // Round-trip it: the reordered document must reload in the new order.
    let reloaded = Document::from_bytes(save_incremental(&s)).unwrap();
    let ids: Vec<ObjId> = pdfcer_core::page_tree::pages(&reloaded)
        .unwrap()
        .iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(ids, after);
}

#[test]
fn reordering_across_ancestors_materializes_what_the_page_used_to_inherit() {
    // Pages 1-2 inherit /Rotate 90 from the branch node; page 3 does not.
    // Moving page 1 into page 3's slot must keep it at 90°, or the
    // reorder has silently rotated the operator's page.
    let source = nested_doc();
    let mut s = session(&source);
    assert_eq!(
        s.pages()
            .unwrap()
            .iter()
            .map(|p| p.rotate)
            .collect::<Vec<_>>(),
        vec![90, 90, 0]
    );
    s.reorder_pages(&[2, 1, 0]).unwrap();
    let rotations: Vec<u16> = s.pages().unwrap().iter().map(|p| p.rotate).collect();
    assert_eq!(
        rotations,
        vec![0, 90, 90],
        "each page keeps its own rotation across the move"
    );

    let reloaded = Document::from_bytes(save_incremental(&s)).unwrap();
    let after: Vec<u16> = pdfcer_core::page_tree::pages(&reloaded)
        .unwrap()
        .iter()
        .map(|p| p.rotate)
        .collect();
    assert_eq!(after, vec![0, 90, 90], "and after a save/reload too");
}

#[test]
fn reorder_keeps_the_trees_shape_rather_than_flattening_it() {
    // R33 by analogy: rebuilding one flat root /Kids would rewrite the
    // whole tree for a two-page swap and orphan the branch node.
    let source = nested_doc();
    let mut s = session(&source);
    s.reorder_pages(&[1, 0, 2]).unwrap();
    let reloaded = Document::from_bytes(save_incremental(&s)).unwrap();
    let branch = reloaded.resolved(ObjId::new(3, 0));
    assert!(
        branch.as_dict().is_some_and(|d| d.contains_key(b"Kids")),
        "the intermediate node must survive a reorder"
    );
}

#[test]
fn an_identity_reorder_records_nothing() {
    let source = three_page_doc();
    let mut s = session(&source);
    s.reorder_pages(&[0, 1, 2]).unwrap();
    assert!(!s.can_undo(), "a no-op must not reach the undo stack");
    assert!(!s.is_modified());
}

#[test]
fn a_reorder_that_is_not_a_permutation_is_refused() {
    // Dropping a page the caller forgot to list would be a DELETE
    // wearing a reorder's name.
    let source = three_page_doc();
    let mut s = session(&source);
    assert!(matches!(
        s.reorder_pages(&[0, 1]).unwrap_err(),
        EditError::NotAPermutation {
            expected: 3,
            got: 2
        }
    ));
    assert!(matches!(
        s.reorder_pages(&[0, 0, 1]).unwrap_err(),
        EditError::NotAPermutation { .. }
    ));
    assert!(!s.is_modified());
}

// ---------------------------------------------------------------------------
// Batch rotate
// ---------------------------------------------------------------------------

#[test]
fn batch_rotate_turns_each_page_from_its_own_current_rotation() {
    // Not "set them all to 90": a selection at 0/90/180 turned by 90 must
    // land at 90/180/270.
    let source = build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 \
             /MediaBox [0 0 10 10] /Resources << >> >>",
        ),
        (3, "<< /Type /Page /Parent 2 0 R >>"),
        (4, "<< /Type /Page /Parent 2 0 R /Rotate 90 >>"),
        (5, "<< /Type /Page /Parent 2 0 R /Rotate 180 >>"),
    ]);
    let mut s = session(&source);
    assert_eq!(s.rotate_pages(&[0, 1, 2], 90).unwrap(), 3);
    let rotations: Vec<u16> = s.pages().unwrap().iter().map(|p| p.rotate).collect();
    assert_eq!(rotations, vec![90, 180, 270]);
    assert_eq!(s.undo_depth(), 1, "one gesture, one undo entry");
}

#[test]
fn batch_rotate_by_a_full_turn_changes_nothing_and_records_nothing() {
    let source = three_page_doc();
    let mut s = session(&source);
    assert_eq!(s.rotate_pages(&[0, 1, 2], 360).unwrap(), 0);
    assert!(!s.can_undo());
    assert!(!s.is_modified());
}

#[test]
fn batch_rotate_refuses_a_non_multiple_of_ninety() {
    let source = three_page_doc();
    let mut s = session(&source);
    assert!(matches!(
        s.rotate_pages(&[0], 45).unwrap_err(),
        EditError::RotationNotMultipleOf90 { degrees: 45 }
    ));
}

// ---------------------------------------------------------------------------
// Dangling-reference disclosure
// ---------------------------------------------------------------------------

#[test]
fn delete_reports_the_bookmarks_and_links_it_orphans() {
    // The UI spec's required core addition: the GUI cannot compute this
    // without independently walking the outline/annotation graph.
    let source = build(&[
        (
            1,
            "<< /Type /Catalog /Pages 2 0 R /Outlines 6 0 R \
             /PageLabels << /Nums [0 << /S /D >>] >> >>",
        ),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 \
             /MediaBox [0 0 10 10] /Resources << >> >>",
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Annots [5 0 R] >>"),
        (4, "<< /Type /Page /Parent 2 0 R >>"),
        (
            5,
            "<< /Type /Annot /Subtype /Link /Rect [0 0 1 1] /Dest [4 0 R /Fit] >>",
        ),
        (6, "<< /Type /Outlines /First 7 0 R /Count 1 >>"),
        (7, "<< /Title (Chapter) /Dest [4 0 R /Fit] >>"),
    ]);
    let mut s = session(&source);
    let outcome = s.delete_pages(&[1]).unwrap();
    assert_eq!(outcome.dangling.outline_items, 1);
    assert_eq!(outcome.dangling.links, 1);
    assert!(
        outcome.dangling.page_labels_stale,
        "Acrobat leaves labels stale AND silent; pdfcer leaves them stale and says so"
    );
    assert!(!outcome.dangling.is_empty());
}

// ---------------------------------------------------------------------------
// Signature gating (§12.8)
// ---------------------------------------------------------------------------

/// A signed document. `perms` inserts the catalog `/Perms` entry that
/// Table 258 turns from detection into enforcement.
fn signed_doc(perms: bool, permission: &str) -> Vec<u8> {
    let catalog = if perms {
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [6 0 R] >> \
         /Perms << /DocMDP 7 0 R >> >>"
    } else {
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [6 0 R] >> >>"
    };
    let params = format!("<< /Type /TransformParams /P {permission} /V /1.2 >>");
    build(&[
        (1, catalog),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 \
             /MediaBox [0 0 10 10] /Resources << >> >>",
        ),
        (3, "<< /Type /Page /Parent 2 0 R >>"),
        (4, "<< /Type /Page /Parent 2 0 R >>"),
        (5, "<< /Type /Annot /Subtype /Widget /Rect [0 0 1 1] >>"),
        (6, "<< /FT /Sig /T (Signature1) /V 7 0 R >>"),
        (
            7,
            "<< /Type /Sig /ByteRange [0 100 200 300] /Reference [8 0 R] >>",
        ),
        (
            8,
            "<< /Type /SigRef /TransformMethod /DocMDP /TransformParams 9 0 R >>",
        ),
        (9, &params),
    ])
}

#[test]
fn an_enforced_certification_refuses_structural_edits_by_name() {
    // Table 258: "consumer applications SHALL enforce the permissions".
    // For an editor, enforcing means declining — not performing the edit
    // and warning afterwards.
    let source = signed_doc(true, "1");
    let mut s = session(&source);
    let err = s.delete_pages(&[0]).unwrap_err();
    assert!(matches!(
        err,
        EditError::CertificationForbidsChange { permission: 1 }
    ));
    assert!(matches!(
        s.reorder_pages(&[1, 0]).unwrap_err(),
        EditError::CertificationForbidsChange { .. }
    ));
    assert!(matches!(
        s.rotate_pages(&[0], 90).unwrap_err(),
        EditError::CertificationForbidsChange { .. }
    ));
    assert!(!s.is_modified(), "a refused edit changes nothing");
}

#[test]
fn a_certification_without_perms_is_detection_and_the_edit_proceeds() {
    // §12.8.1 makes /Perms → /DocMDP OPTIONAL, so its absence means the
    // author asked for detection, not prevention.
    let source = signed_doc(false, "2");
    let mut s = session(&source);
    let outcome = s.delete_pages(&[0]).unwrap();
    assert_eq!(outcome.pages_removed, 1);
    assert_eq!(outcome.signature, SignatureImpact::Invalidated);
}

#[test]
fn an_unsigned_document_reports_no_signature_impact() {
    let source = three_page_doc();
    let mut s = session(&source);
    let outcome = s.delete_pages(&[0]).unwrap();
    assert_eq!(outcome.signature, SignatureImpact::None);
    assert_eq!(
        s.signature_impact_of_save(SaveMode::FullRewrite),
        SignatureImpact::None
    );
}

#[test]
fn a_metadata_only_edit_on_a_signed_document_reports_byte_range_preserved() {
    // The one case that gets the stage-1 name — and even here the front
    // end must not render it alone as "still valid" (§12.8.2.2.2).
    let source = signed_doc(false, "2");
    let mut s = session(&source);
    s.set_info_field(pdfcer_core::edit::InfoField::Title, Some("New"))
        .unwrap();
    assert!(!s.changes_structure());
    assert_eq!(
        s.signature_impact_of_save(SaveMode::Incremental),
        SignatureImpact::ByteRangePreserved
    );
    // ...and a full rewrite fails stage 1 outright.
    assert_eq!(
        s.signature_impact_of_save(SaveMode::FullRewrite),
        SignatureImpact::Invalidated
    );
}

// ---------------------------------------------------------------------------
// Document producers
// ---------------------------------------------------------------------------

#[test]
fn extract_from_an_edited_session_sees_the_unsaved_edits() {
    // The reason `DocumentView` takes a graph rather than a Document:
    // extracting "page 2" must mean page 2 as the operator sees it, not
    // as the file was loaded.
    let source = three_page_doc();
    let mut s = session(&source);
    s.delete_pages(&[0]).unwrap();

    let graph = s.graph();
    let view = DocumentView::new(&graph, s.document().bytes(), s.document().version());
    let (bytes, report) = extract(&view, &[0]).unwrap();
    assert_eq!(report.pages, 1);

    let out = Document::from_bytes(bytes).unwrap();
    let pages = pdfcer_core::page_tree::pages(&out).unwrap();
    // Page index 0 of the EDITED document is the original page 2.
    let Some(Object::Stream(stream)) = out.value(pages[0].contents[0]) else {
        panic!("extracted page lost its content stream");
    };
    assert_eq!(stream.data_span.slice(out.bytes()).unwrap(), b"page2");
}

#[test]
fn merge_then_split_round_trips_the_page_count() {
    let a = Document::from_bytes(three_page_doc()).unwrap();
    let b = Document::from_bytes(three_page_doc()).unwrap();
    let (merged, report) = merge(
        &[
            DocumentView::new(&a, a.bytes(), a.version()),
            DocumentView::new(&b, b.bytes(), b.version()),
        ],
        &[],
    )
    .unwrap();
    assert_eq!(report.pages, 6);

    let m = Document::from_bytes(merged).unwrap();
    let view = DocumentView::new(&m, m.bytes(), m.version());
    let parts = split(&view, &SplitCriterion::EveryN(2), "{stem}_{n}.pdf", "m").unwrap();
    assert_eq!(parts.len(), 3);
    for (part, bytes, _) in parts {
        let out = Document::from_bytes(bytes).unwrap();
        assert_eq!(
            pdfcer_core::page_tree::pages(&out).unwrap().len(),
            part.page_count()
        );
    }
}

#[test]
fn insert_splices_a_source_document_into_a_target() {
    let target = Document::from_bytes(three_page_doc()).unwrap();
    let source = Document::from_bytes(nested_doc()).unwrap();
    let (bytes, report) = insert(
        &DocumentView::new(&target, target.bytes(), target.version()),
        &DocumentView::new(&source, source.bytes(), source.version()),
        &[0, 1],
        InsertPosition::After(0),
    )
    .unwrap();
    assert_eq!(report.pages, 5);

    let out = Document::from_bytes(bytes).unwrap();
    let pages = pdfcer_core::page_tree::pages(&out).unwrap();
    assert_eq!(pages.len(), 5);
    // The inserted pages carried their inherited 90° rotation across.
    assert_eq!(pages[1].rotate, 90);
    assert_eq!(pages[2].rotate, 90);
    assert_eq!(pages[0].rotate, 0);
}

#[test]
fn an_extracted_link_to_a_page_that_stayed_behind_is_reported_not_hidden() {
    // The barrier: the link survives, its destination does not, and the
    // count says so.
    let source = build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 \
             /MediaBox [0 0 10 10] /Resources << >> >>",
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Annots [5 0 R] >>"),
        (4, "<< /Type /Page /Parent 2 0 R >>"),
        (
            5,
            "<< /Type /Annot /Subtype /Link /Rect [0 0 1 1] /Dest [4 0 R /Fit] >>",
        ),
    ]);
    let doc = Document::from_bytes(source).unwrap();
    let view = DocumentView::new(&doc, doc.bytes(), doc.version());
    let (bytes, report) = extract(&view, &[0]).unwrap();
    assert!(
        report.dangling_references >= 1,
        "the broken destination must be counted, not silently dropped"
    );

    let out = Document::from_bytes(bytes).unwrap();
    let pages = pdfcer_core::page_tree::pages(&out).unwrap();
    let annots = out
        .resolved(pages[0].id)
        .as_dict()
        .unwrap()
        .get(b"Annots")
        .map(|o| out.resolve(o))
        .and_then(Object::as_array)
        .expect("the annotation itself must survive");
    let annot = out.resolve(&annots[0]).as_dict().unwrap();
    assert!(annot.contains_key(b"Rect"), "the link keeps its geometry");
    assert!(
        !annot.contains_key(b"Dest"),
        "and loses the destination that left the document"
    );
}

#[test]
fn producers_never_modify_the_source_document() {
    // Extract/merge/split/insert have no undo story precisely because
    // they change nothing — this is that claim, executable.
    let source = three_page_doc();
    let doc = Document::from_bytes(source.clone()).unwrap();
    let view = DocumentView::new(&doc, doc.bytes(), doc.version());
    let _ = extract(&view, &[0]).unwrap();
    let _ = split(&view, &SplitCriterion::EveryN(1), "{stem}_{n}.pdf", "s").unwrap();
    assert_eq!(doc.bytes(), source.as_slice());
}

// ---------------------------------------------------------------------------
// Guard headroom (the veraPDF §6.1.12 standing rule)
// ---------------------------------------------------------------------------

/// Measure how close real files come to Pass 3.2's new resource guards.
///
/// The standing rule exists because **two** guards have already been set
/// by intuition and both were wrong (`MAX_TOKEN_LEN`,
/// `MAX_XOBJECT_DEPTH`). A guard that fires on a conforming file does not
/// crash — it *degrades*, silently, which is the worst way to be wrong.
/// So the guards introduced here are measured against a corpus rather
/// than asserted to be generous.
///
/// `#[ignore]` because it needs a corpus this repository does not ship
/// (`docs/LEGAL.md` §5). Run it explicitly:
///
/// ```text
/// PDFCER_CORPUS="fixtures/external/veraPDF-corpus" \
///   cargo test -p pdfcer-core --test page_ops -- --ignored --nocapture
/// ```
///
/// It reports rather than asserts a threshold: the number that matters
/// is the *ratio* of observed maximum to guard, and a future reviewer
/// needs to see it, not be told it was fine once.
#[test]
#[ignore = "needs an external corpus; see the doc comment"]
fn measure_guard_headroom_against_a_corpus() {
    let Ok(root) = std::env::var("PDFCER_CORPUS") else {
        eprintln!("PDFCER_CORPUS not set — nothing to measure");
        return;
    };
    let mut files = 0usize;
    let mut max_outline = 0usize;
    let mut max_named_dests = 0usize;
    let mut max_pages = 0usize;
    let mut max_tree_depth = 0usize;

    let mut stack = vec![std::path::PathBuf::from(&root)];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "pdf") {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let Ok(doc) = Document::from_bytes(bytes) else {
                continue;
            };
            files += 1;

            let resolver = pdfcer_core::pageops::DestinationResolver::new(&doc);
            max_named_dests = max_named_dests.max(resolver.named_count());

            if let Ok(slots) = pdfcer_core::page_tree::page_slots(&doc) {
                max_pages = max_pages.max(slots.len());
                max_tree_depth = max_tree_depth.max(
                    slots
                        .iter()
                        .map(|slot| slot.ancestors.len())
                        .max()
                        .unwrap_or(0),
                );
            }

            // Count outline items the same way the census does.
            if let Some(outlines) = doc
                .catalog()
                .ok()
                .and_then(|catalog| catalog.get(b"Outlines").map(|o| doc.resolve(o)))
                .and_then(Object::as_dict)
            {
                let mut budget = usize::MAX;
                let mut visited = HashSet::new();
                let mut seen = 0usize;
                pdfcer_core::pageops::references::walk_outline(
                    &doc,
                    outlines.get(b"First").and_then(Object::as_reference),
                    0,
                    &mut budget,
                    &mut visited,
                    &mut |_| seen += 1,
                );
                max_outline = max_outline.max(seen);
            }
        }
    }

    println!("--- Pass 3.2 guard headroom over {files} corpus file(s) ---");
    println!(
        // string-gap-exempt: aligned guard-report column
        "outline items      observed max {max_outline:>8}  guard {:>8}",
        pdfcer_core::pageops::references::MAX_OUTLINE_ITEMS
    );
    println!(
        "named destinations observed max {max_named_dests:>8}  guard {:>8}",
        pdfcer_core::pageops::references::MAX_NAME_TREE_NODES
    );
    println!(
        // string-gap-exempt: aligned guard-report column
        "page-tree depth    observed max {max_tree_depth:>8}  guard {:>8}",
        pdfcer_core::page_tree::MAX_TREE_DEPTH
    );
    println!(
        // string-gap-exempt: aligned guard-report column
        "pages              observed max {max_pages:>8}  guard {:>8}",
        pdfcer_core::page_tree::MAX_PAGES
    );
    assert!(files > 0, "the corpus path yielded no loadable files");
}
