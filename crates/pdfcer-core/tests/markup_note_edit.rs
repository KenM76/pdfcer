//! # Writing a note onto an annotation that already exists (`Pass 154.0`)
//!
//! ## The gap this closes
//!
//! `MarkupOptions` is an **author-time** structure — it reaches
//! `add_markup_with` and `add_text_annotation_with` and nothing else — so
//! until now a note could only be written at the moment an annotation was
//! created.
//!
//! **A geometric markup has no text-entry moment.** A cloud, a rectangle, a
//! highlight and an arrow are authored on mouse-release from geometry alone.
//! The model every reviewer UI converges on is *draw the shape → it is
//! selected → type the comment in the panel beside the page*, and that second
//! arrow needs a verb acting on an existing annotation.
//!
//! Without one, `pdfcer-gui`'s Comments panel shipped **read-only**: it listed
//! comments and could not write one. Four ordinary review acts were simply
//! absent — commenting a shape you just drew, commenting a highlight you just
//! swept, fixing a typo in your own comment, and answering someone else's.
//!
//! ## What these tests pin
//!
//! 1. **The words reach the saved bytes**, read back through the same
//!    extraction path a reader uses — not through the report.
//! 2. **A partial note does not clear what it omits.** Fixing a typo must not
//!    un-sign a comment.
//! 3. **The replaced text is disclosed**, because a note is content that
//!    leaves *no trace on the page* when overwritten — unlike a restyle,
//!    where the geometry still shows.
//! 4. **Undo restores the words and keeps the shape**, which is the whole
//!    reason this is its own `CommandKind` rather than part of a restyle.
//! 5. **Clearing is a different act from setting an empty string.**
//!
//! Fixture provenance: `fixtures/synthetic/annot/PROVENANCE.md`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::annot::page_annotations;
use pdfcer_core::annot_author::{Color, MarkupSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, MarkupNote};
use pdfcer_core::object::ObjId;
use pdfcer_core::page_tree::Rect;
use pdfcer_core::writer::SaveOptions;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(rel)).expect("load fixture"))
}

/// A square with no note, authored the way a reviewer's pen would author it:
/// from geometry alone, with no text-entry moment.
fn drawn_shape() -> (EditSession, ObjId) {
    let mut s = session("annot/demo-annotated.pdf");
    let spec = MarkupSpec::Square {
        rect: Rect::from_corners(100.0, 100.0, 200.0, 160.0),
        border: Some(Color::Gray(0.0)),
        interior: None,
        border_width: 2.0,
        border_effect: None,
    };
    let id = s.add_markup(0, &spec).expect("author the shape");
    (s, id)
}

/// The note as a reader sees it, from the SAVED bytes through the ordinary
/// annotation walk — never from the report that claims to have written it.
fn note_of(s: &EditSession, id: ObjId) -> (Option<String>, Option<String>, Option<String>) {
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("incremental save");
    let doc = Document::from_bytes(bytes).expect("re-parse");
    let pages = pdfcer_core::page_tree::pages(&doc).expect("pages");
    let a = page_annotations(&doc.view(), pages[0].id)
        .into_iter()
        .find(|a| a.id == Some(id))
        .expect("the annotation survives the save");
    (a.contents, a.title, a.mod_date)
}

// ---------------------------------------------------------------------------
// 1. THE ASK — a shape drawn without words can be given words
// ---------------------------------------------------------------------------

#[test]
fn a_note_can_be_written_onto_a_shape_that_already_exists() {
    let (mut s, id) = drawn_shape();
    let out = s
        .set_markup_note(
            id,
            &MarkupNote::new("Check this radius")
                .by("Ken")
                .at("D:20260828120000Z"),
        )
        .expect("writing a note onto an existing annotation must work");

    assert_eq!(out.replaced, None, "the shape had no note before");
    assert_eq!(out.keys_written, vec!["Contents", "T", "M"]);

    let (contents, title, modified) = note_of(&s, id);
    assert_eq!(contents.as_deref(), Some("Check this radius"));
    assert_eq!(title.as_deref(), Some("Ken"));
    assert_eq!(modified.as_deref(), Some("D:20260828120000Z"));
}

// ---------------------------------------------------------------------------
// 2. FIXING A TYPO MUST NOT UN-SIGN THE COMMENT
// ---------------------------------------------------------------------------

/// ★ The one a shell would hit on its second day. "Correct a typo in a
/// comment you wrote a minute ago" is one of the four acts this Pass exists
/// for, and an implementation that wrote all three keys unconditionally would
/// silently strip the author and date on every correction — leaving a review
/// comment from nobody, dated never, and looking exactly like a note somebody
/// else had mangled.
#[test]
fn a_note_without_an_author_leaves_the_existing_author_alone() {
    let (mut s, id) = drawn_shape();
    s.set_markup_note(
        id,
        &MarkupNote::new("Chek this radius")
            .by("Ken")
            .at("D:20260828120000Z"),
    )
    .expect("first note");

    let out = s
        .set_markup_note(id, &MarkupNote::new("Check this radius"))
        .expect("the correction");

    assert_eq!(
        out.keys_written,
        vec!["Contents"],
        "only the words moved, and the report must say so"
    );

    let (contents, title, modified) = note_of(&s, id);
    assert_eq!(contents.as_deref(), Some("Check this radius"));
    assert_eq!(title.as_deref(), Some("Ken"), "still signed");
    assert_eq!(
        modified.as_deref(),
        Some("D:20260828120000Z"),
        "and still dated — a shell supplying a fresh /M is a choice, not a \
         requirement this verb imposes"
    );
}

// ---------------------------------------------------------------------------
// 3. THE DISCLOSURE — the case the consuming project asked for by name
// ---------------------------------------------------------------------------

/// ★★ A note is content the operator **cannot recover from the canvas**.
///
/// A restyled shape still shows its geometry, so `set_markup_style` can be
/// checked by eye. Overwritten words leave *no trace on the page at all* —
/// which is why the previous text is carried, not merely a count of it, so a
/// shell can offer it back rather than only mention its size.
#[test]
fn replacing_a_note_discloses_the_words_it_destroyed() {
    let (mut s, id) = drawn_shape();
    s.set_markup_note(id, &MarkupNote::new("The first remark").by("Ken"))
        .expect("first note");

    let out = s
        .set_markup_note(id, &MarkupNote::new("The second remark").by("Sam"))
        .expect("overwrite");

    assert_eq!(
        out.replaced.as_deref(),
        Some("The first remark"),
        "the destroyed words must come back in the report, verbatim"
    );
    assert_eq!(out.replaced_author.as_deref(), Some("Ken"));
}

// ---------------------------------------------------------------------------
// 4. UNDO — the reason this is its own CommandKind
// ---------------------------------------------------------------------------

/// Undoing a note change restores the previous words on an annotation that
/// **stays**. Undoing an *add* would remove the shape; bundling the two into
/// one command kind would make `Ctrl+Z` ambiguous at exactly the moment a
/// reviewer reaches for it.
#[test]
fn undo_restores_the_previous_words_and_keeps_the_shape() {
    let (mut s, id) = drawn_shape();
    s.set_markup_note(id, &MarkupNote::new("The first remark"))
        .expect("first");
    s.set_markup_note(id, &MarkupNote::new("The second remark"))
        .expect("second");

    assert!(
        s.undo().is_some(),
        "the note change is one undoable command"
    );

    let (contents, _, _) = note_of(&s, id);
    assert_eq!(
        contents.as_deref(),
        Some("The first remark"),
        "undo restores the words"
    );

    // And the shape is still there — asserted separately, because "the words
    // came back" and "the annotation survived" are different claims and a
    // wrong CommandKind would break only the second.
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save");
    let doc = Document::from_bytes(bytes).expect("re-parse");
    let pages = pdfcer_core::page_tree::pages(&doc).expect("pages");
    assert!(
        page_annotations(&doc.view(), pages[0].id)
            .iter()
            .any(|a| a.id == Some(id)),
        "undoing a note change must not remove the annotation"
    );
}

// ---------------------------------------------------------------------------
// 5. CLEARING IS A DIFFERENT ACT FROM AN EMPTY COMMENT
// ---------------------------------------------------------------------------

#[test]
fn clearing_removes_all_three_keys_and_reports_what_went() {
    let (mut s, id) = drawn_shape();
    s.set_markup_note(
        id,
        &MarkupNote::new("Delete me")
            .by("Ken")
            .at("D:20260828120000Z"),
    )
    .expect("note");

    let out = s.clear_markup_note(id).expect("clear");
    assert_eq!(out.replaced.as_deref(), Some("Delete me"));
    assert_eq!(out.replaced_author.as_deref(), Some("Ken"));
    assert_eq!(out.keys_written, vec!["Contents", "T", "M"]);

    let (contents, title, modified) = note_of(&s, id);
    assert_eq!(contents, None);
    assert_eq!(title, None);
    assert_eq!(modified, None);
}

/// An empty comment is a comment. A reviewer who deletes their remark has done
/// something different from one who left a blank one, and the saved bytes must
/// be able to tell them apart — `/Contents ()` present versus absent.
#[test]
fn an_empty_note_is_not_the_same_as_no_note() {
    let (mut s, id) = drawn_shape();
    s.set_markup_note(id, &MarkupNote::new(""))
        .expect("an empty comment is still a comment");
    let (empty, _, _) = note_of(&s, id);
    assert_eq!(empty.as_deref(), Some(""), "present and empty");

    s.clear_markup_note(id).expect("clear");
    let (gone, _, _) = note_of(&s, id);
    assert_eq!(gone, None, "absent");
}

// ---------------------------------------------------------------------------
// 6. REFUSALS
// ---------------------------------------------------------------------------

/// A widget's `/Contents` is its field's **tooltip** (§12.5.6.19), which
/// belongs to the field and may be shared by several widgets. Writing it as
/// though it were a review comment would edit a form's help text from a
/// comments panel.
#[test]
fn a_widget_is_refused_and_the_message_names_the_field_verb() {
    let mut s = session("forms/demo-form.pdf");
    let slots = s.page_slots().expect("page slots");
    let id = page_annotations(&s.graph(), slots[0].id)
        .iter()
        .find_map(|a| a.id)
        .expect("a widget with an object identity");

    match s.set_markup_note(id, &MarkupNote::new("x")) {
        Err(EditError::AnnotationMoveWrongVerb {
            use_instead, why, ..
        }) => {
            assert!(use_instead.starts_with("edit_field"), "{use_instead}");
            assert!(why.contains("tooltip"), "{why}");
        }
        other => panic!("a widget must be refused, got {other:?}"),
    }
}

#[test]
fn a_malformed_date_is_refused_by_name() {
    let (mut s, id) = drawn_shape();
    match s.set_markup_note(id, &MarkupNote::new("x").at("last Tuesday")) {
        Err(EditError::MarkupDateMalformed { .. }) => {}
        other => panic!("a non-7.9.4 date must be refused, got {other:?}"),
    }
}
