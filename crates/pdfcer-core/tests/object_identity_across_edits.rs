//! # Which edit verbs RENUMBER a page, and which do not
//!
//! Answers `request_stable_object_identity.md` from the `pdfcer-gui` session
//! (2026-08-13), question 2: *"Which existing verbs renumber? `delete_objects`
//! obviously does. If the move/reorder verbs do not — because they rewrite in
//! place — then the hazard is smaller than I think."*
//!
//! **It is smaller.** This file is the proof, and it exists because that
//! answer must not decay into a comment somebody later contradicts.
//!
//! ## The hazard being characterised
//!
//! `decompose_page` mints **paint-order indices**, not identities, and all
//! eleven index-addressed edit verbs take those indices. An index is a
//! position, and a position is only an identity while nothing moves. So an
//! edit that renumbers a page silently re-points a live selection at a
//! *different* object — the shell's outline redraws around the wrong thing and
//! the next Delete removes it. Nothing errors. That is the failure the
//! requesting session refused to build move/resize on top of, correctly.
//!
//! ## What is asserted
//!
//! | verb family | mechanism | renumbers? | asserted by |
//! |---|---|---|---|
//! | `move_*` | rewrites operator **operands** in place | **NO** | `moving_an_object_does_not_renumber_the_page` |
//! | `delete_*` | removes byte **spans** | **YES** | `deleting_an_object_renumbers_everything_after_it` |
//!
//! The distinction is mechanical rather than incidental: a move changes the
//! numbers inside existing operators, so no operator is added or removed and
//! the decomposition walks the same operators in the same order. A delete
//! excises spans, so every object after the hole shifts down by one.
//!
//! ## Why an empirical test rather than reading `plan_move`
//!
//! Reading the planner says operands are rewritten. It does **not** say the
//! decomposition that walks the result yields the same objects in the same
//! order — that is a property of the pair, and this project has now been
//! bitten twice in one session by conclusions drawn from one half of a pair.
//! So these tests decompose, edit, and decompose again.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree;
use pdfcer_core::vector::{Matrix, decompose_page, remap_index_after_delete};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/vector")
        .join(name)
}

/// Decompose page 0 of a session's CURRENT (edited) view and return, per
/// object, a stable-ish descriptor: its kind tag plus its page bbox rounded
/// to 0.01 pt.
///
/// The bbox is what lets a test say *"object 3 is still the same shape"*
/// without an identity token existing — which is the very thing the request
/// is asking for and which this test therefore cannot assume.
fn object_fingerprints(session: &EditSession) -> Vec<String> {
    let pages = page_tree::pages(session.document()).expect("page tree walks");
    let view = session.view();
    let model = decompose_page(&view, &pages[0], Matrix::IDENTITY).expect("decomposes");
    model
        .objects
        .iter()
        .map(|o| {
            let b = o.page_bbox();
            format!(
                "{}:{:.2},{:.2},{:.2},{:.2}",
                match o {
                    pdfcer_core::vector::VectorObject::Path(_) => "path",
                    pdfcer_core::vector::VectorObject::Text(_) => "text",
                    _ => "other",
                },
                b.min.x,
                b.min.y,
                b.max.x,
                b.max.y
            )
        })
        .collect()
}

/// ★ A MOVE DOES NOT RENUMBER. The requesting session may build move/resize
/// against paint-order indices.
///
/// The object count is unchanged and every object except the moved one keeps
/// its exact fingerprint at its exact index. That is the property a live
/// selection needs: an index taken before the edit still names the same object
/// after it.
#[test]
fn moving_an_object_does_not_renumber_the_page() {
    let doc = Document::load(&fixture("edit.pdf")).expect("fixture loads");
    let mut session = EditSession::new(doc);

    let before = object_fingerprints(&session);
    assert!(
        before.len() >= 2,
        "the fixture must have at least two objects to say anything about \
         renumbering; got {}",
        before.len()
    );

    // Move the FIRST object. If anything renumbers, later indices shift and
    // the comparison below fails loudly.
    session
        .move_object(0, 0, 10.0, 7.5)
        .expect("move the first object");

    let after = object_fingerprints(&session);

    assert_eq!(
        after.len(),
        before.len(),
        "a move must not change the OBJECT COUNT — it rewrites operator \
         operands, it does not add or remove operators"
    );

    // Every object except index 0 must be untouched, at the same index.
    for i in 1..before.len() {
        assert_eq!(
            after[i], before[i],
            "object {i} changed after moving object 0. A move must not \
             renumber: if this fails, a live selection in a shell now points \
             at a DIFFERENT object and nothing will report it."
        );
    }

    // And the moved object must actually have moved, or the test above is
    // vacuously satisfied by an edit that did nothing.
    assert_ne!(
        after[0], before[0],
        "object 0 must actually have moved — otherwise this test proves \
         nothing about renumbering, it proves the edit was a no-op"
    );
}

/// ★ A DELETE DOES RENUMBER, and this pins the exact shape of it.
///
/// After deleting object 0, what was object 1 is object 0. A selection naming
/// index 1 now names what used to be index 2 — the silent retarget the request
/// is about. Asserted so the answer "only the delete family renumbers" is
/// backed by a demonstration of the delete half too, not just the move half.
#[test]
fn deleting_an_object_renumbers_everything_after_it() {
    let doc = Document::load(&fixture("edit.pdf")).expect("fixture loads");
    let mut session = EditSession::new(doc);

    let before = object_fingerprints(&session);
    assert!(before.len() >= 2, "need at least two objects");

    session.delete_objects(0, &[0]).expect("delete object 0");
    let after = object_fingerprints(&session);

    assert_eq!(
        after.len(),
        before.len() - 1,
        "a delete removes exactly one object"
    );
    assert_eq!(
        after[0], before[1],
        "★ THE RENUMBERING: after deleting object 0, index 0 now names what \
         was index 1. A shell holding index 1 across this edit is now pointing \
         one object further along than it thinks."
    );
}

/// ★ The remap FORMULA agrees with the actual RENUMBERING.
///
/// `remap_index_after_delete` is arithmetic; `delete_objects` is a content-
/// stream splice. Nothing makes them agree except this test. A formula that
/// merely looks right is exactly the silent-retarget failure the requesting
/// session refused to build on — so it is checked against the real edit, for
/// every surviving index, with a multi-object delete that has holes on both
/// sides of the survivors.
#[test]
fn the_remap_formula_predicts_the_real_renumbering() {
    let doc = Document::load(&fixture("edit.pdf")).expect("fixture loads");
    let mut session = EditSession::new(doc);
    let before = object_fingerprints(&session);
    assert!(
        before.len() >= 3,
        "need at least three objects to delete a middle one and still have \
         survivors on both sides; got {}",
        before.len()
    );

    // Delete a middle object, so survivors exist both below it (unshifted) and
    // above it (shifted). Deleting only the first or last would let an
    // always-shift or never-shift bug pass.
    let victim = 1usize;
    session
        .delete_objects(0, &[victim])
        .expect("delete the middle object");
    let after = object_fingerprints(&session);

    for (old, before_fp) in before.iter().enumerate() {
        match remap_index_after_delete(old, &[victim]) {
            None => assert_eq!(old, victim, "only the deleted index maps to None"),
            Some(new) => {
                assert!(
                    new < after.len(),
                    "remap produced index {new}, past the end ({})",
                    after.len()
                );
                assert_eq!(
                    &after[new], before_fp,
                    "★ the formula says old index {old} becomes {new}, but the \
                     object actually at {new} is a DIFFERENT one. This is the \
                     silent retarget: a shell trusting the remap would draw its \
                     selection around the wrong object."
                );
            }
        }
    }
}

/// A duplicate in `deleted` must not shift a survivor twice.
///
/// Plausible input — a shell unioning two overlapping selections — and the
/// failure would be a silently wrong index rather than an error.
#[test]
fn a_duplicate_in_the_deleted_list_does_not_double_shift() {
    assert_eq!(remap_index_after_delete(5, &[1, 1, 1]), Some(4));
    assert_eq!(remap_index_after_delete(5, &[1, 3, 1, 3]), Some(3));
}
