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

#[test]
fn reflow_refuses_after_text_was_added_rather_than_deleting_it() {
    use pdfcer_core::text_edit::{ReflowApplyError, ReflowRequest};

    let mut s = EditSession::new(plain());
    s.add_text(&AddTextRequest::new(0, (100.0, 600.0), "KEEPME").with_size(12.0))
        .expect("add");

    // Reflow plans from the base (which lacks KEEPME); before Pass 251.0 it
    // committed and silently emptied the appended stream, deleting KEEPME.
    // It must now refuse by name instead.
    match s.reflow_block(0, 0, &ReflowRequest::new().with_wrap_width(400.0)) {
        Err(ReflowApplyError::Unsupported(msg)) => {
            assert!(msg.contains("added"), "refusal names the added run: {msg}");
        }
        other => panic!("expected an Unsupported refusal, got {other:?}"),
    }

    // And KEEPME is still there — nothing was deleted.
    let (bytes, _) = s.to_full_bytes(&SaveOptions::default()).expect("save");
    assert_eq!(
        count(&page0_text(&bytes), "KEEPME"),
        1,
        "the added run survived"
    );
}
