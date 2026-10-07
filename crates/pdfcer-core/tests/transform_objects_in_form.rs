//! `EditSession::transform_objects_in_form` (pdfcer-gui request G144):
//! resize and rotate a leaf inside a form XObject, in page space.

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::vector::{Bounds, Matrix, Point, TransformOptions, VectorObject};
use pdfcer_core::writer::SaveOptions;
use std::path::Path;

/// One path inside one form placed at `2 0 0 2 40 30 cm` — a scale, so a
/// matrix applied in form space instead of page space lands visibly wrong.
fn scaled() -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/forms-xobject/scaled-form-placement.pdf");
    EditSession::new(Document::load(&path).unwrap())
}

fn bbox(s: &mut EditSession, i: usize) -> Bounds {
    match &s.page_objects(0).unwrap().leaves[i].object {
        VectorObject::Path(p) => p.page_bbox,
        other => panic!("not a path: {other:?}"),
    }
}

fn centre(b: Bounds) -> Point {
    Point::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn scaling_a_leaf_about_its_centre_doubles_it_in_page_space() {
    let mut s = scaled();
    let before = bbox(&mut s, 0);
    let c = centre(before);
    let out = s
        .transform_objects_in_form(
            0,
            &[0],
            Matrix::scale(2.0, 2.0).about(c),
            TransformOptions::default(),
        )
        .unwrap();
    assert_eq!((out.invocations, out.pages), (1, 1));

    let after = bbox(&mut s, 0);
    let w = |b: Bounds| b.max.x - b.min.x;
    assert!(close(w(after), 2.0 * w(before)), "{before:?} -> {after:?}");
    let ac = centre(after);
    assert!(close(ac.x, c.x) && close(ac.y, c.y), "the pivot stays put");

    // Survives a save.
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let mut r = EditSession::new(Document::from_bytes(bytes).unwrap());
    assert!(close(w(bbox(&mut r, 0)), 2.0 * w(before)));

    s.undo().unwrap();
    assert_eq!(bbox(&mut s, 0), before, "one undo step");
}

#[test]
fn a_quarter_turn_swaps_width_and_height() {
    let mut s = scaled();
    let before = bbox(&mut s, 0);
    let c = centre(before);
    s.transform_objects_in_form(
        0,
        &[0],
        Matrix::rotate(std::f64::consts::FRAC_PI_2).about(c),
        TransformOptions::default(),
    )
    .unwrap();
    let after = bbox(&mut s, 0);
    assert!(close(
        after.max.x - after.min.x,
        before.max.y - before.min.y
    ));
    assert!(close(
        after.max.y - after.min.y,
        before.max.x - before.min.x
    ));
}

#[test]
fn bad_selections_and_a_singular_matrix_change_nothing() {
    let mut s = scaled();
    let id = TransformOptions::default();
    let err = s
        .transform_objects_in_form(0, &[], Matrix::scale(2.0, 2.0), id)
        .unwrap_err();
    assert!(
        matches!(err, EditError::FormLeafOutOfRange { .. }),
        "{err:?}"
    );
    let err = s
        .transform_objects_in_form(0, &[7], Matrix::scale(2.0, 2.0), id)
        .unwrap_err();
    assert!(
        matches!(err, EditError::FormLeafOutOfRange { .. }),
        "{err:?}"
    );
    let err = s
        .transform_objects_in_form(0, &[0], Matrix::scale(0.0, 1.0), id)
        .unwrap_err();
    assert!(matches!(err, EditError::VectorEdit(_)), "{err:?}");
    assert!(s.undo_kind().is_none());
}
