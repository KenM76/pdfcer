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

/// ★★ Claim 1 — the recoverable refusal is reachable from the REAL guard.
///
/// Adds text to a page and then reflows that page in the same session, which
/// is exactly the situation `Pass 251.0` guards: reflow re-emits the page's
/// first content stream only, and the added run lives in a second one.
///
/// This is the assertion that would have caught the defect. Everything else
/// in this file is arithmetic over the type; only this one proves the
/// construction site actually produces the new variant.
#[test]
fn adding_text_then_reflowing_the_same_page_names_the_recoverable_refusal() {
    // The dedicated reflow fixture, so the block IS reflowable and the only
    // thing standing in the way is the guard under test. Using a fixture that
    // could not be reflowed anyway would make this pass for the wrong reason.
    let doc = Document::load(&fixture("reflow/reflow.pdf")).expect("the reflow fixture");
    let mut s = EditSession::new(doc);

    // add_text lands in a NEW content stream, which is exactly what the
    // `Pass 251.0` guard watches for.
    s.add_text(&AddTextRequest::new(0, (72.0, 700.0), "guard trip"))
        .expect(
            "could not add text, so the guard cannot be tripped and this test measures nothing",
        );

    match s.reflow_block(0, 0, &ReflowRequest::new()) {
        Err(ReflowApplyError::PageEditedThisSession) => {}
        Err(ReflowApplyError::Unsupported(msg)) => {
            panic!("the guard still returns Unsupported(String) -- this is the exact defect: {msg}")
        }
        other => panic!(
            "expected PageEditedThisSession from the Pass 251.0 guard, got {other:?}. If the \
             fixture has no reflowable block this test is measuring nothing and needs a \
             different fixture, not a relaxed assertion."
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
