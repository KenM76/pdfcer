//! `EditSession::export_history` / `import_history` (pdfcer-gui request
//! G184): a session's unsaved edits and undo/redo history move to a session
//! opened from the same base bytes, and nothing changes on a refusal.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditSession, HistoryError, InfoField};
use pdfcer_core::text_edit::AddTextRequest;
use pdfcer_core::writer::SaveOptions;

fn base() -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/addtext/plain.pdf"),
    )
    .unwrap()
}

fn session(bytes: &[u8]) -> EditSession {
    EditSession::new(Document::from_bytes(bytes.to_vec()).unwrap())
}

fn saved(s: &EditSession) -> Vec<u8> {
    s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0
}

/// Three commands, the last undone: an authored content stream (staged
/// bytes), an `/Info` edit and a rotation on the redo stack.
fn edited() -> EditSession {
    let mut s = session(&base());
    s.add_text(&AddTextRequest::new(0, (100.0, 650.0), "Moved run"))
        .unwrap();
    s.set_info_field(InfoField::Title, Some("Carried")).unwrap();
    s.rotate_pages(&[0], 90).unwrap();
    s.undo().unwrap();
    s
}

#[test]
fn the_history_arrives_whole_and_saves_identically() {
    let mut a = edited();
    let mut b = session(&base());
    let out = b.import_history(&a.export_history()).unwrap();
    assert_eq!((out.undo_steps, out.redo_steps), (2, 1));
    assert_eq!(
        b.undo_kinds().collect::<Vec<_>>(),
        a.undo_kinds().collect::<Vec<_>>()
    );
    assert_eq!(
        b.redo_kind(),
        Some(CommandKind::RotatePages {
            count: 1,
            delta: 90
        })
    );
    assert_eq!(saved(&b), saved(&a), "same unsaved state");

    for s in [&mut a, &mut b] {
        s.redo().unwrap();
    }
    assert_eq!(saved(&b), saved(&a), "redo carried");
    for s in [&mut a, &mut b] {
        while s.undo().is_some() {}
    }
    assert_eq!(saved(&b), saved(&a), "undo carried to the base");
    assert!(!b.can_undo());
}

#[test]
fn a_refusal_changes_nothing() {
    let history = edited().export_history();

    let mut other_base = base();
    other_base.extend_from_slice(b"\n% another revision\n");
    let mut wrong = session(&other_base);
    assert_eq!(
        wrong.import_history(&history),
        Err(HistoryError::BaseMismatch)
    );

    let mut busy = session(&base());
    busy.set_info_field(InfoField::Author, Some("Here"))
        .unwrap();
    let before = saved(&busy);
    assert_eq!(
        busy.import_history(&history),
        Err(HistoryError::SessionNotPristine)
    );
    assert_eq!(saved(&busy), before);

    let mut fresh = session(&base());
    assert_eq!(
        fresh.import_history(b"%PDF-1.7"),
        Err(HistoryError::NotAHistory)
    );
    for cut in [history.len() - 1, history.len() / 2, 11] {
        let err = fresh.import_history(&history[..cut]).unwrap_err();
        assert!(
            matches!(err, HistoryError::Malformed { .. }),
            "{cut}: {err:?}"
        );
    }
    assert!(!fresh.can_undo());
    assert!(fresh.import_history(&history).is_ok(), "still pristine");
}

#[test]
fn an_unknown_command_is_refused_by_name() {
    let mut s = session(&base());
    s.set_info_field(InfoField::Title, Some("x")).unwrap();
    let history = s.export_history();
    let at = history
        .windows(12)
        .position(|w| w == b"SetInfoField")
        .unwrap();
    let mut renamed = history.clone();
    renamed[at..at + 12].copy_from_slice(b"SetInfoFiele");
    let err = session(&base()).import_history(&renamed).unwrap_err();
    assert_eq!(
        err,
        HistoryError::UnknownCommand {
            name: "SetInfoFiele".into()
        }
    );
}
