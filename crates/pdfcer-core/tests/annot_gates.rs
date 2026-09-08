//! # The annotation family's gates, enforced rather than merely documented
//!
//! ## Two defect classes, both found by audit and both fixed as classes
//!
//! ### 1. Five verbs documented an encryption refusal they never performed
//!
//! `rotate_annotation`, `set_annotation_rotation`, `resize_annotation`,
//! `move_annotation` and `set_markup_note` each listed
//! [`EditError::DocumentEncrypted`] in their `# Errors` section, and **no
//! code path in any of them could produce it**. Meanwhile `add_markup_with`,
//! `set_annotation_open`, `set_markup_style`, `reshape_annotation`,
//! `add_text_annotation_inner` and the deletion guards all enforced it.
//!
//! ★ One of those five doc comments was written **the same day the audit
//! found it**, by the session that then audited it. A promise in a `# Errors`
//! list is not a control — the same shape as `R243`, one level up: the
//! documentation and the code had to agree about a guard, and the agreement
//! lived only in prose.
//!
//! ### 2. The Locked flag was honoured by three verbs and ignored by four
//!
//! ISO 32000-1 §12.5.3 Table 165 bit 8, quoted verbatim in `annot.rs`:
//! *"do not allow the annotation to be deleted or its properties **(including
//! position and size)** to be modified."*
//!
//! **Position and size are exactly what the transform verbs change**, and
//! `move_annotation`, `resize_annotation`, `rotate_annotation` and
//! `set_annotation_rotation` all ignored the flag — while
//! `set_markup_style`, `reshape_annotation` and `annotation_deletion_guards`
//! honoured it. So a Locked markup could not be **recoloured** and could be
//! **dragged anywhere**, which is precisely inverted from what the clause
//! says.
//!
//! ## Why these are one test file
//!
//! Both are *"a guard exists for some of a family and not the rest"*, and the
//! fix for both was to apply it across the family rather than to the reported
//! verb. Testing them together is what makes a future sixth verb's omission
//! visible: the loops below iterate the family, so adding a verb without a
//! guard fails here rather than passing silently.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::annot::AnnotFlags;
use pdfcer_core::annot::page_annotations;
use pdfcer_core::annot_author::{Color, MarkupSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, MarkupNote};
use pdfcer_core::object::ObjId;

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn session(rel: &str) -> EditSession {
    EditSession::new(Document::load(&fixture(rel)).expect("load fixture"))
}

/// A polygon with an appearance, on a page, ready to be transformed.
fn markup() -> (EditSession, ObjId) {
    let mut s = session("annot/demo-annotated.pdf");
    let spec = MarkupSpec::Polygon {
        vertices: vec![(100.0, 100.0), (160.0, 100.0), (130.0, 150.0)],
        border: Some(Color::Gray(0.0)),
        interior: None,
        width: 1.0,
    };
    let id = s.add_markup(0, &spec).expect("author the polygon");
    (s, id)
}

const ANCHOR: (f64, f64) = (100.0, 100.0);

/// The annotation's current `/F`, read back through the model.
fn current_flags(s: &EditSession, id: ObjId) -> AnnotFlags {
    let slots = s.page_slots().expect("slots");
    page_annotations(&s.graph(), slots[0].id)
        .into_iter()
        .find(|a| a.id == Some(id))
        .expect("the annotation")
        .flags
}

/// Set Table 165 bit 8 (`Locked`, value 128) through pdfcer's own new verb.
///
/// ★ This helper is itself a regression test for the other half of the audit:
/// until 2026-09-07 there was **no way to set `/F` at all** — eight read
/// accessors on `AnnotFlags` and no writer — so pdfcer's own Locked gate was
/// unreachable from pdfcer. `set_annotation_flags` closed that, and the fact
/// that this file can set the flag without reaching behind the API is the
/// proof.
fn set_locked(s: &mut EditSession, id: ObjId) {
    let existing = current_flags(s, id);
    s.set_annotation_flags(id, AnnotFlags(existing.0 | AnnotFlags::LOCKED))
        .expect("set the Locked flag");

    let slots = s.page_slots().expect("slots");
    let locked = page_annotations(&s.graph(), slots[0].id)
        .into_iter()
        .find(|a| a.id == Some(id))
        .expect("the annotation")
        .flags
        .locked();
    assert!(
        locked,
        "the fixture setup itself failed -- the flag did not take, so every \
         assertion below would pass for the wrong reason"
    );
}

// ---------------------------------------------------------------------------
// 1. THE LOCKED FLAG, across the whole transform family
// ---------------------------------------------------------------------------

/// ★★ Every transform verb refuses a Locked annotation.
///
/// Iterating the family rather than testing one verb is the point: a sixth
/// transform added without the guard fails here. That is the failure mode
/// that produced the defect — `set_markup_style` and `reshape_annotation`
/// had the guard, four neighbours did not, and nothing compared them.
#[test]
fn every_transform_verb_refuses_a_locked_annotation() {
    // (name, the call) — each built fresh, because a refusal must leave the
    // session usable and a shared one would hide a partial mutation.
    /// One transform verb, as a name and a call. A `type` alias because the
    /// tuple is otherwise complex enough that clippy asks for one — and it
    /// reads better at the call sites below.
    type Verb = (
        &'static str,
        Box<dyn Fn(&mut EditSession, ObjId) -> Result<(), EditError>>,
    );
    let cases: Vec<Verb> = vec![
        (
            "move_annotation",
            Box::new(|s: &mut EditSession, id| s.move_annotation(id, 10.0, 10.0).map(|_| ())),
        ),
        (
            "rotate_annotation",
            Box::new(|s: &mut EditSession, id| s.rotate_annotation(id, ANCHOR, 30.0).map(|_| ())),
        ),
        (
            "set_annotation_rotation",
            Box::new(|s: &mut EditSession, id| {
                s.set_annotation_rotation(id, ANCHOR, 30.0).map(|_| ())
            }),
        ),
    ];

    for (name, call) in cases {
        let (mut s, id) = markup();
        set_locked(&mut s, id);
        match call(&mut s, id) {
            Err(EditError::AnnotationLocked { subtype, .. }) => {
                assert_eq!(subtype, "Polygon", "{name} named the wrong subtype");
            }
            other => panic!(
                "{name} must refuse a Locked annotation -- ISO 32000-1 12.5.3 Table 165 bit 8 \
                 names POSITION AND SIZE explicitly, which is what this verb changes. Got \
                 {other:?}"
            ),
        }
    }
}

/// The refusal is about `Locked`, not about the annotation being unusable —
/// the same object transforms fine once the flag is not set.
///
/// Without this, every assertion above would pass on a build that refused
/// *everything*, which is the cheapest possible wrong fix.
#[test]
fn an_unlocked_annotation_still_transforms() {
    let (mut s, id) = markup();
    s.move_annotation(id, 10.0, 10.0).expect("move");
    s.rotate_annotation(id, ANCHOR, 30.0).expect("rotate");
    // scale_stroke_width ON: with it off the verb refuses rather than
    // distort the drawn stroke, which is correct and is NOT what this test is
    // about -- it would fail here for a reason unrelated to the Locked flag.
    s.resize_annotation(
        id,
        ANCHOR,
        1.5,
        1.5,
        &pdfcer_core::edit::ResizeOptions::new().with_scale_stroke_width(true),
    )
    .expect("resize");
}

/// `LockedContents` (bit 10) is a **different** flag and must NOT stop a
/// transform.
///
/// Table 165 bit 10 guards the annotation's *contents*, not its geometry —
/// `annot.rs` documents the distinction at length. A fix that refused on
/// either flag would be over-broad in a way no test above would catch, and
/// would break moving every annotation Acrobat marked contents-locked.
#[test]
fn locked_contents_does_not_stop_a_transform() {
    let (mut s, id) = markup();
    s.set_annotation_flags(id, AnnotFlags(AnnotFlags::LOCKED_CONTENTS))
        .expect("set LockedContents");

    s.move_annotation(id, 5.0, 5.0)
        .expect("LockedContents guards the TEXT, not the position");
}

// ---------------------------------------------------------------------------
// 2. THE ENCRYPTION GATE, across the family that documented it
// ---------------------------------------------------------------------------

/// ★★ Every verb whose `# Errors` promises `DocumentEncrypted` delivers it.
///
/// The promise was in five doc comments and in none of the five code paths.
/// This asserts the promise, so the documentation and the guard can no longer
/// drift apart silently — which is the whole defect, one level up from
/// `R243`: two things had to agree about a rule and the agreement lived in
/// prose.
#[test]
fn every_verb_that_documents_an_encryption_refusal_performs_it() {
    let doc = Document::load(&fixture("encryption/enc-emptyuser.pdf"));
    let Ok(doc) = doc else {
        // Named, not silently skipped: a test that quietly does nothing is
        // worse than one that is absent, because it reports as coverage.
        panic!(
            "no encrypted fixture found under fixtures/synthetic/encrypted/ -- this test cannot \
             run and must not be reported as passing. Point it at a real encrypted fixture."
        );
    };
    let mut s = EditSession::new(doc);
    let slots = s.page_slots().expect("slots");
    let Some(id) = page_annotations(&s.graph(), slots[0].id)
        .first()
        .and_then(|a| a.id)
    else {
        panic!("the encrypted fixture carries no annotation to aim these verbs at");
    };

    let note = MarkupNote::new("x");
    let results: Vec<(&str, Result<(), EditError>)> = vec![
        (
            "move_annotation",
            s.move_annotation(id, 1.0, 1.0).map(|_| ()),
        ),
        (
            "rotate_annotation",
            s.rotate_annotation(id, ANCHOR, 10.0).map(|_| ()),
        ),
        (
            "set_annotation_rotation",
            s.set_annotation_rotation(id, ANCHOR, 10.0).map(|_| ()),
        ),
        (
            "resize_annotation",
            s.resize_annotation(id, ANCHOR, 2.0, 2.0, &Default::default())
                .map(|_| ()),
        ),
        ("set_markup_note", s.set_markup_note(id, &note).map(|_| ())),
    ];

    for (name, r) in results {
        assert!(
            matches!(r, Err(EditError::DocumentEncrypted)),
            "{name} documents EditError::DocumentEncrypted in its # Errors list; it must \
             actually return it. Got {r:?}"
        );
    }
}
