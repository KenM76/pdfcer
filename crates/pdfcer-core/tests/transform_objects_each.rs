//! `transform_objects_each` — several objects, each by its own page-space
//! matrix, as one command (arrange on a circle, turning each object to its
//! own tangent).
//!
//! The assertions are page-space bounding boxes per object: a matrix that went
//! to the wrong object, or through the wrong CTM, lands somewhere else.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::content::ContentStream;
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::vector::{
    Bounds, Matrix, NoXObjects, Point, TransformOptions, VectorEditError, decompose,
    plan_transform_each,
};

const IMAGE: &str = "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 \
     /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 1 >>\nstream\n\u{0}\nendstream";

/// A one-page PDF drawing `content`, with `/F1` and `/Im1`.
fn pdf(content: &str) -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R >> /XObject << /Im1 6 0 R >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_owned(),
        IMAGE.to_owned(),
    ];
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

fn session(content: &str) -> EditSession {
    EditSession::new(Document::from_bytes(pdf(content)).unwrap())
}

fn boxes(s: &mut EditSession) -> Vec<Bounds> {
    let model = s.page_objects(0).unwrap();
    model.objects.iter().map(|o| o.page_bbox()).collect()
}

fn centre(b: Bounds) -> Point {
    Point {
        x: (b.min.x + b.max.x) / 2.0,
        y: (b.min.y + b.max.y) / 2.0,
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// `after` is `before` shifted by `(dx, dy)`.
fn shifted(before: Bounds, after: Bounds, dx: f64, dy: f64) -> bool {
    close(after.min.x, before.min.x + dx)
        && close(after.min.y, before.min.y + dy)
        && close(after.max.x, before.max.x + dx)
        && close(after.max.y, before.max.y + dy)
}

/// A quarter turn about `b`'s own centre: same centre, width and height swapped.
fn quarter_turned(before: Bounds, after: Bounds) -> bool {
    let (c0, c1) = (centre(before), centre(after));
    close(c0.x, c1.x)
        && close(c0.y, c1.y)
        && close(after.max.x - after.min.x, before.max.y - before.min.y)
        && close(after.max.y - after.min.y, before.max.x - before.min.x)
}

fn turn_about_own_centre(b: Bounds) -> Matrix {
    Matrix::rotate(std::f64::consts::FRAC_PI_2).about(centre(b))
}

/// A 10x5 rectangle under a 2x CTM (page box 20x10), a text object, and an
/// image placed at 5x5.
const MIXED: &str = "q 2 0 0 2 0 0 cm 0 0 10 5 re S Q\n\
                     BT /F1 12 Tf 200 200 Td (hi) Tj ET\n\
                     q 8 0 0 5 40 40 cm /Im1 Do Q";

#[test]
fn every_kind_gets_its_own_matrix_in_page_space() {
    let mut s = session(MIXED);
    let before = boxes(&mut s);
    assert_eq!(before.len(), 3, "precondition: path, text, image");
    let out = s
        .transform_objects_each(
            0,
            &[
                (0, turn_about_own_centre(before[0])),
                (1, Matrix::translate(0.0, 7.0)),
                (2, turn_about_own_centre(before[2])),
            ],
            TransformOptions::default(),
        )
        .expect("a mixed per-object transform");
    assert_eq!(out.objects_transformed, 3);
    assert!(!out.clamped);
    let after = boxes(&mut s);
    assert!(quarter_turned(before[0], after[0]), "path {:?}", after[0]);
    assert!(
        shifted(before[1], after[1], 0.0, 7.0),
        "text {:?}",
        after[1]
    );
    assert!(quarter_turned(before[2], after[2]), "image {:?}", after[2]);
}

/// Each single-object call lands exactly where `transform_objects` puts it.
#[test]
fn one_object_matches_transform_objects() {
    let m = Matrix::scale(1.5, 0.5).about(Point { x: 3.0, y: 4.0 });
    for i in 0..3 {
        let mut a = session(MIXED);
        let mut b = session(MIXED);
        a.transform_objects_each(0, &[(i, m)], TransformOptions::default())
            .unwrap();
        b.transform_objects(0, &[i], m, TransformOptions::default())
            .unwrap();
        assert_eq!(boxes(&mut a), boxes(&mut b), "object {i}");
    }
}

#[test]
fn one_gesture_is_one_undo_entry() {
    let mut s = session(MIXED);
    let before = boxes(&mut s);
    s.transform_objects_each(
        0,
        &[
            (0, turn_about_own_centre(before[0])),
            (2, Matrix::translate(3.0, 3.0)),
        ],
        TransformOptions::default(),
    )
    .unwrap();
    assert_eq!(s.undo_depth(), 1);
    s.undo().unwrap();
    assert_eq!(boxes(&mut s), before);
}

#[test]
fn a_stale_index_refuses_the_whole_call() {
    let mut s = session(MIXED);
    let before = boxes(&mut s);
    let err = s
        .transform_objects_each(
            0,
            &[(0, Matrix::translate(5.0, 0.0)), (9, Matrix::IDENTITY)],
            TransformOptions::default(),
        )
        .expect_err("object 9 does not exist");
    assert!(
        matches!(
            err,
            EditError::VectorEdit(VectorEditError::ObjectOutOfRange { index: 9, .. })
        ),
        "{err:?}"
    );
    assert_eq!(boxes(&mut s), before, "object 0 must not have moved");
    assert_eq!(s.undo_depth(), 0);
}

#[test]
fn a_singular_matrix_on_a_later_object_refuses_the_whole_call() {
    let mut s = session(MIXED);
    let before = boxes(&mut s);
    let err = s
        .transform_objects_each(
            0,
            &[
                (0, Matrix::translate(5.0, 0.0)),
                (2, Matrix::scale(0.0, 1.0)),
            ],
            TransformOptions::default(),
        )
        .expect_err("a zero scale maps area to zero");
    assert!(
        matches!(
            err,
            EditError::VectorEdit(VectorEditError::SingularTransform)
        ),
        "{err:?}"
    );
    assert_eq!(boxes(&mut s), before);
    assert_eq!(s.undo_depth(), 0);
}

#[test]
fn an_object_named_twice_is_refused_by_its_page_index() {
    let mut s = session(MIXED);
    let before = boxes(&mut s);
    let err = s
        .transform_objects_each(
            0,
            &[
                (2, Matrix::translate(5.0, 0.0)),
                (2, Matrix::translate(0.0, 5.0)),
            ],
            TransformOptions::default(),
        )
        .expect_err("two matrices for object 2");
    assert!(
        matches!(
            err,
            EditError::VectorEdit(VectorEditError::DuplicateObjectInMove { index: 2 })
        ),
        "{err:?}"
    );
    assert_eq!(boxes(&mut s), before);
}

#[test]
fn an_empty_list_changes_nothing() {
    let mut s = session(MIXED);
    let before = boxes(&mut s);
    let out = s
        .transform_objects_each(0, &[], TransformOptions::default())
        .unwrap();
    assert_eq!(out.objects_transformed, 0);
    assert_eq!(boxes(&mut s), before);
}

/// Minimal diff: each object is wrapped in its own `q <cm> … Q`, every other
/// byte verbatim, and the emitted `cm` is `CTM × M × CTM⁻¹` per object.
#[test]
fn the_planner_wraps_each_object_with_its_own_local_matrix() {
    let src = b"q 2 0 0 2 0 0 cm 0 0 m 5 5 l S Q 10 10 m 20 20 l S".to_vec();
    let cs = ContentStream::parse(src).unwrap();
    let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
    let each = [
        (&model.objects[0], Matrix::translate(4.0, 0.0)),
        (&model.objects[1], Matrix::translate(0.0, 3.0)),
    ];
    let plan = plan_transform_each(&cs, &each, TransformOptions::default()).unwrap();
    assert_eq!(
        String::from_utf8(plan.content).unwrap(),
        "q 2 0 0 2 0 0 cm q 1 0 0 1 2 0 cm 0 0 m 5 5 l S Q Q q 1 0 0 1 0 3 cm 10 10 m 20 20 l S Q"
    );
}

/// The planner refuses a repeat on its own, naming the list position.
#[test]
fn the_planner_refuses_one_object_twice_by_position() {
    let cs = ContentStream::parse(b"0 0 m 5 5 l S".to_vec()).unwrap();
    let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
    let o = &model.objects[0];
    let err = plan_transform_each(
        &cs,
        &[(o, Matrix::IDENTITY), (o, Matrix::translate(1.0, 0.0))],
        TransformOptions::default(),
    )
    .unwrap_err();
    assert_eq!(err, VectorEditError::DuplicateObjectInMove { index: 1 });
}
