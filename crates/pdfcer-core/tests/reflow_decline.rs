//! # A declined reflow says whether the operator can DO anything about it
//!
//! ## The defect this closes
//!
//! `pdfcer-gui` reported (2026-09-07, against `v0.45.0`) that
//! `ReflowApplyError::Unsupported(String)` carried **ten distinct refusals in
//! one variant**, and that **exactly one of them is recoverable** — the
//! `Pass 251.0` guard that fires when text has been added to the page in this
//! session. Save and reopen, and reflow works.
//!
//! With no discriminant, the only honest thing a shell can print is the
//! weakest sentence true of all ten. So the operator was denied a remedy that
//! existed, on **the commonest** of the ten — during live editing, "text was
//! added to this page this session" is most of the time.
//!
//! **They refused to match on the prose, and were right to.** A shell
//! branching on `starts_with("text was added")` would break silently on the
//! next typo fix, and would be re-deriving pdfcer's control flow from
//! pdfcer's sentences. A sentence is not an API.
//!
//! ## What is pinned here, and why each assertion is not redundant
//!
//! 1. **The recoverable case is reachable and names itself.** Building the
//!    exact document that trips `Pass 251.0`'s guard — add text to a page,
//!    then reflow it in the same session — must yield
//!    `PageEditedThisSession`, not `Unsupported`. This is the only test that
//!    exercises the real construction site; everything below is arithmetic
//!    over the type.
//! 2. **`decline()` is total and its mapping is fixed.** Every variant maps
//!    to a [`ReflowDecline`], and the recoverable arm contains **exactly
//!    one** variant. A future refusal joining the wrong arm is the failure
//!    mode that would quietly re-create the bug at one remove.
//! 3. **`is_recoverable()` agrees with `decline()` on every variant.** They
//!    are one fact with two readers (`R243`); a shell trusting a
//!    disagreeing `is_recoverable()` would offer a remedy that does not work.
//! 4. **The remedy survives in the message.** The sentence moved from a
//!    `String` payload onto the variant, and a move is where wording gets
//!    dropped. If `save and reopen` ever leaves that text, a shell relaying
//!    `Display` loses the instruction even though the discriminant is right.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::text_edit::{AddTextRequest, ReflowApplyError, ReflowDecline, ReflowRequest};

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

/// Every variant that can be constructed without a live failure, so the
/// mapping can be walked exhaustively rather than sampled.
///
/// The `#[from]` wrapper variants (`Extract`, `Content`, `PageTree`, `Write`,
/// `Preview`, `Refused`) are deliberately absent: they need a real underlying
/// error to build, and their arm is asserted through the catch-all reasoning
/// in `decline()` rather than by manufacturing one of each. That is a stated
/// limit, not an oversight — see the module docs' claim 2.
fn simple_variants() -> Vec<ReflowApplyError> {
    vec![
        ReflowApplyError::PageEditedThisSession,
        ReflowApplyError::Encrypted,
        ReflowApplyError::NoProvenance,
        ReflowApplyError::PageIndex(7),
        ReflowApplyError::Unsupported("rotated text refused by name".to_owned()),
    ]
}

/// Claim 1, INVERTED (`G015`) — adding text and then reflowing the same
/// page now SUCCEEDS, and that is why the variant below has no construction
/// site left.
///
/// # What this asserted, and why the assertion flipped
///
/// It proved `PageEditedThisSession` was reachable: *"only this one proves the
/// construction site actually produces the new variant."* That was the right
/// test for `Pass 251.0`'s guard, which refused whenever the page carried a
/// non-empty extra `/Contents` stream — because the planner read the BASE
/// document and could not see a run appended into an extra.
///
/// `Pass 257.0` made the planner read the SESSION's graph.
/// `ContentStream::from_page` concatenates EVERY `/Contents` entry and the plan
/// replaces only the block's own show-operator spans within it, so the appended
/// run is in the plan's source and survives the consolidation. The guard was
/// removed in `G015` after measuring exactly that — see
/// `content_edit_no_duplication::reflow_keeps_text_added_this_session`, which
/// asserts the run survives rather than that the attempt was refused.
///
/// ⚠ So this test now measures the opposite fact, and it is the one that
/// matters: **the operation the refusal was protecting works.** If a future
/// change narrows the planner's source again, this goes red first.
#[test]
fn adding_text_then_reflowing_the_same_page_now_succeeds() {
    let doc = Document::load(&fixture("reflow/reflow.pdf")).expect("the reflow fixture");
    let mut s = EditSession::new(doc);

    // (100, 600), not (72, 700): at the latter the run lands inside block 0's
    // own box and the block becomes multi-font, which reflow defers for an
    // unrelated reason. That refusal is correct and would make this test pass
    // for the wrong reason — the run must form its own block.
    s.add_text(&AddTextRequest::new(0, (100.0, 600.0), "guard trip"))
        .expect("could not add text, so this test measures nothing");

    match s.reflow_block(0, 0, &ReflowRequest::new()) {
        Ok(_) => {}
        Err(ReflowApplyError::PageEditedThisSession) => panic!(
            "the `Pass 251.0` guard is back. If that was deliberate, the planner must have \
             stopped reading the session's whole page — check that before restoring this \
             test, because the guard and the narrow planner only make sense together."
        ),
        other => panic!(
            "expected the reflow to commit, got {other:?}. A refusal for an UNRELATED reason \
             (a multi-font block, a composite font) means the fixture changed and this test \
             is measuring nothing — fix the fixture, not the assertion."
        ),
    }
}

/// Claim 2 — the recoverable arm holds exactly one variant.
///
/// The failure this guards is a future refusal being dropped into
/// `RetryAfterSaveAndReopen` because it superficially resembles one. That
/// would tell the operator to save and reopen for something a save does not
/// fix — worse than the original defect, because it is confidently wrong
/// rather than uselessly vague.
#[test]
fn exactly_one_variant_is_recoverable() {
    let recoverable: Vec<_> = simple_variants()
        .into_iter()
        .filter(ReflowApplyError::is_recoverable)
        .collect();
    assert_eq!(
        recoverable.len(),
        1,
        "expected exactly one recoverable refusal, got {recoverable:?}"
    );
    assert!(matches!(
        recoverable[0],
        ReflowApplyError::PageEditedThisSession
    ));
}

/// Claim 3 — `is_recoverable()` never disagrees with `decline()`.
///
/// They are one fact with two readers, which is the `R243` shape. The
/// accessor derives from the other in the source, so this asserts that the
/// derivation is real and not two parallel matches that happen to agree
/// today.
#[test]
fn is_recoverable_agrees_with_decline_on_every_variant() {
    for e in simple_variants() {
        let by_kind = e.decline() == ReflowDecline::RetryAfterSaveAndReopen;
        assert_eq!(
            e.is_recoverable(),
            by_kind,
            "is_recoverable() and decline() disagree about {e:?}"
        );
    }
}

/// Claim 2, continued — the non-recoverable variants land where a shell
/// expects, so the four buckets carry their stated meaning.
#[test]
fn the_non_recoverable_variants_are_classified_as_documented() {
    assert_eq!(
        ReflowApplyError::Encrypted.decline(),
        ReflowDecline::StructureForbids
    );
    assert_eq!(
        ReflowApplyError::PageIndex(7).decline(),
        ReflowDecline::NotFound,
        "a page index out of range is a CALLER error, not an operator one"
    );
    assert_eq!(
        ReflowApplyError::NoProvenance.decline(),
        ReflowDecline::NotFound,
        "extracting without provenance is the caller not asking for what it needs"
    );
    assert_eq!(
        ReflowApplyError::Unsupported("rotated text".to_owned()).decline(),
        ReflowDecline::NotReflowable,
        "the nine merged conditions are permanent for the document as drawn"
    );
}

/// Claim 4 — the remedy survives in the message.
///
/// The sentence moved from a `String` payload onto the variant, and a move is
/// exactly where wording gets dropped. A shell that relays `Display` while
/// switching on `decline()` would lose the instruction and keep the
/// classification, which reads as working.
#[test]
fn the_recoverable_message_still_tells_the_operator_what_to_do() {
    let msg = ReflowApplyError::PageEditedThisSession.to_string();
    assert!(
        msg.contains("save and reopen"),
        "the remedy must survive in the message, not only in the variant name: {msg}"
    );
    assert!(
        msg.contains("added to this page this session"),
        "the cause must survive too: {msg}"
    );
}
