//! `EditSession::checkpoint` / `rollback`: abandoning a gesture of several
//! verbs restores the document and both history stacks.

use pdfcer_core::edit::{CheckpointError, EditSession, MAX_UNDO_DEPTH};
use pdfcer_core::text_edit::{EditOptions, EditRequest, FormatOptions, FormatRequest};
use pdfcer_core::writer::SaveOptions;

use crate::text_render_mode::page;

fn session() -> EditSession {
    EditSession::new(page("BT /F1 12 Tf 72 700 Td (alpha beta) Tj ET"))
}

fn bytes(s: &EditSession) -> Vec<u8> {
    s.to_full_bytes(&SaveOptions::identity()).unwrap().0
}

fn replace(s: &mut EditSession, find: &str, with: &str) {
    s.edit_text(
        &EditRequest::find_replace(0, find, with),
        &EditOptions::default(),
    )
    .unwrap();
}

/// The request's acceptance case: `edit_text` lands, `format_text` is
/// refused, and the rollback leaves the document and Redo as they were.
#[test]
fn a_refused_second_verb_rolls_back_the_first() {
    let mut s = session();
    replace(&mut s, "alpha", "gamma");
    s.undo();
    let before = bytes(&s);
    let (undo, redo) = (s.undo_depth(), s.redo_depth());
    assert_eq!(redo, 1);

    let cp = s.checkpoint();
    replace(&mut s, "beta", "delta");
    assert!(
        s.format_text(
            &FormatRequest::new(0, "delta").render_mode(8),
            &FormatOptions::default(),
        )
        .is_err()
    );
    let out = s.rollback(cp).unwrap();

    assert_eq!(out.undone, 1);
    assert_eq!(out.history_lost, 0);
    assert_eq!(bytes(&s), before);
    assert_eq!((s.undo_depth(), s.redo_depth()), (undo, redo));
    s.redo();
    assert!(String::from_utf8_lossy(&bytes(&s)).contains("gamma"));
}

/// The abandoned verb is not offered to Redo.
#[test]
fn no_redo_entry_is_left() {
    let mut s = session();
    let cp = s.checkpoint();
    replace(&mut s, "alpha", "gamma");
    replace(&mut s, "beta", "delta");
    assert_eq!(s.rollback(cp).unwrap().undone, 2);
    assert!(!s.can_redo());
    assert!(!s.can_undo());
}

/// A gesture folded into one entry is undone as that one entry.
#[test]
fn a_folded_gesture_rolls_back_as_one() {
    let mut s = session();
    replace(&mut s, "alpha", "gamma");
    let before = bytes(&s);
    let cp = s.checkpoint();
    replace(&mut s, "beta", "delta");
    replace(&mut s, "gamma", "omega");
    assert!(s.coalesce_last(2, pdfcer_core::edit::CommandKind::EditText));
    assert_eq!(s.rollback(cp).unwrap().undone, 1);
    assert_eq!(bytes(&s), before);
    assert_eq!(s.undo_depth(), 1);
}

#[test]
fn another_sessions_checkpoint_is_refused() {
    let other = session();
    let mut s = session();
    replace(&mut s, "alpha", "gamma");
    assert_eq!(
        s.rollback(other.checkpoint()).unwrap_err(),
        CheckpointError::ForeignSession
    );
    assert_eq!(s.undo_depth(), 1);
}

/// An undo that crosses the checkpoint makes it unrestorable; the refusal
/// changes nothing.
#[test]
fn history_changed_under_the_checkpoint_is_refused() {
    let mut s = session();
    replace(&mut s, "alpha", "gamma");
    let cp = s.checkpoint();
    s.undo();
    replace(&mut s, "beta", "delta");
    let held = bytes(&s);
    assert_eq!(s.rollback(cp).unwrap_err(), CheckpointError::HistoryChanged);
    assert_eq!(bytes(&s), held);
    assert_eq!(s.undo_depth(), 1);
}

/// At full depth every verb of the gesture evicts the oldest entry; the
/// rollback puts them back, so the whole history still undoes to the base.
#[test]
fn a_rollback_at_full_depth_restores_the_evicted_history() {
    let mut s = session();
    let base = bytes(&s);
    let words = ["alpha", "gamma"];
    for i in 0..MAX_UNDO_DEPTH {
        replace(&mut s, words[i % 2], words[(i + 1) % 2]);
    }
    let before = bytes(&s);
    let cp = s.checkpoint();
    for _ in 0..3 {
        replace(&mut s, "beta", "delta");
        replace(&mut s, "delta", "beta");
    }
    let out = s.rollback(cp).unwrap();
    assert_eq!((out.undone, out.history_lost), (6, 0));
    assert_eq!(bytes(&s), before);
    assert_eq!(s.undo_depth(), MAX_UNDO_DEPTH);
    while s.undo().is_some() {}
    assert_eq!(bytes(&s), base);
}

/// A gesture longer than the depth bound cannot be undone, so it is refused.
#[test]
fn a_gesture_longer_than_the_bound_is_refused() {
    let mut s = session();
    replace(&mut s, "alpha", "gamma");
    let cp = s.checkpoint();
    let words = ["beta", "delta"];
    for i in 0..MAX_UNDO_DEPTH + 1 {
        replace(&mut s, words[i % 2], words[(i + 1) % 2]);
    }
    assert_eq!(
        s.rollback(cp).unwrap_err(),
        CheckpointError::GestureEvicted { evicted: 1 }
    );
}
