//! Fuzz target: a **sequence** of form-field edits, and the object-wide
//! action-target sweep underneath them (`pdfcer_core::edit` +
//! `pdfcer_core::forms`).
//!
//! `form_model` fuzzes the READ side — `parse_acroform` over untrusted
//! bytes. Nothing fuzzed the WRITE side, and `Pass 184.0` added a
//! traversal that walks **every live object of an arbitrary document**
//! looking for action dictionaries. That is untrusted-input parsing by any
//! reading of `ARCHITECTURE.md` §10, and it shipped without a target.
//!
//! ★ **This target exists because of a gate I skip.** The standing rule is
//! *"a fixture-based test for every new parser branch, AND a `cargo-fuzz`
//! target if the new code touches untrusted-input parsing"*, and the fuzz
//! half is the one that gets forgotten — twice recorded, twice recurred.
//! The sweep's own depth guard (`forms::MAX_ACTION_NEST_DEPTH`) is exactly
//! the kind of bound that is written confidently and never driven.
//!
//! ## What it drives
//!
//! Four verbs, in a sequence derived from the input, over whatever field
//! names the document actually has:
//!
//! - `set_button_action` — writes `/A` on a push button. Reaches the
//!   destination validator, the Table 237 flag-word builder and the
//!   `/Hide` terminal-vs-grouping check.
//! - `rename_field` — and therefore the **repairing** sweep, which
//!   rewrites name strings inside action dictionaries found anywhere in
//!   the object universe, including a second pass over target lists that
//!   live in their own objects.
//! - `delete_field` / `delete_field_group` — the **counting** sweep.
//!
//! ## The hostile shapes this is pointed at
//!
//! - **A deeply nested action.** `/A` inside `/Next` inside an array
//!   inside a dictionary, repeated: the recursion is bounded by
//!   `MAX_ACTION_NEST_DEPTH` and nothing else, and a direct value can be
//!   nested as deeply as the file has bytes for.
//! - **A `/Fields` or `/Hide` `/T` naming its own object**, or naming an
//!   object that is itself a target list — the deferred second pass takes
//!   ids straight out of the file.
//! - **A target list shared by many actions**, which must be deduped or
//!   the same object is staged for write repeatedly.
//! - **Names that do not decode.** A `/Fields` element whose bytes are not
//!   valid PDFDocEncoding or UTF-16BE still has to compare against a
//!   fully-qualified name built by the same decoder.
//! - **A field tree with cycles**, already `form_model`'s ground, reached
//!   here through the WRITE path where a rename derives a new prefix from
//!   a name the tree walk produced.
//!
//! ## What is asserted, and what is deliberately not
//!
//! Asserted:
//!
//! 1. **No panic, ever.** `pdfcer-core` is panic-free by policy, and every
//!    verb here runs on attacker-shaped input.
//! 2. **Whatever is saved, reloads.** An edit that produces a file pdfcer
//!    itself cannot parse is strictly worse than refusing the edit.
//! 3. **Undo-everything ⇒ byte-identical.** The sweep stages its writes
//!    into the SAME command as the rename, so this also proves the
//!    repaired objects are on the undo stack rather than beside it —
//!    which no unit test can prove over documents nobody wrote.
//!
//! Not asserted:
//!
//! - **A refusal is not a failure.** No form, no push button, a grouping
//!   node named where a terminal is required, an encrypted document — all
//!   are correct outcomes. A fuzzer that treats principled refusals as
//!   bugs trains the implementation to guess instead of refuse.
//! - **That any particular action was repaired.** The count is a unit-test
//!   claim; over arbitrary bytes there is no oracle for it.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::edit::{ButtonAction, EditSession, ResetScope, SubmitScope, SubmitSpec};
use pdfcer_core::writer::SaveOptions;

/// How many bytes at the front drive the operation sequence; the rest is
/// the candidate PDF.
///
/// Split at a fixed offset rather than length-prefixed, for the reason
/// `pageops_sequence` gives: a mutation to the program half must not shift
/// the document half, or a fuzzer that has found an interesting document
/// loses it while exploring operations over it.
const PROGRAM_LEN: usize = 8;

/// Maximum verbs applied. Each rename or delete walks every object in the
/// document, so this is deliberately small — an iteration that spends a
/// second is one the fuzzer does not spend finding anything.
const MAX_OPS: usize = 5;

fuzz_target!(|data: &[u8]| {
    let (program, body) = data.split_at(data.len().min(PROGRAM_LEN));
    if body.len() < 32 {
        return;
    }
    // Only inputs that load are interesting: `load_document` fuzzes the
    // parser and re-fuzzing it here would spend the budget on ground
    // another target covers.
    let Ok(doc) = Document::from_bytes(body.to_vec()) else {
        return;
    };
    let original = doc.bytes().to_vec();
    let mut session = EditSession::new(doc);
    let mut applied = 0usize;

    for byte in program.iter().take(MAX_OPS) {
        // The field names as the model currently sees them. Re-read every
        // iteration because a delete removes names and a rename changes
        // them, and driving the second verb with the first verb's stale
        // list would only ever exercise the not-found refusal.
        let names: Vec<String> = pdfcer_core::forms::parse_acroform(&session.graph())
            .map(|form| {
                form.fields
                    .iter()
                    .map(|f| f.fully_qualified_name.clone())
                    .collect()
            })
            .unwrap_or_default();
        if names.is_empty() {
            break;
        }
        let op = byte & 0x07;
        let param = usize::from(byte >> 3);
        let name = names[param % names.len()].clone();

        let ok = match op {
            0 => session
                .set_button_action(
                    &name,
                    Some(ButtonAction::ResetForm {
                        scope: ResetScope::All,
                    }),
                )
                .is_ok(),
            1 => {
                // A reset naming a real field, so the target-existence
                // check passes and the WRITE path is reached.
                session
                    .set_button_action(
                        &name,
                        Some(ButtonAction::ResetForm {
                            scope: ResetScope::Only(vec![name.clone()]),
                        }),
                    )
                    .is_ok()
            }
            2 => {
                let mut spec = SubmitSpec::new("https://example.invalid/collect");
                spec.scope = SubmitScope::Except(vec![name.clone()]);
                session
                    .set_button_action(&name, Some(ButtonAction::SubmitForm(spec)))
                    .is_ok()
            }
            3 => session
                .set_button_action(
                    &name,
                    Some(ButtonAction::SetHidden {
                        targets: vec![name.clone()],
                        hidden: param % 2 == 0,
                    }),
                )
                .is_ok(),
            4 => session.set_button_action(&name, None).is_ok(),
            // The repairing sweep. The new partial name is fixed rather
            // than derived: what matters is that a rename HAPPENS over a
            // document the fuzzer shaped, not which characters it uses,
            // and a derived name would spend iterations on the
            // period-refusal path the unit tests already cover.
            5 => session.rename_field(&name, "fuzzed").is_ok(),
            // The counting sweeps.
            6 => session.delete_field(&name).is_ok(),
            _ => session.delete_field_group(&name).is_ok(),
        };
        if ok {
            applied += 1;
        }
    }

    // --- assertion 2: whatever was saved, reloads.
    if let Ok((bytes, _)) = session.to_incremental_bytes(&SaveOptions::identity()) {
        if let Err(err) = Document::from_bytes(bytes) {
            panic!("a form edit produced an unloadable file: {err}");
        }
    }

    // --- assertion 3: undo everything ⇒ byte-identical.
    //
    // This is the one that covers `Pass 184.0`'s sweep specifically: the
    // repaired objects ride in the same command as the rename, so an
    // implementation that staged them outside the undo entry would restore
    // the name and leave the rewritten actions behind — and the ONLY way
    // to see that is to undo and compare bytes.
    if applied > 0 {
        while session.undo().is_some() {}
        if let Ok((bytes, report)) = session.to_incremental_bytes(&SaveOptions::identity()) {
            assert!(
                report.byte_identical && bytes == original,
                "undoing every form edit did not reproduce the input byte for byte"
            );
        }
    }
});
