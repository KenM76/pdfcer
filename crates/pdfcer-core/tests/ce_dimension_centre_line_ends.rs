//! Moving a centre line's two ends through the vertex verbs (pdfcer-gui
//! request G335).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::dimension::{
    CentreMarkSpec, DEFAULT_GROUP_ID, DimensionId, DimensionKind, FitCircle,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, VertexEdit};
use pdfcer_core::vector::Point;
use pdfcer_core::writer::SaveOptions;
use std::path::Path;

fn hello() -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/hello.pdf");
    EditSession::new(Document::load(&path).expect("load fixture"))
}

fn save(s: &EditSession) -> Vec<u8> {
    s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0
}

fn with_line() -> (EditSession, DimensionId) {
    let mut s = hello();
    let (_, id) = s
        .add_dimension(
            0,
            DEFAULT_GROUP_ID,
            DimensionKind::CentreLine {
                a: Point::new(100.0, 200.0),
                b: Point::new(300.0, 200.0),
            },
        )
        .unwrap();
    (s, id)
}

fn ends(s: &EditSession, id: DimensionId) -> (Point, Point) {
    match s.dimension_model().dimension(id).unwrap().kind {
        DimensionKind::CentreLine { a, b } => (a, b),
        _ => panic!("not a centre line"),
    }
}

#[test]
fn each_end_moves_saves_reopens_and_undoes_as_one_entry() {
    let (mut s, id) = with_line();
    let before = save(&s);
    let depth = s.undo_depth();

    let outcome = s.move_dimension_vertex(id, 0, -20.0, 0.0).unwrap();
    assert_eq!(outcome.vertices, 2);
    assert!(!outcome.closed);
    // A centre line measures nothing: both labels are empty.
    assert!(outcome.label.is_empty() && outcome.previous_label.is_empty());
    assert_eq!(s.undo_depth(), depth + 1);
    assert_eq!(
        ends(&s, id),
        (Point::new(80.0, 200.0), Point::new(300.0, 200.0))
    );

    s.move_dimension_vertex(id, 1, 15.0, 0.0).unwrap();
    let reopened = EditSession::new(Document::from_bytes(save(&s)).unwrap());
    assert_eq!(
        ends(&reopened, id),
        (Point::new(80.0, 200.0), Point::new(315.0, 200.0))
    );

    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(save(&s), before);
}

#[test]
fn the_count_is_fixed_and_the_preview_says_so_first() {
    let (mut s, id) = with_line();
    let insert = VertexEdit::Insert {
        after: 0,
        at: Point::new(200.0, 200.0),
    };
    let remove = VertexEdit::Remove { index: 0 };
    for edit in [insert, remove] {
        assert!(matches!(
            s.vertex_edit_preview(id, edit),
            Err(EditError::DimensionVertexCountFixed { count: 2, .. })
        ));
    }
    assert!(matches!(
        s.insert_dimension_vertex(id, 0, Point::new(200.0, 200.0)),
        Err(EditError::DimensionVertexCountFixed { count: 2, .. })
    ));
    assert!(matches!(
        s.remove_dimension_vertex(id, 1),
        Err(EditError::DimensionVertexCountFixed { count: 2, .. })
    ));
    assert!(matches!(
        s.move_dimension_vertex(id, 2, 1.0, 0.0),
        Err(EditError::VertexIndexOutOfRange {
            index: 2,
            count: 2,
            ..
        })
    ));
    let preview = s
        .vertex_edit_preview(
            id,
            VertexEdit::Move {
                index: 1,
                dx: 5.0,
                dy: 0.0,
            },
        )
        .unwrap();
    assert!(preview.label.is_empty());
}

#[test]
fn coincident_ends_are_refused_before_anything_is_written() {
    let (mut s, id) = with_line();
    let bytes = save(&s);
    let depth = s.undo_depth();
    for (index, dx) in [(1, -200.0), (0, 200.0)] {
        assert!(matches!(
            s.move_dimension_vertex(id, index, dx, 0.0),
            Err(EditError::CentreGeometryDegenerate { .. })
        ));
    }
    assert_eq!(s.undo_depth(), depth);
    assert_eq!(save(&s), bytes);
}

#[test]
fn a_centre_mark_still_has_no_vertices() {
    let mut s = hello();
    let (_, id) = s
        .add_dimension(
            0,
            DEFAULT_GROUP_ID,
            DimensionKind::CentreMark {
                fit: FitCircle {
                    center: Point::new(200.0, 200.0),
                    radius: 40.0,
                    residual: 0.0,
                },
                mark: CentreMarkSpec::default(),
            },
        )
        .unwrap();
    assert!(matches!(
        s.move_dimension_vertex(id, 0, 1.0, 0.0),
        Err(EditError::DimensionHasNoVertices { .. })
    ));
}
