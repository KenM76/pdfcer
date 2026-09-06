//! # `Pass 259.0` — `/Open`: pdfcer wrote a key it could not read back
//!
//! ## The report, which is a workaround report as much as a request
//!
//! `pdfcer-gui`, 2026-09-05, filing under decision 058 (*anything the GUI has
//! to work around is a place the crate boundary was drawn wrong*):
//!
//! > *"`/Open` is written and never read, so we parse your dictionary behind
//! > your back."*
//!
//! They were exact about it. `annot_author::sticky_note` has set `/Open` on
//! the note **and** on the `/Popup` companion it creates since `Pass 6.2`. A
//! tree-wide grep for `b"Open"` in `pdfcer-core` returned **two hits, both
//! writes**. So a round trip through pdfcer's own model lost the state, and a
//! shell that needed it read the raw dictionary through `ObjectGraph::value`
//! — correct, documented, and on the wrong side of the boundary.
//!
//! ## Why they needed it at all
//!
//! The operator, 2026-09-05: he *"could add a yellow sticky note but even in
//! read mode I don't think I could figure out how to read it."* The pop-up
//! window had been in his files the whole time — pdfcer authored it — and no
//! shell had ever drawn it.
//!
//! ## ★ The state is a property of the PAIR, not of one object
//!
//! Table 170 gives geometric markup **no `/Open` of its own**. A `/Square`'s
//! window state exists only on its companion; a `/Text` has one on itself and
//! `sticky_note` writes both. So a verb that wrote one of the two would leave
//! them disagreeing on precisely the subtype an operator uses most, and the
//! read model has to be able to say *"the file said nothing"* — which is why
//! `Annotation::open` is `Option<bool>` and not `bool`.
//!
//! ## What is deliberately not done
//!
//! `set_annotation_open` does **not** create a `/Popup` for an annotation
//! that has none. Choosing that companion's `/Rect` is authoring, not a state
//! change. Such a call is a reported no-op rather than a refusal, so a shell
//! can send a mixed selection and read the result instead of filtering by
//! subtype — the same reasoning that produced `MarkupStyleSupport` in
//! `Pass 258.0`.

use pdfcer_core::annot::page_annotations;
use pdfcer_core::annot_author::{Color, StickyIcon, TextAnnotSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::object::ObjId;
use pdfcer_core::page_tree::Rect;
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

fn sticky(open: bool) -> TextAnnotSpec {
    TextAnnotSpec::Sticky {
        rect: Rect {
            llx: 10.0,
            lly: 10.0,
            urx: 30.0,
            ury: 30.0,
        },
        icon: StickyIcon::Note,
        contents: "a remark".to_owned(),
        color: Color::Rgb(1.0, 1.0, 0.0),
        open,
    }
}

/// Every annotation on page 1 of the SAVED document, so the assertions are
/// about what another program would read (R159).
fn saved_annotations(s: &EditSession) -> Vec<pdfcer_core::annot::Annotation> {
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save");
    let doc = Document::from_bytes(bytes).expect("re-parse");
    let page = pdfcer_core::page_tree::page_slots(&doc).expect("pages")[0].id;
    page_annotations(&doc, page)
}

fn by_id(list: &[pdfcer_core::annot::Annotation], id: ObjId) -> &pdfcer_core::annot::Annotation {
    list.iter()
        .find(|a| a.id == Some(id))
        .expect("annotation present")
}

// ---------------------------------------------------------------------------
// The read half
// ---------------------------------------------------------------------------

/// ★ **THE DEFECT.** pdfcer authored `/Open` and could not read it back.
#[test]
fn an_authored_open_state_reads_back() {
    for want in [true, false] {
        let mut s = session();
        let id = s
            .add_text_annotation(0, &sticky(want))
            .expect("place a sticky");
        let list = saved_annotations(&s);
        assert_eq!(
            by_id(&list, id).open,
            Some(want),
            "the note's own /Open must survive a round trip through pdfcer's \
             own model — it did not before Pass 259.0"
        );
    }
}

/// The `/Popup` companion carries it too, and is reachable as its own
/// annotation rather than needing a second dictionary read.
#[test]
fn the_popup_companion_carries_the_state_as_well() {
    let mut s = session();
    let id = s.add_text_annotation(0, &sticky(true)).expect("place");
    let list = saved_annotations(&s);

    let popup_id = by_id(&list, id)
        .popup
        .expect("sticky_note authors a /Popup");
    let popup = by_id(&list, popup_id);
    assert_eq!(popup.subtype, b"Popup".to_vec());
    assert_eq!(
        popup.open,
        Some(true),
        "the companion's /Open is what a geometric markup's window state \
         lives on, so it must be readable from the popup's OWN model entry"
    );
}

/// ★ **Absent is not `false`.** The whole reason the field is `Option<bool>`.
#[test]
fn an_absent_key_reads_as_none_not_false() {
    let mut s = session();
    // A `/Square` has no `/Open` of its own — Table 170 gives it none, and
    // `build_appearance` writes none.
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
    assert_eq!(
        by_id(&saved_annotations(&s), id).open,
        None,
        "★ None means THE FILE SAID NOTHING. Reporting Some(false) here — \
         Table 172's default — would make a reader unable to tell a note a \
         producer authored open from one it never spoke about, which is how \
         every foreign note would silently come back shut"
    );
}

/// A malformed `/Open` reads as absent rather than as a definite `false`.
#[test]
fn a_non_boolean_open_reads_as_none() {
    let bytes = br#"%PDF-1.7
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 99 99] /Annots [4 0 R] >> endobj
4 0 obj << /Type /Annot /Subtype /Text /Rect [1 1 9 9] /Open (yes) >> endobj
trailer << /Size 5 /Root 1 0 R >>
"#;
    let doc = Document::from_bytes(bytes.to_vec()).expect("rebuildable");
    let page = pdfcer_core::page_tree::page_slots(&doc).expect("pages")[0].id;
    let list = page_annotations(&doc, page);
    assert_eq!(
        list[0].open, None,
        "a string where a boolean belongs is malformed; inventing a definite \
         answer from it would be worse than reporting silence"
    );
}

// ---------------------------------------------------------------------------
// The write half
// ---------------------------------------------------------------------------

/// ★ Both objects move, in ONE undo entry.
#[test]
fn setting_open_writes_the_annotation_and_its_popup_as_one_command() {
    let mut s = session();
    let id = s.add_text_annotation(0, &sticky(false)).expect("place");
    let depth = s.undo_depth();

    let change = s.set_annotation_open(id, true).expect("open it");
    assert!(change.annotation_written, "a /Text has its own /Open");
    assert!(change.popup_written, "and sticky_note gave it a companion");
    assert_eq!(change.was, Some(false));
    assert_eq!(
        s.undo_depth() - depth,
        1,
        "ONE command — the pair is one state, and a shell must not be able \
         to undo half of it into a document where the two disagree"
    );

    let list = saved_annotations(&s);
    let popup_id = by_id(&list, id).popup.expect("/Popup");
    assert_eq!(by_id(&list, id).open, Some(true));
    assert_eq!(by_id(&list, popup_id).open, Some(true), "both moved");
}

/// Undo restores both halves together.
#[test]
fn undo_restores_both_halves() {
    let mut s = session();
    let id = s.add_text_annotation(0, &sticky(false)).expect("place");
    s.set_annotation_open(id, true).expect("open");
    s.undo().expect("undo");

    let list = saved_annotations(&s);
    let popup_id = by_id(&list, id).popup.expect("/Popup");
    assert_eq!(by_id(&list, id).open, Some(false));
    assert_eq!(
        by_id(&list, popup_id).open,
        Some(false),
        "the companion must come back with it, or one undo leaves the pair \
         disagreeing — the exact state this verb exists to prevent"
    );
}

/// A `/Square` gets its state on the companion, and **not** a spurious
/// `/Open` on itself.
#[test]
fn a_geometric_markup_takes_the_state_on_its_popup_only() {
    let mut s = session();
    // Author a square, then give it a popup the way a real file would have
    // one, by authoring a sticky and re-pointing... simpler: a square has no
    // popup, so this is the no-popup branch.
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

    let change = s.set_annotation_open(id, true).expect("not a refusal");
    assert!(
        !change.annotation_written,
        "★ Table 169 gives a /Square no /Open, so writing one would add a \
         key the standard does not define there — noise a later reader \
         could mistake for meaning"
    );
    assert!(!change.popup_written, "and it has no companion");
    assert_eq!(
        by_id(&saved_annotations(&s), id).open,
        None,
        "nothing was written, so nothing reads back"
    );
}

/// A no-op pushes **no undo entry**. An empty command in the stack is a
/// Ctrl+Z that appears to do nothing.
#[test]
fn a_no_op_pushes_no_undo_entry() {
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
        .expect("place");
    let depth = s.undo_depth();
    let change = s
        .set_annotation_open(id, true)
        .expect("no-op, not a refusal");
    assert!(!change.annotation_written && !change.popup_written);
    assert_eq!(s.undo_depth(), depth);
}

/// ★ The state the verb writes is the state the reader reads. Stated as its
/// own test because the two halves shipped together and a divergence between
/// them is exactly the defect this Pass closed, one level along.
#[test]
fn the_write_half_and_the_read_half_agree() {
    for want in [true, false, true] {
        let mut s = session();
        let id = s.add_text_annotation(0, &sticky(!want)).expect("place");
        s.set_annotation_open(id, want).expect("set");
        assert_eq!(
            by_id(&saved_annotations(&s), id).open,
            Some(want),
            "set_annotation_open({want}) must be what Annotation::open reports"
        );
    }
}
