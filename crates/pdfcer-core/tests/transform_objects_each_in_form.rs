//! `EditSession::transform_objects_each_in_form` (pdfcer-gui request G180):
//! each leaf of a placed form gets its own page-space matrix, in one undo
//! step, all-or-nothing (Pass 577.0).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::vector::{Bounds, Matrix, Point, TransformOptions, VectorObject};
use pdfcer_core::writer::SaveOptions;

/// Leaves 0 and 1: a 10x4 and a 6x6 rectangle in `Fm0`, placed at
/// `2 0 0 2 10 10 cm`, so a matrix applied in form space lands visibly wrong.
fn fixture() -> Vec<u8> {
    let page = "q 2 0 0 2 10 10 cm /Fm0 Do Q\n";
    let fm0 = "0 0 10 4 re S\n20 0 6 6 re S\n";
    let stream = |dict: &str, body: &str| {
        format!(
            "<< {dict} /Length {} >>\nstream\n{body}endstream",
            body.len()
        )
    };
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources << /XObject \
         << /Fm0 5 0 R >> >> /Contents 4 0 R >>"
            .to_owned(),
        stream("", page),
        stream("/Type /XObject /Subtype /Form /BBox [0 0 40 40]", fm0),
    ];
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objs.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = offsets.len() + 1;
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

fn session() -> EditSession {
    EditSession::new(Document::from_bytes(fixture()).unwrap())
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

fn size(b: Bounds) -> (f64, f64) {
    (b.max.x - b.min.x, b.max.y - b.min.y)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

#[test]
fn each_leaf_turns_and_scales_about_its_own_centre_in_one_step() {
    let mut s = session();
    let (b0, b1) = (bbox(&mut s, 0), bbox(&mut s, 1));
    let (c0, c1) = (centre(b0), centre(b1));
    let out = s
        .transform_objects_each_in_form(
            0,
            &[
                (0, Matrix::rotate(std::f64::consts::FRAC_PI_2).about(c0)),
                (1, Matrix::scale(2.0, 2.0).about(c1)),
            ],
            TransformOptions::default(),
        )
        .unwrap();
    assert_eq!((out.invocations, out.pages), (1, 1));

    let (a0, a1) = (bbox(&mut s, 0), bbox(&mut s, 1));
    let ((w0, h0), (w1, h1)) = (size(b0), size(b1));
    assert!(close(size(a0).0, h0) && close(size(a0).1, w0), "{a0:?}");
    assert!(close(size(a1).0, 2.0 * w1) && close(size(a1).1, 2.0 * h1));
    for (c, a) in [(c0, a0), (c1, a1)] {
        let ac = centre(a);
        assert!(close(ac.x, c.x) && close(ac.y, c.y), "pivot stays put");
    }

    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let mut r = EditSession::new(Document::from_bytes(bytes).unwrap());
    assert!(close(size(bbox(&mut r, 1)).0, 2.0 * w1), "saved");

    s.undo().unwrap();
    assert_eq!(
        (bbox(&mut s, 0), bbox(&mut s, 1)),
        (b0, b1),
        "one undo step"
    );
}

#[test]
fn a_refusal_on_one_leaf_changes_none() {
    let mut s = session();
    let before = (bbox(&mut s, 0), bbox(&mut s, 1));
    let id = TransformOptions::default();
    let err = s
        .transform_objects_each_in_form(
            0,
            &[(0, Matrix::scale(2.0, 2.0)), (1, Matrix::scale(0.0, 1.0))],
            id,
        )
        .unwrap_err();
    assert!(matches!(err, EditError::VectorEdit(_)), "{err:?}");
    let twice = s
        .transform_objects_each_in_form(
            0,
            &[(0, Matrix::IDENTITY), (0, Matrix::scale(2.0, 2.0))],
            id,
        )
        .unwrap_err();
    assert!(matches!(twice, EditError::VectorEdit(_)), "{twice:?}");
    let empty = s.transform_objects_each_in_form(0, &[], id).unwrap_err();
    assert!(matches!(empty, EditError::FormLeafOutOfRange { .. }));
    assert_eq!((bbox(&mut s, 0), bbox(&mut s, 1)), before);
    assert!(s.undo_kind().is_none());
}
