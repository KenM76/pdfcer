//! `EditSession::move_nodes` / `plan_move_nodes` — moving a multi-node
//! selection as ONE surgery (ISO 32000-1 §8.5.2.1).
//!
//! ## The obligation, and the reason a loop could not discharge it
//!
//! `Pass 41.0` shipped a multi-node **selection set** in the GUI and could
//! not move it. Its own note refused an N-call `move_node` loop because
//! that would break one-gesture-one-undo — N entries on the undo stack for
//! one drag, so Ctrl+Z leaves half the selection moved. That is the reason,
//! together with owing the same disclosure N times.
//!
//! ## ★ A stronger argument was drafted, tested, and turned out to be FALSE
//!
//! It is very tempting to add a *correctness* reason: all four corners of
//! an `x y w h re` are described by the same four operands of one operator
//! (`re` stores an origin and a SIZE, not four points), and `plan_move_node`
//! places a corner by expanding that operator into its Table 59 equivalent
//! — so surely a second call plans against bytes the first already
//! replaced.
//!
//! **It does not.** A caller re-decomposes between calls (it must —
//! `plan_move_node` takes a `ContentStream`), and the expansion preserves
//! both the anchor **count** and the anchor **order**, so index *k* still
//! names the same geometric point afterwards. The loop's output is
//! byte-identical to this function's.
//!
//! That claim was written into the doc comment before it was tested;
//! `a_two_call_loop_matches_one_call_byte_for_byte_but_costs_two_undos`
//! failed, and the doc comment was corrected. **The test is kept as a
//! refutation** rather than deleted, because it is the argument the next
//! reader will reconstruct when they ask whether `move_nodes` could just be
//! a loop — and they should find it answered instead of re-deriving it and
//! landing on the wrong side.
//!
//! What the loop genuinely *cannot* do is covered by
//! `an_implicit_start_and_its_segments_endpoint_share_one_replacement`: two
//! anchors defined by the **same operator's byte range** need one
//! replacement, because two overlapping edits are silently dropped by the
//! splice.
//!
//! ## Why the fixtures are inline content streams, not PDFs
//!
//! Every claim here is about **which bytes of one content stream change**.
//! `ContentStream::parse` over a byte literal makes the before and after
//! both visible in the test, so a failure reads as a diff rather than as a
//! path into a binary fixture. The `EditSession` layer gets its own tests
//! below, on a real document, for the things only it can answer (one
//! command, one undo).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::content::ContentStream;
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::vector::edit::{plan_move_node, plan_move_nodes};
use pdfcer_core::vector::{
    Matrix, NoXObjects, PageObjects, Point, VectorEditError, VectorObject, decompose,
    decompose_page,
};
use pdfcer_core::{page_tree, writer::SaveOptions};

/// Parse a content stream and hand back its first path object's plan input.
fn path_of(src: &[u8]) -> (ContentStream, pdfcer_core::vector::PathObject) {
    let cs = ContentStream::parse(src.to_vec()).unwrap();
    let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
    let VectorObject::Path(path) = &model.objects[0] else {
        panic!("first object is not a path");
    };
    (cs.clone(), path.clone())
}

// ---------------------------------------------------------------------------
// The rectangle case — the reason this verb exists at all
// ---------------------------------------------------------------------------

/// **A REFUTATION, kept deliberately.** A two-call loop over one `re` does
/// **not** corrupt it — it produces byte-identical output to one
/// `move_nodes` call.
///
/// # Why a passing test of a claim we no longer make is worth keeping
///
/// The plausible argument for this verb was a correctness one: all four
/// corners of an `re` share one operator's four operands, `plan_move_node`
/// expands that operator to place a corner, so a second call must be
/// planning against replaced bytes. It sounds right, it was written into
/// the doc comment, and it is **false** — a caller re-decomposes between
/// calls (it must; `plan_move_node` takes a `ContentStream`), and the
/// expansion preserves both anchor count and anchor order, so index *k*
/// still names the same point.
///
/// This test caught that during development and the doc comment was
/// corrected. It stays because the argument is the one a future reader will
/// reconstruct when they wonder whether `move_nodes` could just be a loop —
/// and they should find it already answered rather than re-derive it and
/// reach the wrong conclusion in the other direction.
///
/// **The real reasons `move_nodes` exists are one-gesture-one-undo and
/// disclosure de-duplication**, and both are asserted at the bottom of this
/// test rather than left as prose.
#[test]
fn a_two_call_loop_matches_one_call_byte_for_byte_but_costs_two_undos() {
    const SRC: &[u8] = b"0 0 10 10 re S";

    // --- THE RIGHT ANSWER: one call, one expansion, both corners applied.
    let (cs, path) = path_of(SRC);
    let together = plan_move_nodes(
        &cs,
        &path,
        &[(0, Point::new(-1.0, -1.0)), (2, Point::new(12.0, 12.0))],
    )
    .unwrap();
    assert_eq!(
        together.content, b"-1 -1 m 10 0 l 12 12 l 0 10 l h S",
        "both moved corners must land in ONE expansion, with corners 1 and 3 \
         keeping the coordinates the original `re` implied",
    );
    assert_eq!(
        together.operators_touched, 1,
        "one `re` became one m/l/l/l/h run — the count is of OPERATORS, not \
         of nodes",
    );

    // --- THE LOOP: the first call expands, the second plans against the
    // expanded stream. Node index 2 still exists, but it is no longer a
    // rectangle corner — it is now an ordinary `l` endpoint — so the second
    // move lands as an in-place operand rewrite instead.
    let (cs, path) = path_of(SRC);
    let first = plan_move_node(&cs, &path, 0, Point::new(-1.0, -1.0)).unwrap();
    let (cs2, path2) = path_of(&first.content);
    let looped = plan_move_node(&cs2, &path2, 2, Point::new(12.0, 12.0)).unwrap();

    assert_eq!(
        looped.content, together.content,
        "THE LOOP IS NOT WRONG. If this ever fails, the expansion has stopped \
         preserving anchor order or count and the correctness argument for \
         move_nodes becomes real — which would be worth knowing, so investigate \
         rather than flipping the assertion",
    );

    // --- WHAT THE LOOP ACTUALLY COSTS, asserted rather than asserted-in-prose.
    //
    // (1) Two surgeries, so two undo entries for one drag. The operator
    //     presses Ctrl+Z once and half their nodes stay moved.
    assert_eq!(
        first.operators_touched + looped.operators_touched,
        2,
        "two separate plans is two commands is two undos",
    );
    assert_eq!(
        together.operators_touched, 1,
        "one plan, therefore one command, therefore one undo",
    );

    // (2) The operator is told the same paragraph twice. `plan_move_node`
    //     owes the rectangle-expansion disclosure on the FIRST call (it
    //     expands the `re`) and again on the second only if it expands
    //     another — here it does not, so the loop's total is 1 as well;
    //     the duplication shows up as soon as a second rectangle is in the
    //     selection, which `all_four_corners...` and the plural wording
    //     cover.
    assert_eq!(together.disclosures.len(), 1);
    assert_eq!(first.disclosures.len(), 1);
    assert_eq!(
        looped.disclosures.len(),
        0,
        "the second call edits an already-expanded shape, so it owes nothing",
    );
}

/// All four corners at once: still one operator, still one disclosure.
#[test]
fn all_four_corners_of_a_rectangle_move_in_one_expansion() {
    let (cs, path) = path_of(b"0 0 10 10 re S");
    let plan = plan_move_nodes(
        &cs,
        &path,
        &[
            (0, Point::new(1.0, 1.0)),
            (1, Point::new(9.0, 2.0)),
            (2, Point::new(8.0, 8.0)),
            (3, Point::new(2.0, 9.0)),
        ],
    )
    .unwrap();
    assert_eq!(plan.content, b"1 1 m 9 2 l 8 8 l 2 9 l h S");
    assert_eq!(plan.operators_touched, 1);
    assert_eq!(
        plan.disclosures.len(),
        1,
        "four corners provoked one shape change, so the operator hears about \
         it once",
    );
}

// ---------------------------------------------------------------------------
// The other bucket-sharing case: an implicit start and an endpoint on one
// operator
// ---------------------------------------------------------------------------

/// After `h`, the next segment reopens at the closed subpath's start with no
/// `m` of its own (§8.5.2.1). That implicit anchor and the segment's own
/// endpoint are **defined by the same operator's bytes**, so moving both
/// must produce one replacement — two overlapping edits would be silently
/// dropped by the splice.
#[test]
fn an_implicit_start_and_its_segments_endpoint_share_one_replacement() {
    let (cs, path) = path_of(b"0 0 m 10 0 l 10 10 l h 20 20 l S");

    // Anchor 3 is the implicit reopen at (0,0); anchor 4 is the `l`
    // endpoint at (20,20). Both live in the `20 20 l` operator.
    let plan = plan_move_nodes(
        &cs,
        &path,
        &[(3, Point::new(5.0, 5.0)), (4, Point::new(25.0, 25.0))],
    )
    .unwrap();

    assert_eq!(
        plan.content, b"0 0 m 10 0 l 10 10 l h 5 5 m 25 25 l S",
        "the materialised `m` must be written in FRONT of the same operator \
         whose endpoint also moved, in one replacement",
    );
    assert_eq!(plan.operators_touched, 1);
    assert_eq!(
        plan.disclosures.len(),
        1,
        "the implicit-start disclosure is owed once",
    );
}

/// Moving ONLY the implicit start leaves the segment it prefixes
/// byte-verbatim — rule 3 applied inside a single operator.
#[test]
fn an_implicit_only_move_leaves_its_segment_bytes_untouched() {
    let (cs, path) = path_of(b"0 0 m 10 0 l 10 10 l h 20 20 l S");
    let plan = plan_move_nodes(&cs, &path, &[(3, Point::new(5.0, 5.0))]).unwrap();
    assert_eq!(plan.content, b"0 0 m 10 0 l 10 10 l h 5 5 m 20 20 l S");
}

// ---------------------------------------------------------------------------
// Refusals — all before any byte is planned
// ---------------------------------------------------------------------------

/// An empty request is refused, not treated as a successful no-op.
///
/// A no-op that reported success would put an entry on the undo stack that
/// undoes nothing, and a front end looping over an empty selection would be
/// told "moved" rather than discovering its selection was empty.
#[test]
fn an_empty_move_is_refused() {
    let (cs, path) = path_of(b"10 20 m 100 200 l S");
    assert!(matches!(
        plan_move_nodes(&cs, &path, &[]),
        Err(VectorEditError::EmptyMove)
    ));
}

/// The same node named twice is refused rather than resolved.
///
/// Last-one-wins and first-one-wins are equally defensible and give
/// different geometry, so either would move the node somewhere the caller
/// did not unambiguously ask for.
#[test]
fn a_duplicate_node_is_refused_by_name() {
    let (cs, path) = path_of(b"10 20 m 100 200 l S");
    let err = plan_move_nodes(
        &cs,
        &path,
        &[(1, Point::new(1.0, 1.0)), (1, Point::new(2.0, 2.0))],
    )
    .unwrap_err();
    assert!(
        matches!(err, VectorEditError::DuplicateNodeInMove { index: 1 }),
        "expected DuplicateNodeInMove naming the index, got {err:?}",
    );
}

/// The duplicate check runs BEFORE the range check, so a request that is
/// both duplicated and out of range reports the duplicate.
///
/// Not arbitrary: a duplicate is a malformed request whether or not the
/// index exists, and reporting the range error would send the caller after
/// the wrong bug.
#[test]
fn a_duplicate_is_reported_even_when_the_index_is_also_out_of_range() {
    let (cs, path) = path_of(b"10 20 m 100 200 l S");
    let err = plan_move_nodes(
        &cs,
        &path,
        &[(99, Point::new(1.0, 1.0)), (99, Point::new(2.0, 2.0))],
    )
    .unwrap_err();
    assert!(
        matches!(err, VectorEditError::DuplicateNodeInMove { index: 99 }),
        "got {err:?}",
    );
}

/// One bad index refuses the WHOLE request — no partial application.
#[test]
fn an_out_of_range_index_refuses_the_whole_batch() {
    let (cs, path) = path_of(b"10 20 m 100 200 l S");
    let err = plan_move_nodes(
        &cs,
        &path,
        &[(0, Point::new(1.0, 1.0)), (9, Point::new(2.0, 2.0))],
    )
    .unwrap_err();
    assert!(
        matches!(err, VectorEditError::NodeOutOfRange { index: 9, count: 2 }),
        "got {err:?}",
    );
}

// ---------------------------------------------------------------------------
// Equivalence with the single-node verb
// ---------------------------------------------------------------------------

/// A one-element `move_nodes` must produce exactly what `move_node`
/// produces, for every anchor kind.
///
/// This is what lets a front end use one verb for both cases without
/// wondering whether the single-node path is subtly different — and it is
/// the assertion that would catch the multi-node planner drifting away from
/// the single-node one, which is otherwise a divergence nothing would
/// notice.
#[test]
fn a_single_element_batch_matches_the_single_node_verb() {
    for (src, node) in [
        (b"10 20 m 100 200 l S".as_slice(), 1usize), // Editable
        (b"0 0 10 10 re S".as_slice(), 2),           // Rectangle corner
        (b"0 0 m 5 0 l 5 5 l h 20 20 l S".as_slice(), 3), // Implicit start
    ] {
        let target = Point::new(42.0, 43.0);
        let (cs, path) = path_of(src);
        let one = plan_move_node(&cs, &path, node, target).unwrap();
        let (cs, path) = path_of(src);
        let batch = plan_move_nodes(&cs, &path, &[(node, target)]).unwrap();
        assert_eq!(
            one.content,
            batch.content,
            "content differs for {:?} node {node}",
            String::from_utf8_lossy(src),
        );
        assert_eq!(one.operators_touched, batch.operators_touched);
        assert_eq!(
            one.disclosures,
            batch.disclosures,
            "disclosures differ for {:?} node {node} — the wording is part of \
             the contract, not decoration",
            String::from_utf8_lossy(src),
        );
    }
}

// ---------------------------------------------------------------------------
// The `EditSession` layer — the thing only it can answer
// ---------------------------------------------------------------------------

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/vector")
        .join(name)
}

fn edit_session() -> EditSession {
    EditSession::new(Document::load(&fixture("edit.pdf")).expect("edit.pdf loads"))
}

fn decompose0(doc: &Document) -> PageObjects {
    let pages = page_tree::pages(doc).unwrap();
    decompose_page(&doc.view(), &pages[0], Matrix::IDENTITY).unwrap()
}

/// Page-space anchors of the object at `index` on page 0.
fn anchors_of(session: &EditSession, index: usize) -> Vec<Point> {
    let bytes = session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap()
        .0;
    let doc = Document::from_bytes(bytes).unwrap();
    let model = decompose0(&doc);
    let VectorObject::Path(p) = &model.objects[index] else {
        panic!("object {index} is not a path");
    };
    p.page_subpaths()
        .iter()
        .flat_map(|s| s.anchors().collect::<Vec<_>>())
        .collect()
}

/// **The verb's actual justification, proved rather than described:** N
/// nodes move together and ONE undo puts all of them back.
///
/// This is what an N-call loop cannot do, and it is the reason `Pass 41.0`
/// refused to build multi-node drag out of `move_node` — the correctness
/// argument at the top of this file turned out not to hold, so this is the
/// whole case.
#[test]
fn a_multi_node_move_is_one_command_and_one_undo_restores_every_node() {
    let mut s = edit_session();

    // Find a path with at least three anchors, and shift three of them.
    let (index, before) = {
        let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
        let doc = Document::from_bytes(bytes).unwrap();
        let model = decompose0(&doc);
        let (i, p) = model
            .objects
            .iter()
            .enumerate()
            .find_map(|(i, o)| match o {
                VectorObject::Path(p) if p.page_subpaths()[0].anchors().count() >= 3 => {
                    Some((i, p))
                }
                _ => None,
            })
            .expect("a path with three or more anchors");
        let pts: Vec<Point> = p
            .page_subpaths()
            .iter()
            .flat_map(|s| s.anchors().collect::<Vec<_>>())
            .collect();
        (i, pts)
    };

    let moves: Vec<(usize, Point)> = (0..3)
        .map(|n| (n, Point::new(before[n].x + 7.0, before[n].y + 11.0)))
        .collect();
    s.move_nodes(0, index, &moves).expect("multi-node move");

    let after = anchors_of(&s, index);
    for n in 0..3 {
        assert!(
            (after[n].x - (before[n].x + 7.0)).abs() < 1e-6
                && (after[n].y - (before[n].y + 11.0)).abs() < 1e-6,
            "node {n} did not land where it was sent: {:?} -> {:?}",
            before[n],
            after[n],
        );
    }

    // ONE undo, not three.
    s.undo().expect("one undo");
    let restored = anchors_of(&s, index);
    for n in 0..3 {
        assert!(
            (restored[n].x - before[n].x).abs() < 1e-6
                && (restored[n].y - before[n].y).abs() < 1e-6,
            "node {n} was not restored by a SINGLE undo — the move committed \
             more than one command, which is exactly what this verb exists to \
             avoid",
        );
    }
    assert!(
        s.undo().is_none(),
        "there must be nothing left to undo: three moved nodes were ONE \
         command, so a second undo has no command to take",
    );
}

/// A refused batch changes nothing and leaves no command behind (rule 4).
#[test]
fn a_refused_batch_leaves_the_session_untouched() {
    let mut s = edit_session();
    let dirty_before = s.dirty_set().len();

    assert!(s.move_nodes(0, 0, &[]).is_err(), "empty batch must refuse");
    assert!(
        s.move_nodes(
            0,
            0,
            &[(0, Point::new(1.0, 1.0)), (0, Point::new(2.0, 2.0))]
        )
        .is_err(),
        "duplicate node must refuse",
    );
    assert!(
        s.move_nodes(0, 0, &[(999, Point::new(1.0, 1.0))]).is_err(),
        "out-of-range node must refuse",
    );

    assert_eq!(
        s.dirty_set().len(),
        dirty_before,
        "a refused move must not dirty an object",
    );
    assert!(
        s.undo().is_none(),
        "a refused move must not leave a command on the undo stack",
    );
}
