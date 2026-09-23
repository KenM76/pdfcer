//! `plan_move_text_run` / `EditSession::move_text_run` — moving ONE show
//! operator inside a text object (`G017`, ISO 32000-1 §9.4).
//!
//! ## The gap this closes
//!
//! Every other part kind in the crate had both halves. A subpath could be
//! moved and deleted; an anchor could be moved and deleted; a text run could
//! only be deleted. `pdfcer-gui` filed it after resolving a drag at the Part
//! rung on a `VectorObject::Text`, finding no verb to call, and declining the
//! gesture.
//!
//! The operator's words, their `OPERATOR_REQUESTS.md` O188: *"In text that is
//! grouped together or whatever it is called, such as in my title blocks, I
//! would like a way to move the individual text blocks within it around."*
//! One `BT`…`ET` on a SolidWorks title block holds every string in it — the
//! sibling case measures at 237 dimension labels in one text object — so the
//! title block was the text he most wanted to nudge and the one thing on the
//! page that could not be nudged.
//!
//! ## What is hard about it, and therefore what these tests are for
//!
//! A path operator's operands are in user space, so `plan_move_subpath`
//! converts a drag with **one** matrix and rewrites the numbers. Text has a
//! second space in the way (§9.4.4: `Trm = params × Tm × CTM`), and the
//! operator that has to be rewritten is not always the same one — or present
//! at all:
//!
//! | how the run is placed | what the move does | fixture |
//! |---|---|---|
//! | `Tm` | rewrite `e`/`f`, **user** space | `runs-two-explicit.pdf` |
//! | `Td` | rewrite both operands, **text** space | `runs-td-relative.pdf` |
//! | `TD` / `T*` / none | **insert** a `Td`, and disclose it | `runs-tstar-leading.pdf` |
//! | a rotated `Tm` | both inverses, or it moves the wrong way | `runs-rotated-td.pdf` |
//! | inherited | **refused** — there is no coordinate | `runs-inherited.pdf` |
//!
//! ## Why the fixtures are real PDFs and not inline content streams
//!
//! A bare `ContentStream::parse` has no resource dictionary, so `/F1 10 Tf`
//! resolves to nothing, no run lays out, and `TextObject::runs` comes back
//! **empty** — every assertion below would pass against a verb that did
//! nothing at all. The same constraint `text_run_delete.rs` records, and the
//! same answer: real (standard-14, non-embedded) fonts, and a run count
//! asserted before anything else.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::content::ContentStream;
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree;
use pdfcer_core::vector::edit::{plan_move_text_run, text_run_move_refusal};
use pdfcer_core::vector::{Bounds, Matrix, PlannedEdit, TextObject, VectorObject, decompose_page};
use pdfcer_core::writer::SaveOptions;

/// How close two page-space coordinates must be to count as the same place.
///
/// A move is arithmetic on decimal operands that are re-emitted as text, so
/// the round trip through `emit_number` is not bit-exact. One thousandth of a
/// point is roughly a ten-thousandth of a pixel at 100 % zoom — far below
/// anything a renderer or an operator can resolve, and far above the rounding.
const EPS: f64 = 1e-3;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/text")
        .join(name)
}

fn bytes_of(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name))
        .unwrap_or_else(|e| panic!("missing fixture {}: {e}", fixture(name).display()))
}

/// The first text object on page 0, with the content stream it came from.
fn fixture_text(name: &str) -> (ContentStream, TextObject) {
    let doc = Document::from_bytes(bytes_of(name)).expect("fixture parses");
    let pages = page_tree::pages(&doc).expect("pages");
    let page = pages.first().expect("one page").clone();
    let cs = ContentStream::from_page(&doc.view(), &page).expect("content decodes");
    let model = decompose_page(&doc.view(), &page, Matrix::IDENTITY).expect("decomposes");
    let text = model
        .objects
        .iter()
        .find_map(|o| match o {
            VectorObject::Text(t) => Some(t.clone()),
            _ => None,
        })
        .expect("a text object");
    (cs, text)
}

fn session(name: &str) -> EditSession {
    EditSession::new(Document::from_bytes(bytes_of(name)).expect("fixture parses"))
}

fn planned(plan: &PlannedEdit) -> String {
    String::from_utf8_lossy(&plan.content).into_owned()
}

/// Every run's page-space box, as the file stands.
fn run_boxes(name: &str) -> Vec<Bounds> {
    fixture_text(name).1.runs.iter().map(|r| r.bounds).collect()
}

/// Every run's page-space box **after** the session verb moved one, saved,
/// and the result was re-opened from bytes.
///
/// Going through save-and-reopen rather than reading the live session is the
/// point: the operator's claim is about what the FILE says, and a plan that
/// produced the right in-memory geometry and the wrong bytes would pass a
/// weaker check. It is also the only way to exercise the incremental writer
/// on this verb's output.
fn run_boxes_after_move(name: &str, object: usize, run: usize, dx: f64, dy: f64) -> Vec<Bounds> {
    let mut s = session(name);
    s.move_text_run(0, object, run, dx, dy)
        .unwrap_or_else(|e| panic!("move must succeed on {name}: {e}"));
    let saved = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("saves")
        .0;
    let doc = Document::from_bytes(saved).expect("the saved file re-opens");
    let pages = page_tree::pages(&doc).expect("pages");
    let model = decompose_page(&doc.view(), pages.first().expect("page"), Matrix::IDENTITY)
        .expect("decomposes");
    model
        .objects
        .iter()
        .find_map(|o| match o {
            VectorObject::Text(t) => Some(t.runs.iter().map(|r| r.bounds).collect()),
            _ => None,
        })
        .expect("a text object")
}

/// Assert `after` is `before` displaced by exactly `(dx, dy)`.
fn assert_shifted(label: &str, before: Bounds, after: Bounds, dx: f64, dy: f64) {
    for (what, got, want) in [
        ("min.x", after.min.x, before.min.x + dx),
        ("min.y", after.min.y, before.min.y + dy),
        ("max.x", after.max.x, before.max.x + dx),
        ("max.y", after.max.y, before.max.y + dy),
    ] {
        assert!(
            (got - want).abs() <= EPS,
            "{label}: {what} should be {want} (moved by {dx},{dy}) but is {got}",
        );
    }
}

// ===========================================================================
// The three placement shapes
// ===========================================================================

/// **The whole claim.** The run the operator grabbed moves by exactly
/// what was asked, and **every other run in the same `BT`…`ET` stays exactly
/// where it was**.
///
/// The second half is the one that is easy to get wrong and impossible to
/// see: `Td` translates the LINE matrix, so a move that adjusts the run and
/// stops there slides every later run in the object with it. On a title block
/// that is "I nudged the drawing number and the whole block walked."
#[test]
fn moving_one_run_leaves_every_other_run_exactly_where_it_was() {
    let before = run_boxes("runs-td-relative.pdf");
    assert_eq!(before.len(), 3, "the fixture must have three runs");

    let after = run_boxes_after_move("runs-td-relative.pdf", 0, 1, 7.0, -3.0);
    assert_eq!(after.len(), 3, "a move must not add or drop a run");

    assert_shifted("the moved run", before[1], after[1], 7.0, -3.0);
    assert_shifted("the run before it", before[0], after[0], 0.0, 0.0);
    assert_shifted("the run after it", before[2], after[2], 0.0, 0.0);
}

/// A run placed by its own `Tm` moves through that `Tm`'s `e`/`f`, and its
/// successor — also absolute — costs nothing at all.
///
/// `Tm` re-establishes both the text and line matrices from its six operands
/// (Table 108), so nothing that happened before it survives into it. That is
/// what makes this the cheapest case and, on CAD output, the common one.
#[test]
fn an_absolute_run_moves_through_its_own_tm_and_its_successor_costs_nothing() {
    let (cs, text) = fixture_text("runs-two-explicit.pdf");
    assert_eq!(text.runs.len(), 2, "the fixture must have two runs");

    let plan = plan_move_text_run(&cs, &text, 0, 5.0, 0.0).expect("move run 0");
    let out = planned(&plan);

    assert!(
        out.contains("1 0 0 1 77 700 Tm"),
        "the moved run's own Tm must carry the delta: {out}",
    );
    assert!(
        out.contains("1 0 0 1 72 680 Tm"),
        "the absolute successor must be byte-verbatim: {out}",
    );
    assert_eq!(
        plan.operators_touched, 1,
        "an absolute successor needs no compensation at all",
    );
    assert!(
        plan.disclosures.is_empty(),
        "nothing was materialised, so nothing is owed: {:?}",
        plan.disclosures,
    );
}

/// A `Td`-placed run has its operands rewritten, and the **next** `Td` has the
/// same delta subtracted — the compensation that keeps the rest of the object
/// still.
#[test]
fn a_relative_run_is_rewritten_and_its_successor_is_put_back() {
    let (cs, text) = fixture_text("runs-td-relative.pdf");
    assert_eq!(text.runs.len(), 3);

    let plan = plan_move_text_run(&cs, &text, 1, 6.0, 0.0).expect("move run 1");
    let out = planned(&plan);

    assert!(
        out.contains("46 0 Td"),
        "run 1's own Td must gain the delta (40 + 6): {out}",
    );
    assert!(
        out.contains("34 0 Td"),
        "run 2's Td must lose it again (40 - 6), or the rest of the object slides: {out}",
    );
    assert_eq!(plan.operators_touched, 2, "one rewrite each: {out}");
    assert!(
        plan.disclosures.is_empty(),
        "both operators already existed: {:?}",
        plan.disclosures,
    );
}

/// **`TD`'s `ty` is the leading, and must never be nudged.**
///
/// Table 108: `TD` sets `TL` to `−ty` *and then* translates. A move that
/// treated it as an ordinary relative pair would re-space **every later `T*`
/// in the object** — bytes well-formed, file round-tripping, and two lines of
/// text nobody selected quietly further apart. So the verb inserts a `Td`
/// instead and says it did.
#[test]
fn the_leading_is_never_rewritten_and_the_insertion_is_disclosed() {
    let (cs, text) = fixture_text("runs-tstar-leading.pdf");
    assert_eq!(text.runs.len(), 3, "the fixture must have three runs");

    let plan = plan_move_text_run(&cs, &text, 0, 5.0, 0.0).expect("move run 0");
    let out = planned(&plan);

    assert!(
        out.contains("0 -20 TD"),
        "the leading operator must be byte-verbatim: {out}",
    );
    assert!(
        out.contains("5 0 Td"),
        "a Td must have been inserted for the move: {out}",
    );
    assert!(
        out.contains("-5 0 Td"),
        "and one to put the T*-placed successor back: {out}",
    );
    assert_eq!(
        plan.disclosures.len(),
        2,
        "one sentence per materialised operator (rule 4): {:?}",
        plan.disclosures,
    );
}

/// And the file that comes out of the `TD`/`T*` case really does move one run
/// and only one — the geometric half of the assertion above.
#[test]
fn an_inserted_positioning_operator_moves_exactly_one_run() {
    let before = run_boxes("runs-tstar-leading.pdf");
    let after = run_boxes_after_move("runs-tstar-leading.pdf", 0, 0, 5.0, 0.0);

    assert_shifted("the moved run", before[0], after[0], 5.0, 0.0);
    assert_shifted("the T*-placed run after it", before[1], after[1], 0.0, 0.0);
    assert_shifted("and the one after that", before[2], after[2], 0.0, 0.0);
}

// ===========================================================================
// Both transforms, not one
// ===========================================================================

/// **A page-space drag is not a text-space drag.**
///
/// Under `0 1 -1 0 300 300 Tm` the run's own x axis points up the page. A
/// nudge of `(5, 0)` in page space is `(0, −5)` in that run's text space, so
/// the `Td` must read `40 -5`. An implementation that converted through the
/// CTM only — which is correct on every axis-aligned fixture in this file —
/// writes `45 0` and slides the text along its own baseline instead of across
/// it.
#[test]
fn a_rotated_text_matrix_converts_the_drag_through_both_transforms() {
    let (cs, text) = fixture_text("runs-rotated-td.pdf");
    assert_eq!(text.runs.len(), 2, "the fixture must have two runs");

    let plan = plan_move_text_run(&cs, &text, 1, 5.0, 0.0).expect("move run 1");
    let out = planned(&plan);
    assert!(
        out.contains("40 -5 Td"),
        "the delta must cross the text matrix, not just the CTM: {out}",
    );
}

/// …and the run lands where the drag asked, in page space.
#[test]
fn a_run_under_a_rotated_matrix_lands_where_the_drag_asked() {
    let before = run_boxes("runs-rotated-td.pdf");
    let after = run_boxes_after_move("runs-rotated-td.pdf", 0, 1, 5.0, 0.0);

    assert_shifted("the moved run", before[1], after[1], 5.0, 0.0);
    assert_shifted("the run before it", before[0], after[0], 0.0, 0.0);
}

// ===========================================================================
// The two refusals
// ===========================================================================

/// A run whose origin is INHERITED (§9.4.2) has no coordinate to move.
///
/// `runs-inherited.pdf`'s run 1 has nothing between it and run 0: it starts
/// wherever run 0's string left the pen, and that position is written nowhere
/// in the file. Writing a `Td` for it would be worse than an estimate — `Td`
/// resets the text matrix to the LINE matrix, so the run would jump to the
/// start of its line.
#[test]
fn moving_a_run_that_inherits_its_origin_is_refused_with_a_remedy() {
    let (cs, text) = fixture_text("runs-inherited.pdf");
    assert_eq!(text.runs.len(), 4, "the fixture must have four runs");

    let err = plan_move_text_run(&cs, &text, 1, 5.0, 0.0).expect_err("run 1 inherits");
    let msg = err.to_string();
    assert!(
        msg.contains("position of its own"),
        "the refusal must name the cause: {msg}",
    );
    assert!(
        msg.contains("move the whole text object"),
        "and its remedy: {msg}",
    );
}

/// The move-side twin of `DeleteWouldMoveNextRun`: moving run 0 would drag
/// run 1 along, because run 1 starts where run 0 ends.
///
/// Deliberately a **different** message from delete's. Delete's remedy is *do
/// the later one first*, and there is no reordering that helps a move, so one
/// message serving both verbs would have to name neither.
#[test]
fn moving_a_run_whose_successor_inherits_is_refused_separately_from_delete() {
    let (cs, text) = fixture_text("runs-inherited.pdf");

    let err = plan_move_text_run(&cs, &text, 0, 5.0, 0.0).expect_err("run 1 inherits from run 0");
    let msg = err.to_string();
    assert!(
        msg.contains("would move the run after it"),
        "the refusal must name what it is protecting: {msg}",
    );
    assert!(
        !msg.contains("delete the later run first"),
        "delete's remedy does not apply to a move and must not be offered: {msg}",
    );
}

/// **The pre-check IS the guard** (`R221`/`R243`).
///
/// `text_run_move_refusal` exists so a shell can put the remedy in front of
/// the gesture. If it were a second implementation of the same rule it would
/// one day enable a control the engine refuses, or grey out one it would have
/// allowed. This asserts they agree on every run of the fixture that has all
/// four cases in it, plus an index that does not exist.
#[test]
fn the_exported_pre_check_and_the_planner_never_disagree() {
    let (cs, text) = fixture_text("runs-inherited.pdf");
    for i in 0..=text.runs.len() {
        let pre = text_run_move_refusal(&text, i);
        let planned = plan_move_text_run(&cs, &text, i, 3.0, 0.0);
        match (pre, planned) {
            (None, Ok(_)) => {}
            (Some(a), Err(b)) => assert_eq!(
                a.to_string(),
                b.to_string(),
                "run {i}: the pre-check and the planner must refuse with the SAME sentence",
            ),
            (pre, planned) => panic!(
                "run {i}: pre-check said {pre:?} and the planner said {}",
                planned.map_or_else(|e| format!("Err({e})"), |_| "Ok".to_owned()),
            ),
        }
    }
}

// ===========================================================================
// Through the session
// ===========================================================================

/// One run moved is ONE undoable command, and undo takes it back.
#[test]
fn moving_a_run_is_one_command_and_undoes() {
    let mut s = session("runs-two-explicit.pdf");
    s.move_text_run(0, 0, 0, 5.0, 0.0).expect("move run 0");
    assert!(s.is_modified(), "the edit must be staged");

    s.undo().expect("one undo");
    assert!(
        s.undo().is_none(),
        "one run move is ONE command — a second undo has nothing to take",
    );
}

/// The §9.4.2 guard reaches the session verb, not only the planner, and a
/// refused edit changes nothing.
#[test]
fn the_session_verb_refuses_and_stages_nothing() {
    let mut s = session("runs-inherited.pdf");
    let err = s.move_text_run(0, 0, 0, 5.0, 0.0).expect_err("must refuse");
    assert!(
        err.to_string().contains("move the whole text object"),
        "the remedy must survive the trip through EditSession: {err}",
    );
    assert!(!s.is_modified(), "a refused edit must change nothing");
}

/// Pointing the text verb at a PATH object is refused by name rather than
/// silently doing nothing.
#[test]
fn aiming_the_move_verb_at_a_path_object_is_refused() {
    let mut s = session("runs-single.pdf");
    // Object 0 is the rule; object 1 is the text.
    let err = s
        .move_text_run(0, 0, 0, 5.0, 0.0)
        .expect_err("object 0 is the path, not the text");
    assert!(
        err.to_string().contains("path"),
        "the refusal must name the kind that WAS found: {err}",
    );
    // …and the real text object still moves.
    s.move_text_run(0, 1, 0, 5.0, 0.0)
        .expect("object 1 is the text");
}

/// An out-of-range run index is refused with the count, not a panic.
#[test]
fn an_out_of_range_run_is_refused_with_the_count() {
    let mut s = session("runs-two-explicit.pdf");
    let err = s.move_text_run(0, 0, 9, 1.0, 1.0).expect_err("no run 9");
    assert!(
        err.to_string().contains('2'),
        "the refusal must say how many there are: {err}",
    );
}

/// **A move renumbers nothing** — not the page's objects, not the object's
/// runs.
///
/// `docs/core-api/02-editing-and-saving.md` puts the whole move family in the
/// "does not renumber" row and the whole delete family in the other one, and a
/// shell builds its live selection on that: an index taken before the drag
/// must still name the same run after it. The move verb INSERTS operators in
/// two of its three cases, which is exactly the thing that could have made it
/// the exception — a new `Td` is a new operator in the stream, and if the
/// decomposition counted it as anything the run indices would shift under a
/// selection nobody touched.
///
/// Measured on the fixture that takes the INSERT path, because that is the
/// case with something to prove; `runs-td-relative.pdf` rewrites operands and
/// could not renumber if it tried.
#[test]
fn moving_a_run_renumbers_neither_the_objects_nor_the_runs() {
    let (_, before) = fixture_text("runs-tstar-leading.pdf");
    let names_before: Vec<String> = (0..before.runs.len())
        .map(|i| before.run_text(i).unwrap_or("?").to_owned())
        .collect();
    assert_eq!(
        names_before,
        vec!["ALPHA".to_owned(), "BETA".to_owned(), "GAMMA".to_owned()],
        "the fixture must decode, or this test asserts nothing",
    );

    let mut s = session("runs-tstar-leading.pdf");
    let objects_before = s.page_objects(0).expect("decomposes").objects.len();
    s.move_text_run(0, 0, 0, 5.0, 0.0).expect("move run 0");

    let model = s.page_objects(0).expect("re-decomposes");
    assert_eq!(
        model.objects.len(),
        objects_before,
        "a move must not change the page's object count",
    );
    let VectorObject::Text(after) = &model.objects[0] else {
        panic!("object 0 must still be the text object");
    };
    let names_after: Vec<String> = (0..after.runs.len())
        .map(|i| after.run_text(i).unwrap_or("?").to_owned())
        .collect();
    assert_eq!(
        names_after, names_before,
        "run N must still be run N — an inserted positioning operator is not a run",
    );
}
