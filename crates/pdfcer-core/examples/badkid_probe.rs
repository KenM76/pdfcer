//! Throwaway measurement (`R238`): **what does the SHIPPING build do** on the
//! `annot_delete_sequence` `BadKid(ObjId 3)` route?
//!
//! # Why this exists rather than a reading of the code
//!
//! `docs/NEXT_SESSION.md` carried this route as the one open item, with its
//! severity explicitly **unmeasured**, and standing rule `R238` — minted the
//! day before, from a sizing that was wrong in exactly this way — says how to
//! close that gap: *a `debug_assert` tells you **where the check runs**, never
//! **what the shipping build does when the property is false***. The answer is
//! one of three, and only measurement distinguishes them:
//!
//! 1. **panics anyway** (some other guard catches it) — a robustness bug;
//! 2. **returns an error** — correct behaviour, the assert is a tripwire only;
//! 3. **returns `Ok` and writes wrong bytes** — release-visible corruption,
//!    and the only one of the three that is urgent.
//!
//! The predecessor item was sized from the compile-time gating alone
//! (*"it's a `debug_assert`, so in release it's a wrong number in a
//! disclosure"*), that sentence propagated verbatim to three documents, and
//! the measurement then found **three of four shapes release-visible** — two of
//! them worse than a wrong number. So this probe is deliberately built to
//! answer the question in the RELEASE profile, where `debug_assertions` is off
//! and `debug_assert_page_tree_still_walks` compiles to nothing.
//!
//! # The input
//!
//! `fuzz/corpus/annot_delete_sequence/seed_openbug_badkid_dupobjnum.bin` —
//! 1,618 bytes, of which the first `PROGRAM_LEN` = 8 drive the operation
//! sequence and the rest is the candidate PDF. Its shape:
//!
//! - **two `3 0 obj` definitions**, the first a garbage dictionary
//!   (`<< /Type /ount 1 >>`), the second the real `/Type /Page`;
//! - `/Kids [3 0 R]` on the page-tree node, so object 3 **is** the page;
//! - a truncated/corrupt `xref` (entry 4's offset field is overwritten with
//!   document text), which is what puts the loader on the rebuild-by-scan
//!   path — and a scan has to choose *which* `3 0 obj` wins.
//!
//! # What is printed, and why each line is there
//!
//! For every verb in the program: the verb, its `Result`, and then the two
//! **observables** that a `debug_assert` cannot report in a release build —
//! whether `page_tree::pages_in` still walks the live graph, and whether an
//! incremental save round-trips through `Document::from_bytes`. A verb that
//! returns `Ok` and leaves either of those false is case 3 above.
//!
//! Run both profiles; the pair is the measurement, not either half:
//!
//! ```text
//! cargo run -p pdfcer-core --example badkid_probe            # debug_assertions ON
//! cargo run -p pdfcer-core --release --example badkid_probe  # the shipping build
//! ```
//!
//! # ★★★ WHAT IT MEASURED (2026-08-31) — case 3, the urgent one
//!
//! `cut_selection` returned **`Ok`**, and `to_full_bytes` wrote a **904-byte
//! file that reloads with `PageTreeError::BadKid`** — a saved PDF with no
//! walkable page. Not a wrong number in a disclosure; a destroyed document,
//! reported as success.
//!
//! **Two accidents hid it, and both are worth naming** because either one on
//! its own would have supported a "harmless" sizing:
//!
//! 1. `debug_assert_page_tree_still_walks` is `#[cfg(debug_assertions)]`, so
//!    the shipping build says nothing at all.
//! 2. The **incremental** save path refused this document for a completely
//!    unrelated reason — its base cross-reference was invalid, so pdfcer
//!    requires a full rewrite — which meant the fuzz harness's own
//!    save-and-reload assertion never ran. `save_full` is the *sanctioned*
//!    path for a recovered document and therefore the one an operator
//!    actually reaches, and it carried the corruption straight through. **A
//!    refusal that fires for an unrelated reason is not a guard**; it is a
//!    coincidence that suppresses evidence, and this probe measures both save
//!    paths for exactly that reason.
//!
//! # The cause, and why the fix is a type test
//!
//! Cascade 3 (`EditSession::appearance_streams_owned_by`) branched on the
//! resolved type of `/AP` `/N`: a **stream** was doomed directly, a
//! **dictionary** was treated as an appearance-state subdictionary and had
//! *every reference inside it* harvested. Here `/N` named a `/Widget`
//! dictionary, whose `/P` names the **page** — so the page was collected as
//! though it were an appearance state, and deleted.
//!
//! §12.5.5 settles it: *"Each appearance stream is a form XObject"*, and a
//! Table 168 subdictionary *"shall define multiple appearance streams"*. Every
//! object that cascade reaches shall be a stream. Fixed in `Pass 191.0`; pinned
//! by `crates/pdfcer-core/tests/annot_ap_cascade_streams.rs`, whose two hostile
//! cases were confirmed to go red **in the release profile** under sabotage —
//! so the regression test protects the build that had the defect, rather than
//! re-testing the `debug_assert` that did not catch it.
//!
//! **Post-fix, this probe walks the whole program with the page tree intact at
//! every step**, and the run gets *further* than it did before (the corruption
//! used to empty the page, ending the sequence early).

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::object::ObjId;
use pdfcer_core::writer::SaveOptions;

/// Mirrors `fuzz/fuzz_targets/annot_delete_sequence.rs`'s constant. Split at a
/// fixed offset so a mutation to the program half does not shift the document
/// half.
const PROGRAM_LEN: usize = 8;
const MAX_OPS: usize = 6;
const MAX_CUT_TARGETS: usize = 4;

/// Every annotation id the session can currently see, page order.
///
/// Re-read between operations for the same reason the fuzz target re-reads:
/// a deletion removes ids and a cascade removes a second id the operator never
/// named, so a stale list would only ever drive the `AnnotationNotFound`
/// refusal and the probe would measure nothing.
fn annotation_ids(session: &EditSession) -> Vec<ObjId> {
    let Ok(slots) = session.page_slots() else {
        return Vec::new();
    };
    let graph = session.graph();
    let mut out = Vec::new();
    for slot in &slots {
        for annot in pdfcer_core::annot::page_annotations(&graph, slot.id) {
            if let Some(id) = annot.id {
                out.push(id);
            }
        }
    }
    out
}

/// The annotation ids on one page in `/Annots` order — the index space
/// `cut_selection` addresses.
fn page_annotation_ids(session: &EditSession, page_index: usize) -> Vec<ObjId> {
    let Ok(slots) = session.page_slots() else {
        return Vec::new();
    };
    let Some(slot) = slots.get(page_index) else {
        return Vec::new();
    };
    let graph = session.graph();
    pdfcer_core::annot::page_annotations(&graph, slot.id)
        .into_iter()
        .filter_map(|a| a.id)
        .collect()
}

/// The two release-visible observables, printed after every verb.
///
/// `walks` is the property `debug_assert_page_tree_still_walks` asserts,
/// restated as something a caller can see in any profile. `reloads` is the
/// stronger one — a file pdfcer itself cannot reopen is strictly worse than a
/// refusal, and it is the shape the 2026-08-20 `/Contents` corruption had.
fn observe(session: &EditSession, label: &str) {
    let walks = pdfcer_core::page_tree::pages_in(&session.graph());
    let incremental = match session.to_incremental_bytes(&SaveOptions::identity()) {
        Ok((bytes, _)) => match Document::from_bytes(bytes) {
            Ok(_) => "yes".to_owned(),
            Err(e) => format!("NO ({e})"),
        },
        Err(e) => format!("refused ({e})"),
    };
    // ★ The full rewrite is measured too, and it is the half that matters
    // here. This input is a RECOVERED document (its base xref is invalid), so
    // the incremental path refuses it by policy — which would hide the
    // corruption behind an unrelated safety net and let the route be sized as
    // harmless. `save_full` is the sanctioned path for exactly this document,
    // so it is the one an operator would actually reach.
    let full = match session.to_full_bytes(&SaveOptions::identity()) {
        Ok((bytes, _)) => match Document::from_bytes(bytes.clone()) {
            Ok(d) => match pdfcer_core::page_tree::pages_in(d.view().graph()) {
                Ok(p) => format!("yes, {} page(s), {} B", p.len(), bytes.len()),
                Err(e) => format!("★ RELOADS BUT PAGE TREE IS BROKEN ({e:?})"),
            },
            Err(e) => format!("★ UNLOADABLE ({e})"),
        },
        Err(e) => format!("refused ({e})"),
    };
    println!(
        "      -> walks: {:<34} incr: {:<22} full: {}",
        match &walks {
            Ok(p) => format!("yes ({} page(s))", p.len()),
            Err(e) => format!("NO ({e:?})"),
        },
        incremental,
        full
    );
    let _ = label;
}

fn main() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fuzz/corpus/annot_delete_sequence/seed_openbug_badkid_dupobjnum.bin"
    );
    let data = std::fs::read(path).expect("the tracked reproducer must be present");
    println!("profile: debug_assertions = {}", cfg!(debug_assertions));
    println!("input: {} bytes", data.len());

    let (program, body) = data.split_at(data.len().min(PROGRAM_LEN));
    let doc =
        Document::from_bytes(body.to_vec()).expect("the reproducer's document half must load");
    let original = doc.bytes().to_vec();
    let mut session = EditSession::new(doc);

    println!(
        "loaded: {} page(s), annotations visible: {:?}",
        session.page_slots().map(|s| s.len()).unwrap_or(0),
        annotation_ids(&session)
    );
    observe(&session, "after load");

    let mut applied = 0usize;
    for (step, byte) in program.iter().take(MAX_OPS).enumerate() {
        let op = byte & 0x07;
        let param = usize::from(byte >> 3);

        if op == 7 {
            let undone = if param % 2 == 0 {
                session.undo().is_some()
            } else {
                session.redo().is_some()
            };
            println!("step {step}: op={op} param={param} undo/redo -> {undone}");
            observe(&session, "after undo/redo");
            continue;
        }

        let ids = annotation_ids(&session);
        if ids.is_empty() {
            println!("step {step}: op={op} param={param} -- no annotations left, stopping");
            break;
        }
        let target = ids[param % ids.len()];

        match op {
            0..=3 => {
                let result = session.delete_annotation(target);
                println!(
                    "step {step}: op={op} param={param} delete_annotation({target:?}) -> {}",
                    match &result {
                        Ok(d) => format!("Ok(route={:?}, subtype={:?})", d.route, d.subtype),
                        Err(e) => format!("Err({e})"),
                    }
                );
                if result.is_ok() {
                    applied += 1;
                }
                observe(&session, "after delete_annotation");
            }
            4 => {
                let result = session.delete_redaction_mark(target);
                println!(
                    "step {step}: op={op} param={param} delete_redaction_mark({target:?}) -> {}",
                    match &result {
                        Ok(_) => "Ok".to_owned(),
                        Err(e) => format!("Err({e})"),
                    }
                );
                observe(&session, "after delete_redaction_mark");
            }
            5 => {
                let dims: Vec<_> = {
                    let model = session.dimension_model();
                    model.dimensions().iter().map(|d| d.id).collect()
                };
                println!(
                    "step {step}: op={op} param={param} dimensions={}",
                    dims.len()
                );
                if dims.is_empty() {
                    continue;
                }
                let _ = session.delete_dimension(dims[param % dims.len()]);
                observe(&session, "after delete_dimension");
            }
            _ => {
                let page_count = session.page_slots().map(|s| s.len()).unwrap_or(0);
                if page_count == 0 {
                    println!("step {step}: op={op} param={param} cut_selection -- no pages");
                    continue;
                }
                let page_index = param % page_count;
                let on_page = page_annotation_ids(&session, page_index);
                if on_page.is_empty() {
                    println!("step {step}: op={op} param={param} cut_selection -- page empty");
                    continue;
                }
                let start = param % on_page.len();
                let count = 1 + (param % MAX_CUT_TARGETS);
                let indices: Vec<usize> = (start..on_page.len()).take(count).collect();
                let result = session.cut_selection(page_index, &[], &indices);
                println!(
                    "step {step}: op={op} param={param} cut_selection(page={page_index}, annots={indices:?}) -> {}",
                    match &result {
                        Ok(c) => format!("Ok({c:?})"),
                        Err(e) => format!("Err({e})"),
                    }
                );
                if result.is_ok() {
                    applied += 1;
                }
                observe(&session, "after cut_selection");
            }
        }
    }

    // The undo-to-identity contract, measured rather than asserted: three
    // cascades write to objects the caller never named, and one staged outside
    // its command restores the annotation while leaving the rest rewritten.
    if applied > 0 {
        while session.undo().is_some() {}
        match session.to_incremental_bytes(&SaveOptions::identity()) {
            Ok((bytes, report)) => println!(
                "undo-all: byte_identical={} bytes_match={}",
                report.byte_identical,
                bytes == original
            ),
            Err(e) => println!("undo-all: save refused ({e})"),
        }
    }
}
