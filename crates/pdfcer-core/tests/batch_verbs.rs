//! Batch verbs (`Pass 583.0`-`586.0`): several widgets or annotations moved
//! or deleted as ONE undo entry, all-or-nothing. A refusal part-way leaves the
//! document, the undo stack and the redo stack as they were.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::annot::AnnotFlags;
use pdfcer_core::annot_author::{Color, MarkupSpec};
use pdfcer_core::dimension::{DEFAULT_GROUP_ID, DimensionKind};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{AnnotationBatchMove, EditError, EditSession, NewRadioButton};
use pdfcer_core::forms;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::vector::{AxisConstraint, Point};
use std::path::Path;

fn session(rel: &str) -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel);
    EditSession::new(Document::load(&path).expect("load fixture"))
}

/// A radio group `Colour` with one member per on-state, stacked down page 1.
fn radio_group(states: &[&str]) -> EditSession {
    let mut s = session("dimension/plain-base.pdf");
    for (i, v) in states.iter().enumerate() {
        let top = 720.0 - (i as f64) * 30.0;
        let rect = Rect::from_corners(72.0, top - 20.0, 92.0, top);
        s.add_radio_button(&NewRadioButton::new(0, "Colour", rect, *v).declining_tooltip())
            .unwrap();
    }
    s
}

fn widgets(s: &EditSession, fqn: &str) -> Vec<forms::Widget> {
    let form = forms::parse_acroform(&s.graph()).expect("form parses");
    form.fields
        .into_iter()
        .find(|f| f.fully_qualified_name == fqn)
        .map(|f| f.widgets)
        .unwrap_or_default()
}

fn square(at: f64) -> MarkupSpec {
    MarkupSpec::Square {
        rect: Rect {
            llx: at,
            lly: at,
            urx: at + 40.0,
            ury: at + 40.0,
        },
        border: Some(Color::Rgb(1.0, 0.0, 0.0)),
        interior: None,
        border_width: 1.0,
        border_effect: None,
    }
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

fn lock(s: &mut EditSession, id: ObjId) {
    s.set_annotation_flags(id, AnnotFlags(AnnotFlags::PRINT | AnnotFlags::LOCKED))
        .expect("lock");
}

fn linear() -> DimensionKind {
    DimensionKind::Linear {
        a: Point::new(100.0, 200.0),
        b: Point::new(300.0, 200.0),
        constraint: AxisConstraint::Horizontal,
        offset: 0.0,
        text_along: 0.0,
        extension_gap: [None; 2],
    }
}

// ---------------------------------------------------------------------------
// move_widgets_each (583.0)
// ---------------------------------------------------------------------------

#[test]
fn moving_two_widgets_is_one_undo_entry_and_discloses_no_sibling_left_behind() {
    let mut s = radio_group(&["A", "B", "C"]);
    let before: Vec<_> = widgets(&s, "Colour").iter().map(|w| w.rect).collect();
    let depth = s.undo_depth();

    let moved = s
        .move_widgets_each(&[("Colour", 0, 10.0, 0.0), ("Colour", 2, 0.0, -5.0)])
        .unwrap();
    assert_eq!(moved.len(), 2);
    assert_eq!(s.undo_depth(), depth + 1, "one entry for the batch");
    assert_eq!(
        moved[0].siblings_left_behind, 1,
        "only `B` stays: `C` moves in the same batch"
    );
    assert_eq!(moved[1].siblings_left_behind, 1);
    let after = widgets(&s, "Colour");
    assert_eq!(after[0].rect.unwrap().llx, before[0].unwrap().llx + 10.0);
    assert_eq!(after[1].rect, before[1], "`B` was not named");
    assert_eq!(after[2].rect.unwrap().lly, before[2].unwrap().lly - 5.0);

    s.undo().unwrap();
    let undone: Vec<_> = widgets(&s, "Colour").iter().map(|w| w.rect).collect();
    assert_eq!(undone, before, "one undo puts every box back");
}

#[test]
fn a_refused_widget_move_moves_nothing_and_leaves_no_redo() {
    let mut s = radio_group(&["A", "B"]);
    let before: Vec<_> = widgets(&s, "Colour").iter().map(|w| w.rect).collect();
    let depth = s.undo_depth();

    let err = s
        .move_widgets_each(&[("Colour", 0, 10.0, 0.0), ("Colour", 9, 10.0, 0.0)])
        .unwrap_err();
    assert!(
        matches!(err, EditError::WidgetIndexOutOfRange { .. }),
        "{err}"
    );
    let after: Vec<_> = widgets(&s, "Colour").iter().map(|w| w.rect).collect();
    assert_eq!(after, before, "the first box is not left moved");
    assert_eq!(s.undo_depth(), depth);
    assert!(!s.can_redo(), "Redo cannot re-apply half the batch");
}

// ---------------------------------------------------------------------------
// delete_widgets (586.0)
// ---------------------------------------------------------------------------

#[test]
fn deleting_boxes_by_their_selected_indices_owns_the_renumbering() {
    let mut s = radio_group(&["A", "B", "C"]);
    let depth = s.undo_depth();

    // Ascending, as a selection would list them: deleting 0 first would
    // renumber C to 1 and make index 2 out of range.
    let done = s.delete_widgets(&[("Colour", 0), ("Colour", 2)]).unwrap();
    assert_eq!(done.len(), 2);
    assert_eq!(s.undo_depth(), depth + 1);
    let left = widgets(&s, "Colour");
    assert_eq!(left.len(), 1);
    assert_eq!(
        left[0].on_states,
        vec![b"B".to_vec()],
        "the unselected box survives"
    );

    s.undo().unwrap();
    assert_eq!(widgets(&s, "Colour").len(), 3);
}

#[test]
fn deleting_every_box_of_a_field_removes_the_field_in_one_entry() {
    let mut s = radio_group(&["A", "B"]);
    let depth = s.undo_depth();
    let done = s
        .delete_widgets(&[("Colour", 1), ("Colour", 0), ("Colour", 1)])
        .unwrap();
    assert_eq!(done.len(), 2, "a box named twice is deleted once");
    assert!(done[1].field_removed, "the last box took the field with it");
    assert!(widgets(&s, "Colour").is_empty());
    assert_eq!(s.undo_depth(), depth + 1);
    s.undo().unwrap();
    assert_eq!(widgets(&s, "Colour").len(), 2);
}

#[test]
fn an_out_of_range_box_refuses_before_anything_is_deleted() {
    let mut s = radio_group(&["A", "B"]);
    let depth = s.undo_depth();
    let err = s
        .delete_widgets(&[("Colour", 0), ("Colour", 5)])
        .unwrap_err();
    assert!(
        matches!(err, EditError::WidgetIndexOutOfRange { .. }),
        "{err}"
    );
    assert_eq!(widgets(&s, "Colour").len(), 2);
    assert_eq!(s.undo_depth(), depth);
}

// ---------------------------------------------------------------------------
// delete_annotations (585.0)
// ---------------------------------------------------------------------------

#[test]
fn deleting_two_annotations_is_one_entry_and_undo_restores_both() {
    let mut s = session("hello.pdf");
    let a = s.add_markup(0, &square(20.0)).unwrap();
    let b = s.add_markup(0, &square(90.0)).unwrap();
    let depth = s.undo_depth();

    let done = s.delete_annotations(&[a, b, a]).unwrap();
    assert_eq!(done.len(), 2, "an id named twice is taken once");
    assert!(s.value(a).is_none() && s.value(b).is_none());
    assert_eq!(s.undo_depth(), depth + 1);

    s.undo().unwrap();
    assert!(s.value(a).is_some() && s.value(b).is_some());
}

#[test]
fn a_locked_annotation_refuses_the_batch_with_nothing_deleted() {
    let mut s = session("hello.pdf");
    let a = s.add_markup(0, &square(20.0)).unwrap();
    let b = s.add_markup(0, &square(90.0)).unwrap();
    lock(&mut s, b);
    let depth = s.undo_depth();

    let err = s.delete_annotations(&[a, b]).unwrap_err();
    assert!(matches!(err, EditError::AnnotationLocked { .. }), "{err}");
    assert!(
        s.value(a).is_some(),
        "the pre-flight refused before `a` went"
    );
    assert_eq!(s.undo_depth(), depth);
}

// ---------------------------------------------------------------------------
// move_annotations_each (584.0)
// ---------------------------------------------------------------------------

#[test]
fn a_markup_and_a_ce_dimension_move_together_in_one_entry() {
    let mut s = session("dimension/plain-base.pdf");
    let markup = s.add_markup(0, &square(20.0)).unwrap();
    let (annot, dim) = s.add_dimension(0, DEFAULT_GROUP_ID, linear()).unwrap();
    assert_eq!(s.dimension_for_annotation(annot), Some(dim));
    assert_eq!(s.dimension_for_annotation(markup), None);
    let markup_before = rect_of(&s, markup);
    let depth = s.undo_depth();

    let moved = s
        .move_annotations_each(&[(markup, 5.0, 7.0), (annot, 0.0, 30.0)])
        .unwrap();
    assert_eq!(s.undo_depth(), depth + 1);
    assert!(matches!(moved[0], AnnotationBatchMove::Annotation(_)));
    let AnnotationBatchMove::Dimension {
        dimension,
        placement,
    } = moved[1]
    else {
        panic!("the ce dimension is placed, not moved: {:?}", moved[1]);
    };
    assert_eq!(dimension, dim);
    assert!(
        (placement.0 - 30.0).abs() < 1e-9,
        "standoff follows the drag"
    );
    assert_eq!(rect_of(&s, markup)[0], markup_before[0] + 5.0);
    let DimensionKind::Linear { a, b, offset, .. } =
        s.dimension_model().dimension(dim).unwrap().kind.clone()
    else {
        panic!("still linear");
    };
    assert_eq!((a, b), (Point::new(100.0, 200.0), Point::new(300.0, 200.0)));
    assert!((offset - 30.0).abs() < 1e-9);

    s.undo().unwrap();
    assert_eq!(rect_of(&s, markup), markup_before);
    assert_eq!(s.dimension_model().dimension(dim).unwrap().kind, linear());
}

#[test]
fn a_locked_ce_dimension_refuses_placement_and_the_batch() {
    let mut s = session("dimension/plain-base.pdf");
    let markup = s.add_markup(0, &square(20.0)).unwrap();
    let (annot, dim) = s.add_dimension(0, DEFAULT_GROUP_ID, linear()).unwrap();
    lock(&mut s, annot);
    let markup_before = rect_of(&s, markup);
    let depth = s.undo_depth();

    for err in [
        s.place_dimension(dim, 10.0, 0.0).unwrap_err(),
        s.move_dimension(dim, 10.0, 0.0).unwrap_err(),
        s.move_annotations_each(&[(markup, 5.0, 0.0), (annot, 0.0, 30.0)])
            .unwrap_err(),
    ] {
        assert!(matches!(err, EditError::AnnotationLocked { .. }), "{err}");
    }
    assert_eq!(
        rect_of(&s, markup),
        markup_before,
        "the markup is not left moved"
    );
    assert_eq!(s.undo_depth(), depth);
    assert!(!s.can_redo());
}
