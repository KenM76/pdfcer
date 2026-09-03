//! Fuzz target: the **form-scoped geometry verbs** — content surgery inside a
//! form XObject (`Pass 188.0`).
//!
//! ## Why this target and not a widened `vector_edit`
//!
//! `vector_edit` drives the PLANNERS (`plan_move_node`, `plan_move_subpath`,
//! …) over a decomposed content stream. It is thorough about operand
//! rewriting and says nothing about the machinery `Pass 188.0` actually added,
//! which sits a level up in `EditSession`:
//!
//! - resolving a caller-supplied `leaf_index` against `PageObjects::leaves`;
//! - reading `containment.last()` to find the form to write;
//! - re-decomposing that form's stream **from the leaf's placement matrix**,
//!   with the form's own `/Resources` or the page's inherited ones;
//! - resolving a multi-leaf selection and refusing one that spans two
//!   invocations;
//! - counting the form's reach across the page tree;
//! - staging the rewritten stream back into the form object.
//!
//! Every one of those is index arithmetic and graph-walking over
//! **attacker-controlled structure**, which is exactly what `ARCHITECTURE.md`
//! §10.2 asks to be driven. None of it is reachable from `vector_edit`, whose
//! whole input is one already-parsed content stream.
//!
//! ## ★ The specific shapes this is built to survive
//!
//! A mutated document can produce a leaf whose form is:
//!
//! - **not a stream** — `/XObject` naming a dictionary, an array, a page, or
//!   the catalog. The `Do` classifier and the surgery must disagree about
//!   nothing.
//! - **self-referential or mutually recursive.** The decomposer's own depth
//!   and cycle guards run during `page_objects`; the surgery then
//!   re-decomposes ONE form and must not reintroduce a way around them.
//! - **degenerate in placement** — a `cm` of all zeros makes the placement
//!   non-invertible, and the planners convert a page-space target by inverting
//!   it. That must be a named `DegenerateCtm` refusal, not a panic and not a
//!   `NaN` written into the file.
//! - **shared with a page** — the reach walk decomposes every page to count
//!   invocations, on a document whose page tree may be damaged.
//!
//! ## Invariant
//!
//! `ARCHITECTURE.md` §10's panic-free policy: for ANY input, none of these
//! verbs panics, aborts, or runs unbounded. Each returns `Ok` or a by-name
//! `EditError`, and a save after any accepted sequence produces bytes that
//! parse.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::vector::Point;
use pdfcer_core::writer::SaveOptions;

/// How many bytes at the front drive the operation sequence; the rest is the
/// candidate PDF.
///
/// Split at a fixed offset rather than length-prefixed, for the reason
/// `pageops_sequence` gives: a mutation to the program half must not shift the
/// document half, or a fuzzer that has found an interesting document loses it
/// while exploring operations over it.
const PROGRAM_LEN: usize = 8;

/// Maximum verbs applied. Each one re-decomposes a page and a form, and the
/// reach walk decomposes every page — so this is deliberately small. An
/// iteration that spends a second is one the fuzzer does not spend finding
/// anything.
const MAX_OPS: usize = 4;

/// Page-space targets, including the ones that break arithmetic. A degenerate
/// placement plus a degenerate target is the pair most likely to write a `NaN`
/// into a content stream, which is the failure this list exists to reach.
const TARGETS: [f64; 6] = [0.0, 1.0, -1e300, 1e300, f64::NAN, f64::INFINITY];

fuzz_target!(|data: &[u8]| {
    let (program, body) = data.split_at(data.len().min(PROGRAM_LEN));
    if body.len() < 32 {
        return;
    }
    // Only inputs that load are interesting: `load_document` fuzzes the parser
    // and re-fuzzing it here would spend the budget on ground another target
    // covers.
    let Ok(doc) = Document::from_bytes(body.to_vec()) else {
        return;
    };
    let mut session = EditSession::new(doc);
    let mut applied = 0usize;

    for byte in program.iter().take(MAX_OPS) {
        // Re-read the model every iteration. An accepted edit rewrites the
        // form's stream, which can change how many leaves it yields — driving
        // the next verb from a stale list would only ever exercise the
        // out-of-range refusal, which is the least interesting branch here.
        let Ok(pages) = session.pages() else { break };
        if pages.is_empty() {
            break;
        }
        let page_index = usize::from(*byte) % pages.len();
        let Ok(model) = session.page_objects(page_index) else {
            continue;
        };
        let leaf_count = model.leaves.len();
        if leaf_count == 0 {
            continue;
        }

        let op = byte & 0x07;
        let param = usize::from(byte >> 3);
        // Deliberately allowed to OVERRUN by one, so the out-of-range refusal
        // is reached as well as the in-range path.
        let leaf = param % (leaf_count + 1);
        let n = param % 6;
        let (x, y) = (TARGETS[n], TARGETS[(n + 3) % 6]);

        let ok = match op {
            0 => session
                .move_node_in_form(page_index, leaf, param % 8, Point { x, y })
                .is_ok(),
            1 => session
                .move_nodes_in_form(
                    page_index,
                    leaf,
                    // Two DISTINCT indices: the duplicate-node refusal is its
                    // own branch and is reached by the `param % 8` collisions
                    // that happen anyway.
                    &[
                        (param % 8, Point { x, y }),
                        (param % 5, Point { x: y, y: x }),
                    ],
                )
                .is_ok(),
            2 => session
                .move_handle_in_form(
                    page_index,
                    leaf,
                    param % 8,
                    if param % 2 == 0 {
                        pdfcer_core::vector::Handle::Incoming
                    } else {
                        pdfcer_core::vector::Handle::Outgoing
                    },
                    Point { x, y },
                )
                .is_ok(),
            3 => session
                .move_subpath_in_form(page_index, leaf, param % 4, x, y)
                .is_ok(),
            4 => session
                .move_objects_in_form(page_index, &[leaf], x, y)
                .is_ok(),
            // A multi-leaf selection, which reaches `leaf_siblings` — the
            // spans-two-invocations refusal and the collision it guards.
            5 => session
                .move_objects_in_form(page_index, &[leaf, param % (leaf_count + 1)], x, y)
                .is_ok(),
            6 => session.delete_objects_in_form(page_index, &[leaf]).is_ok(),
            _ => session
                .delete_objects_in_form(page_index, &[leaf, (leaf + 1) % (leaf_count + 1)])
                .is_ok(),
        };
        if ok {
            applied += 1;
        }
    }

    if applied == 0 {
        return;
    }
    // Undo everything, then redo it. Both walks must terminate and neither may
    // panic — a form edit stages ONE object write, so a command whose `before`
    // and `after` disagree about the object's shape shows up here.
    for _ in 0..applied {
        session.undo();
    }
    for _ in 0..applied {
        session.redo();
    }

    // The saved bytes must parse. A form edit rewrites a stream AND its
    // `/Length`; a disagreement between the two produces a file that loads as
    // something else entirely, which no in-memory assertion can see.
    if let Ok((bytes, _)) = session.to_incremental_bytes(&SaveOptions::identity())
        && let Err(err) = Document::from_bytes(bytes)
    {
        panic!("a form geometry edit produced an unloadable file: {err}");
    }
});
