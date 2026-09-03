//! Fuzz target: a **sequence** of structural page operations
//! (`pdfcer_core::edit` + `pdfcer_core::pageops`).
//!
//! `writer_roundtrip` proves that a save does not corrupt an untouched
//! document, and that one edit does not perturb its neighbours. Neither
//! is the claim Pass 3.2 introduces. This target's claim is:
//!
//! > no *sequence* of structural operations, on any input that loads at
//! > all, produces a document pdfcer cannot read back — and undoing all
//! > of them reproduces the input byte for byte.
//!
//! The distinction matters because structural operations **compose in
//! ways single operations do not**. A delete changes the page indices a
//! following reorder addresses; a reorder changes which ancestor a
//! following rotate inherits from; a delete that empties a page-tree
//! node frees that node, which a following delete must not free twice.
//! Every one of those is a two-operation bug that a one-operation test
//! cannot reach, and decision 007 **W9** names the failure shape they
//! share — *"files Acrobat tolerates and stricter readers reject … the
//! obvious test passes."*
//!
//! ## How the input is split
//!
//! The first bytes drive a small op-code machine; the rest is the PDF.
//! The split is at a fixed offset rather than being length-prefixed so
//! that a mutation to the program half never shifts the document half —
//! a fuzzer that has found an interesting document keeps it while it
//! explores operation sequences over it, which is the whole point of
//! putting the two in one corpus entry.
//!
//! ## What is asserted, and what is deliberately not
//!
//! Asserted:
//!
//! 1. **No panic, ever.** `pdfcer-core` is panic-free by policy (decision
//!    001 §6.1 item 5) and every operation here runs on attacker-shaped
//!    input.
//! 2. **Whatever is saved, reloads.** A structural edit that produces a
//!    file pdfcer itself cannot parse is the worst outcome available —
//!    strictly worse than refusing the edit.
//! 3. **The page count the writer produced is the page count the reader
//!    finds.** Catches a `/Count` left inconsistent with `/Kids`, a
//!    freed node still referenced, and a free entry that resurrects an
//!    object.
//! 4. **Undo-everything ⇒ byte-identical save** (§11.1). The hardest
//!    assertion here, and the one that catches a deletion tracked
//!    outside the save-time diff: values-only undo would restore the
//!    page tree and leave the free entries behind.
//!
//! Not asserted:
//!
//! - **A refusal is not a failure.** Deleting the last page, a
//!   certification signature that forbids the change, a page tree with a
//!   cycle — all are correct outcomes, and a fuzzer that treats
//!   principled refusals as bugs trains the implementation to guess
//!   instead of refuse.
//! - **Semantic equivalence of the produced document.** Reordering pages
//!   is *supposed* to change what the document means; there is no
//!   invariant to state there beyond structural integrity.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::pageops::{DocumentView, SplitCriterion};
use pdfcer_core::writer::SaveOptions;

/// How many bytes at the front of the input drive the operation
/// sequence. Everything after is the candidate PDF.
const PROGRAM_LEN: usize = 8;

/// Maximum operations applied. Bounded because each one walks the page
/// tree, and a fuzz iteration that spends a second is an iteration the
/// fuzzer does not spend finding anything.
const MAX_OPS: usize = 6;

fuzz_target!(|data: &[u8]| {
    let (program, body) = data.split_at(data.len().min(PROGRAM_LEN));
    if body.len() < 32 {
        return;
    }
    // Only inputs that load are interesting: `load_document` already
    // fuzzes the parser, and re-fuzzing it here would spend the budget
    // on ground another target covers.
    let Ok(doc) = Document::from_bytes(body.to_vec()) else {
        return;
    };
    // A document with no readable page tree has no structural operation
    // to perform; refusing is the correct behaviour and is covered by
    // the unit tests.
    if pdfcer_core::page_tree::page_slots(&doc).is_err() {
        return;
    }

    let original = doc.bytes().to_vec();
    let mut session = EditSession::new(doc);
    let mut applied = 0usize;

    for byte in program.iter().take(MAX_OPS) {
        let Ok(pages) = session.pages() else { break };
        let count = pages.len();
        if count == 0 {
            break;
        }
        // Two nibbles: which operation, and a parameter. Deriving the
        // page index modulo the CURRENT count keeps every op in range
        // without the program having to track a count it cannot see —
        // out-of-range refusals are covered by the unit tests, and
        // spending fuzz iterations on them would crowd out the
        // composition bugs this target exists to find.
        let op = byte & 0x03;
        let param = usize::from(byte >> 2);
        let index = param % count;

        let ok = match op {
            0 => session.delete_pages(&[index]).is_ok(),
            1 => {
                // A rotation of the whole document by a derived turn.
                let delta = (i32::try_from(param % 4).unwrap_or(0) - 1) * 90;
                let all: Vec<usize> = (0..count).collect();
                session.rotate_pages(&all, delta).is_ok()
            }
            2 => {
                // Rotate one page, so the single-page and batch paths
                // both get exercised in composition.
                session.rotate_pages(&[index], 90).is_ok()
            }
            _ => {
                // A rotation of the page order: move `index` to the
                // front. Cheap to derive, and it reliably moves pages
                // between ancestors in a nested tree — which is where
                // the attribute-materialization rule lives.
                let mut order: Vec<usize> = Vec::with_capacity(count);
                order.push(index);
                order.extend((0..count).filter(|i| *i != index));
                session.reorder_pages(&order).is_ok()
            }
        };
        if ok {
            applied += 1;
        }
    }

    // --- assertion 2/3: whatever was saved, reloads, with the page
    //     count the in-memory view reported.
    let expected_pages = session.pages().map(|pages| pages.len());
    if let Ok((bytes, _)) = session.to_incremental_bytes(&SaveOptions::identity()) {
        match Document::from_bytes(bytes) {
            Ok(reloaded) => {
                if let (Ok(want), Ok(got)) = (
                    expected_pages.as_ref().map_err(|_| ()),
                    pdfcer_core::page_tree::pages(&reloaded).map(|p| p.len()),
                ) {
                    assert_eq!(
                        *want, got,
                        "the saved file's page count disagrees with the edited view"
                    );
                }
                // The producers run over the RESULT of the edits, so
                // they see page trees no fixture would have built —
                // spliced, partly freed, re-parented.
                exercise_producers(&reloaded);
            }
            Err(err) => panic!("a structural edit produced an unloadable file: {err}"),
        }
    }

    // --- assertion 4: undo everything ⇒ byte-identical.
    if applied > 0 {
        while session.undo().is_some() {}
        if let Ok((bytes, report)) = session.to_incremental_bytes(&SaveOptions::identity()) {
            assert!(
                report.byte_identical && bytes == original,
                "undoing every structural edit did not reproduce the input \
                 (ARCHITECTURE.md §11.1: the dirty set is a diff against the base, \
                 never the union of the commands run)"
            );
        }
    }
});

/// Put the document-producing operations over `doc`.
///
/// Split out because they take a completely different path from the
/// editing ones — a deep-copy closure with a barrier, rather than a
/// page-tree splice — and because they are the ones that build a file
/// from nothing, where a malformed output has no prior revision to fall
/// back on.
fn exercise_producers(doc: &Document) {
    let Ok(pages) = pdfcer_core::page_tree::pages(doc) else {
        return;
    };
    if pages.is_empty() {
        return;
    }
    let view = DocumentView::new(doc, doc.bytes(), doc.version());

    // Extract the first page. Every produced file must reload — an
    // extraction that cannot be opened is the failure mode with no
    // recovery, since there is no earlier revision inside it.
    if let Ok((bytes, report)) = pdfcer_core::pageops::extract(&view, &[0]) {
        let reloaded = Document::from_bytes(bytes).expect("extract produced an unloadable file");
        let got = pdfcer_core::page_tree::pages(&reloaded)
            .expect("extract produced an unwalkable page tree")
            .len();
        assert_eq!(
            got, report.pages,
            "extract's page count is not what it wrote"
        );
    }
    // Split, which is repeated extract and additionally exercises the
    // naming/collision path.
    if pages.len() > 1
        && let Ok(parts) =
            pdfcer_core::pageops::split(&view, &SplitCriterion::EveryN(1), "{stem}_{n}.pdf", "fuzz")
    {
        for (_, bytes, _) in parts {
            Document::from_bytes(bytes).expect("split produced an unloadable part");
        }
    }
    // Merge the document with itself: the only shape in which every
    // cross-source name collision fires at once, which is what the
    // duplicate-field renaming exists for.
    let second = DocumentView::new(doc, doc.bytes(), doc.version());
    if let Ok((bytes, _)) = pdfcer_core::pageops::merge(&[view, second], &[]) {
        Document::from_bytes(bytes).expect("merge produced an unloadable file");
    }
}
