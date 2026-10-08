//! `set_text_run_width_in_form`, `merge_text_runs_in_form` and
//! `split_text_object_in_form` (pdfcer-gui request G158).

use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditError, EditSession};
use pdfcer_core::text_edit::{FormatError, MergeOptions};
use pdfcer_core::vector::VectorObject;

/// `Fm0` drawn twice: at `2 0 0 2 10 10 cm` and at `1 0 0 1 10 120 cm`.
/// Its objects: 0 text `(Inv) (oice)`, 1 text `(AB)` then `(CD)` at its own
/// position, 2 text `(Wide)`, 3 a path. Leaves 0–3 are the first drawing.
fn fixture() -> Vec<u8> {
    let page = "q 2 0 0 2 10 10 cm /Fm0 Do Q\nq 1 0 0 1 10 120 cm /Fm0 Do Q\n";
    let fm0 = "BT /F1 6 Tf 0 0 Td (Inv) Tj (oice) Tj ET\n\
               BT /F1 6 Tf 0 10 Td (AB) Tj 20 0 Td (CD) Tj ET\n\
               BT /F1 6 Tf 0 20 Td (Wide) Tj ET\n0 30 5 5 re f\n";
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
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 80 40] /Resources << /Font << /F1 6 0 R \
             >> >>",
            fm0,
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
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

fn runs(s: &mut EditSession, leaf: usize) -> usize {
    match &s.page_objects(0).unwrap().leaves[leaf].object {
        VectorObject::Text(t) => t.runs.len(),
        other => panic!("leaf {leaf} is not text: {other:?}"),
    }
}

fn width(s: &mut EditSession, leaf: usize) -> f64 {
    let b = s.page_objects(0).unwrap().leaves[leaf].object.page_bbox();
    b.max.x - b.min.x
}

#[test]
fn a_width_is_set_in_page_points_at_this_placement_and_shows_at_both() {
    let mut s = session();
    let (wide, other) = (width(&mut s, 2), width(&mut s, 6));
    let out = s.set_text_run_width_in_form(0, 2, 0, wide * 1.5).unwrap();
    assert_eq!((out.invocations, out.pages), (2, 1));
    assert!(out.report.h_scale_change.is_some());
    let (got, got_other) = (width(&mut s, 2), width(&mut s, 6));
    assert!((got - wide * 1.5).abs() < 0.5, "{wide} -> {got}");
    assert!(
        (got_other - other * 1.5).abs() < 0.5,
        "the half-size drawing scales too: {other} -> {got_other}"
    );
    assert_eq!(s.undo(), Some(CommandKind::FormatText));
    assert!((width(&mut s, 2) - wide).abs() < 1e-6);
}

#[test]
fn runs_merge_inside_the_form() {
    let mut s = session();
    let out = s
        .merge_text_runs_in_form(0, 0, &[0, 1], &MergeOptions::default())
        .unwrap();
    assert_eq!(out.report.runs_merged, 2);
    assert_eq!(out.report.text, "Invoice");
    assert_eq!((runs(&mut s, 0), runs(&mut s, 4)), (1, 1));
    assert_eq!(s.undo(), Some(CommandKind::MergeTextRuns));
    assert_eq!(runs(&mut s, 0), 2);
}

#[test]
fn an_object_splits_inside_the_form() {
    let mut s = session();
    let before = s.page_objects(0).unwrap().leaves.len();
    let out = s.split_text_object_in_form(0, 1, &[1]).unwrap();
    assert_eq!(out.invocations, 2);
    assert_eq!(
        s.page_objects(0).unwrap().leaves.len(),
        before + 2,
        "one more object in each drawing"
    );
    assert_eq!(s.undo(), Some(CommandKind::SplitTextObject));
    assert_eq!(s.page_objects(0).unwrap().leaves.len(), before);
}

#[test]
fn bad_leaves_are_refused_before_any_change() {
    let mut s = session();
    let depth = s.undo_kinds().len();
    assert!(matches!(
        s.set_text_run_width_in_form(0, 99, 0, 10.0).unwrap_err(),
        FormatError::FormLeafOutOfRange { index: 99, .. }
    ));
    assert!(matches!(
        s.merge_text_runs_in_form(0, 99, &[0, 1], &MergeOptions::default())
            .unwrap_err(),
        FormatError::FormLeafOutOfRange { .. }
    ));
    assert!(matches!(
        s.split_text_object_in_form(0, 99, &[1]).unwrap_err(),
        EditError::FormLeafOutOfRange { .. }
    ));
    assert!(
        s.set_text_run_width_in_form(0, 3, 0, 10.0).is_err(),
        "a path"
    );
    assert!(s.set_text_run_width_in_form(0, 2, 0, 0.0).is_err());
    assert!(
        s.merge_text_runs_in_form(0, 1, &[0], &MergeOptions::default())
            .is_err(),
        "one run is not a merge"
    );
    assert_eq!(s.undo_kinds().len(), depth);
}
