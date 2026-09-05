//! `Pass 250.1` — applying a redaction INTO an `EditSession` (deferred to Save).
//!
//! The operator asked for redaction to behave like every other edit: mark,
//! then let Save decide where the bytes land. `EditSession::apply_redactions`
//! does that by COLLAPSING the session onto a clean redacted base (finalizing
//! the document; undo is cleared, by operator ruling 2026-09-04). This test
//! proves the two things that matter: the removed text is GONE from the saved
//! bytes (no leak), and the apply left a coherent, still-saveable session.

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::writer::SaveOptions;
use std::path::Path;

fn hello() -> Document {
    Document::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/hello.pdf"),
    )
    .expect("load hello.pdf")
}

#[test]
fn a_redaction_applied_into_the_session_removes_the_text_from_saved_bytes() {
    let mut session = EditSession::new(hello());
    let marked = session
        .mark_redactions_by_search("Hello", false)
        .expect("mark redactions");
    assert!(!marked.is_empty(), "the search found and marked 'Hello'");

    let report = session.apply_redactions().expect("apply the redaction");
    assert!(
        report.redacted_text.iter().any(|t| t.contains("Hello")),
        "the report discloses the removed text, got {:?}",
        report.redacted_text
    );

    // The session is finalized: undo is cleared and the flag is set.
    assert_eq!(session.undo_depth(), 0, "apply finalizes: undo is cleared");
    assert!(session.has_applied_redaction());

    // BOTH save modes are allowed (the base is clean) and NEITHER leaks.
    for opts in [SaveOptions::identity(), SaveOptions::default()] {
        let (bytes, _) = session
            .to_incremental_bytes(&opts)
            .or_else(|_| session.to_full_bytes(&opts))
            .expect("a redacted session still saves");
        // The removed word must not survive anywhere in the output bytes.
        assert!(
            !contains(&bytes, b"Hello"),
            "redacted text must not appear in the saved bytes"
        );
        // And the output must reopen.
        let reopened = Document::from_bytes(bytes).expect("redacted output reopens");
        assert!(pdfcer_core::page_tree::pages(&reopened).is_ok());
    }
}

#[test]
fn apply_without_marks_is_refused_by_name() {
    use pdfcer_core::redact::RedactError;
    let mut session = EditSession::new(hello());
    assert!(matches!(
        session.apply_redactions(),
        Err(RedactError::NothingToApply)
    ));
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

// -- Pass 250.2: the undo-preserving DEFERRED variant -----------------------

#[test]
fn a_deferred_redaction_preserves_undo_and_refuses_ordinary_saves() {
    use pdfcer_core::writer::WriteError;

    let mut session = EditSession::new(hello());
    // A prior, ordinary edit so there is undo history to preserve.
    session
        .set_info_field(pdfcer_core::edit::InfoField::Title, Some("before"))
        .expect("set a title");
    let undo_before = session.undo_depth();
    assert!(undo_before >= 1, "there is undo history to preserve");

    let marked = session
        .mark_redactions_by_search("Hello", false)
        .expect("mark redactions");
    assert!(!marked.is_empty());

    // Stage the redaction (deferred). The preview report discloses the removal.
    let report = session
        .apply_redactions_deferred()
        .expect("stage the deferred redaction");
    assert!(
        report.redacted_text.iter().any(|t| t.contains("Hello")),
        "the preview report discloses the text that WILL be removed"
    );
    assert!(session.has_pending_redaction());

    // Undo history is UNTOUCHED -- that is the whole point of the deferred
    // variant (contrast apply_redactions, which clears it).
    assert!(
        session.undo_depth() >= undo_before,
        "staging a deferred redaction must not clear undo"
    );
    assert!(session.can_undo());

    // Both ordinary save modes are refused by name while pending.
    assert!(matches!(
        session.to_incremental_bytes(&SaveOptions::default()),
        Err(WriteError::RedactionPending)
    ));
    assert!(matches!(
        session.to_full_bytes(&SaveOptions::default()),
        Err(WriteError::RedactionPending)
    ));

    // The applying save succeeds, removes the text, and does NOT mutate the
    // session (it takes &self): undo still works afterward.
    let (bytes, _) = session
        .save_applying_redaction(&SaveOptions::default())
        .expect("the applying save produces redacted bytes");
    assert!(
        !contains(&bytes, b"Hello"),
        "the deferred redaction removed the text from the saved bytes"
    );
    assert!(
        Document::from_bytes(bytes).is_ok(),
        "redacted output reopens"
    );

    // Session untouched by the save: still pending, still undoable.
    assert!(session.has_pending_redaction());
    assert!(
        session.can_undo(),
        "the &self save left the undo history intact"
    );
}

#[test]
fn cancelling_a_deferred_redaction_restores_ordinary_saves() {
    let mut session = EditSession::new(hello());
    session
        .mark_redactions_by_search("Hello", false)
        .expect("mark");
    session
        .apply_redactions_deferred()
        .expect("stage the redaction");
    assert!(session.has_pending_redaction());

    session.cancel_pending_redaction();
    assert!(!session.has_pending_redaction());
    // Ordinary saves work again (the session was never mutated by staging).
    assert!(session.to_full_bytes(&SaveOptions::default()).is_ok());
}

// -- Pass 250.3: the encryption writers also refuse while a redaction is staged
//    (the leak vector pdfcer-gui found in v0.38.0 — set_encryption ignored the
//    pending flag and would have written the un-redacted content encrypted).

#[test]
fn the_encryption_writers_refuse_while_a_redaction_is_staged() {
    use pdfcer_core::edit::{EncryptError, EncryptionSettings};

    let mut session = EditSession::new(hello());
    session
        .mark_redactions_by_search("Hello", false)
        .expect("mark");
    session
        .apply_redactions_deferred()
        .expect("stage the redaction");
    assert!(session.has_pending_redaction());

    let opts = SaveOptions::default();
    let settings = EncryptionSettings::new(b"u".to_vec(), b"o".to_vec());

    // The redaction guard fires FIRST, before AlreadyEncrypted/NotEncrypted, so
    // all three refuse by name on this (unencrypted) staged session.
    assert!(matches!(
        session.set_encryption(&settings, &opts),
        Err(EncryptError::RedactionPending)
    ));
    assert!(matches!(
        session.set_permissions(&settings, &opts),
        Err(EncryptError::RedactionPending)
    ));
    assert!(matches!(
        session.remove_encryption(&opts),
        Err(EncryptError::RedactionPending)
    ));

    // Cancel, and the guard lifts (set_encryption now reaches its real logic and
    // succeeds on the unencrypted doc).
    session.cancel_pending_redaction();
    assert!(session.set_encryption(&settings, &opts).is_ok());
}
