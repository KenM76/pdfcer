//! `plan_delete_text_run` / `EditSession::delete_text_run` — removing ONE
//! show operator out of a text object (`Pass 32.0`, ISO 32000-1 §9.4).
//!
//! ## The measured defect
//!
//! Text deletion has been object-granular since Pass 9c-min, and a CAD
//! exporter's `BT`…`ET` boundary reflects its own graphics-state batching
//! rather than anything the draughtsman drew. On the operator's own
//! drawing **one text object holds all 237 dimension labels**, so deleting
//! "a label" deleted every one of them. The hit-test half has been per-run
//! since Pass 18.5 — a run could already be *selected* and could not be
//! *removed*.
//!
//! ## Why the fixtures are real PDFs and not inline content streams
//!
//! A bare `ContentStream::parse` has no resource dictionary, so `/F1 10 Tf`
//! resolves to nothing, no run lays out, and `TextObject::runs` comes back
//! **empty**. Every assertion below would then be vacuous — they would pass
//! against a verb that did nothing at all. The fixtures therefore carry a
//! real (standard-14, non-embedded) font, and each test asserts a run count
//! before asserting anything about deletion.
//!
//! That is not a hypothetical: the first draft of this file used inline
//! streams and five of its tests failed with `count: 0`, which is how the
//! constraint was found.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::content::ContentStream;
use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_core::vector::edit::plan_delete_text_run;
use pdfcer_core::vector::{
    Matrix, PlannedEdit, RunPositioning, TextObject, VectorEditError, VectorObject, decompose_page,
};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/text")
        .join(name)
}

fn fixture_text(name: &str) -> (ContentStream, TextObject) {
    let bytes = std::fs::read(fixture(name))
        .unwrap_or_else(|e| panic!("missing fixture {}: {e}", fixture(name).display()));
    let doc = Document::from_bytes(bytes).expect("fixture parses");
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

fn planned(plan: &PlannedEdit) -> String {
    String::from_utf8_lossy(&plan.content).into_owned()
}

/// Two runs, each with its own `Tm`. Deleting the first leaves the second
/// **byte-verbatim** — the whole claim of the Pass.
#[test]
fn deleting_one_run_leaves_its_siblings_untouched() {
    let (cs, text) = fixture_text("runs-two-explicit.pdf");
    assert_eq!(text.runs.len(), 2, "the fixture must have two runs");

    let out = planned(&plan_delete_text_run(&cs, &text, 0).expect("delete run 0"));
    assert!(!out.contains("(ALPHA)"), "run 0 must be gone: {out}");
    assert!(out.contains("(BETA)"), "run 1 must survive: {out}");
    assert!(
        out.contains("1 0 0 1 72 680 Tm"),
        "the survivor's own positioning must be untouched: {out}",
    );
    assert!(
        out.contains("BT") && out.contains("ET"),
        "the text object itself must survive: {out}",
    );
}

/// **The §9.4.2 guard.** Deleting a run whose successor INHERITS its
/// position is refused, and the refusal names the remedy.
///
/// Without this the edit is byte-minimal, round-trips, passes
/// `--verify-undo` — and moves a label the operator never selected. That
/// combination (correct-looking, verifiable, wrong) is exactly the class
/// decision 027 says to refuse rather than guess at.
#[test]
fn deleting_a_run_whose_successor_inherits_is_refused() {
    let (cs, text) = fixture_text("runs-inherited.pdf");
    assert_eq!(
        text.runs[1].positioned_by,
        RunPositioning::Inherited,
        "the fixture must still have an inheriting run, or this proves nothing",
    );

    let err = plan_delete_text_run(&cs, &text, 0).expect_err("must refuse");
    assert!(
        matches!(err, VectorEditError::DeleteWouldMoveNextRun { index: 0 }),
        "got {err:?}",
    );
    assert!(
        err.to_string().contains("delete the later run first"),
        "the refusal must name the remedy, because there IS one and it always \
         works: {err}",
    );
}

/// ...and the named remedy is actually permitted.
///
/// A refusal that pointed at an impossible remedy would be worse than one
/// that named none, so this is asserted rather than assumed.
#[test]
fn the_named_remedy_is_actually_permitted() {
    let (cs, text) = fixture_text("runs-inherited.pdf");
    // Run 3 (`DELTA`) is last, so nothing inherits from it.
    let out = planned(&plan_delete_text_run(&cs, &text, 3).expect("the later run deletes"));
    assert!(!out.contains("(DELTA)"), "{out}");
    assert!(out.contains("(GAMMA)"), "{out}");
}

/// Deleting the ONLY run deletes the whole text object — a `BT`…`ET` that
/// shows nothing is not an object. Same rule as the last subpath.
#[test]
fn deleting_the_only_run_removes_the_text_object() {
    let (cs, text) = fixture_text("runs-single.pdf");
    assert_eq!(text.runs.len(), 1);

    let out = planned(&plan_delete_text_run(&cs, &text, 0).expect("delete the only run"));
    assert!(!out.contains("BT"), "the whole BT…ET must go: {out}");
    assert!(!out.contains("(ONLY)"), "{out}");
    assert!(
        out.contains("72 396 m 540 396 l S"),
        "the unrelated path must survive verbatim — without it in the fixture, \
         'the text object went' and 'the stream was emptied' look identical: {out}",
    );
}

/// An out-of-range run index is refused with the count that was there.
#[test]
fn an_out_of_range_run_index_is_refused_with_the_count() {
    let (cs, text) = fixture_text("runs-single.pdf");
    let err = plan_delete_text_run(&cs, &text, 7).expect_err("must refuse");
    assert!(
        matches!(
            err,
            VectorEditError::TextRunOutOfRange { index: 7, count: 1 }
        ),
        "got {err:?}",
    );
}

/// **The measured complaint, in miniature.** Four labels in ONE text
/// object: deleting one leaves the other three, where object-granular
/// deletion removed all four.
#[test]
fn deleting_one_label_of_four_leaves_the_other_three() {
    let (cs, text) = fixture_text("runs-inherited.pdf");
    assert_eq!(text.runs.len(), 4);

    // Run 2 (`GAMMA`) is followed by `DELTA`, which inherits — so GAMMA is
    // refused and DELTA must go first. That ordering constraint is the
    // Pass's honest cost, exercised here rather than only in isolation.
    assert!(matches!(
        plan_delete_text_run(&cs, &text, 2),
        Err(VectorEditError::DeleteWouldMoveNextRun { index: 2 })
    ));

    let plan = plan_delete_text_run(&cs, &text, 3).expect("the last run deletes");
    let out = planned(&plan);
    assert!(!out.contains("(DELTA)"), "{out}");
    for survivor in ["(ALPHA)", "(BETA)", "(GAMMA)"] {
        assert!(out.contains(survivor), "{survivor} must survive: {out}");
    }
    assert_eq!(
        plan.operators_touched, 1,
        "one run removed is one operator touched",
    );
}

/// The result still parses as a content stream — the edit produced a real
/// document, not merely different bytes.
#[test]
fn the_edited_stream_re_parses() {
    let (cs, text) = fixture_text("runs-two-explicit.pdf");
    let plan = plan_delete_text_run(&cs, &text, 1).expect("delete run 1");

    let after = ContentStream::parse(plan.content).expect("the result re-parses");
    let s = String::from_utf8_lossy(&after.buf);
    assert!(s.contains("(ALPHA)") && !s.contains("(BETA)"), "{s}");
    assert!(
        s.contains("BT") && s.contains("ET"),
        "the surviving run keeps its text object: {s}",
    );
}

// ---------------------------------------------------------------------------
// The `EditSession` layer — where the verb is actually reached from
// ---------------------------------------------------------------------------

use pdfcer_core::edit::EditSession;

fn session(name: &str) -> EditSession {
    let bytes = std::fs::read(fixture(name)).expect("fixture");
    EditSession::new(Document::from_bytes(bytes).expect("parses"))
}

/// **★ REGRESSION: the session's own decomposition must see text runs.**
///
/// `EditSession::vector_surgery` decomposed with an XObject resolver and
/// **no font resolver**. That was invisible for as long as every verb
/// reaching it was a PATH verb — paths need no font — but `runs` is
/// populated by *laying out* each show operator, which needs a resolvable
/// `Tf`. So every text object arrived with **zero** runs and
/// `delete_text_run` refused every real document with "the object has 0
/// run(s)", while `object-list` (which does pass fonts) reported four for
/// the same file.
///
/// Found by running the CLI against a fixture, not by reading the code —
/// and it would have shipped as "the verb exists and never works".
#[test]
fn the_session_decomposition_resolves_fonts_and_therefore_sees_runs() {
    let mut s = session("runs-inherited.pdf");
    // If fonts are not resolved this is `TextRunOutOfRange { count: 0 }`.
    s.delete_text_run(0, 0, 3)
        .expect("the session must see the fixture's four runs");
}

/// One run deleted is one undoable command, and undo restores it.
#[test]
fn deleting_a_run_is_one_command_and_undoes() {
    let mut s = session("runs-two-explicit.pdf");
    s.delete_text_run(0, 0, 0).expect("delete run 0");
    assert!(s.is_modified(), "the edit must be staged");

    s.undo().expect("one undo");
    assert!(
        s.undo().is_none(),
        "one run deletion is ONE command — a second undo has nothing to take",
    );
}

/// The §9.4.2 guard reaches the session verb, not only the planner.
#[test]
fn the_session_verb_refuses_a_move_inducing_deletion() {
    let mut s = session("runs-inherited.pdf");
    let err = s.delete_text_run(0, 0, 0).expect_err("must refuse");
    assert!(
        err.to_string().contains("delete the later run first"),
        "the remedy must survive the trip through EditSession: {err}",
    );
    assert!(!s.is_modified(), "a refused edit must change nothing");
}

/// Pointing the text verb at a PATH object is refused by name rather than
/// silently doing nothing.
#[test]
fn aiming_the_text_verb_at_a_path_object_is_refused() {
    let mut s = session("runs-single.pdf");
    // Object 0 is the rule; object 1 is the text.
    let err = s
        .delete_text_run(0, 0, 0)
        .expect_err("object 0 is the path, not the text");
    assert!(
        err.to_string().contains("path"),
        "the refusal must name the kind that WAS found: {err}",
    );
    // ...and the real text object still works.
    s.delete_text_run(0, 1, 0).expect("object 1 is the text");
}

// ---------------------------------------------------------------------------
// `hit_test_text_runs` — WHICH run, not merely whether
// ---------------------------------------------------------------------------

use pdfcer_core::vector::{Point, hit_test_text_runs};

fn page_model(name: &str) -> pdfcer_core::vector::PageObjects {
    let bytes = std::fs::read(fixture(name)).expect("fixture");
    let doc = Document::from_bytes(bytes).expect("parses");
    let pages = page_tree::pages(&doc).expect("pages");
    decompose_page(&doc.view(), pages.first().expect("page"), Matrix::IDENTITY).expect("decomposes")
}

/// A click inside one label's box picks **that** run.
///
/// `hit_test_point` has answered *whether* a text object was hit since
/// Pass 18.5, which is what stopped a 237-label object being a page-wide
/// hit — but a shell that wants to delete "this label" needs the index,
/// and that is what this adds.
#[test]
fn a_click_inside_a_run_reports_that_run_first() {
    let model = page_model("runs-two-explicit.pdf");
    let VectorObject::Text(t) = &model.objects[0] else {
        panic!("object 0 is not text");
    };
    for want in 0..t.runs.len() {
        let b = t.runs[want].bounds;
        let mid = Point::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0);
        let hits = hit_test_text_runs(&model, 0, mid, 0.5);
        assert_eq!(
            hits.first().copied(),
            Some(want),
            "a click in the middle of run {want} must report it first; got {hits:?}",
        );
    }
}

/// A click nowhere near any label reports nothing — the per-run query does
/// **not** inherit `text_hit`'s page-bbox fallback.
///
/// That fallback exists to keep an unmeasurable object *selectable*, which
/// is the right answer to "did I hit this object". It is the wrong answer
/// to "which run", because naming run 0 for an object whose runs were
/// never laid out hands the caller a target it can then delete — the wrong
/// one, silently.
#[test]
fn a_click_in_the_gap_between_labels_reports_no_run() {
    let model = page_model("runs-two-explicit.pdf");
    let VectorObject::Text(t) = &model.objects[0] else {
        panic!("object 0 is not text");
    };
    // Well right of both labels, still inside the object's enclosing box.
    let far = Point::new(t.page_bbox.max.x + 200.0, t.page_bbox.min.y + 1.0);
    assert!(
        hit_test_text_runs(&model, 0, far, 0.5).is_empty(),
        "a miss must be a miss",
    );
}

/// A non-text object and an out-of-range index both report nothing rather
/// than panicking or guessing.
#[test]
fn the_run_hit_test_is_empty_for_a_non_text_or_missing_object() {
    let model = page_model("runs-single.pdf");
    // Object 0 is the rule (a path), object 1 the text.
    assert!(hit_test_text_runs(&model, 0, Point::new(300.0, 396.0), 2.0).is_empty());
    assert!(hit_test_text_runs(&model, 99, Point::new(0.0, 0.0), 2.0).is_empty());
}
