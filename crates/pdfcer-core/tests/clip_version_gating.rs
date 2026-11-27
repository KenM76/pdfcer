//! # A clip written by yesterday's build still reads today
//!
//! The operator runs two builds side by side out of two folders and copies in
//! one to paste in the other, so the clip version is **content-dependent**: a
//! clip is written at the lowest version that represents it, and a markup
//! (carried raw, tag 3) needs only version 2. A clip from a newer build is
//! refused by name, and so is one holding the retired modelled-markup form
//! (tag 0): its length cannot be skipped, so reading on would mis-parse every
//! annotation after it.
//!
//! A positional field cannot be missed without the parse losing its place,
//! which is why `to_bytes` and `from_bytes` are run against each other here
//! rather than copy and paste inside one session, where neither is called.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::annot::page_annotations;
use pdfcer_core::annot_author::{BorderDash, Color, MarkupSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, MarkupNote, MarkupOptions};
use pdfcer_core::object::Object;
use pdfcer_core::page_tree::Rect;
use pdfcer_core::vector::{
    CLIP_MAGIC, CLIP_VERSION, CLIP_VERSION_PRE_ANCHORS, CLIP_VERSION_PRE_LABEL_OVERRIDE,
    CLIP_VERSION_PRE_REPLY_COUNT, ClipAnnotation, ObjectClip,
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

/// A clip of `n` authored squares, built through the REAL copy path.
///
/// `ObjectClip` is `#[non_exhaustive]`, so a consumer of this crate cannot
/// assemble one from a struct literal -- which is a constraint only a test
/// living outside the crate can feel, and the reason this helper drives
/// `copy_annotations` instead of hand-building the payload. The clip under
/// test is therefore the clip a shell would actually hold.
///
/// `rich` makes only the FIRST square carry a dash, opacity, note and
/// author, so a reader that applied one annotation's keys to all would show.
fn clip_of_squares(n: usize, rich: bool) -> ObjectClip {
    let mut s = EditSession::new(
        Document::load(&fixture("annot/demo-annotated.pdf")).expect("load the fixture"),
    );
    let loaded = MarkupOptions {
        dash: BorderDash::new(vec![4.0, 2.0]),
        opacity: Some(0.5),
        note: Some(MarkupNote::new("a note").by("Ken")),
        layer: None,
        hand_signature: None,
        highlighter: false,
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

/// A markup clip, plain or carrying author-time keys, is written at version
/// 2: the raw carrier needs no newer reader, so it must not demand one.
#[test]
fn a_markup_clip_declares_version_two_whatever_it_carries() {
    assert_eq!(
        clip_of_squares(1, false).version,
        CLIP_VERSION_PRE_LABEL_OVERRIDE
    );
    assert_eq!(
        clip_of_squares(1, true).version,
        CLIP_VERSION_PRE_LABEL_OVERRIDE
    );
}

/// Two markups round-trip the byte format, each with its own keys.
///
/// Two, deliberately: a reader that over-consumed would swallow the second
/// annotation into the first rather than running out of bytes.
#[test]
fn a_version_two_payload_with_two_markups_reads_back_intact() {
    let c = clip_of_squares(2, true);
    let back = ObjectClip::from_bytes(&c.to_bytes()).expect("round trip");
    assert_eq!(back, c, "the byte format is lossless");
    assert_eq!(back.annotations.len(), 2);
    let keys: Vec<(Option<Object>, Option<Object>)> = back
        .annotations
        .iter()
        .map(|a| {
            let ClipAnnotation::Raw(raw) = a else {
                panic!("a markup travels raw: {a:?}")
            };
            assert_eq!(raw.subtype, "Square");
            (raw.dict.get(b"CA").cloned(), raw.dict.get(b"T").cloned())
        })
        .collect();
    assert!(keys[0].0.is_some() && keys[0].1.is_some(), "{keys:?}");
    assert_eq!(
        keys[1],
        (None, None),
        "the plain one gains nothing: {keys:?}"
    );
}

/// A payload holding the retired modelled-markup form (tag 0) is refused,
/// naming what to do, rather than read as a raw annotation's subtype.
#[test]
fn a_retired_markup_form_is_refused_with_a_remedy() {
    let mut bytes = CLIP_MAGIC.to_vec();
    bytes.extend_from_slice(&CLIP_VERSION_PRE_LABEL_OVERRIDE.to_le_bytes());
    for _ in 0..4 {
        bytes.extend_from_slice(&0.0f64.to_le_bytes());
    }
    bytes.extend_from_slice(&0u32.to_le_bytes()); // items
    bytes.extend_from_slice(&0u32.to_le_bytes()); // objects
    bytes.extend_from_slice(&1u32.to_le_bytes()); // annotations
    bytes.push(0);
    let err = ObjectClip::from_bytes(&bytes).expect_err("tag 0 is refused");
    assert!(err.to_string().contains("copy it again"), "{err}");
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

/// A copied reply loses its `/IRT` link, and the clip counts it (`Pass 253.4`).
///
/// Copies a parent square plus its reply; the count must be 1, survive the
/// byte round trip, and reach the paste as a disclosure. A clip with no reply
/// stays at its older version, which `a_plain_markup_clip_still_declares_version_two`
/// already pins.
#[test]
fn a_copied_reply_is_counted_and_disclosed_on_paste() {
    let mut s = EditSession::new(
        Document::load(&fixture("annot/demo-annotated.pdf")).expect("load the fixture"),
    );
    let parent = s
        .add_markup_with(0, &square(40.0), &MarkupOptions::default())
        .expect("author");
    let reply = s
        .add_reply(parent, &MarkupNote::new("a reply").by("Ken"))
        .expect("reply")
        .reply_id;
    let all = page_annotations(&s.graph(), s.page_slots().expect("slots")[0].id);
    let idx: Vec<usize> = [parent, reply]
        .iter()
        .map(|id| all.iter().position(|a| a.id == Some(*id)).expect("on page"))
        .collect();
    let clip = s.copy_annotations(0, &idx).expect("copy");
    assert_eq!(
        clip.replies_unthreaded, 1,
        "only the reply had a link to lose"
    );
    assert_eq!(clip.version, CLIP_VERSION_PRE_ANCHORS);

    let back = ObjectClip::from_bytes(&clip.to_bytes()).expect("round trip");
    assert_eq!(back.replies_unthreaded, 1);
    assert_eq!(back.annotations.len(), clip.annotations.len());

    // An older-format write of the same clip drops the count and still parses.
    let mut old = clip.clone();
    old.version = CLIP_VERSION_PRE_REPLY_COUNT;
    let old_back = ObjectClip::from_bytes(&old.to_bytes()).expect("old payload");
    assert_eq!(old_back.replies_unthreaded, 0);
    assert_eq!(old_back.annotations.len(), clip.annotations.len());

    let preview = s
        .paste_preview(0, &back, pdfcer_core::vector::Matrix::IDENTITY)
        .expect("preview");
    let pasted = s
        .paste_objects(0, &back, pdfcer_core::vector::Matrix::IDENTITY)
        .expect("paste");
    for outcome in [&preview, &pasted] {
        assert_eq!(outcome.replies_unthreaded, 1);
        assert!(
            outcome
                .disclosures
                .iter()
                .any(|d| d.contains("1 annotation(s) were replies")),
            "the lost thread must be disclosed: {:?}",
            outcome.disclosures
        );
    }
}
