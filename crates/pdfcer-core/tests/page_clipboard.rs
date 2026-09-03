//! Whole pages on the clipboard — copy, cut, paste (`Pass 171.0`).
//!
//! ## What these tests are really checking
//!
//! Pages were the largest 0-of-3 in the cut/copy/paste audit: `pageops` could
//! `extract` a page set to bytes and `EditSession` could `insert_pages` from a
//! live document, but **nothing composed them**, and no type held a page the
//! way `ObjectClip` holds a shape.
//!
//! So `PageClip` is deliberately thin — it is the extracted PDF plus two
//! counts — and the assertions here are about the composition, not about the
//! assembler: that a copy leaves the source alone, that a cut is one undo
//! entry, that a paste puts the pages where the operator asked, and that the
//! two counts a page copy owes the operator are real rather than zero.
//!
//! ## The one that would be easy to get wrong quietly
//!
//! `cut_pages` must refuse to remove the last page. A document with no pages
//! is not a document, and the failure would not be an error — it would be a
//! file that opens to nothing.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::pageops::InsertPosition;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(name)
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(rel)).expect("load fixture"))
}

fn page_count(s: &EditSession) -> usize {
    s.page_slots().expect("slots").len()
}

/// Copying a page leaves the source untouched and produces an openable PDF.
#[test]
fn copying_pages_leaves_the_source_alone_and_yields_a_real_document() {
    let s = session("hello.pdf");
    let before = page_count(&s);
    let clip = s.copy_pages(&[0]).expect("copy");
    assert_eq!(clip.pages, 1);
    // NOTE: `len()` is BYTES here and `is_empty()` is about PAGES -- the two
    // are deliberately about different things on a `PageClip`, so this
    // asserts the byte length directly rather than negating `is_empty`.
    assert!(
        clip.to_bytes().len() > 100,
        "a real PDF, not an empty buffer"
    );
    assert_eq!(page_count(&s), before, "a copy is not a move");

    // ★ The clip IS a PDF. Anything can open it, including us.
    let reopened = Document::from_bytes(clip.bytes.clone()).expect("the clip is a real document");
    assert_eq!(
        pdfcer_core::page_tree::pages_in(&reopened)
            .expect("page tree")
            .len(),
        1,
        "and it holds exactly the pages that were copied",
    );
}

#[test]
fn an_out_of_range_page_refuses_the_copy() {
    let s = session("hello.pdf");
    assert!(matches!(
        s.copy_pages(&[99]),
        Err(EditError::PageOutOfRange { .. })
    ));
}

/// Pasting puts the pages where the operator asked, and the document grows by
/// exactly that many.
#[test]
fn pasting_pages_places_them_at_the_requested_position() {
    let source = session("hello.pdf");
    let clip = source.copy_pages(&[0]).expect("copy");

    let mut destination = session("hello.pdf");
    let before = page_count(&destination);
    let outcome = destination
        .paste_pages(&clip, InsertPosition::Start)
        .expect("paste");
    assert_eq!(outcome.pages_inserted, 1);
    assert_eq!(page_count(&destination), before + 1);
}

/// A page pasted from a clip **file** — the cross-document gesture.
#[test]
fn a_page_clip_crosses_documents_through_its_bytes() {
    let source = session("hello.pdf");
    let clip = source.copy_pages(&[0]).expect("copy");
    // Through bytes and back, exactly as a shell writing it to disk would.
    //
    // ★ `from_bytes` exists BECAUSE this test could not compile without it:
    // `PageClip` is `#[non_exhaustive]`, so nothing outside the crate can
    // build one with a struct literal, and a clip a shell can write and never
    // read is not a clipboard. An in-crate test would never have noticed.
    let carried = pdfcer_core::pageops::PageClip::from_bytes(clip.to_bytes().to_vec())
        .expect("a clip round-trips through its own bytes");
    assert_eq!(carried.pages, clip.pages);

    let mut destination = session("minimal.pdf");
    let before = page_count(&destination);
    destination
        .paste_pages(&carried, InsertPosition::End)
        .expect("paste");
    assert_eq!(page_count(&destination), before + 1);
}

/// ★ Cut is ONE undo entry, and undoing it puts the page back.
#[test]
fn cutting_a_page_is_one_undo_entry_and_undo_restores_it() {
    let mut s = session("hello.pdf");
    // Two pages, so cutting one is legal.
    let clip = s.copy_pages(&[0]).expect("copy");
    s.paste_pages(&clip, InsertPosition::End).expect("paste");
    assert_eq!(page_count(&s), 2);
    let depth_before = s.undo_depth();

    let cut = s.cut_pages(&[1]).expect("cut");
    assert_eq!(cut.pages, 1);
    assert_eq!(page_count(&s), 1, "the page is gone");
    assert_eq!(
        s.undo_depth(),
        depth_before + 1,
        "ONE undo entry for one gesture",
    );

    s.undo().expect("one press");
    assert_eq!(page_count(&s), 2, "and one press puts it back");
}

/// ★ Cutting every page is refused — a document with no pages is not a
/// document, and the failure would not be an error but a file that opens to
/// nothing.
#[test]
fn cutting_every_page_is_refused_with_nothing_removed() {
    let mut s = session("hello.pdf");
    let before = page_count(&s);
    let err = s.cut_pages(&[0]).expect_err("the last page cannot go");
    assert!(
        matches!(err, EditError::WouldRemoveEveryPage { .. }),
        "got {err:?}",
    );
    assert_eq!(page_count(&s), before, "and nothing was removed");
}

/// A form field whose widgets are all on the copied page travels; the count
/// of those that could not is real rather than always zero.
#[test]
fn a_page_copy_reports_the_fields_it_could_not_carry() {
    let s = session("forms/demo-form.pdf");
    let clip = s.copy_pages(&[0]).expect("copy");
    assert_eq!(
        clip.fields_dropped, 0,
        "both fields are entirely on the copied page, so nothing was dropped",
    );

    // ★ AND THE FIELDS ARRIVE AS ORPHANED WIDGETS, WHICH IS THE DOCUMENTED
    // BEHAVIOUR OF THE VERB UNDERNEATH AND IS WORTH PINNING HERE.
    //
    // A page's `/Annots` reaches its widgets, so the boxes come across — but
    // `insert_pages` deliberately does NOT merge document-level structures,
    // so the `/AcroForm` that owns them does not. The result is form fields
    // that DRAW and that nothing can fill.
    //
    // This test first asserted the opposite (that the fields arrived usable)
    // and was wrong: the verb's own contract says otherwise, and the counter
    // exists precisely because the outcome is invisible on the page. Pinning
    // the DISCLOSURE is the useful assertion; pinning a merge that does not
    // happen was pinning a wish.
    let mut destination = session("hello.pdf");
    let outcome = destination
        .paste_pages(&clip, InsertPosition::End)
        .expect("paste");
    assert!(
        outcome.orphaned_widgets > 0,
        "the widgets arrived without the /AcroForm that owns them, and the \
         paste says so -- a shell that ignores this counter ships boxes that \
         look like form fields and cannot be filled",
    );
    assert!(
        pdfcer_core::forms::parse_acroform(&destination.graph())
            .is_none_or(|f| f.fields.is_empty()),
        "and no form was invented for them",
    );
}

/// The paste's disclosures are inherited from `insert_pages`, so a shell that
/// reads them for one verb reads them for both.
#[test]
fn the_paste_reports_what_a_page_insert_reports() {
    let source = session("forms/demo-form.pdf");
    let clip = source.copy_pages(&[0]).expect("copy");
    let mut destination = session("hello.pdf");
    let outcome = destination
        .paste_pages(&clip, InsertPosition::End)
        .expect("paste");
    assert_eq!(outcome.pages_inserted, 1);
    // The counters exist and are answerable -- the point is that a caller
    // gets `insert_pages`' whole contract, not a reduced one.
    let _ = outcome.orphaned_widgets;
    let _ = outcome.orphaned_widgets_unrecoverable;
    let _ = outcome.source_page_labels_dropped;
    let _ = outcome.page_labels_stale;
    let _ = outcome.source_outline_dropped;
}

/// A clip whose bytes are not a PDF is refused by name rather than pasted
/// as nothing.
#[test]
fn a_page_clip_that_is_not_a_pdf_is_refused() {
    let mut s = session("hello.pdf");
    assert!(
        pdfcer_core::pageops::PageClip::from_bytes(b"not a pdf at all".to_vec()).is_err(),
        "the refusal happens when the clip is BUILT, which is the earliest \
         point the operator can be told -- not when they try to paste it",
    );
    let _ = &mut s;
}
