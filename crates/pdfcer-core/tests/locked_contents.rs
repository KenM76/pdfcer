//! # `LockedContents` and `Locked` forbid near-opposite things
//!
//! ## The flag that was read and never consulted
//!
//! ISO 32000-1 §12.5.3 Table 165 bit 10, `LockedContents` (value **512** --
//! bit N has value 2^(N-1), and 1024 is the wrong answer this project has now
//! had to correct twice; the code has always been right, `1 << 9`,
//! PDF 1.7): *"If set, do not allow the contents of the annotation to be
//! modified."* Followed immediately by the sentence that makes it a separate
//! gate rather than a synonym: it *"does not restrict deletion or other
//! property changes."*
//!
//! `AnnotFlags::locked_contents` existed from the day annotations shipped.
//! **Nothing in the crate called it.** That is what an unenforced flag with no
//! display consequence looks like from the inside: nothing renders
//! differently, every test passes, and every document behaves identically —
//! except the ones where it matters, which nobody had.
//!
//! ## The table that makes conflating them a two-way error
//!
//! | flag | bit | value | forbids | permits |
//! |---|---|---|---|---|
//! | `Locked` | 8 | 128 | deletion, position, size, properties | **editing the comment text** |
//! | `LockedContents` | 10 | 512 | **editing the comment text** | deletion, moving, restyling |
//!
//! They are close to complements. An implementation that raised one refusal
//! for both would **refuse a permitted edit** on one document and **permit a
//! forbidden one** on another, and from inside the verb that did it neither
//! would look wrong. So this file does not merely assert that the new gate
//! fires — it asserts, in both directions, that each flag leaves the other's
//! territory alone. A test that only checked "locked contents refuses a note
//! edit" would pass just as happily against an implementation that had wired
//! the wrong flag.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::annot::{AnnotFlags, page_annotations};
use pdfcer_core::annot_author::{Color, MarkupSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{
    EditError, EditSession, MarkupNote, MarkupStyle, ResizeOptions, StyleEdit,
};
use pdfcer_core::object::ObjId;
use pdfcer_core::page_tree::Rect;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

/// A session carrying one authored square, with `flags` set on it.
///
/// The flags are applied through `set_annotation_flags` rather than baked
/// into a fixture, which keeps the two halves of this file honest: the same
/// verb an operator would use to *clear* the flag is the one that sets it, so
/// the documented recovery path is exercised on every test here rather than
/// asserted in prose.
fn square_with_flags(flags: AnnotFlags) -> (EditSession, ObjId) {
    let mut s = EditSession::new(
        Document::load(&fixture("annot/demo-annotated.pdf")).expect("load the fixture"),
    );
    let spec = MarkupSpec::Square {
        rect: Rect {
            llx: 40.0,
            lly: 40.0,
            urx: 140.0,
            ury: 100.0,
        },
        border: Some(Color::Gray(0.0)),
        interior: None,
        border_width: 1.0,
        border_effect: None,
    };
    let id = s.add_markup(0, &spec).expect("author a square");
    s.set_markup_note(id, &MarkupNote::new("the original words").by("Ken"))
        .expect("the note goes on BEFORE the lock, or there is nothing to protect");
    s.set_annotation_flags(id, flags).expect("set the flags");
    (s, id)
}

/// The annotation's `/Contents`, read back out of the graph.
fn contents(s: &EditSession, id: ObjId) -> Option<String> {
    let annots = page_annotations(&s.graph(), s.page_slots().expect("slots")[0].id);
    annots
        .into_iter()
        .find(|a| a.id == Some(id))
        .and_then(|a| a.contents)
}

/// ★★ `LockedContents` refuses a note edit — the gate that did not exist.
#[test]
fn locked_contents_refuses_a_note_edit() {
    let (mut s, id) = square_with_flags(AnnotFlags(AnnotFlags::LOCKED_CONTENTS));
    let err = s
        .set_markup_note(id, &MarkupNote::new("overwritten"))
        .expect_err("bit 10 says the contents shall not be modified");
    assert!(
        matches!(err, EditError::AnnotationContentsLocked { .. }),
        "refused by name, not by a generic failure: {err:?}"
    );
    assert_eq!(
        contents(&s, id).as_deref(),
        Some("the original words"),
        "a refusal must leave the words alone -- a verb that refused AFTER \
         writing would satisfy the error assertion and lose the comment"
    );
}

/// And it refuses a note *deletion* too, which is still a contents change.
///
/// `clear_markup_note` is a different verb with its own entry point, and the
/// clause is about modifying contents rather than about one spelling of it.
/// Deleting a reviewer's remark is the most destructive contents change there
/// is, so a gate that caught only the overwrite would be guarding the lesser
/// case.
#[test]
fn locked_contents_refuses_clearing_the_note() {
    let (mut s, id) = square_with_flags(AnnotFlags(AnnotFlags::LOCKED_CONTENTS));
    let err = s
        .clear_markup_note(id)
        .expect_err("deleting the contents is modifying them");
    assert!(matches!(err, EditError::AnnotationContentsLocked { .. }));
    assert_eq!(contents(&s, id).as_deref(), Some("the original words"));
}

/// ★★ `LockedContents` does **not** restrict a restyle or a resize.
///
/// Table 165 says so in as many words. This is the half a one-directional
/// test would miss: an implementation that treated bit 10 as a general
/// write-lock would pass the two tests above and start refusing edits the
/// standard explicitly permits.
#[test]
fn locked_contents_permits_property_changes() {
    let (mut s, id) = square_with_flags(AnnotFlags(AnnotFlags::LOCKED_CONTENTS));

    let style = MarkupStyle {
        stroke: Some(StyleEdit::Set(Color::Rgb(1.0, 0.0, 0.0))),
        ..Default::default()
    };
    s.set_markup_style(id, &style)
        .expect("bit 10 does not restrict property changes");

    s.resize_annotation(
        id,
        (40.0, 40.0),
        1.5,
        1.5,
        &ResizeOptions::new().with_scale_stroke_width(true),
    )
    .expect("nor geometry -- that is bit 8's business, and bit 8 is not set");

    s.delete_annotation(id)
        .expect("nor deletion, which Table 165 names explicitly");
}

/// ★★ And the mirror: `Locked` **permits** the note edit it does not mention.
///
/// Bit 8 forbids deletion and *"properties (including position and size)"*.
/// A comment is neither. Without this assertion, wiring the note gate to bit 8
/// would look correct from every other test in this file.
#[test]
fn locked_permits_a_note_edit_but_refuses_geometry() {
    let (mut s, id) = square_with_flags(AnnotFlags(AnnotFlags::LOCKED));

    s.set_markup_note(
        id,
        &MarkupNote::new("a locked shape can still be commented on"),
    )
    .expect("bit 8 says nothing about contents");
    assert_eq!(
        contents(&s, id).as_deref(),
        Some("a locked shape can still be commented on")
    );

    let err = s
        .resize_annotation(id, (40.0, 40.0), 1.5, 1.5, &ResizeOptions::new())
        .expect_err("but geometry is exactly what bit 8 forbids");
    assert!(
        matches!(err, EditError::AnnotationLocked { .. }),
        "and it is the OTHER error, so a shell can tell the two apart: {err:?}"
    );
}

/// Both flags together refuse everything, and the contents refusal is the one
/// reported for a contents edit.
///
/// Which error wins matters: the message names the flag to clear, and naming
/// `Locked` to an operator trying to fix a comment would send them to clear a
/// flag that was never blocking them.
#[test]
fn both_flags_report_the_one_that_applies() {
    let (mut s, id) =
        square_with_flags(AnnotFlags(AnnotFlags::LOCKED | AnnotFlags::LOCKED_CONTENTS));

    let err = s
        .set_markup_note(id, &MarkupNote::new("nope"))
        .expect_err("contents are locked");
    assert!(
        matches!(err, EditError::AnnotationContentsLocked { .. }),
        "a contents edit reports the CONTENTS lock, so the operator is sent \
         to the flag that is actually stopping them: {err:?}"
    );

    let err = s
        .resize_annotation(id, (40.0, 40.0), 1.5, 1.5, &ResizeOptions::new())
        .expect_err("geometry is locked");
    assert!(matches!(err, EditError::AnnotationLocked { .. }));
}

/// Clearing the flag is the documented recovery, and it works.
///
/// Unlike `Locked` — whose remedy is a property change that `Locked` itself
/// forbids, so the message has to send the operator to another application —
/// `LockedContents` explicitly permits property changes, which means pdfcer
/// can undo the lock itself. The error message promises exactly that; this
/// pins the promise.
#[test]
fn clearing_the_flag_restores_the_edit() {
    let (mut s, id) = square_with_flags(AnnotFlags(AnnotFlags::LOCKED_CONTENTS));
    s.set_markup_note(id, &MarkupNote::new("nope"))
        .expect_err("locked");

    s.set_annotation_flags(id, AnnotFlags(0))
        .expect("clearing the flag is a PROPERTY change, which bit 10 permits");

    s.set_markup_note(id, &MarkupNote::new("now it takes"))
        .expect("and the edit goes through");
    assert_eq!(contents(&s, id).as_deref(), Some("now it takes"));
}
