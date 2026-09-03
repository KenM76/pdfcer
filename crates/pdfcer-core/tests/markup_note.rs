//! # Note text on geometric markup — `/Contents`, `/T`, `/M` (`Pass 150.0`)
//!
//! Geometric markup could be authored with a shape and a colour and **no
//! words**. `FEATURES.md` carried it as *"note text (`/Contents`) still cannot
//! [be set]"*, and separately as its own planned row: *"`/Contents` with `/T`
//! and `/M` together, since a note listed with no author reads as a broken
//! panel. The read side already ships. **A shell is waiting on this.**"*
//!
//! ## Out of crate, deliberately
//!
//! `MarkupNote` is `#[non_exhaustive]`, so a consuming crate **cannot build it
//! with a struct literal** — only through the builder. Inside `pdfcer-core` that
//! attribute is inert, so an in-crate test would happily write a struct
//! expression no consumer can compile and would prove nothing about the
//! ergonomics being shipped. These tests build it the way `pdfcer` has to.
//!
//! That is also the contrast worth keeping in view: `MarkupOptions` is
//! deliberately **not** `#[non_exhaustive]` (its own doc chose constructability
//! over an unconstructable type), so adding `note` to it **was** a breaking
//! change and dropping its `Copy` was a second one. `MarkupNote` is the other
//! choice, and a field added to it later breaks nobody.
//!
//! ## ★ The decision these tests pin hardest
//!
//! **pdfcer does not read a clock.** `/M` is the caller's string, written
//! verbatim, and a malformed one is refused rather than written. Two reasons:
//! byte-identical output for identical input is an acceptance criterion across
//! this project, and a timestamp pdfcer chose would be a value it *inferred* and
//! wrote silently into the operator's document (rule 4). The `/PieceInfo`
//! sidecar already took this position with a fixed date constant.
//!
//! Fixture provenance: `fixtures/synthetic/annot/PROVENANCE.md`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::annot::page_annotations;
use pdfcer_core::annot_author::{Color, MarkupSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, MarkupNote, MarkupOptions};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::writer::SaveOptions;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn session() -> EditSession {
    EditSession::new(Document::load(&fixture("annot/demo-annotated.pdf")).expect("fixture"))
}

fn square() -> MarkupSpec {
    MarkupSpec::Square {
        rect: Rect {
            llx: 10.0,
            lly: 10.0,
            urx: 60.0,
            ury: 40.0,
        },
        border: Some(Color::Gray(0.0)),
        interior: None,
        border_width: 1.0,
        border_effect: None,
    }
}

/// Author a square with `opts`, save incrementally, re-parse, and return the
/// last annotation on page 1 as the reader sees it.
///
/// Every assertion goes through the bytes rather than the session overlay, so
/// what is checked is what another program would read.
fn authored(opts: &MarkupOptions) -> pdfcer_core::annot::Annotation {
    let mut s = session();
    s.add_markup_with(0, &square(), opts).expect("author");
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save");
    let doc = Document::from_bytes(bytes).expect("re-parse");
    let slots = pdfcer_core::page_tree::pages(&doc).expect("pages");
    let annots = page_annotations(&doc.view(), slots[0].id);
    annots.last().cloned().expect("the authored annotation")
}

// ---------------------------------------------------------------------------
// 1. The three keys, written and read back
// ---------------------------------------------------------------------------

#[test]
fn text_author_and_date_all_reach_the_saved_bytes() {
    let a = authored(&MarkupOptions {
        note: Some(
            MarkupNote::new("Check this dimension")
                .by("Ken")
                .at("D:20260828073200Z"),
        ),
        ..Default::default()
    });
    assert_eq!(a.contents.as_deref(), Some("Check this dimension"));
    assert_eq!(a.title.as_deref(), Some("Ken"));
    assert_eq!(a.mod_date.as_deref(), Some("D:20260828073200Z"));
}

#[test]
fn no_note_writes_none_of_the_three() {
    // The default must stay byte-compatible with every markup authored before
    // this field existed — a caller that does not ask for a note gets exactly
    // what it always got.
    let a = authored(&MarkupOptions::default());
    assert_eq!(a.contents, None);
    assert_eq!(a.title, None);
    assert_eq!(a.mod_date, None);
}

#[test]
fn an_author_alone_is_allowed_without_any_text() {
    // "Attribute this shape to me, I have nothing to say about it" is a real
    // request. Requiring text first would refuse it for no reason.
    let a = authored(&MarkupOptions {
        note: Some(MarkupNote::new("").by("Ken")),
        ..Default::default()
    });
    assert_eq!(a.title.as_deref(), Some("Ken"));
    assert_eq!(
        a.contents.as_deref(),
        Some(""),
        "an empty note is written, not omitted — it is a deliberate blank, \
         distinct from never having had one"
    );
}

#[test]
fn a_note_survives_beside_an_opacity_without_either_disturbing_the_other() {
    // Both are `MarkupOptions` fields applied to the same dictionary; a
    // regression where one overwrote the other's insertion point would be
    // invisible in a single-field test.
    let a = authored(&MarkupOptions {
        opacity: Some(0.4),
        note: Some(MarkupNote::new("half visible").by("Ken")),
    });
    assert_eq!(a.contents.as_deref(), Some("half visible"));
    assert_eq!(a.title.as_deref(), Some("Ken"));
}

#[test]
fn non_ascii_text_round_trips() {
    // `/Contents` is a text string (§7.9.2), so it must survive a name that
    // ASCII cannot hold. A `Vec<u8>` written raw would corrupt this.
    let a = authored(&MarkupOptions {
        note: Some(MarkupNote::new("mesure — Ø 12,5 mm").by("Zoë")),
        ..Default::default()
    });
    assert_eq!(a.contents.as_deref(), Some("mesure — Ø 12,5 mm"));
    assert_eq!(a.title.as_deref(), Some("Zoë"));
}

// ---------------------------------------------------------------------------
// 2. THE DATE DECISION — refused, never invented
// ---------------------------------------------------------------------------

#[test]
fn a_malformed_date_is_refused_by_name_and_nothing_is_written() {
    let mut s = session();
    let before = s.undo_depth();
    let err = s
        .add_markup_with(
            0,
            &square(),
            &MarkupOptions {
                note: Some(MarkupNote::new("hi").at("28 August 2026")),
                ..Default::default()
            },
        )
        .expect_err("a human date is not a PDF date");
    assert!(
        matches!(err, EditError::MarkupDateMalformed { .. }),
        "a NAMED refusal, not a string: {err}"
    );
    assert!(
        err.to_string().contains("28 August 2026"),
        "and it shows what was rejected: {err}"
    );
    assert_eq!(s.undo_depth(), before, "nothing was committed");
}

#[test]
fn every_shape_section_7_9_4_permits_is_accepted() {
    // Every trailing component of a PDF date is optional, so a bare year is a
    // conforming date. A validator that demanded full precision would refuse
    // documents the standard permits.
    for d in [
        "D:2026",
        "D:202608",
        "D:20260828",
        "D:2026082807",
        "D:202608280732",
        "D:20260828073200",
        "D:20260828073200Z",
        "D:20260828073200-04'00",
        "D:20260828073200+05'30",
    ] {
        MarkupNote::new("x")
            .at(d)
            .validate()
            .unwrap_or_else(|e| panic!("{d} is a conforming PDF date, but: {e}"));
    }
}

#[test]
fn the_shapes_it_refuses_are_refused_for_a_stated_reason() {
    for (d, expect) in [
        ("20260828", "starts with `D:`"),
        ("D:26", "four digits"),
        ("D:2026082", "two-digit pairs"),
        ("D:20260828073200X", "must begin with"),
    ] {
        let err = MarkupNote::new("x")
            .at(d)
            .validate()
            .expect_err("malformed: {d}");
        let text = err.to_string();
        assert!(text.contains(expect), "for {d}: {text}");
    }
}

#[test]
fn an_impossible_but_well_formed_date_is_accepted() {
    // February 31st. §7.9.4 gives a SHAPE, not a calendar, and no clause
    // rejects this — so refusing it would be pdfcer inventing a conformance
    // rule. The caller's nonsense date is their claim about their document.
    MarkupNote::new("x")
        .at("D:20260231000000Z")
        .validate()
        .expect("well-formed even though February has no 31st");
}

// ---------------------------------------------------------------------------
// 3. Validation lands on BOTH author routes by construction
// ---------------------------------------------------------------------------

#[test]
fn the_sticky_note_route_refuses_the_same_bad_date() {
    // `MarkupOptions::validate` is called by both `add_markup_with` and
    // `add_text_annotation_with`, which is the property that function's own
    // doc promised — "a future field's validation lands on both routes by
    // construction rather than by whoever remembers". This is that future
    // field, checking the promise held.
    use pdfcer_core::annot_author::{StickyIcon, TextAnnotSpec};
    let mut s = session();
    let spec = TextAnnotSpec::Sticky {
        rect: Rect {
            llx: 20.0,
            lly: 20.0,
            urx: 40.0,
            ury: 40.0,
        },
        icon: StickyIcon::default(),
        contents: String::new(),
        color: Color::Gray(0.0),
        open: false,
    };
    let err = s.add_text_annotation_with(
        0,
        &spec,
        &MarkupOptions {
            note: Some(MarkupNote::new("hi").at("nonsense")),
            ..Default::default()
        },
    );
    assert!(
        matches!(err, Err(EditError::MarkupDateMalformed { .. })),
        "the other author route must refuse identically, got {err:?}"
    );
}
