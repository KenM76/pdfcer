//! # A clip written by yesterday's build still reads today
//!
//! ## The defect this file exists for
//!
//! `Pass 270.0` taught the clipboard to carry a markup's dash, opacity, note
//! and author. It did so by appending a **second positional COS object** per
//! markup annotation — and left `CLIP_VERSION` at `3`.
//!
//! So a reader from that build, handed a payload an older build wrote, took a
//! second object that was not there. What it actually consumed was the **next
//! annotation's tag byte and its spec**, mis-parsing every annotation that
//! followed. Silently: `decode_carry` cannot fail by design, because refusing
//! a whole paste over a garbled optional property would lose the geometry too.
//! A guard that cannot report going wrong is a guard that must not be reached
//! by accident.
//!
//! ## ★ Why the rule that exists for this did not fire
//!
//! Decision 105 says a format field cannot be wired into the writer without
//! wiring the decider that says whether to read it. It was written about
//! **droppable dictionary keys** — a key a reader can miss and still know
//! where it is. A *positional* field has no such property: miss it, or take
//! one that is not there, and the parse is off by one object for the rest of
//! the payload.
//!
//! The rule was right. Its stated scope did not reach the change that needed
//! it, and neither did any test: `markup_clip_carry.rs` exercises copy and
//! paste **inside one session**, where `to_bytes` and `from_bytes` are never
//! called. Two shipping functions that are never run against each other are
//! not an inverse pair, whatever their names suggest.
//!
//! ## The property being pinned
//!
//! The operator runs two builds side by side out of two folders and copies in
//! one to paste in the other. That is why the version is **content-dependent**
//! rather than a constant: a plain square still writes at version 2 and still
//! pastes into the older build. Only a clip that would genuinely lose
//! something demands a reader that can carry it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::annot::page_annotations;
use pdfcer_core::annot_author::{BorderDash, Color, MarkupCarry, MarkupSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, MarkupNote, MarkupOptions};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::vector::{
    CLIP_VERSION, CLIP_VERSION_PRE_LABEL_OVERRIDE, ClipAnnotation, ObjectClip,
};

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn square(llx: f64) -> MarkupSpec {
    MarkupSpec::Square {
        rect: Rect {
            llx,
            lly: 40.0,
            urx: llx + 60.0,
            ury: 100.0,
        },
        border: Some(Color::Gray(0.0)),
        interior: None,
        border_width: 1.0,
        border_effect: None,
    }
}

fn rich_carry() -> MarkupCarry {
    let mut c = MarkupCarry::default();
    c.dash = BorderDash::new(vec![4.0, 2.0]);
    c.opacity = Some(0.5);
    c.contents = Some("a note".to_owned());
    c.author = Some("Ken".to_owned());
    c
}

/// A clip of `n` authored squares, built through the REAL copy path.
///
/// `ObjectClip` is `#[non_exhaustive]`, so a consumer of this crate cannot
/// assemble one from a struct literal -- which is a constraint only a test
/// living outside the crate can feel, and the reason this helper drives
/// `copy_annotations` instead of hand-building the payload. The clip under
/// test is therefore the clip a shell would actually hold.
///
/// `rich` makes the **FIRST** square carry the four author-time properties;
/// every later one is plain. Deliberately not "all of them": a clip in which
/// every annotation carries something cannot distinguish a reader that reads
/// the carry per annotation from one that reads a single carry and applies it
/// to all, and it cannot show that an EMPTY carry inside a version-4 payload
/// round-trips as empty rather than being skipped -- which, if it were
/// skipped, would desynchronise the payload the writer just declared.
fn clip_of_squares(n: usize, rich: bool) -> ObjectClip {
    let mut s = EditSession::new(
        Document::load(&fixture("annot/demo-annotated.pdf")).expect("load the fixture"),
    );
    let loaded = MarkupOptions {
        dash: BorderDash::new(vec![4.0, 2.0]),
        opacity: Some(0.5),
        note: Some(MarkupNote::new("a note").by("Ken")),
    };
    let mut ids = Vec::new();
    for i in 0..n {
        let opts = if rich && i == 0 {
            loaded.clone()
        } else {
            MarkupOptions::default()
        };
        ids.push(
            s.add_markup_with(0, &square(40.0 + 160.0 * i as f64), &opts)
                .expect("author"),
        );
    }
    let all = page_annotations(&s.graph(), s.page_slots().expect("slots")[0].id);
    let idx: Vec<usize> = ids
        .iter()
        .map(|id| {
            all.iter()
                .position(|a| a.id == Some(*id))
                .expect("on the page")
        })
        .collect();
    s.copy_annotations(0, &idx).expect("copy")
}

/// ★ A clip with nothing to carry is still written at version 2.
///
/// This is the two-folders property, and it is the reason the fix is a
/// version *gate* rather than a blanket bump. A bump would have been simpler
/// and would have broken every paste between the operator's two builds from
/// the day it shipped, to protect a field most clips do not have.
#[test]
fn a_plain_markup_clip_still_declares_version_two() {
    let c = clip_of_squares(1, false);
    assert_eq!(
        c.version, CLIP_VERSION_PRE_LABEL_OVERRIDE,
        "an empty carry needs no new reader, so it must not demand one"
    );
}

/// And a clip that would lose something declares the version that keeps it.
#[test]
fn a_carrying_markup_clip_declares_the_new_version() {
    let c = clip_of_squares(1, true);
    assert_eq!(c.version, CLIP_VERSION);
}

/// ★★★ THE REGRESSION. An old-format payload, read by the new reader.
///
/// The payload is not hand-assembled and not reasoned about: it is produced by
/// **this build's own writer**, driven at version 2 — which is byte-for-byte
/// the shape every pre-`Pass 270.0` build emitted, since the carry is the only
/// thing that changed and version 2 does not write it.
///
/// Two annotations, deliberately. With one, a reader that over-consumed would
/// simply run out of bytes and could plausibly error; with two, it consumes
/// the **second annotation's tag and spec** as if they were the first's carry,
/// which is the silent, wrong-answer failure rather than the loud one.
#[test]
fn a_version_two_payload_with_two_markups_reads_back_intact() {
    let mut c = clip_of_squares(2, false);
    c.version = CLIP_VERSION_PRE_LABEL_OVERRIDE;

    let bytes = c.to_bytes();
    let back = ObjectClip::from_bytes(&bytes).expect("an old payload must still parse");

    assert_eq!(
        back.annotations.len(),
        2,
        "both annotations must survive -- a reader that took a phantom second \
         object would swallow the second annotation into the first"
    );
    for (i, a) in back.annotations.iter().enumerate() {
        let ClipAnnotation::Markup(spec, carry) = a else {
            panic!(
                "annotation {i} came back as the wrong variant, which is \
                    exactly what an off-by-one-object parse produces"
            );
        };
        assert!(
            matches!(**spec, MarkupSpec::Square { .. }),
            "annotation {i} must still be a square"
        );
        assert_eq!(
            **carry,
            MarkupCarry::default(),
            "a version-2 payload has no carry to read, so the reader must \
             supply the empty one rather than inventing values"
        );
    }
}

/// ★★ And the forward direction: a version-4 payload round-trips its carry.
///
/// Without this the fix could pass every backward-compatibility assertion by
/// simply never writing the carry at all — which would silently re-open the
/// data loss `Pass 270.0` was written to close, while looking like a
/// conservative choice.
#[test]
fn a_version_four_payload_round_trips_the_carry() {
    let c = clip_of_squares(2, true);
    assert_eq!(c.version, CLIP_VERSION);

    let back = ObjectClip::from_bytes(&c.to_bytes()).expect("round trip");
    assert_eq!(back.annotations.len(), 2);

    let ClipAnnotation::Markup(_, first) = &back.annotations[0] else {
        panic!("wrong variant")
    };
    assert_eq!(**first, rich_carry(), "every carried property survives");

    // ★ The second annotation carries NOTHING, in a clip written at version 4.
    // Both objects are present for both annotations at this version -- the
    // gate is per-CLIP, not per-annotation -- so this pins that the empty
    // carry is written and read as empty rather than the writer skipping it
    // and desynchronising the payload it just claimed to be version 4.
    let ClipAnnotation::Markup(_, second) = &back.annotations[1] else {
        panic!("wrong variant")
    };
    assert_eq!(**second, MarkupCarry::default());
}

/// A clip a future build writes is refused by name, not mis-parsed.
///
/// The pre-existing guard, pinned here because the version constant moved and
/// a comparison that was `>` against `3` reading as `>` against `4` is the
/// kind of change that alters a refusal boundary without touching its line.
#[test]
fn a_newer_payload_is_refused_rather_than_guessed_at() {
    let mut c = clip_of_squares(1, true);
    c.version = CLIP_VERSION + 1;
    assert!(
        ObjectClip::from_bytes(&c.to_bytes()).is_err(),
        "a payload from a newer build must be refused, not read optimistically"
    );
}
