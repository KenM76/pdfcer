//! # `Pass 258.1` — a `/FreeText`'s note now repaints the box, and the
//! text-bearing subtypes can finally be read back
//!
//! ## The defect, in the requester's own words
//!
//! > *"`set_markup_note` on a `/FreeText` changes the dictionary and leaves
//! > the page painting the old words. The two values start **identical** at
//! > authoring time, so the divergence has no visible first moment."*
//!
//! Three measured facts made it inevitable, and none of them was a mistake
//! on its own: `annot_author::free_text` bakes `/AP` `/N` from the caller's
//! text and then writes the same string into `/Contents`, so the two agree
//! exactly when the box is placed; `write_markup_note`'s command carried the
//! annotation object only, never the appearance stream; and R43 means pdfcer
//! paints from `/AP`. The verb did exactly what it said. The gap was that
//! for this one subtype its documentation — *"a note is content the operator
//! cannot recover from the canvas"* — is **backwards**.
//!
//! ## ★ It is `/FreeText` only, and the family is deliberately not uniform
//!
//! | `/Subtype` | is `/Contents` painted? | `set_markup_note` before | after |
//! |---|---|---|---|
//! | `/FreeText` | **yes** — it is the appearance's own input | stale | re-baked |
//! | `/Text` (sticky) | no — shown by the reader's popup | correct | unchanged |
//! | `/Stamp` | no — a comment *about* the stamp; never written by `stamp()` | correct | unchanged |
//!
//! The stamp row is the opposite of a defect and the requester said so
//! explicitly: *"Please do not 'fix' it by making a stamp's note drive its
//! label — that would break the one of the three that is currently right."*
//! Asserted below rather than merely intended.
//!
//! ## The blocker under three surfaces
//!
//! `annot_author::text_spec_from_dict` did not exist. Without it there was
//! no way to read a placed `/FreeText` back into the model needed to
//! re-author it, which also blocked changing a sticky note's icon and colour
//! and blocked the clipboard's `FreeText | Text | Stamp` arm. One reader,
//! three surfaces.
//!
//! ## ★★ `multiline` is not in the file, so it is MEASURED
//!
//! §12.5.6.6 gives `/FreeText` no multiline flag — `/Ff` is a form-field key
//! and a `/FreeText` is not a field. So the re-bake bakes the ORIGINAL text
//! both ways and compares each against the appearance already on disk. A
//! match names the layout **and** proves the appearance is pdfcer's own; no
//! match means it is foreign and must be left alone. One measurement, both
//! answers — the same trick `set_markup_style` uses, with one more degree of
//! freedom.

use pdfcer_core::annot_author::{
    Color, StampName, StampStyle, StickyIcon, TextAnnotSpec, text_spec_from_dict,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, MarkupNote, MarkupOptions};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::vartext::{Quadding, TextColor};
use pdfcer_core::writer::SaveOptions;
use std::path::{Path, PathBuf};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(name)
}

fn session() -> EditSession {
    EditSession::new(Document::load(&fixture("annot/no-ap-circle.pdf")).expect("load fixture"))
}

fn free_text(text: &str) -> TextAnnotSpec {
    TextAnnotSpec::FreeText {
        rect: Rect {
            llx: 20.0,
            lly: 20.0,
            urx: 220.0,
            ury: 70.0,
        },
        text: text.to_owned(),
        font: pdfcer_core::fontdata::Std14::Helvetica,
        font_size: 12.0,
        color: TextColor::Gray(0.0),
        quadding: Quadding::Left,
        multiline: false,
        border: Some(Color::Gray(0.0)),
        border_width: 1.0,
    }
}

/// The words the SAVED appearance stream actually paints.
///
/// Read off the saved bytes, not the session overlay: the whole defect was a
/// file whose dictionary and appearance disagreed, so the assertion has to
/// be about what another program would draw (R159).
fn painted_text(s: &EditSession, id: ObjId) -> String {
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("incremental save");
    let doc = Document::from_bytes(bytes).expect("re-parse");
    let Object::Dict(annot) = &doc.get(id).expect("annotation present").value else {
        panic!("not a dictionary");
    };
    let Some(Object::Dict(ap)) = annot.get(b"AP").map(|o| doc.resolve(o)) else {
        return String::new();
    };
    let Object::Stream(stream) = doc.resolve(ap.get(b"N").expect("/AP /N")) else {
        return String::new();
    };
    let raw = stream
        .data_span
        .slice(doc.bytes())
        .expect("appearance bytes")
        .to_vec();
    String::from_utf8_lossy(&raw).into_owned()
}

/// The annotation's `/Contents`, as the dictionary holds it.
fn contents(s: &EditSession, id: ObjId) -> String {
    let graph = s.graph();
    let Some(Object::Dict(annot)) = graph.value(id) else {
        return String::new();
    };
    match annot.get(b"Contents").map(|o| graph.resolve(o)) {
        Some(Object::String(bytes)) => pdfcer_core::textstring::decode_text_string(bytes).text,
        _ => String::new(),
    }
}

// ---------------------------------------------------------------------------
// The headline
// ---------------------------------------------------------------------------

/// ★ **THE DEFECT.** Edit a placed text box's note; the page must read the
/// new words.
#[test]
fn editing_a_free_texts_note_repaints_the_box() {
    let mut s = session();
    let id = s
        .add_text_annotation(0, &free_text("original words"))
        .expect("place a text box");

    assert!(painted_text(&s, id).contains("original words"));

    let change = s
        .set_markup_note(id, &MarkupNote::new("corrected text"))
        .expect("edit the note");

    assert_eq!(contents(&s, id), "corrected text");
    assert!(
        painted_text(&s, id).contains("corrected text"),
        "★ the PAGE must read the new words. This is the defect: the \
         dictionary changed and the appearance kept painting the old text, \
         and under R43 the appearance is what pdfcer draws"
    );
    assert!(
        !painted_text(&s, id).contains("original words"),
        "the old words must be gone, not merely overdrawn"
    );
    assert!(
        change.appearance_rebaked,
        "the report must SAY the appearance moved — rule 4, and the shell \
         is listening for exactly this"
    );
}

/// Both halves land in **one** undo entry. A shell that could undo the
/// repaint without undoing the words would have re-created the divergence
/// this Pass closed, one keystroke later.
#[test]
fn the_note_and_the_repaint_are_one_undo_entry() {
    let mut s = session();
    let id = s
        .add_text_annotation(0, &free_text("first"))
        .expect("place");
    let before = s.undo_depth();

    s.set_markup_note(id, &MarkupNote::new("second"))
        .expect("edit");
    assert_eq!(s.undo_depth() - before, 1, "one command, not two");

    s.undo().expect("undo");
    assert_eq!(contents(&s, id), "first");
    assert!(
        painted_text(&s, id).contains("first"),
        "undo must restore BOTH halves — the dictionary and the picture"
    );
}

/// A multi-line box keeps its wrapping. `multiline` is not in the file, so
/// this is the assertion that the measurement works rather than the
/// `false` default silently winning.
#[test]
fn a_multiline_box_stays_multiline_through_an_edit() {
    let mut s = session();
    let spec = match free_text("one two three four five six seven eight nine ten") {
        TextAnnotSpec::FreeText {
            rect,
            text,
            font,
            font_size,
            color,
            quadding,
            border,
            border_width,
            ..
        } => TextAnnotSpec::FreeText {
            rect,
            text,
            font,
            font_size,
            color,
            quadding,
            multiline: true,
            border,
            border_width,
        },
        other => other,
    };
    let id = s
        .add_text_annotation(0, &spec)
        .expect("place a wrapped box");
    // One `Tj` per laid-out line -- measured from the generator's actual
    // output rather than assumed: a wrapped box emits `Tm`, `Tj`, then `Td`
    // + `Tj` per further line, while a single-line box emits exactly one
    // `Tj`. So the Tj COUNT is what "did it wrap" means here.
    let before = painted_text(&s, id);
    let lines_before = before.matches("Tj").count();

    let change = s
        .set_markup_note(
            id,
            &MarkupNote::new("alpha beta gamma delta epsilon zeta eta theta iota kappa"),
        )
        .expect("edit");
    assert!(change.appearance_rebaked);

    let after = painted_text(&s, id);
    let lines_after = after.matches("Tj").count();
    assert!(
        lines_before >= 2 && lines_after >= 2,
        "a wrapped box must still wrap after the edit — multiline is not \
         recoverable from the dictionary and is MEASURED from the existing \
         appearance; a silent fall back to single-line would show up here \
         (before={lines_before}, after={lines_after})"
    );
}

// ---------------------------------------------------------------------------
// The two subtypes that were already right
// ---------------------------------------------------------------------------

/// A sticky note's `/Contents` is not painted, so nothing is re-baked.
#[test]
fn a_sticky_notes_appearance_is_not_rebaked() {
    let mut s = session();
    let id = s
        .add_text_annotation(
            0,
            &TextAnnotSpec::Sticky {
                rect: Rect {
                    llx: 10.0,
                    lly: 10.0,
                    urx: 30.0,
                    ury: 30.0,
                },
                icon: StickyIcon::Note,
                contents: "a remark".to_owned(),
                color: Color::Rgb(1.0, 1.0, 0.0),
                open: false,
            },
        )
        .expect("place a sticky");
    let before = painted_text(&s, id);

    let change = s
        .set_markup_note(id, &MarkupNote::new("a different remark"))
        .expect("edit");

    assert_eq!(contents(&s, id), "a different remark");
    assert!(
        !change.appearance_rebaked,
        "a sticky note's words are shown by the reader's popup, never \
         painted — re-baking its icon would be damage, not a fix"
    );
    assert_eq!(
        painted_text(&s, id),
        before,
        "the icon must be byte-identical"
    );
}

/// ★ A stamp's `/Contents` is a comment ABOUT the stamp. The requester asked
/// by name for this not to become a "fix".
#[test]
fn a_stamps_note_does_not_become_its_label() {
    let mut s = session();
    let id = s
        .add_text_annotation(
            0,
            &TextAnnotSpec::Stamp {
                rect: Rect {
                    llx: 10.0,
                    lly: 10.0,
                    urx: 120.0,
                    ury: 50.0,
                },
                name: StampName::Approved,
                label: None,
                color: Color::Gray(0.0),
                style: StampStyle::default(),
            },
        )
        .expect("place a stamp");
    let before = painted_text(&s, id);

    let change = s
        .set_markup_note(id, &MarkupNote::new("looks fine to me"))
        .expect("comment on the stamp");

    assert!(!change.appearance_rebaked);
    assert_eq!(
        painted_text(&s, id),
        before,
        "★ a stamp's face must NOT be driven by a comment about it — this \
         is the one of the three subtypes that was already correct"
    );
}

// ---------------------------------------------------------------------------
// The gate: a foreign appearance is reported, not overwritten
// ---------------------------------------------------------------------------

/// An appearance pdfcer would not have drawn is **left alone**, and the
/// report says so — the narrow honest version the requester preferred.
#[test]
fn a_foreign_appearance_is_left_alone_and_disclosed() {
    let mut first = session();
    let id = first
        .add_text_annotation(0, &free_text("placed by pdfcer"))
        .expect("place");

    // Make the appearance foreign the way a foreign appearance actually
    // arrives: in the FILE. Save, then overwrite bytes INSIDE the
    // appearance stream's own span with a same-length edit — same length so
    // every offset, `/Length` and xref entry stays valid, and the reloaded
    // document is a legitimate PDF that simply carries a stream pdfcer's
    // generator would not have produced for these properties.
    let (mut bytes, _) = first
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save");
    let span = {
        let doc = Document::from_bytes(bytes.clone()).expect("re-parse");
        let Object::Dict(annot) = &doc.get(id).expect("annotation").value else {
            panic!("dict");
        };
        let Some(Object::Dict(ap)) = annot.get(b"AP").map(|o| doc.resolve(o)) else {
            panic!("/AP");
        };
        let Object::Stream(stream) = doc.resolve(ap.get(b"N").expect("/AP /N")) else {
            panic!("/AP /N is a stream");
        };
        stream.data_span
    };
    let (start, end) = (span.start as usize, span.end() as usize);
    let needle = b" w\n";
    let at = bytes[start..end]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| start + p)
        .expect("the border's line-width operator is in the appearance");
    // `1 w` -> `3 w`: a stroke three points wide, which pdfcer would never
    // emit for an annotation declaring `/BS /W 1`.
    bytes[at - 1] = b'3';

    let mut s = EditSession::new(Document::from_bytes(bytes).expect("reload the patched file"));
    let change = s
        .set_markup_note(id, &MarkupNote::new("new words"))
        .expect("edit the note");

    assert_eq!(
        contents(&s, id),
        "new words",
        "the note itself is still committed — the comment body is real \
         content a reviewer needs, and refusing would put the Comments \
         panel back to read-only for one subtype"
    );
    assert!(
        !change.appearance_rebaked,
        "a hand-authored appearance — a shadow, a gradient, an image — must \
         be reported rather than replaced by pdfcer's plainer rendering"
    );
    assert!(
        painted_text(&s, id).contains("3 w"),
        "the foreign stream must survive INTACT, not merely go unreported"
    );
    assert!(
        painted_text(&s, id).contains("placed by pdfcer"),
        "and it must still paint the words it painted before, since pdfcer \
         declined to redraw it"
    );

    // ★ THE CONTROL, and the test is worth little without it. "Not
    // re-baked" is equally true of a re-bake that is DISABLED, so this
    // assertion measured nothing until the same edit was shown to re-bake
    // when the appearance is left alone. Verified by sabotage: with the
    // re-bake switched off, the block above still passed and this block
    // fails.
    let mut control = session();
    let control_id = control
        .add_text_annotation(0, &free_text("placed by pdfcer"))
        .expect("place");
    let control_change = control
        .set_markup_note(control_id, &MarkupNote::new("new words"))
        .expect("edit");
    assert!(
        control_change.appearance_rebaked,
        "control: an unmodified pdfcer appearance MUST re-bake, or the \
         refusal above is an absence rather than a decision"
    );
    assert!(painted_text(&control, control_id).contains("new words"));
}

// ---------------------------------------------------------------------------
// The authoring-time trap the request flagged before it cost anyone anything
// ---------------------------------------------------------------------------

/// Passing both `spec.text` and `options.note` with DIFFERENT words is
/// refused: for a `/FreeText` they are the same key, and there is no
/// defensible way to pick a winner.
#[test]
fn authoring_a_free_text_with_a_conflicting_note_is_refused() {
    let mut s = session();
    let err = s
        .add_text_annotation_with(
            0,
            &free_text("painted words"),
            &MarkupOptions {
                note: Some(MarkupNote::new("different words")),
                ..Default::default()
            },
        )
        .expect_err("the two arguments look independent and are not");

    match err {
        EditError::FreeTextNoteConflictsWithText {
            spec_text,
            note_text,
        } => {
            assert_eq!(spec_text, "painted words");
            assert_eq!(note_text, "different words");
        }
        other => panic!("expected FreeTextNoteConflictsWithText, got {other:?}"),
    }
}

/// Identical strings are harmless — that is what the authoring path writes
/// anyway — so they are allowed rather than refused on principle.
#[test]
fn authoring_a_free_text_with_a_matching_note_is_allowed() {
    let mut s = session();
    let id = s
        .add_text_annotation_with(
            0,
            &free_text("same words"),
            &MarkupOptions {
                note: Some(MarkupNote::new("same words").by("Ken")),
                ..Default::default()
            },
        )
        .expect("identical strings are not a conflict");
    assert_eq!(contents(&s, id), "same words");
    assert!(painted_text(&s, id).contains("same words"));
}

// ---------------------------------------------------------------------------
// The reader itself — the blocker under all three surfaces
// ---------------------------------------------------------------------------

/// A placed `/FreeText` reads back into the model it was authored from.
#[test]
fn a_free_text_reads_back() {
    let mut s = session();
    let id = s
        .add_text_annotation(0, &free_text("round trip"))
        .expect("place");
    let graph = s.graph();
    let Some(Object::Dict(annot)) = graph.value(id) else {
        panic!("dict");
    };
    let spec = text_spec_from_dict(&graph, annot).expect("reads back");

    match spec {
        TextAnnotSpec::FreeText {
            text,
            font,
            font_size,
            quadding,
            border_width,
            multiline,
            ..
        } => {
            assert_eq!(text, "round trip");
            assert_eq!(font, pdfcer_core::fontdata::Std14::Helvetica);
            assert!((font_size - 12.0).abs() < 1e-9);
            assert_eq!(quadding, Quadding::Left);
            assert!((border_width - 1.0).abs() < 1e-9);
            assert!(
                !multiline,
                "multiline is NOT in the file (§12.5.6.6 has no such key) \
                 and the reader says so by always returning false — a \
                 caller that needs it must measure it"
            );
        }
        other => panic!("expected a FreeText, got {other:?}"),
    }
}

/// A sticky note reads back — the second surface the missing reader blocked.
#[test]
fn a_sticky_note_reads_back_including_its_icon_and_colour() {
    let mut s = session();
    let id = s
        .add_text_annotation(
            0,
            &TextAnnotSpec::Sticky {
                rect: Rect {
                    llx: 10.0,
                    lly: 10.0,
                    urx: 30.0,
                    ury: 30.0,
                },
                icon: StickyIcon::Help,
                contents: "why is this here".to_owned(),
                color: Color::Rgb(0.0, 1.0, 0.0),
                open: true,
            },
        )
        .expect("place");
    let graph = s.graph();
    let Some(Object::Dict(annot)) = graph.value(id) else {
        panic!("dict");
    };
    match text_spec_from_dict(&graph, annot).expect("reads back") {
        TextAnnotSpec::Sticky {
            icon,
            contents,
            color,
            open,
            ..
        } => {
            assert_eq!(icon, StickyIcon::Help, "the icon is the point");
            assert_eq!(contents, "why is this here");
            assert_eq!(color, Color::Rgb(0.0, 1.0, 0.0), "and so is the colour");
            assert!(open);
        }
        other => panic!("expected a Sticky, got {other:?}"),
    }
}

/// A subtype outside the three is refused by name rather than guessed at.
#[test]
fn a_non_text_subtype_is_refused_by_name() {
    use pdfcer_core::annot_author::SpecReadError;
    let mut s = session();
    let id = s
        .add_markup(
            0,
            &pdfcer_core::annot_author::MarkupSpec::Square {
                rect: Rect {
                    llx: 1.0,
                    lly: 1.0,
                    urx: 9.0,
                    ury: 9.0,
                },
                border: Some(Color::Gray(0.0)),
                interior: None,
                border_width: 1.0,
                border_effect: None,
            },
        )
        .expect("place a square");
    let graph = s.graph();
    let Some(Object::Dict(annot)) = graph.value(id) else {
        panic!("dict");
    };
    match text_spec_from_dict(&graph, annot) {
        Err(SpecReadError::UnsupportedSubtype { subtype }) => assert_eq!(subtype, "Square"),
        other => panic!("expected UnsupportedSubtype, got {other:?}"),
    }
}
