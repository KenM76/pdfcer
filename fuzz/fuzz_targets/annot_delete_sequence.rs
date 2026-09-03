//! Fuzz target: a **sequence** of annotation DELETIONS, and the three
//! cascades plus two delegations underneath them
//! (`pdfcer_core::edit::EditSession::delete_annotation`,
//! `delete_redaction_mark`, `delete_dimension`, `cut_selection`).
//!
//! ## Why this target is owed, by name
//!
//! `R236`: *a `debug_assert` postcondition over state derived from
//! untrusted input is a **tripwire for a fuzzer, not a guard for an
//! operator** — so it owes a fuzz target over the verbs it guards, or a
//! written exemption at the site.* `delete_annotation` carries exactly
//! such a postcondition, immediately before the `/Annots` patch loop:
//!
//! ```text
//! debug_assert!(
//!     !pages_touched.is_empty(),
//!     "the target was located on some page, so at least that page must be patched"
//! );
//! ```
//!
//! Every input to that claim comes out of the file. `pages_touched` is
//! built by re-walking `page_slots()` and asking
//! `annot::page_annotations` whether each page lists anything in
//! `removing` — a page tree the file shapes, `/Annots` arrays the file
//! shapes, and object ids the file names. The claim is that the second
//! walk cannot disagree with the first (`locate_annotation`'s). It reads
//! as obviously true, which is the property that makes it worth driving:
//! a postcondition nobody doubts is a postcondition nobody has tested.
//!
//! ## The coverage hole this fills
//!
//! `annot_walk` **reads** annotations. `annot_author` **writes** them.
//! **Neither DELETES**, and no other target touches the deletion verbs at
//! all. So every refusal, every cascade, both delegations and the
//! multi-target fold shipped with zero untrusted-input coverage.
//!
//! ## What it drives
//!
//! A sequence derived from the input, over whatever annotations the
//! document actually has, re-read between operations:
//!
//! - **`delete_annotation`** — the router, its four guards
//!   (`/F` bit 8 locked, `/TrapNet`, `/Widget`, encrypted/certified) and
//!   the general path: cascade 1 (`/Popup` forward and reverse), cascade 2
//!   (`/IRT` referrers un-linked, split by Table 170's `/RT` default),
//!   cascade 3 (appearance streams no survivor reaches), then the
//!   `/Annots` patch that the postcondition above guards.
//! - **`annotation_deletion_preview`** — called immediately before every
//!   deletion, so the shared-plan contract is an assertion rather than a
//!   comment (see "assertion 3").
//! - **`delete_redaction_mark`** — reached both by delegation (a `/Redact`
//!   that `redact::redaction_marks` recognises) and called directly, so
//!   its own `NotARedactionMark` refusal is driven over annotations that
//!   are not marks.
//! - **`delete_dimension`** — reached by delegation (an annotation the
//!   `/PieceInfo` sidecar claims as a ce dimension) and called directly
//!   over sidecar-derived `DimensionId`s, so the sidecar record and the
//!   annotation are removed together over documents nobody wrote.
//! - **`cut_selection`** — the multi-target form and the **fold**. N
//!   deletions must collapse into ONE undo entry (`R179`/`R49`), which is
//!   measured from the undo stack's own depth rather than counted; a fold
//!   that silently became N entries is invisible to every unit test that
//!   does not count them.
//! - **`undo` / `redo`** interleaved, so a deletion is unwound and
//!   re-applied *mid-sequence* rather than only at the end. A cascade that
//!   staged a write outside its own command survives an undo, and only a
//!   later operation over the half-restored state finds it.
//!
//! ## The hostile shapes this is pointed at
//!
//! - **A `/Popup` on a different page from its parent.** §12.5.6.2 binds
//!   nothing to one page, so `removing` can span pages and the patch loop
//!   must sweep all of them — the exact reason the postcondition talks
//!   about *"at least"* one page rather than *"the"* page.
//! - **An annotation listed by two pages.** §12.5.2 forbids it; a file can
//!   still do it. Half-removed references are the failure mode.
//! - **A `/Popup` naming its own parent, or naming itself.** Cascade 1
//!   guards this by checking the pointee really is a pop-up on this
//!   document's `/Annots`; the guard is what a fuzzer should try to walk
//!   past.
//! - **`/IRT` cycles and self-reply.** A referrer inside `removing` is
//!   excluded; a reply chain the file made circular is not.
//! - **An appearance stream shared by forty annotations**, where deleting
//!   one must not blank the other thirty-nine — and its mirror, a stream
//!   nothing else reaches, which must not be orphaned.
//! - **A ce dimension whose sidecar and whose annotation disagree**: a
//!   `DimensionRecord` naming an `/Annots` entry that is gone, or a
//!   `/Redact` that `redaction_marks` does not recognise, so the router
//!   sends the target down the general path instead.
//!
//! ## What is asserted, and what is deliberately not
//!
//! Asserted:
//!
//! 1. **No panic, abort or unbounded run.** `pdfcer-core` is panic-free by
//!    policy (`ARCHITECTURE.md` §10) and every verb here runs on
//!    attacker-shaped input.
//! 2. **Whatever is saved, reloads.** A deletion that produces a file
//!    pdfcer itself cannot parse is strictly worse than refusing it.
//! 3. **The preview did not lie.** `annotation_deletion_preview` and
//!    `delete_annotation` share `plan_annotation_deletion` precisely so a
//!    shell's warning cannot disagree with the act; over arbitrary
//!    documents that sharing is checkable and here it is checked.
//! 4. **★ The postcondition's OBSERVABLE consequence.** After a successful
//!    general-route deletion the target is no longer listed on any page.
//!    This is the same claim `edit.rs`'s `debug_assert` makes — *some page
//!    was patched* — restated in terms a caller can see, so it holds in a
//!    build with `debug-assertions` off as well as in one with them on.
//!    A `debug_assert` that only fires under `cargo fuzz`'s flags is one
//!    linker option away from silence.
//! 5. **Undoing every accepted deletion reproduces the input byte for
//!    byte.** Three cascades write to objects the operator never named —
//!    a parent's `/Popup`, a referrer's `/IRT` and `/RT`, and appearance
//!    streams — and a cascade staged beside its command instead of inside
//!    it restores the annotation and leaves those rewrites behind. Nothing
//!    but an undo-and-compare over documents nobody wrote can see that.
//!
//! Not asserted:
//!
//! - **A refusal is not a failure.** No annotations, a locked one, a
//!   `/TrapNet`, a `/Widget`, an encrypted or certified document, a
//!   selection too large for one undo entry — all are correct outcomes. A
//!   fuzzer that treats principled refusals as bugs trains the
//!   implementation to guess instead of refuse.
//! - **`parent_popup_cleared`.** The preview reports it from the plan
//!   alone; the verb also requires `self.value(parent)` to be an
//!   `Object::Dict`, and `Object::as_dict` — which the annotation walk
//!   uses — accepts a **stream's** dictionary too. So a `/Popup` parent
//!   that is a stream makes the two disagree legitimately. Named here
//!   rather than asserted, because the divergence is in the *reporting*
//!   of a cascade that correctly did nothing, not in the cascade.
//! - **`appearance_streams_removed`.** The preview documents itself as
//!   returning 0 without computing it. Comparing them would assert the
//!   documented gap is closed when it is deliberately open.
//! - **That any particular annotation was deletable.** Over arbitrary
//!   bytes there is no oracle for that; the counts are unit-test claims.
//!
//! ## ★ What it found on its first run (2026-08-31), unfixed here
//!
//! At roughly 2,450 executions — about ten seconds past `INITED` — a
//! **different** postcondition fired, `edit.rs`'s
//! `debug_assert_page_tree_still_walks`:
//!
//! ```text
//! POSTCONDITION VIOLATED: the command just committed left a page tree this
//! crate's own reader rejects (Some(NoPageTreeRoot)), and it walked before
//! the edit.
//! ```
//!
//! Reduced to 173 bytes, the shape is: an `/Annots` array containing an
//! indirect reference to a **structural** object — here the document
//! catalog. `annot::page_annotations` accepts any `/Annots` entry that
//! resolves to a dictionary (§7.3.10 makes a dangling entry null, not an
//! error, so the walk skips only non-dictionaries), and the deletion
//! guards test `/F` bit 8, `/TrapNet` and `/Widget` — **none of them tests
//! that the target is an annotation at all.** So the catalog is deleted as
//! though it were one, the verb returns `Ok`, and the page tree no longer
//! has a root.
//!
//! Reproduced by `cut_selection` first and then, more directly, by a bare
//! `delete_annotation` on the same document — so the defect is in the
//! general verb, not in the multi-target wrapper. **Reported, not fixed:**
//! `crates/` was owned by other work when this target was written.
//!
//! The finding is worth more than the bug. `R236` predicted the *class* —
//! a `debug_assert` postcondition over file-derived state is a tripwire —
//! and the tripwire that actually fired was one this target was not
//! written for, three thousand lines away from the one it was.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::annot::page_annotations;
use pdfcer_core::document::Document;
use pdfcer_core::edit::{AnnotationDeletionRoute, EditSession};
use pdfcer_core::object::ObjId;
use pdfcer_core::writer::SaveOptions;
use std::cell::RefCell;
use std::sync::Once;

thread_local! {
    /// The input currently being executed, so the panic hook below can emit
    /// it.
    ///
    /// A `Vec` refilled per iteration rather than a raw pointer: the copy is
    /// a few kilobytes against an iteration that walks a page tree several
    /// times, and a dangling pointer read from a panic hook would be a fuzz
    /// harness that lies about the input it crashed on. (The doc comment
    /// lives INSIDE the macro because `rustdoc` does not document a macro
    /// invocation, and an outer one is an `unused_doc_comments` warning.)
    static CURRENT_INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

static HOOK: Once = Once::new();

/// ★ **Emit the crashing input ourselves, because on Windows nothing else
/// will.**
///
/// Rust's abort path exits the process with `0xC0000409`
/// (`STATUS_STACK_BUFFER_OVERRUN` — MSVC's `__fastfail`, not an actual
/// overrun) **before libFuzzer's crash handler runs**, and writing the
/// offending input to `fuzz/artifacts/<target>/` is *its* job. So a real,
/// observed crash leaves nothing to `cargo fuzz tmin` and nothing to pin a
/// regression test to. Measured on this very target's first run, and
/// already recorded as
/// `D:\dev\rag\rust\cargo_fuzz_windows_abort_writes_no_artifact_use_seed.md`,
/// whose third recommendation is exactly this: *"make the target print its
/// own input on panic, rather than trusting a harness that cannot."*
///
/// Both channels, deliberately. The file is what a reproducer needs; the
/// hex on stderr is what survives when the file cannot be written (a
/// read-only temp dir, a sandbox), and a crash report with no input is the
/// state in which people conclude *"it went away"*.
///
/// The previous hook is chained rather than replaced, so libfuzzer-sys
/// still prints its own message and still aborts — this adds a channel, it
/// does not take one away.
fn install_input_dump_hook() {
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            CURRENT_INPUT.with(|cell| {
                if let Ok(input) = cell.try_borrow() {
                    let path = std::env::temp_dir().join("pdfcer-fuzz-panic-input.bin");
                    let wrote = std::fs::write(&path, input.as_slice()).is_ok();
                    let hex: String = input.iter().map(|b| format!("{b:02x}")).collect();
                    eprintln!(
                        "fuzz input ({} bytes, written to {}: {}):",
                        input.len(),
                        path.display(),
                        wrote
                    );
                    eprintln!("{hex}");
                }
            });
            previous(info);
        }));
    });
}

/// How many bytes at the front drive the operation sequence; the rest is
/// the candidate PDF.
///
/// Split at a **fixed offset** rather than length-prefixed, for the reason
/// `pageops_sequence` gives and `form_edit_sequence` repeats: a mutation to
/// the program half must not shift the document half, or a fuzzer that has
/// found an interesting document loses it while exploring operations over
/// it.
const PROGRAM_LEN: usize = 8;

/// Maximum verbs applied per input.
///
/// Every deletion walks the page tree twice — once to locate, once to
/// patch — and `cut_selection` walks it again per target, so this is
/// deliberately small. An iteration that spends a second is one the fuzzer
/// does not spend finding anything.
const MAX_OPS: usize = 6;

/// Ceiling on the annotations one iteration will enumerate.
///
/// Bounds the per-iteration cost on a document that lists thousands, and
/// it is a **view** bound only: the target ids are chosen from this capped
/// list and assertion 4 re-reads through the same cap, so the before and
/// after views cannot disagree because of the cap itself.
const MAX_ANNOTS_SCANNED: usize = 256;

/// Ceiling on pages walked when enumerating. Same reasoning.
const MAX_PAGES_SCANNED: usize = 64;

/// Ceiling on targets handed to one `cut_selection`.
///
/// The verb refuses a selection larger than `MAX_UNDO_DEPTH` by name, and
/// that refusal is worth reaching occasionally rather than every time —
/// so this stays far below it and the refusal is left to the verb's own
/// arithmetic (a cut also counts its content-object command).
const MAX_CUT_TARGETS: usize = 4;

/// Every annotation id the session can currently see, page order, capped.
///
/// Re-read between operations rather than computed once: a deletion
/// removes ids, a cascade removes a *second* id the operator never named,
/// and an interleaved undo puts both back. Driving the next verb from a
/// stale list would only ever exercise the `AnnotationNotFound` refusal —
/// which is the failure mode the sibling target records having had to fix.
fn annotation_ids(session: &EditSession) -> Vec<ObjId> {
    let Ok(slots) = session.page_slots() else {
        return Vec::new();
    };
    let graph = session.graph();
    let mut out: Vec<ObjId> = Vec::new();
    for slot in slots.iter().take(MAX_PAGES_SCANNED) {
        for annot in page_annotations(&graph, slot.id) {
            // A direct (non-indirect) `/Annots` entry has no id, and every
            // deletion verb is addressed by id. Skipped, not an error.
            if let Some(id) = annot.id {
                out.push(id);
            }
            if out.len() >= MAX_ANNOTS_SCANNED {
                return out;
            }
        }
    }
    out
}

/// The annotation ids on one page, in `/Annots` order — the index space
/// `cut_selection` addresses.
fn page_annotation_ids(session: &EditSession, page_index: usize) -> Vec<ObjId> {
    let Ok(slots) = session.page_slots() else {
        return Vec::new();
    };
    let Some(slot) = slots.get(page_index) else {
        return Vec::new();
    };
    let graph = session.graph();
    page_annotations(&graph, slot.id)
        .into_iter()
        .take(MAX_ANNOTS_SCANNED)
        .filter_map(|a| a.id)
        .collect()
}

fuzz_target!(|data: &[u8]| {
    install_input_dump_hook();
    CURRENT_INPUT.with(|cell| {
        if let Ok(mut current) = cell.try_borrow_mut() {
            current.clear();
            current.extend_from_slice(data);
        }
    });

    let (program, body) = data.split_at(data.len().min(PROGRAM_LEN));
    if body.len() < 32 {
        return;
    }
    // Only inputs that load are interesting: `load_document` fuzzes the
    // parser, and re-fuzzing it here would spend the budget on ground
    // another target already covers.
    let Ok(doc) = Document::from_bytes(body.to_vec()) else {
        return;
    };
    let original = doc.bytes().to_vec();
    let mut session = EditSession::new(doc);
    let mut applied = 0usize;

    for byte in program.iter().take(MAX_OPS) {
        let op = byte & 0x07;
        let param = usize::from(byte >> 3);

        // The undo/redo churn needs no annotations, so it is dispatched
        // before the emptiness check: unwinding a deletion and re-applying
        // it is exactly the state a later verb should be run against.
        if op == 7 {
            if param % 2 == 0 {
                let _ = session.undo();
            } else {
                let _ = session.redo();
            }
            continue;
        }

        let ids = annotation_ids(&session);
        if ids.is_empty() {
            // Nothing left to delete. Not a refusal and not a failure —
            // a document with no annotations, or one this sequence has
            // already emptied.
            break;
        }
        let target = ids[param % ids.len()];

        match op {
            // The general verb, four times out of eight. It is the router,
            // so the delegated routes are reached through it too whenever
            // the document carries a mark or a ce dimension; weighting it
            // this heavily is what keeps the postcondition site hot.
            0..=3 => {
                // Assertion 3, first half: whatever the verb decides, the
                // preview taken an instant earlier over unchanged state
                // must have decided the same. Captured BEFORE the call,
                // because after it the target is gone and the preview
                // could only answer `AnnotationNotFound`.
                let previewed = session.annotation_deletion_preview(target);
                let Ok(done) = session.delete_annotation(target) else {
                    continue;
                };
                applied += 1;

                let Ok(preview) = previewed else {
                    panic!(
                        "delete_annotation succeeded where annotation_deletion_preview refused: a shell disabling its control on the preview would have disabled a working verb"
                    );
                };
                assert_eq!(
                    (
                        preview.route,
                        &preview.subtype,
                        preview.popup_removed,
                        preview.replies_orphaned,
                        preview.group_members_promoted,
                    ),
                    (
                        done.route,
                        &done.subtype,
                        done.popup_removed,
                        done.replies_orphaned,
                        done.group_members_promoted,
                    ),
                    "the deletion preview and the deletion itself disagree about what the deletion did"
                );

                // Assertion 4: the `edit.rs` postcondition, restated as an
                // observable. The general route ends in the `/Annots`
                // patch loop that `debug_assert!(!pages_touched.is_empty())`
                // guards; if that loop patched nothing, the annotation is
                // still listed and the file still paints it.
                //
                // Restricted to `General` on purpose. The delegated verbs
                // remove their target too, but they are a different
                // postcondition at a different site, and asserting a
                // neighbour's contract here would make a failure ambiguous
                // about which verb broke.
                if done.route == AnnotationDeletionRoute::General {
                    assert!(
                        !annotation_ids(&session).contains(&target),
                        "delete_annotation reported a general-route deletion but the annotation is still listed on a page: no page /Annots array was patched"
                    );
                }
            }
            // The redaction-mark verb, called DIRECTLY. Most targets are
            // not marks, so this mostly drives the `NotARedactionMark`
            // refusal — which is the point: that guard is what stops this
            // becoming a back door around the general verb's cascades.
            4 => {
                if session.delete_redaction_mark(target).is_ok() {
                    applied += 1;
                }
            }
            // The ce-dimension verb, called directly over a sidecar-derived
            // id. The ids are collected into an owned `Vec` first: the
            // model borrows the session immutably and the verb needs it
            // mutably, and threading that borrow is what the collect
            // avoids.
            5 => {
                let dims: Vec<_> = {
                    let model = session.dimension_model();
                    model.dimensions().iter().map(|d| d.id).collect()
                };
                if dims.is_empty() {
                    continue;
                }
                if session.delete_dimension(dims[param % dims.len()]).is_ok() {
                    applied += 1;
                }
            }
            // The multi-target form and the fold. Content objects are
            // deliberately NOT selected: `copy_selection` skips the page
            // decomposition entirely when `object_indices` is empty, and a
            // decomposition per iteration would spend the whole budget
            // re-parsing content streams `vector_decompose` already fuzzes.
            _ => {
                let page_count = session.page_slots().map(|s| s.len()).unwrap_or(0);
                if page_count == 0 {
                    continue;
                }
                let page_index = param % page_count;
                let on_page = page_annotation_ids(&session, page_index);
                if on_page.is_empty() {
                    continue;
                }
                // A contiguous run starting at a byte-derived offset, so a
                // cut can take a `/Popup` and the parent it belongs to in
                // one gesture — the case whose doc comment says a cascade
                // may consume a later target before its turn comes.
                let start = param % on_page.len();
                let count = 1 + (param % MAX_CUT_TARGETS);
                let indices: Vec<usize> = (start..on_page.len()).take(count).collect();
                if session.cut_selection(page_index, &[], &indices).is_ok() {
                    applied += 1;
                }
            }
        }
    }

    // --- assertion 2: whatever was saved, reloads.
    if let Ok((bytes, _)) = session.to_incremental_bytes(&SaveOptions::identity()) {
        if let Err(err) = Document::from_bytes(bytes) {
            panic!("an annotation deletion produced an unloadable file: {err}");
        }
    }

    // --- assertion 5: undo everything ⇒ byte-identical.
    //
    // The one assertion that covers the CASCADES specifically. Deleting a
    // comment also clears a parent's `/Popup`, strips `/IRT` and `/RT` off
    // every referrer, and collects appearance streams — three sets of
    // writes to objects the caller never named. An implementation that
    // staged any of them outside the command would restore the annotation
    // on undo and leave the rest rewritten, and the difference is visible
    // nowhere except in the saved bytes.
    if applied > 0 {
        while session.undo().is_some() {}
        if let Ok((bytes, report)) = session.to_incremental_bytes(&SaveOptions::identity()) {
            assert!(
                report.byte_identical && bytes == original,
                "undoing every annotation deletion did not reproduce the input byte for byte"
            );
        }
    }
});
