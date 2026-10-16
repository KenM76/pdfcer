//! `resize_annotations` / `rotate_annotations` and their previews: several
//! annotations scaled or turned as ONE undo entry, keeping the members that
//! applied when others refuse.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::annot_author::{Color, MarkupSpec, StickyIcon, TextAnnotSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, MAX_UNDO_DEPTH, ResizeOptions};
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::page_tree::Rect;
use std::path::Path;

fn session() -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/annot/demo-annotated.pdf");
    EditSession::new(Document::load(&path).expect("load fixture"))
}

fn rect(at: f64) -> Rect {
    Rect {
        llx: at,
        lly: at,
        urx: at + 40.0,
        ury: at + 40.0,
    }
}

fn square(s: &mut EditSession, at: f64) -> ObjId {
    s.add_markup(
        0,
        &MarkupSpec::Square {
            rect: rect(at),
            border: Some(Color::Gray(0.0)),
            interior: None,
            border_width: 1.0,
            border_effect: None,
        },
    )
    .expect("author a square")
}

fn sticky(s: &mut EditSession, at: f64) -> ObjId {
    s.add_text_annotation(
        0,
        &TextAnnotSpec::Sticky {
            rect: rect(at),
            icon: StickyIcon::Note,
            contents: "N".to_owned(),
            color: Color::Rgb(1.0, 0.9, 0.2),
            open: false,
        },
    )
    .expect("author a sticky")
}

fn rect_of(s: &EditSession, id: ObjId) -> Vec<f64> {
    let Some(Object::Dict(d)) = s.value(id) else {
        panic!("{id} is not a dictionary");
    };
    let Some(Object::Array(r)) = d.get(b"Rect") else {
        panic!("{id} has no /Rect");
    };
    r.iter()
        .map(|o| match o {
            Object::Integer(i) => *i as f64,
            Object::Real(f) => *f,
            other => panic!("non-number in /Rect: {other:?}"),
        })
        .collect()
}

#[test]
fn a_resize_keeps_the_applied_members_and_reports_the_refused_one() {
    let mut s = session();
    let a = square(&mut s, 100.0);
    let note = sticky(&mut s, 200.0);
    let b = square(&mut s, 300.0);
    let (ra, rn, rb) = (rect_of(&s, a), rect_of(&s, note), rect_of(&s, b));
    let depth = s.undo_depth();

    let opts = ResizeOptions::new();
    let out = s
        .resize_annotations(&[(a, opts), (note, opts), (b, opts)], (0.0, 0.0), 2.0, 2.0)
        .unwrap();

    assert_eq!(out.applied(), 2);
    let refused: Vec<_> = out.refused().collect();
    assert_eq!(refused.len(), 1);
    assert_eq!(refused[0].0, note);
    assert!(
        matches!(refused[0].1, EditError::ResizeFixedSizeMarker { .. }),
        "{}",
        refused[0].1
    );
    assert_eq!(
        out.results.iter().map(|r| r.0).collect::<Vec<_>>(),
        [a, note, b]
    );
    assert_ne!(rect_of(&s, a), ra);
    assert_ne!(rect_of(&s, b), rb);
    assert_eq!(rect_of(&s, note), rn, "the refused member is untouched");
    assert_eq!(s.undo_depth(), depth + 1, "one entry for the gesture");

    s.undo().unwrap();
    assert_eq!(rect_of(&s, a), ra);
    assert_eq!(rect_of(&s, b), rb);
    assert_eq!(s.undo_depth(), depth);
}

#[test]
fn a_rotate_is_one_undo_entry_and_names_each_member_once() {
    let mut s = session();
    let a = square(&mut s, 100.0);
    let b = square(&mut s, 300.0);
    let (ra, rb) = (rect_of(&s, a), rect_of(&s, b));
    let depth = s.undo_depth();

    let out = s
        .rotate_annotations(&[a, b, a], (200.0, 200.0), 30.0)
        .unwrap();
    assert_eq!(out.results.len(), 2, "a repeated id is turned once");
    assert_eq!(out.applied(), 2, "{:?}", out.results);
    assert_eq!(s.undo_depth(), depth + 1);

    s.undo().unwrap();
    assert_eq!(rect_of(&s, a), ra);
    assert_eq!(rect_of(&s, b), rb);
}

#[test]
fn when_nothing_applies_nothing_is_pushed() {
    let mut s = session();
    let note = sticky(&mut s, 200.0);
    let depth = s.undo_depth();
    let out = s
        .resize_annotations(&[(note, ResizeOptions::new())], (0.0, 0.0), 2.0, 2.0)
        .unwrap();
    assert_eq!(out.applied(), 0);
    assert_eq!(s.undo_depth(), depth);
}

#[test]
fn a_preview_reports_the_outcome_and_changes_nothing() {
    let mut s = session();
    let a = square(&mut s, 100.0);
    square(&mut s, 300.0);
    s.undo().unwrap(); // leave a redo entry the preview must keep
    assert!(s.can_redo());
    let ra = rect_of(&s, a);
    let depth = s.undo_depth();
    let redo_before = s.can_redo();

    let opts = ResizeOptions::new();
    let peek = s
        .resize_annotations_preview(&[(a, opts)], (0.0, 0.0), 2.0, 2.0)
        .unwrap();
    assert_eq!(peek.applied(), 1);
    assert_eq!(rect_of(&s, a), ra, "the preview left the document alone");
    assert_eq!(s.undo_depth(), depth);
    assert_eq!(s.can_redo(), redo_before, "the redo stack survives");

    let peek = s
        .rotate_annotations_preview(&[a], (0.0, 0.0), 45.0)
        .unwrap();
    assert_eq!(peek.applied(), 1);
    assert_eq!(rect_of(&s, a), ra);
    assert_eq!(s.undo_depth(), depth);
    assert_eq!(s.can_redo(), redo_before);
}

#[test]
fn too_large_a_selection_refuses_before_anything_runs() {
    let mut s = session();
    let a = square(&mut s, 100.0);
    let depth = s.undo_depth();
    // Distinct ids past the bound; none needs to exist, the count refuses first.
    let ids: Vec<ObjId> = (0..=MAX_UNDO_DEPTH as u32)
        .map(|n| ObjId::new(90_000 + n, 0))
        .chain([a])
        .collect();
    let err = s.rotate_annotations(&ids, (0.0, 0.0), 10.0).unwrap_err();
    assert!(
        matches!(err, EditError::SelectionTooLargeForOneUndo { .. }),
        "{err}"
    );
    assert_eq!(s.undo_depth(), depth);
}
