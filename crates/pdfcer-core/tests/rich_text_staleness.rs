//! # Editing a comment must not leave the OTHER copy saying the old words
//!
//! ## PDF stores a comment twice
//!
//! `/Contents` is the plain string. **`/RC` is a rich-text version of the same
//! comment** (§12.7.3.4, the same grammar as a form field's `/RV`), and
//! §12.5.6.2 pairs them explicitly in its group-attribute list —
//! *"`Contents` **or** (`RC` and `DS`)"*.
//!
//! pdfcer writes `/Contents` and cannot author rich text. So before `Pass
//! 273.0`, editing a note left the two **disagreeing**: `/Contents` held the
//! new words and `/RC` still held the old ones.
//!
//! ★★ **That is worse than the losses fixed alongside it.** Those lost
//! content; this produces **wrong** content, stated confidently, and *which*
//! an operator sees depends on their reader:
//!
//! | subtype | what `/RC` is | what a stale one does |
//! |---|---|---|
//! | any markup (Table 170) | the text *"displayed in the pop-up window"* | the pop-up shows the **old** comment |
//! | `/FreeText` (Table 174) | *"used to generate the appearance"* | **the page itself** can show the old words |
//!
//! Measured on both subtypes through the release binary before the fix:
//! `/Contents` updated, `/RC` stale, report silent.
//!
//! ## Removed, not regenerated — and `/DS` only on `/FreeText`
//!
//! Synthesising `/RC` from plain text would invent formatting nobody chose,
//! and §12.7.3.4 gives no meaning to an empty rich value. An absent key is the
//! unambiguous way to say *"this annotation has no rich version"*.
//!
//! `/DS` goes with it **only on `/FreeText`**: §12.7.3.4 NOTE 1 is explicit
//! that other markup subtypes *"do not use a default style string"*, and
//! Table 170 has no `/DS` row at all. Touching one elsewhere would assert a
//! key the standard does not define for that subtype.
//!
//! ★ The forms side already did exactly this — replacing a rich `/RV` with a
//! plain `/V` removes `/RV` **and** `/DS`, with the reasoning written at that
//! site. The guard existed for fields and not for annotations, which is the
//! shape `R245` names.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::annot::page_annotations;
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, MarkupNote};
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::{ObjId, Object};

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

fn session(rel: &str) -> (EditSession, ObjId) {
    let s = EditSession::new(Document::load(&fixture(rel)).expect("fixture parses"));
    let id = page_annotations(&s.graph(), s.page_slots().expect("slots")[0].id)
        .first()
        .and_then(|a| a.id)
        .expect("the fixture's one annotation");
    (s, id)
}

/// Is `key` present on the annotation's dictionary?
fn has_key(s: &EditSession, id: ObjId, key: &[u8]) -> bool {
    let g = s.graph();
    let Some(Object::Dict(d)) = g.resolved(id).as_dict().map(|d| Object::Dict(d.clone())) else {
        return false;
    };
    d.get(key).is_some()
}

/// ★ The fixtures say what these tests think they say.
///
/// Without this, every assertion below is unfalsifiable: "the stale key is
/// gone" passes trivially against a fixture that never had one.
#[test]
fn the_fixtures_start_with_rich_text() {
    let (s, id) = session("annot/rich-text-square.pdf");
    assert!(has_key(&s, id, b"RC"), "the square must start with /RC");
    assert!(
        !has_key(&s, id, b"DS"),
        "and must NOT have /DS -- Table 170 has no such row, so a fixture \
         carrying one would be testing a shape the standard does not define"
    );

    let (s, id) = session("annot/rich-text-freetext.pdf");
    assert!(has_key(&s, id, b"RC"), "the free text must start with /RC");
    assert!(
        has_key(&s, id, b"DS"),
        "and with /DS -- Table 174 defines it"
    );
}

/// ★★★ Editing the note drops the stale rich text, and says so.
#[test]
fn editing_a_note_drops_the_stale_rich_text() {
    let (mut s, id) = session("annot/rich-text-square.pdf");
    let change = s
        .set_markup_note(id, &MarkupNote::new("THE NEW WORDS"))
        .expect("the note is editable");

    assert!(
        !has_key(&s, id, b"RC"),
        "a stale /RC would leave the pop-up showing the OLD comment while \
         /Contents holds the new one"
    );
    assert_eq!(
        change.rich_text_dropped,
        vec!["RC".to_owned()],
        "and it is DISCLOSED -- rule 4: pdfcer decided to drop a key the \
         operator did not ask about, so it says which"
    );
}

/// ★★★ On a `/FreeText`, `/DS` goes with it — and that subtype is the one
/// where a stale `/RC` can reach the printed page.
#[test]
fn a_free_text_drops_the_default_style_string_too() {
    let (mut s, id) = session("annot/rich-text-freetext.pdf");
    let change = s
        .set_markup_note(id, &MarkupNote::new("THE NEW WORDS"))
        .expect("the note is editable");

    assert!(!has_key(&s, id, b"RC"));
    assert!(
        !has_key(&s, id, b"DS"),
        "/DS styles the rich value and means nothing without one"
    );
    assert_eq!(
        change.rich_text_dropped,
        vec!["RC".to_owned(), "DS".to_owned()],
        "both disclosed, in the order removed"
    );
}

/// ★★ A `/Square` must NOT gain a `/DS` removal it never had.
///
/// The mirror of the test above, and the half that a one-directional test
/// would miss: an implementation that removed `/DS` unconditionally would
/// satisfy both tests above and start asserting a key on subtypes where
/// §12.7.3.4 NOTE 1 says it does not exist. It would also over-report.
#[test]
fn a_square_never_reports_dropping_a_key_it_had_no_business_having() {
    let (mut s, id) = session("annot/rich-text-square.pdf");
    let change = s
        .set_markup_note(id, &MarkupNote::new("THE NEW WORDS"))
        .expect("editable");
    assert_eq!(
        change.rich_text_dropped,
        vec!["RC".to_owned()],
        "exactly one key -- /DS is not defined for this subtype"
    );
}

/// ★★★ A stray `/DS` on a `/Square` is LEFT ALONE — it is not pdfcer's key.
///
/// Real producers emit keys the standard does not define for a subtype.
/// Table 170 has no `/DS` row and §12.7.3.4 NOTE 1 says this subtype does not
/// use one, so pdfcer neither writes it nor removes it: the round-trip
/// invariant says a key pdfcer does not own is re-emitted untouched.
///
/// This test exists because a sabotage survived without it. Dropping `/DS`
/// unconditionally stayed green against every other test in this file — the
/// ordinary `/Square` fixture has no `/DS` to lose, so an over-eager removal
/// reported nothing and looked identical to correct behaviour.
#[test]
fn a_stray_default_style_string_on_a_square_survives() {
    let (mut s, id) = session("annot/rich-text-square-stray-ds.pdf");
    assert!(
        has_key(&s, id, b"DS"),
        "the fixture must start with the stray key, or this proves nothing"
    );

    let change = s
        .set_markup_note(id, &MarkupNote::new("THE NEW WORDS"))
        .expect("editable");

    assert!(
        !has_key(&s, id, b"RC"),
        "the stale rich text still goes -- that key IS pdfcer's business here"
    );
    assert!(
        has_key(&s, id, b"DS"),
        "but the stray /DS is not: removing a key the standard does not define \
         for this subtype is a change nobody asked for, and the round-trip \
         invariant forbids it"
    );
    assert_eq!(
        change.rich_text_dropped,
        vec!["RC".to_owned()],
        "and the disclosure names only what actually went"
    );
}

/// Clearing the note drops it too, which is the more destructive direction.
///
/// `clear_markup_note` is a separate verb with its own entry point. Leaving
/// `/RC` behind there would be the worst outcome of all: `/Contents` gone and
/// a rich-text copy of the deleted comment still sitting in the file, which
/// reads as *"the comment is still there"* to any reader that prefers `/RC`.
#[test]
fn clearing_the_note_drops_the_rich_text_as_well() {
    let (mut s, id) = session("annot/rich-text-freetext.pdf");
    let change = s.clear_markup_note(id).expect("clearable");

    assert!(!has_key(&s, id, b"RC"));
    assert!(!has_key(&s, id, b"DS"));
    assert_eq!(
        change.rich_text_dropped,
        vec!["RC".to_owned(), "DS".to_owned()]
    );
}

/// An annotation with NO rich text reports nothing and is otherwise unchanged.
///
/// The ordinary case, and the guard against a phantom disclosure: a report
/// that named a dropped key on every edit would train an operator to ignore
/// it, which is the same failure `dropped_properties` avoids elsewhere.
#[test]
fn an_annotation_without_rich_text_reports_nothing() {
    let (mut s, id) = session("annot/demo-annotated.pdf");
    let change = s
        .set_markup_note(id, &MarkupNote::new("a plain note"))
        .expect("editable");
    assert!(
        change.rich_text_dropped.is_empty(),
        "nothing was dropped, so nothing is reported: {:?}",
        change.rich_text_dropped
    );
}
