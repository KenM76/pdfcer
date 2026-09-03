//! Multi-target vector verbs — `delete_objects` / `move_objects` (Pass 47.0,
//! standing rule R168).
//!
//! ## What these tests are really checking
//!
//! R168: *a verb offered on an N-target selection acts on the whole
//! selection, or refuses with a stated reason — never a silent subset.*
//!
//! The defect that minted the rule was not a crash and not an error. The GUI
//! read `canvas_selection.iter().next()` and deleted **one of five**, leaving
//! the other four on the page and outlined as though they had gone. Nothing
//! failed. So the load-bearing assertion here is a **count**: N in, N gone —
//! because a verb that removes one object of five passes any test that only
//! asks "did a delete happen".
//!
//! Two properties beyond the count, each of which a looping implementation
//! would get wrong:
//!
//! 1. **ONE undo entry.** A loop over the single-object verb would give N,
//!    and a half-undone multi-delete is a document state the operator never
//!    chose. Asserted by undoing once and checking everything is back.
//! 2. **Indices resolve against ONE decomposition.** Splicing per object
//!    shifts every later byte span, so a loop would cut the wrong bytes from
//!    the second object onward. Asserted by deleting a non-adjacent set and
//!    checking the survivors are the ones that should survive — a test that
//!    passes trivially for adjacent indices and is the whole point for
//!    scattered ones.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::vector::{self, Matrix};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(name)
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(rel)).expect("load fixture"))
}

/// How many objects page 0 decomposes to, read through the same path the GUI's
/// provider uses.
fn object_count(s: &EditSession) -> usize {
    page_objects(s).len()
}

/// The page-space bounding boxes of page 0's objects, in paint order — the
/// identity we track across an edit. Bounds rather than indices, because
/// indices renumber when an object is removed and bounds do not.
fn page_objects(s: &EditSession) -> Vec<(f64, f64)> {
    let pages = s.pages().expect("pages");
    let page = pages.first().expect("one page");
    let view = s.view();
    let model = vector::decompose_page(&view, page, Matrix::IDENTITY).expect("decompose");
    model
        .objects
        .iter()
        .map(|o| {
            let b = o.page_bbox();
            (b.min.x, b.min.y)
        })
        .collect()
}

/// THE HEADLINE: N selected, N deleted. The count is the assertion.
#[test]
fn deleting_several_objects_removes_every_one_of_them() {
    let mut s = session("vector/paths.pdf");
    let before = object_count(&s);
    assert!(
        before >= 4,
        "precondition: the fixture needs enough objects for 'several' to \
         mean something — has {before}"
    );

    s.delete_objects(0, &[0, 1, 2]).expect("delete three");
    assert_eq!(
        object_count(&s),
        before - 3,
        "three were selected and three must be gone — the defect this Pass \
         exists for removed ONE and reported success"
    );
}

/// Non-adjacent indices resolve against ONE decomposition.
///
/// A loop over the single-object verb would splice after the first deletion,
/// shifting every later byte span, and cut the wrong bytes from the second
/// object onward. Adjacent indices can hide that; scattered ones cannot.
#[test]
fn scattered_indices_delete_the_objects_that_were_named() {
    let mut s = session("vector/paths.pdf");
    let before = page_objects(&s);
    assert!(before.len() >= 4);
    let n = before.len();

    // First and last — maximally far apart in the byte stream.
    s.delete_objects(0, &[0, n - 1]).expect("delete ends");

    let after = page_objects(&s);
    assert_eq!(after.len(), n - 2);
    // The survivors are exactly the middle ones, in order.
    let expected: Vec<(f64, f64)> = before[1..n - 1].to_vec();
    assert_eq!(
        after, expected,
        "the objects that survived must be the ones that were NOT named — a \
         stale-index splice would have cut into a neighbour instead"
    );
}

/// One gesture, ONE undo entry. A loop would give N.
#[test]
fn a_multi_delete_is_a_single_undoable_command() {
    let mut s = session("vector/paths.pdf");
    let before = page_objects(&s);
    let depth_before = s.can_undo();
    assert!(
        !depth_before,
        "precondition: a fresh session has nothing to undo"
    );

    s.delete_objects(0, &[0, 1, 2]).expect("delete three");
    assert!(s.can_undo());

    s.undo().expect("one undo");
    assert_eq!(
        page_objects(&s),
        before,
        "ONE undo restores ALL THREE — three commands would have left two \
         objects still deleted, a state the operator never chose"
    );
    assert!(
        !s.can_undo(),
        "and there is nothing left to undo: the multi-delete was one command, \
         not three"
    );
}

/// An out-of-range index refuses the WHOLE call — it does not delete the
/// valid remainder.
///
/// Deleting the part that happened to resolve is the silent-subset behaviour
/// R168 exists to end, arriving from the other direction: the caller's
/// selection disagrees with the document, and acting on the agreeing half
/// hides that.
#[test]
fn one_bad_index_refuses_the_whole_delete() {
    let mut s = session("vector/paths.pdf");
    let before = page_objects(&s);
    let n = before.len();

    let err = s
        .delete_objects(0, &[0, n + 99])
        .expect_err("an out-of-range index must refuse");
    assert!(
        err.to_string().contains("out of range"),
        "the refusal names the condition: {err}"
    );
    assert_eq!(
        page_objects(&s),
        before,
        "and NOTHING was deleted — not even the index that was valid"
    );
}

/// A duplicate index is harmless — the planner de-duplicates spans.
///
/// Worth a test because a naive implementation splices the same byte range
/// twice and produces a torn stream, and because a GUI selection round-trip
/// can legitimately produce one.
#[test]
fn a_duplicated_index_deletes_that_object_once() {
    let mut s = session("vector/paths.pdf");
    let before = object_count(&s);
    s.delete_objects(0, &[1, 1, 1])
        .expect("duplicates are fine");
    assert_eq!(object_count(&s), before - 1);
}

/// An empty selection is a no-op, not an error — a caller need not
/// special-case it.
#[test]
fn deleting_nothing_is_allowed_and_changes_nothing() {
    let mut s = session("vector/paths.pdf");
    let before = page_objects(&s);
    s.delete_objects(0, &[]).expect("empty is not an error");
    assert_eq!(page_objects(&s), before);
}

/// THE MOVE HEADLINE: every selected object travels, by the same delta.
#[test]
fn moving_several_objects_moves_all_of_them_by_the_same_delta() {
    let mut s = session("vector/paths.pdf");
    let before = page_objects(&s);
    assert!(before.len() >= 3);

    let (dx, dy) = (25.0, 40.0);
    s.move_objects(0, &[0, 1, 2], dx, dy).expect("move three");

    let after = page_objects(&s);
    assert_eq!(after.len(), before.len(), "a move deletes nothing");
    for i in 0..3 {
        let (bx, by) = before[i];
        let (ax, ay) = after[i];
        assert!(
            (ax - (bx + dx)).abs() < 0.01 && (ay - (by + dy)).abs() < 0.01,
            "object {i} must have moved by exactly ({dx}, {dy}): \
             ({bx}, {by}) -> ({ax}, {ay})"
        );
    }
    // And an object that was NOT selected stayed put — the counter-assertion
    // that makes the three above mean something.
    if before.len() > 3 {
        assert_eq!(
            after[3], before[3],
            "an unselected object must not move — otherwise the test above \
             would pass for a verb that moved the whole page"
        );
    }
}

/// A multi-move is one undoable command, same as the delete.
#[test]
fn a_multi_move_is_a_single_undoable_command() {
    let mut s = session("vector/paths.pdf");
    let before = page_objects(&s);
    s.move_objects(0, &[0, 1, 2], 15.0, -10.0).expect("move");
    assert_ne!(page_objects(&s), before, "precondition: something moved");

    s.undo().expect("one undo");
    assert_eq!(page_objects(&s), before, "ONE undo puts all three back");
    assert!(!s.can_undo());
}

/// Moving nothing is a no-op, matching the delete's empty case.
#[test]
fn moving_nothing_is_allowed_and_changes_nothing() {
    let mut s = session("vector/paths.pdf");
    let before = page_objects(&s);
    s.move_objects(0, &[], 10.0, 10.0)
        .expect("empty is not an error");
    assert_eq!(page_objects(&s), before);
}
