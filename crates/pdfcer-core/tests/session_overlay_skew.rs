//! **Every content-editing verb resolves its page from the SESSION, not from
//! the file on disk** (`Pass 186.0`).
//!
//! ## What this file is defending, and why nothing else could
//!
//! `EditSession` had two page-tree readers living side by side. The authoring
//! verbs used the overlay-aware `EditSession::pages()`; every content-editing
//! verb used `page_tree::pages(&self.base)` — the document as it was on disk.
//! That produced two independent defects from one cause:
//!
//! - **A capability was lost.** Content added this session — an image, pasted
//!   objects, flattened fields, added text — lands in a NEW content stream and
//!   often a NEW resource, both of which live only in the staging overlay. The
//!   base-derived page's `/Contents` did not name the new stream, and the base
//!   view could not resolve the new `/XObject`, so the object was invisible to
//!   the model the editing verbs address. The operator found the workaround
//!   himself: *"When I add a new image to a pdf I can't edit it unless I save
//!   the document first."*
//!
//! - **★ The verbs addressed the wrong sheet, and returned `Ok`.** A page
//!   index is computed by a front end against the page set the operator is
//!   looking at. `delete_pages`, `insert_pages`, `reorder_pages` and the merge
//!   verbs all commit into the overlay. So after any structural edit, index N
//!   meant one sheet to the shell and a different sheet to the engine.
//!   `delete_objects` on the wrong page destroys real content with no refusal
//!   and no disclosure.
//!
//! ## ★★ Why the entire existing suite was green on both halves
//!
//! Worth stating plainly, because it is the reusable lesson. Every test in
//! this crate that exercised a content-editing verb did so on a session whose
//! page set had **not** been structurally edited and whose content had **not**
//! been appended to this session. On such a session base and overlay agree by
//! construction, so every assertion passed identically before and after the
//! fix. The suite was not weak; it was **vacuous with respect to this
//! property**, and running it harder would never have said so.
//!
//! The property needs **two verbs in one session** to be visible at all. That
//! is the shape of every test below, and it is the only shape that works.
//!
//! Reported by `pdfcer-gui`
//! (`request_edit_verbs_read_the_base_not_the_overlay`, 2026-08-31), which
//! measured both halves in its own tree first. §4's destructive statement —
//! *"an edit the operator makes on what he sees as page 0 is planned and
//! committed against base page 0, a different sheet"* — was filed as an
//! arithmetic consequence they had **not** driven, because it needs a fixture
//! with distinguishable per-page content.
//! [`an_edit_after_a_page_delete_reaches_the_sheet_the_operator_sees`] drives
//! it on `pageops/four-pages.pdf`, and before the fix it reproduced exactly:
//! `page_objects(3)` on a three-page document returned the text of page four.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, NewImage};
use pdfcer_core::image_import::{self, ImportedImage};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::vector::{Matrix, TextPreview, TransformOptions, VectorObject};
use std::path::{Path, PathBuf};

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn imported(name: &str) -> ImportedImage {
    image_import::import(&std::fs::read(fixture(&format!("images/{name}"))).unwrap()).unwrap()
}

/// A 400 x 400 single-page document with one content stream.
fn plain() -> EditSession {
    EditSession::new(Document::load(&fixture("dimension/plain-base.pdf")).unwrap())
}

/// Four pages whose text is distinguishable: "Page One" .. "Page Four". The
/// distinguishability is the whole point — a per-sheet assertion cannot be
/// made against a fixture whose pages look alike.
fn four_pages() -> EditSession {
    EditSession::new(Document::load(&fixture("pageops/four-pages.pdf")).unwrap())
}

/// The decoded text of every text object the engine models on `page_index`.
///
/// This reads through `page_objects`, deliberately: it is the exact model the
/// geometry verbs address, so asserting on it asserts on what a `delete` or a
/// `move` would actually hit.
fn page_text(s: &mut EditSession, page_index: usize) -> String {
    let objects = s.page_objects(page_index).unwrap();
    let mut out = String::new();
    for o in &objects.objects {
        if let VectorObject::Text(t) = o
            && let TextPreview::Decoded { text, .. } = &t.preview
        {
            out.push_str(text);
            out.push(' ');
        }
    }
    out
}

fn place_image(s: &mut EditSession, page_index: usize) {
    let img = imported("rgb8.png");
    s.add_image(&NewImage::new(
        page_index,
        Rect {
            llx: 50.0,
            lly: 50.0,
            urx: 150.0,
            ury: 150.0,
        },
        &img,
    ))
    .unwrap();
}

// -------------------------------------------------------------------------
// Symptom A — content added this session is editable immediately
// -------------------------------------------------------------------------

/// The headline: `add_image` must be visible to the model the geometry verbs
/// address, with no save and reopen in between.
#[test]
fn an_image_added_this_session_is_in_the_model_immediately() {
    let mut s = plain();
    let before = s.page_objects(0).unwrap().objects.len();
    place_image(&mut s, 0);
    let after = s.page_objects(0).unwrap().objects.len();
    assert_eq!(
        after,
        before + 1,
        "add_image must add exactly one object to the page model"
    );
}

/// ★ The memo is the half that could still fail after the page tree was
/// fixed, and it would fail silently. `add_image` touches neither the first
/// content object nor its staged span, so the old cache key — which was
/// exactly `(content_id, that span)` — could not see it, and served the
/// pre-insert model back to every caller.
///
/// The first call here is what fills the cache. Without it the test would
/// pass on a broken key, which makes that line load-bearing rather than
/// incidental.
#[test]
fn the_model_memo_notices_an_appended_content_stream() {
    let mut s = plain();
    let primed = s.page_objects(0).unwrap().objects.len();
    place_image(&mut s, 0);
    let after = s.page_objects(0).unwrap().objects.len();
    assert!(
        after > primed,
        "a cache hit served a stale model: {primed} -> {after}"
    );
}

/// The operator's actual gesture: place an image, then drag it. This is the
/// call that used to answer `ObjectOutOfRange` for an object plainly on the
/// page and plainly visible on the canvas.
#[test]
fn a_just_placed_image_can_be_transformed() {
    let mut s = plain();
    place_image(&mut s, 0);
    let last = s.page_objects(0).unwrap().objects.len() - 1;
    s.transform_objects(
        0,
        &[last],
        Matrix::translate(10.0, 10.0),
        TransformOptions::default(),
    )
    .expect("the image placed a moment ago must be transformable");
}

/// ★ The interaction the request named and asked to land in the same change.
///
/// A page's first text edit folds every later content stream into the first
/// and empties them in place. On a base-read model the appended image was not
/// part of what got folded, so the fold **erased it** — and because the model
/// never contained the image, nothing upstream could notice. This asserts the
/// image survives its page's first text edit.
#[test]
fn a_text_edit_does_not_erase_an_image_added_this_session() {
    let mut s = four_pages();
    // ★ The baseline is taken BEFORE the image, and `with_image` is asserted
    // against it rather than merely recorded. Without that assertion this test
    // is vacuous on a regression: if the image is invisible to the model, both
    // measurements are the base count and `after == with_image` holds happily
    // while the image is being erased. Confirmed by sabotage, not assumed.
    let base_count = s.page_objects(0).unwrap().objects.len();
    place_image(&mut s, 0);
    let with_image = s.page_objects(0).unwrap().objects.len();
    assert_eq!(
        with_image,
        base_count + 1,
        "the image must be in the model before the text edit is even attempted"
    );

    let req = pdfcer_core::text_edit::EditRequest::find_replace(0, "Page One", "Sheet One");
    s.edit_text(&req, &pdfcer_core::text_edit::EditOptions::default())
        .expect("the text being replaced is present on page 1");

    let after = s.page_objects(0).unwrap().objects.len();
    assert_eq!(
        after,
        base_count + 1,
        "the text edit dropped the image it folded over"
    );
    assert!(
        page_text(&mut s, 0).contains("Sheet One"),
        "the text edit itself did not take"
    );
}

// -------------------------------------------------------------------------
// Symptom B — a page index means the same sheet on both sides
// -------------------------------------------------------------------------

/// The crisp count mismatch: page 3 of a three-page document must not resolve.
#[test]
fn a_page_index_past_the_end_of_the_edited_document_is_refused() {
    let mut s = four_pages();
    assert_eq!(s.pages().unwrap().len(), 4);
    s.delete_pages(&[0]).unwrap();
    assert_eq!(s.pages().unwrap().len(), 3);
    assert!(
        s.page_objects(3).is_err(),
        "page 3 of a 3-page document resolved"
    );
}

/// ★★ THE DESTRUCTIVE STATEMENT, driven.
///
/// After deleting page one, the sheet the operator calls page 0 is the one
/// that says "Page Two". If the engine answers with "Page One" content it is
/// editing a sheet nobody selected, and every layer above it — including the
/// undo entry — records the wrong thing. There is no refusal available to
/// catch this: the index is in range on both sides.
#[test]
fn an_edit_after_a_page_delete_reaches_the_sheet_the_operator_sees() {
    let mut s = four_pages();
    assert!(page_text(&mut s, 0).contains("Page One"));

    s.delete_pages(&[0]).unwrap();

    let t0 = page_text(&mut s, 0);
    assert!(
        t0.contains("Page Two"),
        "index 0 after deleting page one must be the sheet that says Page Two, got: {t0}"
    );
    let t2 = page_text(&mut s, 2);
    assert!(
        t2.contains("Page Four"),
        "index 2 must be the last remaining sheet, got: {t2}"
    );
}

/// The same skew asserted over the whole surviving range, and asserted on
/// **which sheet each index names** rather than only on whether it resolves.
///
/// ★ The identity assertion is what makes this bite. An earlier cut checked
/// only `is_ok()`/`is_err()`, and sabotage showed it passing against the
/// base-reading engine — because deleting the LAST page removes its content
/// object from the overlay, so `page_objects(3)` failed for an unrelated
/// reason and looked like the right answer. A test that passes for the wrong
/// reason is indistinguishable from one that works until the day it matters.
#[test]
fn every_surviving_index_names_the_right_sheet_after_a_structural_edit() {
    let mut s = four_pages();
    s.delete_pages(&[1]).unwrap();
    assert_eq!(s.pages().unwrap().len(), 3);
    for (i, expected) in ["Page One", "Page Three", "Page Four"].iter().enumerate() {
        let got = page_text(&mut s, i);
        assert!(
            got.contains(expected),
            "index {i} must name the sheet that says {expected}, got: {got}"
        );
    }
    assert!(s.page_objects(3).is_err(), "index 3 must not resolve");
}

/// `reflow_block`'s planner is base-indexed and cannot be handed an overlay
/// page index. It must say so **by name** rather than splice one sheet's
/// reflowed bytes into another sheet's content object — which is what the
/// naive half of this Pass would have made it do.
#[test]
fn reflow_refuses_once_the_page_set_has_changed() {
    let mut s = four_pages();
    s.delete_pages(&[0]).unwrap();
    let err = s
        .reflow_block(0, 0, &pdfcer_core::text_edit::ReflowRequest::default())
        .expect_err("reflow must refuse after a structural page edit");
    let msg = err.to_string();
    assert!(
        msg.contains("page set was changed"),
        "the refusal must name its reason, got: {msg}"
    );
}

// -------------------------------------------------------------------------
// The cross-check a shell can run continuously
// -------------------------------------------------------------------------

/// `page_content_generation` is what lets a front end assert model agreement
/// without paying for a second decomposition. It must be stable across a
/// repeat call and must change when the page's drawable content does.
#[test]
fn the_page_generation_moves_only_when_the_page_model_does() {
    let mut s = plain();
    let g0 = s.page_content_generation(0).unwrap();
    assert_eq!(
        g0,
        s.page_content_generation(0).unwrap(),
        "the generation must not move on its own"
    );

    place_image(&mut s, 0);
    let g1 = s.page_content_generation(0).unwrap();
    assert_ne!(
        g0, g1,
        "an appended content stream must move the generation"
    );

    let last = s.page_objects(0).unwrap().objects.len() - 1;
    s.transform_objects(
        0,
        &[last],
        Matrix::translate(5.0, 5.0),
        TransformOptions::default(),
    )
    .unwrap();
    assert_ne!(
        g1,
        s.page_content_generation(0).unwrap(),
        "a content rewrite must move the generation"
    );
}

/// It is indexed against the SESSION's page set, like every other verb now —
/// and the sheet that moves keeps its number, which is what makes it usable
/// as an agreement check across a structural edit.
#[test]
fn the_page_generation_is_indexed_against_the_edited_page_set() {
    let mut s = four_pages();
    let g_second_sheet = s.page_content_generation(1).unwrap();
    s.delete_pages(&[0]).unwrap();
    assert_eq!(
        s.page_content_generation(0).unwrap(),
        g_second_sheet,
        "the sheet that was index 1 is index 0 now, and is the same sheet"
    );
    assert!(s.page_content_generation(3).is_err());
}
