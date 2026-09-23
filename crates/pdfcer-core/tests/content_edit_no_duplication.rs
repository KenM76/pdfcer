//! `Pass 251.0` — an `add_text` run placed AFTER an earlier content edit must
//! not be duplicated by the next content surgery (pdfcer-gui bug, 2026-09-04).
//!
//! `add_text` appends a new stream to the page's `/Contents`; every
//! content-surgery verb concatenates the whole `/Contents`, splices, and writes
//! the result into `contents[0]`, so it must empty the extras or the appended
//! run renders twice. The old code swept the extras only on the FIRST rewrite of
//! `contents[0]`, so any run appended after that first rewrite was folded in and
//! left in place. These tests drive the session the way the operator did.

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree;
use pdfcer_core::text_edit::EditableTextModel;
use pdfcer_core::text_edit::{AddTextRequest, BlockRecognitionOptions, EditOptions, EditRequest};
use pdfcer_core::text_extract::{self, ExtractOptions};
use pdfcer_core::writer::SaveOptions;
use std::path::Path;

fn plain() -> Document {
    Document::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/addtext/plain.pdf"),
    )
    .expect("load plain.pdf")
}

fn page0_text(bytes: &[u8]) -> String {
    let doc = Document::from_bytes(bytes.to_vec()).expect("output reloads");
    let pages = page_tree::pages(&doc).expect("page tree walks");
    let page = text_extract::extract_page(&doc, &pages[0], 0, &ExtractOptions::default())
        .expect("extract");
    let model = EditableTextModel::recognize(&page, &BlockRecognitionOptions::default());
    model
        .blocks()
        .iter()
        .map(|b| model.block_text(b))
        .collect::<Vec<_>>()
        .join("\n")
}

fn count(hay: &str, needle: &str) -> usize {
    hay.matches(needle).count()
}

#[test]
fn a_run_added_after_an_earlier_edit_is_not_duplicated_by_the_next_edit() {
    let mut s = EditSession::new(plain());

    // T1, then a content rewrite (the first_edit sweep), then T2, then another
    // rewrite — the sequence that armed the duplication before Pass 251.0.
    s.add_text(&AddTextRequest::new(0, (100.0, 640.0), "MARKERONE").with_size(12.0))
        .expect("add T1");
    s.edit_text(
        &EditRequest::find_replace(0, "Original", "Changed"),
        &EditOptions::default(),
    )
    .expect("first content rewrite");
    s.add_text(&AddTextRequest::new(0, (100.0, 610.0), "MARKERTWO").with_size(12.0))
        .expect("add T2");
    s.edit_text(
        &EditRequest::find_replace(0, "Changed", "Changed2"),
        &EditOptions::default(),
    )
    .expect("second content rewrite");

    let (bytes, _) = s.to_full_bytes(&SaveOptions::default()).expect("save");
    let text = page0_text(&bytes);

    assert_eq!(
        count(&text, "MARKERTWO"),
        1,
        "the run added after the first edit must appear ONCE, not duplicated by the next surgery: {text:?}"
    );
    assert_eq!(
        count(&text, "MARKERONE"),
        1,
        "the first run stays single too: {text:?}"
    );
}

#[test]
fn each_further_edit_does_not_add_another_copy() {
    let mut s = EditSession::new(plain());
    s.edit_text(
        &EditRequest::find_replace(0, "Original", "Changed"),
        &EditOptions::default(),
    )
    .expect("prime with a first rewrite");
    s.add_text(&AddTextRequest::new(0, (100.0, 600.0), "COMPOUND").with_size(12.0))
        .expect("add");
    // Three further surgeries; the old bug produced one extra copy per surgery.
    for (from, to) in [("Changed", "Ch2"), ("Ch2", "Ch3"), ("Ch3", "Ch4")] {
        s.edit_text(
            &EditRequest::find_replace(0, from, to),
            &EditOptions::default(),
        )
        .expect("further rewrite");
    }
    let (bytes, _) = s.to_full_bytes(&SaveOptions::default()).expect("save");
    let text = page0_text(&bytes);
    assert_eq!(
        count(&text, "COMPOUND"),
        1,
        "the added run must not gain a copy per subsequent edit: {text:?}"
    );
}

/// TEXT ADDED THIS SESSION SURVIVES A REFLOW — which is what this test
/// was always about, and it can now assert it directly.
///
/// # What this replaced, and why the replacement is stronger
///
/// This asserted `ReflowApplyError::PageEditedThisSession`: reflow REFUSED
/// whenever the page carried a non-empty extra `/Contents` stream, because an
/// appended run lives in one and `Pass 251.0`'s planner read the BASE
/// document, which did not contain it. Committing would have emptied the extra
/// and deleted the run. The refusal was the only protection available.
///
/// `Pass 257.0` made the planner read the SESSION's graph.
/// `ContentStream::from_page` concatenates EVERY `/Contents` entry, and the
/// plan replaces only the block's own show-operator spans within that
/// concatenation — so the appended run is in the plan's source and is carried
/// through. The extras are emptied because their content has already been
/// folded into the first stream.
///
/// ⚠ The guard's comment asserted **"Still true after `Pass 257.0`"** and
/// nothing re-measured it. It was false from that commit, and it cost the
/// consuming project reflow on every producer-split page — a CAD sheet with
/// eight producer-authored streams was refused with *"text was added to this
/// page this session"* on a session that had edited nothing, and the remedy it
/// offered ("save and reopen") could not work, because the streams are in the
/// file (`G015`).
///
/// THE NAME IS THE POINT. The old test was called
/// `reflow_refuses_after_text_was_added_rather_than_deleting_it` — the refusal
/// was never the goal, it was the means, and the clause after "rather than" is
/// what actually mattered. Asserting the end instead of the means also makes
/// this the thing that fails if a future change narrows the planner's source
/// again: the guard would have to come back, and this test is what notices.
#[test]
fn reflow_keeps_text_added_this_session() {
    use pdfcer_core::text_edit::ReflowRequest;

    // The REFLOW fixture, not `plain()`: on `plain.pdf` the appended run lands
    // inside block 0's own box, making it a multi-font block, which reflow
    // defers for an unrelated reason (`the block mixes more than one font
    // resource`). That refusal is correct and is not what this test is about —
    // using a page where the appended run forms its own block keeps the
    // assertion pointed at the consolidation.
    let doc = Document::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/reflow/reflow.pdf"),
    )
    .expect("load reflow.pdf");
    let mut s = EditSession::new(doc);
    s.add_text(&AddTextRequest::new(0, (100.0, 600.0), "KEEPME").with_size(12.0))
        .expect("add");

    let report = s
        .reflow_block(0, 0, &ReflowRequest::new().with_wrap_width(400.0))
        .expect("reflow must COMMIT now, not refuse — the appended run is in the plan's source");

    assert!(
        report.extra_objects_emptied >= 1,
        "the extra stream must be consolidated, which is the step that used to be \
         the hazard: {report:?}"
    );

    let (bytes, _) = s
        .to_full_bytes(&SaveOptions::identity())
        .expect("save the reflowed document");
    let text = page0_text(&bytes);
    assert_eq!(
        count(&text, "KEEPME"),
        1,
        "the run appended this session must survive the reflow exactly once — not \
         deleted by the consolidation, and not duplicated by it: {text:?}"
    );
}
