//! `move_objects_each` / `move_objects_each_in_form` — several objects, each
//! by its own page-space delta, as one command (`G071`: align, distribute,
//! arrange).
//!
//! The assertions are page-space bounding boxes per object: a delta that went
//! to the wrong object, or through the wrong matrix, lands somewhere else.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::content::ContentStream;
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::vector::{
    Bounds, Matrix, NoXObjects, VectorEditError, decompose, plan_move_objects_each,
};

const IMAGE: &str = "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 \
     /ColorSpace /DeviceGray /BitsPerComponent 8 /Length 1 >>\nstream\n\u{0}\nendstream";

/// A one-page PDF drawing `content`, with `/F1`, `/Im1` and a form `/Fm1`
/// whose own content is `form`.
fn pdf(content: &str, form: &str) -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 5 0 R >> /XObject << /Im1 6 0 R /Fm1 7 0 R >> >> >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_owned(),
        IMAGE.to_owned(),
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 500 500] \
             /Resources << /XObject << /Im1 6 0 R >> >> /Length {} >>\nstream\n{form}\nendstream",
            form.len()
        ),
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

fn session(content: &str, form: &str) -> EditSession {
    EditSession::new(Document::from_bytes(pdf(content, form)).unwrap())
}

fn boxes(s: &mut EditSession) -> Vec<Bounds> {
    let model = s.page_objects(0).unwrap();
    model.objects.iter().map(|o| o.page_bbox()).collect()
}

fn leaf_boxes(s: &mut EditSession) -> Vec<Bounds> {
    let model = s.page_objects(0).unwrap();
    model.leaves.iter().map(|l| l.object.page_bbox()).collect()
}

/// `after` is `before` shifted by `(dx, dy)`.
fn shifted(before: Bounds, after: Bounds, dx: f64, dy: f64) -> bool {
    let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
    close(after.min.x, before.min.x + dx)
        && close(after.min.y, before.min.y + dy)
        && close(after.max.x, before.max.x + dx)
        && close(after.max.y, before.max.y + dy)
}

/// Path, text and image under non-identity CTMs, each with its own delta.
const MIXED: &str = "q 2 0 0 2 0 0 cm 0 0 10 10 re S Q\n\
                     BT /F1 12 Tf 20 20 Td (hi) Tj ET\n\
                     q 5 0 0 5 40 40 cm /Im1 Do Q";

#[test]
fn every_kind_moves_by_its_own_delta_in_page_space() {
    let mut s = session(MIXED, "");
    let before = boxes(&mut s);
    assert_eq!(before.len(), 3, "precondition: path, text, image");
    let notes = s
        .move_objects_each(0, &[(0, 5.0, 0.0), (1, 0.0, 7.0), (2, -3.0, 4.0)])
        .expect("a mixed per-object move");
    assert!(notes.is_empty(), "nothing to disclose here: {notes:?}");
    let after = boxes(&mut s);
    assert!(
        shifted(before[0], after[0], 5.0, 0.0),
        "path {:?}",
        after[0]
    );
    assert!(
        shifted(before[1], after[1], 0.0, 7.0),
        "text {:?}",
        after[1]
    );
    assert!(
        shifted(before[2], after[2], -3.0, 4.0),
        "image {:?}",
        after[2]
    );
}

#[test]
fn one_gesture_is_one_undo_entry() {
    let mut s = session(MIXED, "");
    let before = boxes(&mut s);
    s.move_objects_each(0, &[(0, 5.0, 0.0), (2, -3.0, 4.0)])
        .unwrap();
    assert_eq!(s.undo_depth(), 1);
    s.undo().unwrap();
    assert_eq!(boxes(&mut s), before);
}

#[test]
fn a_stale_index_refuses_the_whole_move() {
    let mut s = session(MIXED, "");
    let before = boxes(&mut s);
    let err = s
        .move_objects_each(0, &[(0, 5.0, 0.0), (9, 1.0, 1.0)])
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
fn an_object_named_twice_is_refused_by_its_page_index() {
    let mut s = session(MIXED, "");
    let before = boxes(&mut s);
    let err = s
        .move_objects_each(0, &[(2, 5.0, 0.0), (2, 0.0, 5.0)])
        .expect_err("two deltas for object 2");
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
fn an_inserted_positioning_operator_is_disclosed() {
    let mut s = session("BT /F1 12 Tf (hi) Tj ET 0 0 10 10 re S", "");
    let notes = s
        .move_objects_each(0, &[(0, 3.0, 0.0), (1, 0.0, 3.0)])
        .unwrap();
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("position instruction"), "{notes:?}");
}

/// Minimal diff: operands rewritten in place, the (inline) image wrapped,
/// every other byte verbatim.
#[test]
fn the_planner_rewrites_operands_and_wraps_only_the_image() {
    let src = b"0 0 m 5 5 l S q BI /W 1 /H 1 /CS /G /BPC 8 ID x EI Q".to_vec();
    let cs = ContentStream::parse(src).unwrap();
    let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
    let moves = [(&model.objects[0], 1.0, 2.0), (&model.objects[1], 3.0, 4.0)];
    let plan = plan_move_objects_each(&cs, &moves).unwrap();
    assert_eq!(
        String::from_utf8(plan.content).unwrap(),
        "1 2 m 6 7 l S q q 1 0 0 1 3 4 cm BI /W 1 /H 1 /CS /G /BPC 8 ID x EI Q Q"
    );
}

/// Two leaves of one form placed at 2x: page-space deltas are halved into
/// form space, so each leaf lands where it was asked to.
#[test]
fn form_leaves_move_by_their_own_page_space_deltas() {
    let mut s = session(
        "q 2 0 0 2 100 100 cm /Fm1 Do Q",
        "0 0 10 10 re S q 4 0 0 4 20 20 cm /Im1 Do Q",
    );
    let before = leaf_boxes(&mut s);
    assert_eq!(before.len(), 2, "precondition: path and image leaves");
    let out = s
        .move_objects_each_in_form(0, &[(0, 6.0, 0.0), (1, 0.0, -8.0)])
        .expect("an in-form per-object move");
    assert_eq!(out.invocations, 1);
    let after = leaf_boxes(&mut s);
    assert!(
        shifted(before[0], after[0], 6.0, 0.0),
        "path {:?}",
        after[0]
    );
    assert!(
        shifted(before[1], after[1], 0.0, -8.0),
        "image {:?}",
        after[1]
    );
    assert_eq!(s.undo_depth(), 1);
}

/// The planner refuses a repeat on its own, naming the list position.
#[test]
fn the_planner_refuses_one_object_twice_by_position() {
    let cs = ContentStream::parse(b"0 0 m 5 5 l S".to_vec()).unwrap();
    let model = decompose(&cs, Matrix::IDENTITY, &NoXObjects);
    let o = &model.objects[0];
    let err = plan_move_objects_each(&cs, &[(o, 1.0, 0.0), (o, 0.0, 1.0)]).unwrap_err();
    assert_eq!(err, VectorEditError::DuplicateObjectInMove { index: 1 });
}
