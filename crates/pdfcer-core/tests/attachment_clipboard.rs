//! Embedded files on the clipboard — copy, cut, paste (`Pass 173.0`).
//!
//! ## Why cut matters more here than anywhere else in the family
//!
//! For every other clipboard, a cut that carried nothing would lose something
//! the operator can see and re-make: a shape, a comment, a field. **An
//! embedded file is the only copy of that data in the document.** A cut whose
//! copy half failed would destroy it outright, with nothing on the page to
//! hint it was ever there.
//!
//! So the ordering assertion — copy first, always — is the load-bearing one,
//! and it is tested against a refusal rather than assumed from the code.
//!
//! ## The clip has no serialisation method, deliberately
//!
//! Its payload IS the file. Write `bytes` out under `name` and you have the
//! attachment; hand the pair to `paste_attachment` and it goes into another
//! document. A private envelope around a file that is already a file would be
//! a format nobody needs.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::attachments::{AttachmentClip, list_attachments};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(name)
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(rel)).expect("load fixture"))
}

/// A session with one attachment, and the key it was filed under.
fn with_attachment(s: &mut EditSession) -> Vec<u8> {
    s.attach_file(
        "spec-rev-C.txt",
        b"the revision C notes",
        Some("Revision C notes"),
    )
    .expect("attach");
    list_attachments(&s.graph())
        .first()
        .map(|a| a.name_bytes.clone())
        .expect("it is listed")
}

#[test]
fn copying_an_attachment_carries_the_file_and_leaves_the_source_alone() {
    let mut s = session("hello.pdf");
    let key = with_attachment(&mut s);

    let clip = s.copy_attachment(&key).expect("copy");
    assert_eq!(clip.name, "spec-rev-C.txt");
    assert_eq!(clip.bytes, b"the revision C notes");
    assert_eq!(clip.description.as_deref(), Some("Revision C notes"));
    assert_eq!(clip.len(), 20);
    assert!(!clip.is_empty());
    assert_eq!(
        list_attachments(&s.graph()).len(),
        1,
        "a copy is not a move",
    );
}

#[test]
fn an_attachment_that_is_not_there_is_refused_by_name() {
    let s = session("hello.pdf");
    assert!(matches!(
        s.copy_attachment(b"nothing.txt"),
        Err(EditError::AttachmentNotFound)
    ));
}

/// The file crosses into another document.
#[test]
fn a_copied_attachment_pastes_into_another_document() {
    let mut source = session("hello.pdf");
    let key = with_attachment(&mut source);
    let clip = source.copy_attachment(&key).expect("copy");

    let mut destination = session("minimal.pdf");
    assert!(list_attachments(&destination.graph()).is_empty());
    destination.paste_attachment(&clip).expect("paste");

    let landed = list_attachments(&destination.graph());
    assert_eq!(landed.len(), 1);
    let arrived = landed.first().expect("one");
    assert_eq!(arrived.name, "spec-rev-C.txt");
    assert_eq!(arrived.description.as_deref(), Some("Revision C notes"));

    // And the BYTES arrived, not just the entry.
    let view = destination.view();
    assert_eq!(
        pdfcer_core::attachments::attachment_bytes(&view, arrived),
        Some(b"the revision C notes".to_vec()),
    );
}

/// ★ Cut is one undo entry, and undoing it puts the file back — bytes and
/// all, not just the name-tree entry.
#[test]
fn cutting_an_attachment_is_one_undo_entry_and_undo_restores_the_bytes() {
    let mut s = session("hello.pdf");
    let key = with_attachment(&mut s);
    let depth_before = s.undo_depth();

    let clip = s.cut_attachment(&key).expect("cut");
    assert_eq!(clip.bytes, b"the revision C notes");
    assert!(
        list_attachments(&s.graph()).is_empty(),
        "the attachment is gone",
    );
    assert_eq!(s.undo_depth(), depth_before + 1, "ONE undo entry");

    s.undo().expect("one press");
    let back = list_attachments(&s.graph());
    assert_eq!(back.len(), 1, "the entry is back");
    let view = s.view();
    assert_eq!(
        back.first()
            .and_then(|a| pdfcer_core::attachments::attachment_bytes(&view, a)),
        Some(b"the revision C notes".to_vec()),
        "and so are its bytes -- an attachment is the only copy of its data in \
         the document, so a half-restored one would be a silent loss",
    );
}

/// ★ A cut whose COPY half refuses detaches nothing.
///
/// The ordering matters more for attachments than for anything else on the
/// clipboard: the embedded file is the only copy of that data in the
/// document, so a cut that carried nothing would destroy it with nothing on
/// the page to hint it was ever there.
#[test]
fn a_cut_whose_copy_refuses_detaches_nothing() {
    let mut s = session("hello.pdf");
    let _ = with_attachment(&mut s);
    let before = list_attachments(&s.graph()).len();

    assert!(matches!(
        s.cut_attachment(b"a key that is not there"),
        Err(EditError::AttachmentNotFound)
    ));
    assert_eq!(
        list_attachments(&s.graph()).len(),
        before,
        "nothing was detached",
    );
}

/// A shell can build a clip from a file the operator dropped on the window —
/// which is the honest way to implement "attach this": it is a paste.
#[test]
fn a_shell_can_build_a_clip_from_a_file_it_already_has() {
    let clip = AttachmentClip::new(
        "dropped.bin",
        vec![1, 2, 3, 4],
        Some("dragged onto the window".to_owned()),
    );
    let mut s = session("hello.pdf");
    s.paste_attachment(&clip).expect("paste");
    assert_eq!(
        list_attachments(&s.graph()).first().map(|a| a.name.clone()),
        Some("dropped.bin".to_owned()),
    );
}

/// An empty attachment is legal and is carried as one, not refused.
#[test]
fn an_empty_attachment_is_a_question_not_a_refusal() {
    let clip = AttachmentClip::new("marker", Vec::new(), None);
    assert!(clip.is_empty());
    let mut s = session("hello.pdf");
    s.paste_attachment(&clip).expect("an empty file is legal");
    assert_eq!(list_attachments(&s.graph()).len(), 1);
}
