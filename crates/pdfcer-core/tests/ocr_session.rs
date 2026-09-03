//! # OCR as an **edit to the open session**, not a different file somewhere
//!
//! Integration test for [`EditSession::add_ocr_layer`].
//!
//! # What was wrong, and it reached the operator
//!
//! `ocr::layer::add_ocr_layer` takes an immutable `&Document` and returns a
//! whole new PDF, which made recognition the one capability in pdfcer that was
//! not an edit. A shell holding an open session could only offer *"here is a
//! different file, somewhere else"* — its own in-place save path cannot be
//! used on a document it does not have. The operator's words, relayed on the
//! request channel: *"Why do I have to save a copy instead of just go back
//! into my pdf and save over it?"*
//!
//! ## ★★ The trap underneath it, which is the half a guard could not fix
//!
//! The one-shot reads the document's **base** revision. Run after any edit, it
//! would produce a recognised copy that **silently omitted that edit** — so
//! the consuming shell refused to run OCR once `edit_epoch != 0`, which was
//! the right call. But `edit_epoch` never returns to zero, not even after a
//! successful save, so **OCR died for the rest of the session the first time
//! anything was edited.**
//!
//! `session_ocr_sees_an_edit_made_earlier_in_the_session` is the test that
//! matters here: it makes an edit, then OCRs, then checks the edit is still
//! there. A verb that read the base would fail it, and no guard anywhere could
//! have made the base-reading version correct.
//!
//! # The properties asserted
//!
//! | Property | Asserted by |
//! |---|---|
//! | the layer lands in the session and survives its ordinary save | `the_layer_is_in_the_session_and_saves_through_the_normal_path` |
//! | OCR composes with earlier edits instead of dropping them | `session_ocr_sees_an_edit_made_earlier_in_the_session` |
//! | a forty-page run is **one** undo, not forty | `a_multi_page_run_is_one_undo_entry` |
//! | undo restores the pages byte-identically | `undo_restores_the_document_to_what_it_was` |
//! | one page named twice is refused, not silently half-written | `naming_one_page_twice_is_refused` |
//! | a refused run leaves the session and its undo stack untouched | `a_refused_run_leaves_no_trace` |
//!
//! ## Why extraction is the only oracle
//!
//! Everything this feature does is **invisible by construction** (mode 3,
//! §9.3.6 Table 106). The page looks identical whether the layer is perfect,
//! mis-scaled, mirrored or absent. So every positive assertion here goes
//! through a real text extraction of the saved bytes; asserting on the
//! session's object graph would prove only that objects were created.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditSession, OcrPageLayer};
use pdfcer_core::ocr::layer::{OcrLayerError, OcrLayerOptions};
use pdfcer_core::ocr::{OcrPage, RecognizedWord};
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_core::text_extract::{self, ExtractOptions};
use pdfcer_core::writer::SaveOptions;

fn fixture(dir: &str, name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(dir)
        .join(name)
}

/// A one-page session.
fn one_page_session() -> EditSession {
    let doc = Document::load(&fixture("addtext", "plain.pdf")).expect("fixture loads");
    EditSession::new(doc)
}

/// A two-page session — enough to tell one undo entry from N.
fn two_page_session() -> EditSession {
    let doc = Document::load(&fixture("pageops", "two-pages.pdf")).expect("fixture loads");
    EditSession::new(doc)
}

fn word(text: &str, llx: f64, lly: f64, urx: f64, ury: f64) -> RecognizedWord {
    RecognizedWord {
        text: text.to_owned(),
        rect: Rect::from_corners(llx, lly, urx, ury),
        confidence: Some(0.87),
    }
}

/// A recognised page whose single word is `text`, on a plausible line.
fn one_word(text: &str) -> OcrPage {
    OcrPage {
        words: vec![word(text, 72.0, 700.0, 200.0, 712.0)],
        confidence_available: true,
    }
}

/// Every text run extracted from `page_index` of `bytes`, concatenated.
fn page_text(bytes: &[u8], page_index: usize) -> String {
    let doc = Document::from_bytes(bytes.to_vec()).expect("output reloads");
    let pages = page_tree::pages(&doc).expect("page tree walks");
    let page = text_extract::extract_page(
        &doc,
        &pages[page_index],
        page_index,
        &ExtractOptions::default(),
    )
    .expect("page extracts");
    page.runs
        .iter()
        .map(|r| r.text.as_str())
        .collect::<String>()
}

/// Save incrementally through the real writer, the way any caller would.
fn save(session: &EditSession) -> Vec<u8> {
    session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("session saves")
        .0
}

/// ★ The headline: the layer is part of the session, and the session's own
/// save writes it. No second file, no copy, no separate path.
#[test]
fn the_layer_is_in_the_session_and_saves_through_the_normal_path() {
    let mut s = one_page_session();
    let recognised = one_word("INVOICE");
    let reports = s
        .add_ocr_layer(
            &[OcrPageLayer {
                page_index: 0,
                recognised: &recognised,
            }],
            &OcrLayerOptions::new(),
        )
        .expect("the layer is written");

    assert_eq!(reports.len(), 1, "one report per requested page");
    assert!(reports[0].words_written > 0);

    let text = page_text(&save(&s), 0);
    assert!(
        text.contains("INVOICE"),
        "the recognised word must come back out of the SESSION's own save; got {text:?}"
    );
}

/// ★★★ THE TEST THAT NO GUARD COULD HAVE SATISFIED.
///
/// Make an ordinary edit, then run OCR, then check the edit is still there.
/// The one-shot reads the document's BASE revision, so a session verb built on
/// it would produce a document missing the earlier edit — silently. That is
/// why the consuming shell refused to run OCR at all once `edit_epoch != 0`,
/// and why moving the read onto the session graph removes the problem rather
/// than policing it.
#[test]
fn session_ocr_sees_an_edit_made_earlier_in_the_session() {
    let mut s = one_page_session();

    // An ordinary edit first. Any verb that changes page 0's content will do;
    // `add_text` is used because its output is extractable, so the assertion
    // below can see it rather than infer it.
    // ★ `AddTextRequest::new`, not a struct literal: the type is
    // `#[non_exhaustive]`, so a literal does not compile out-of-crate. An
    // in-crate test would never have discovered that, which is exactly why
    // this file is an integration test.
    let req = pdfcer_core::text_edit::AddTextRequest::new(0, (72.0, 600.0), "EDITED-FIRST");
    s.add_text(&req).expect("the text is added");
    assert!(
        !s.dirty_set().is_empty(),
        "the session must be dirty for this test to mean anything"
    );

    let recognised = one_word("OCRSECOND");
    s.add_ocr_layer(
        &[OcrPageLayer {
            page_index: 0,
            recognised: &recognised,
        }],
        &OcrLayerOptions::new(),
    )
    .expect("OCR runs on a dirty session");

    let text = page_text(&save(&s), 0);
    assert!(
        text.contains("EDITED-FIRST"),
        "★ the earlier edit was DROPPED -- the verb read the base revision \
         rather than the session. This is the exact silent omission the \
         consuming shell was refusing to risk; got {text:?}"
    );
    assert!(
        text.contains("OCRSECOND"),
        "the OCR layer must be there too; got {text:?}"
    );
}

/// ★★ A multi-page run is ONE undo entry.
///
/// Recognising forty pages and then pressing undo forty times is not a
/// feature. Asserted on a two-page document because two is enough to
/// distinguish one entry from N, and the loop that would produce N is the same
/// loop at any size.
#[test]
fn a_multi_page_run_is_one_undo_entry() {
    let mut s = two_page_session();
    let a = one_word("PAGEONE");
    let b = one_word("PAGETWO");

    let reports = s
        .add_ocr_layer(
            &[
                OcrPageLayer {
                    page_index: 0,
                    recognised: &a,
                },
                OcrPageLayer {
                    page_index: 1,
                    recognised: &b,
                },
            ],
            &OcrLayerOptions::new(),
        )
        .expect("both pages are written");
    assert_eq!(reports.len(), 2);

    let bytes = save(&s);
    assert!(page_text(&bytes, 0).contains("PAGEONE"));
    assert!(page_text(&bytes, 1).contains("PAGETWO"));

    assert_eq!(
        s.undo_kind(),
        Some(CommandKind::AddOcrLayer),
        "the run must be one AddOcrLayer command"
    );
    assert_eq!(s.undo(), Some(CommandKind::AddOcrLayer));
    assert_eq!(
        s.undo_kind(),
        None,
        "★ ONE undo must remove the WHOLE run -- a second entry here means \
         the verb committed per page"
    );
}

/// Undo puts the document back, on every page the run touched.
#[test]
fn undo_restores_the_document_to_what_it_was() {
    let mut s = two_page_session();
    let before = save(&s);

    let a = one_word("PAGEONE");
    let b = one_word("PAGETWO");
    s.add_ocr_layer(
        &[
            OcrPageLayer {
                page_index: 0,
                recognised: &a,
            },
            OcrPageLayer {
                page_index: 1,
                recognised: &b,
            },
        ],
        &OcrLayerOptions::new(),
    )
    .expect("written");
    s.undo().expect("undo runs");

    let after = save(&s);
    assert_eq!(
        page_text(&after, 0),
        page_text(&before, 0),
        "page 0's text must be what it was before the run"
    );
    assert_eq!(page_text(&after, 1), page_text(&before, 1));
}

/// ★ One page named twice is refused.
///
/// Every page is planned against the graph as it stands *before* the commit —
/// that is what makes a multi-page run one undo entry. Two entries for one
/// page would both append to that page's ORIGINAL `/Contents`, and the second
/// page-dict write would clobber the first: one layer written, one lost, and a
/// report claiming both. A refusal is the only answer that is not a quiet
/// wrong result.
#[test]
fn naming_one_page_twice_is_refused() {
    let mut s = one_page_session();
    let a = one_word("FIRST");
    let b = one_word("SECOND");
    let err = s
        .add_ocr_layer(
            &[
                OcrPageLayer {
                    page_index: 0,
                    recognised: &a,
                },
                OcrPageLayer {
                    page_index: 0,
                    recognised: &b,
                },
            ],
            &OcrLayerOptions::new(),
        )
        .expect_err("a duplicated page must be refused");
    assert!(
        matches!(err, OcrLayerError::DuplicatePage { page_index: 0 }),
        "expected DuplicatePage, got {err:?}"
    );
}

/// ★★ A refused run leaves NOTHING behind — not an object, not an undo entry.
///
/// The verb plans every page before allocating anything, so a run that refuses
/// on its second page has not already burnt the first page's object numbers or
/// pushed its bytes into the staging buffer. Asserted through the two things a
/// caller can actually observe: the undo stack is still empty, and the saved
/// bytes are unchanged.
#[test]
fn a_refused_run_leaves_no_trace() {
    let mut s = one_page_session();
    let before = save(&s);
    let good = one_word("GOOD");
    let empty = OcrPage {
        words: Vec::new(),
        confidence_available: false,
    };

    // Page 0 plans fine; the second entry has no placeable word, so the run
    // refuses AFTER page 0 has been planned and BEFORE anything is allocated.
    let err = s
        .add_ocr_layer(
            &[
                OcrPageLayer {
                    page_index: 0,
                    recognised: &good,
                },
                OcrPageLayer {
                    page_index: 1,
                    recognised: &empty,
                },
            ],
            &OcrLayerOptions::new(),
        )
        .expect_err("a page with nothing to write must refuse the run");
    assert!(
        matches!(
            err,
            OcrLayerError::NothingToWrite | OcrLayerError::PageIndex(_)
        ),
        "expected NothingToWrite or PageIndex, got {err:?}"
    );

    assert_eq!(
        s.undo_kind(),
        None,
        "★ a refused run must not leave an undo entry"
    );
    assert_eq!(
        save(&s),
        before,
        "a refused run must not change the document's bytes"
    );
}

/// An empty request is a no-op that commits nothing.
///
/// Not a refusal: asking for zero pages of OCR is a legitimate thing for a
/// caller with a filtered page list to do. But it must not leave an undo entry
/// that undoes nothing, which is what a `commit` outside the emptiness check
/// would produce.
#[test]
fn an_empty_request_commits_nothing() {
    let mut s = one_page_session();
    let reports = s
        .add_ocr_layer(&[], &OcrLayerOptions::new())
        .expect("an empty run is not an error");
    assert!(reports.is_empty());
    assert_eq!(s.undo_kind(), None, "no undo entry for a no-op");
    assert!(
        s.dirty_set().is_empty(),
        "an empty run must not dirty the session"
    );
}
