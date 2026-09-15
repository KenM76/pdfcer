//! `plan_split_text_object` / `EditSession::split_text_object` — cutting one
//! `BT`…`ET` into several (`Pass 306.0`, ISO 32000-1 §9.4).
//!
//! ## The gap this closes
//!
//! A CAD exporter may put every string on a sheet inside ONE text object — the
//! operator's SolidWorks drawing has 237 dimension labels in one `BT`…`ET`,
//! placed by a single absolute `Tm` and then a chain of relative `Td` steps,
//! and its general notes are a second such object. Two things follow, and an
//! operator meets both within a minute of trying to edit such a drawing:
//!
//! * **A line is not addressable.** The object is the whole sheet and a run is
//!   one show operator, so between them there is no handle for *"move this
//!   line"* — the gesture a title block invites.
//! * **Neighbours are coupled.** `Td` translates the LINE matrix, so any verb
//!   that adjusts one step perturbs every step after it. `plan_move_text_run`
//!   compensates the successor for exactly this reason, and refuses outright
//!   when the successor has no position of its own.
//!
//! The operator's words, 2026-09-15: *"I assume this is due to all the text of
//! all the lines being part of a larger block. Since we've got the reflow text
//! figured out maybe we can add a tool to make each reflowed area its own text
//! object so each line can be moved and manipulated on its own."*
//!
//! ## The mechanism, and therefore what these tests pin
//!
//! At each cut, `ET BT <the run's own six-coefficient Tm>` is inserted
//! immediately before the run's show operator and **nothing else changes**. It
//! is correct because `BT` resets only `Tm`/`Tlm` (§9.4.1 Table 107) — and a
//! run whose position is `Explicit` was placed by an operator that sets those
//! two EQUAL (Table 108), so one restated matrix serves both — while every
//! other thing a text object depends on (`Tf`, `Tc`, `Tw`, `Tz`, `TL`, `Ts`,
//! `Tr`, colour, the CTM) is graphics state that `ET`/`BT` do not touch
//! (§9.3).
//!
//! So the load-bearing assertions are:
//!
//! 1. **Everything that is not the cut survives BYTE-VERBATIM.** This is the
//!    property the mechanism buys, and the one a "rebuild the text object"
//!    implementation fails while still rendering correctly.
//! 2. **The restated `Tm` is the run's own matrix**, so the run draws where it
//!    drew.
//! 3. **No state preamble is re-emitted**, because none is needed. A test that
//!    tolerated a redundant `Tf` would let a future change quietly turn this
//!    re-framing into a rebuild — and a rebuild owes `restore_ops` (R88).
//! 4. **Each refusal fires before any byte is produced, and names its own
//!    reason** rather than a shared one.
//!
//! ## Why the fixtures are real PDFs and not inline content streams
//!
//! The same constraint `text_run_move.rs` and `text_run_delete.rs` record: a
//! bare `ContentStream::parse` has no resource dictionary, so `/F1 10 Tf`
//! resolves to nothing, no run lays out, and `TextObject::runs` comes back
//! **empty** — every assertion below would then pass against a verb that did
//! nothing at all. It is not a hypothetical: the first draft of this file used
//! inline streams and seven of its eight tests failed on `runs.len() == 0`.
//! Real (standard-14, non-embedded) fonts, and a run count asserted before
//! anything else.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::content::ContentStream;
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree;
use pdfcer_core::vector::edit::{
    plan_split_text_object, text_object_split_points, text_split_refusal,
};
use pdfcer_core::vector::{
    Matrix, SplitGranularity, TextObject, VectorEditError, VectorObject, decompose_page,
};
use pdfcer_core::writer::SaveOptions;

/// How many whitespace-separated tokens of `out` are exactly `op`.
///
/// `str::matches` cannot be used for this and the reason is a live trap: the
/// fixtures' own strings contain the operator names as substrings — `(BETA)`
/// contains `ET` — so a substring count reports operators that are not there.
/// Caught by three tests failing at once with an off-by-one.
fn op_count(out: &str, op: &str) -> usize {
    out.split_whitespace().filter(|t| *t == op).count()
}

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

#[test]
fn a_cut_inserts_et_bt_and_the_runs_own_matrix() {
    let (cs, t) = fixture_text("runs-two-explicit.pdf");
    assert_eq!(t.runs.len(), 2, "fixture shape");

    let plan = plan_split_text_object(&cs, &t, &[1]).expect("a Tm-placed run splits");
    let out = String::from_utf8(plan.content).expect("ascii");

    assert_eq!(op_count(&out, "BT"), 2, "one text object became two");
    assert_eq!(op_count(&out, "ET"), 2);
    assert_eq!(plan.operators_touched, 1, "one cut, one touched site");
    assert!(
        plan.disclosures.is_empty(),
        "an explicit cut infers nothing and discloses nothing"
    );
    // The new piece states the run's own origin — which the producer had
    // already written, so the value appears twice in the output. That is the
    // point: this is a re-framing, not a recomputation.
    assert_eq!(
        out.matches("1 0 0 1 72 680 Tm").count(),
        2,
        "the producer's Tm survives AND is restated: {out}"
    );
}

#[test]
fn everything_that_is_not_the_cut_survives_byte_verbatim() {
    let (cs, t) = fixture_text("runs-td-relative.pdf");
    assert_eq!(t.runs.len(), 3, "fixture shape");

    let plan = plan_split_text_object(&cs, &t, &[2]).expect("a Td-placed run splits");
    let out = String::from_utf8(plan.content).expect("ascii");

    // ★ The strongest form of the claim: excise the ONE inserted prelude and
    // what is left is the input, byte for byte. A re-spelled operand, a
    // normalised number or a re-emitted `Tf` all fail this and all pass a
    // looser "renders the same" check — which is precisely the class of defect
    // a geometry-only suite cannot see.
    let original = String::from_utf8_lossy(&cs.buf).into_owned();
    let at = out.find("ET\nBT\n").expect("the prelude is there");
    let end = out[at..]
        .find(" Tm\n")
        .map(|i| at + i + " Tm\n".len())
        .expect("the prelude ends in a Tm");
    let mut without = String::with_capacity(original.len());
    without.push_str(&out[..at]);
    without.push_str(&out[end..]);
    assert_eq!(without, original, "only the prelude was added");
}

#[test]
fn the_state_preamble_is_not_re_emitted() {
    let (cs, t) = fixture_text("runs-td-relative.pdf");
    let plan = plan_split_text_object(&cs, &t, &[1, 2]).expect("two cuts");
    let out = String::from_utf8(plan.content).expect("ascii");

    // `Tf` is graphics state (§9.3): `ET`/`BT` do not reset it, so re-stating
    // it would be dead bytes. This assertion is what stops a future change
    // from "helpfully" adding a preamble and turning a re-framing into a
    // rebuild.
    assert_eq!(
        op_count(&out, "Tf"),
        1,
        "one Tf, exactly as in the input: {out}"
    );
    assert_eq!(op_count(&out, "BT"), 3, "three pieces");
    assert_eq!(op_count(&out, "ET"), 3);
}

#[test]
fn cuts_are_deduplicated_and_ordered() {
    let (cs, t) = fixture_text("runs-td-relative.pdf");
    // A shell collecting indices from a marquee may hand them over twice and
    // out of order. Neither is worth refusing; cutting the same point twice
    // would emit an empty text object, which is.
    let plan = plan_split_text_object(&cs, &t, &[2, 1, 2]).expect("tolerated");
    assert_eq!(plan.operators_touched, 2);
    let out = String::from_utf8(plan.content).expect("ascii");
    assert_eq!(op_count(&out, "BT"), 3);
    assert_eq!(op_count(&out, "ET"), 3);
}

#[test]
fn line_granularity_groups_by_baseline_in_stream_order() {
    // Three runs advanced along ONE baseline by `40 0 Td`: one visual line,
    // therefore no cut at all under `Line`…
    let (_, chain) = fixture_text("runs-td-relative.pdf");
    assert!(
        text_object_split_points(&chain, SplitGranularity::Line).is_empty(),
        "one visual line stays one object"
    );
    // …while `Run` cuts before every run but the first.
    assert_eq!(
        text_object_split_points(&chain, SplitGranularity::Run),
        vec![1, 2]
    );

    // Two runs on two baselines: one cut under either granularity.
    let (_, two) = fixture_text("runs-two-explicit.pdf");
    assert_eq!(
        text_object_split_points(&two, SplitGranularity::Line),
        vec![1]
    );
    assert_eq!(
        text_object_split_points(&two, SplitGranularity::Run),
        vec![1]
    );
}

#[test]
fn run_zero_is_never_a_split_point() {
    for name in ["runs-two-explicit.pdf", "runs-td-relative.pdf"] {
        let (_, t) = fixture_text(name);
        for g in [SplitGranularity::Run, SplitGranularity::Line] {
            assert!(
                !text_object_split_points(&t, g).contains(&0),
                "{name}: a cut before the first run divides nothing"
            );
        }
    }
}

#[test]
fn an_inherited_run_is_refused_by_name() {
    let (cs, t) = fixture_text("runs-inherited.pdf");
    assert_eq!(t.runs.len(), 4, "fixture shape");
    // Run 1 has NOTHING between it and run 0 — §9.4.2, no coordinates
    // anywhere in the file, so there is no origin to restate.
    assert!(matches!(
        text_split_refusal(&cs, &t, 1),
        Some(VectorEditError::SplitRunInheritsPosition { index: 1 })
    ));
    // Run 2 is placed by a `Td`, so it splits — which proves the guard is the
    // §9.4.2 one and not "anything not placed by `Tm`".
    assert!(text_split_refusal(&cs, &t, 2).is_none());
    // Run 3 inherits again: a one-shot latch that cleared and never re-armed
    // is caught here.
    assert!(matches!(
        text_split_refusal(&cs, &t, 3),
        Some(VectorEditError::SplitRunInheritsPosition { index: 3 })
    ));
}

#[test]
fn a_quote_shown_run_is_refused_by_name() {
    let (cs, t) = fixture_text("runs-quote-show.pdf");
    assert!(t.runs.len() >= 2, "fixture shape: {}", t.runs.len());
    // `'` moves to the next line and THEN shows (Table 109), so the recorded
    // matrix is post-move and an injected `Tm` would apply the move twice.
    assert!(matches!(
        text_split_refusal(&cs, &t, 1),
        Some(VectorEditError::SplitAtLineShowOperator { index: 1 })
    ));
}

#[test]
fn a_cut_inside_marked_content_is_refused_by_name() {
    let (cs, t) = fixture_text("runs-marked-content.pdf");
    assert_eq!(t.runs.len(), 2, "fixture shape");
    // §14.6: `BDC … ET BT … EMC` overlaps rather than nests.
    assert!(matches!(
        text_split_refusal(&cs, &t, 1),
        Some(VectorEditError::SplitInsideMarkedContent { index: 1 })
    ));
}

#[test]
fn the_range_and_empty_refusals_produce_no_bytes() {
    let (cs, t) = fixture_text("runs-two-explicit.pdf");
    assert!(matches!(
        text_split_refusal(&cs, &t, 0),
        Some(VectorEditError::SplitAtObjectStart)
    ));
    assert!(matches!(
        text_split_refusal(&cs, &t, 9),
        Some(VectorEditError::TextRunOutOfRange { index: 9, count: 2 })
    ));
    assert!(matches!(
        plan_split_text_object(&cs, &t, &[]),
        Err(VectorEditError::EmptySplit)
    ));
    // ★ One bad index refuses the WHOLE split, not the good half of it: a
    // half-applied cut list is worse than none, because the operator cannot
    // tell which cuts landed without reading the bytes.
    assert!(matches!(
        plan_split_text_object(&cs, &t, &[1, 9]),
        Err(VectorEditError::TextRunOutOfRange { index: 9, .. })
    ));
}

#[test]
fn the_session_verb_splits_the_object_and_undoes_byte_identically() {
    let src = bytes_of("runs-td-relative.pdf");
    let doc = Document::from_bytes(src.clone()).expect("fixture parses");
    let mut s = EditSession::new(doc);

    let before = s.page_objects(0).expect("objects").objects.len();
    let idx = text_object_index(&mut s);
    let (points, disclosures) = s
        .text_object_split_plan(0, idx, SplitGranularity::Run)
        .expect("plan");
    assert_eq!(points, vec![1, 2]);
    assert!(
        disclosures.is_empty(),
        "Run granularity infers nothing: {disclosures:?}"
    );

    let d = s.split_text_object(0, idx, &points).expect("splits");
    assert!(d.is_empty(), "the surgery guesses nothing: {d:?}");

    let after = s.page_objects(0).expect("objects").objects.len();
    assert_eq!(after, before + 2, "two cuts, two new objects");

    // The edit is one undo entry, and undoing it restores the base byte for
    // byte — the minimal-diff contract (`ARCHITECTURE.md` §11.1), asserted
    // through a real save rather than against `DirtySet::is_empty`.
    assert!(s.undo().is_some(), "one command on the stack");
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::default())
        .expect("saves");
    assert_eq!(bytes, src, "split → undo → save is byte-identical");
}

#[test]
fn line_granularity_discloses_its_inference() {
    let doc = Document::from_bytes(bytes_of("runs-two-explicit.pdf")).expect("parses");
    let mut s = EditSession::new(doc);
    let idx = text_object_index(&mut s);
    let (points, disclosures) = s
        .text_object_split_plan(0, idx, SplitGranularity::Line)
        .expect("plan");
    assert_eq!(points, vec![1]);
    // Rule 4: an untagged content stream does not record where its lines are
    // (§14.8), so pdfcer guessing that is disclosed — off-canvas, at the point
    // the guess is made, never silently.
    assert_eq!(disclosures.len(), 1, "one sentence for one inference");
    let d = &disclosures[0];
    assert!(d.contains("1 line break"), "names the count: {d}");
    assert!(d.contains("14.8"), "cites the clause: {d}");
}

/// The paint-order index of the page's first text object.
fn text_object_index(s: &mut EditSession) -> usize {
    let model = s.page_objects(0).expect("objects");
    model
        .objects
        .iter()
        .position(|o| matches!(o, VectorObject::Text(_)))
        .expect("a text object")
}
