//! `EditSession::set_object_stroke_style` (pdfcer-gui request G143): width,
//! dash and constant alpha on chosen paths, and the `PathObject` fields that
//! read them back.

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, PaintRefusalReason};
use pdfcer_core::page_tree;
use pdfcer_core::vector::{
    Dash, Matrix, PathObject, StrokeStyle, VectorEditError, VectorObject, decompose_page,
};
use pdfcer_core::writer::SaveOptions;

/// Object 0: a plain stroked square. 1: text. 2: a path under `[2 2] 1 d`.
/// 3: a path under `/GS0 gs` (`/D [[4 1] 0]`, `/CA 0.5`, `/ca 0.25`).
fn fixture() -> Vec<u8> {
    let content = "0 0 20 20 re S\nBT /F1 12 Tf 50 50 Td (A) Tj ET\n\
                   q [2 2] 1 d 30 0 m 40 10 l S Q\n\
                   q /GS0 gs 60 0 m 70 10 l S Q\n";
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /Font \
         << /F1 5 0 R >> /ExtGState << /GS0 6 0 R >> >> /Contents 4 0 R >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        "<< /Type /ExtGState /D [[4 1] 0] /CA 0.5 /ca 0.25 >>".to_owned(),
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

fn session(bytes: &[u8]) -> EditSession {
    EditSession::new(Document::from_bytes(bytes.to_vec()).unwrap())
}

fn save(s: &EditSession) -> Vec<u8> {
    s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0
}

fn path(s: &mut EditSession, i: usize) -> PathObject {
    match &s.page_objects(0).unwrap().objects[i] {
        VectorObject::Path(p) => p.clone(),
        other => panic!("object {i} is not a path: {other:?}"),
    }
}

/// Decompose page 0 of saved bytes, as a fresh open would.
fn reopened_path(bytes: &[u8], i: usize) -> PathObject {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    let model = decompose_page(&doc.view(), &pages[0], Matrix::IDENTITY).unwrap();
    match &model.objects[i] {
        VectorObject::Path(p) => p.clone(),
        other => panic!("object {i} is not a path: {other:?}"),
    }
}

fn full_style() -> StrokeStyle {
    StrokeStyle {
        width: Some(3.5),
        dash: Some(Dash::new(vec![5.0, 2.0], 1.0)),
        stroke_alpha: Some(0.4),
        fill_alpha: Some(0.6),
    }
}

#[test]
fn the_readers_see_the_dash_operator_and_the_ext_gstate() {
    let mut s = session(&fixture());
    let plain = path(&mut s, 0);
    assert!(plain.dash.is_solid());
    assert_eq!((plain.stroke_alpha, plain.fill_alpha), (1.0, 1.0));
    assert_eq!(path(&mut s, 2).dash, Dash::new(vec![2.0, 2.0], 1.0));
    let gs = path(&mut s, 3);
    assert_eq!(gs.dash, Dash::new(vec![4.0, 1.0], 0.0));
    assert_eq!((gs.stroke_alpha, gs.fill_alpha), (0.5, 0.25));
}

#[test]
fn width_dash_and_alpha_are_set_and_read_back_after_a_save() {
    let mut s = session(&fixture());
    let out = s.set_object_stroke_style(0, &[0], &full_style()).unwrap();
    assert_eq!(out.changed, vec![0]);
    assert!(out.refused.is_empty());

    let p = path(&mut s, 0);
    assert_eq!(p.line_width, 3.5);
    assert_eq!(p.dash, Dash::new(vec![5.0, 2.0], 1.0));
    assert!((p.stroke_alpha - 0.4).abs() < 1e-6);
    assert!((p.fill_alpha - 0.6).abs() < 1e-6);
    assert!(
        path(&mut s, 2).dash == Dash::new(vec![2.0, 2.0], 1.0),
        "a neighbour is untouched"
    );

    let bytes = save(&s);
    let again = reopened_path(&bytes, 0);
    assert_eq!(again.line_width, 3.5);
    assert_eq!(again.dash, Dash::new(vec![5.0, 2.0], 1.0));
    assert!((again.stroke_alpha - 0.4).abs() < 1e-6);
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("/pdfcerGS"),
        "the alpha is bound under a fresh name"
    );
    assert!(text.contains("/GS0"), "the existing entry is kept");
}

#[test]
fn width_alone_binds_no_ext_gstate() {
    let mut s = session(&fixture());
    let style = StrokeStyle {
        width: Some(0.0),
        ..StrokeStyle::default()
    };
    s.set_object_stroke_style(0, &[3], &style).unwrap();
    assert_eq!(path(&mut s, 3).line_width, 0.0);
    assert!(!String::from_utf8_lossy(&save(&s)).contains("/pdfcerGS"));
}

#[test]
fn one_undo_entry_restores_the_bytes() {
    let mut s = session(&fixture());
    let before = save(&s);
    s.set_object_stroke_style(0, &[0, 2], &full_style())
        .unwrap();
    assert_eq!(s.undo_depth(), 1);
    s.undo().unwrap();
    assert_eq!(save(&s), before);
}

#[test]
fn text_is_refused_by_index_and_the_path_beside_it_still_changes() {
    let mut s = session(&fixture());
    let out = s
        .set_object_stroke_style(0, &[1, 0], &full_style())
        .unwrap();
    assert_eq!(out.changed, vec![0]);
    assert_eq!(out.refused.len(), 1);
    assert_eq!(out.refused[0].object, 1);
    assert_eq!(out.refused[0].reason, PaintRefusalReason::NotAPath);
}

#[test]
fn invalid_values_change_nothing() {
    let mut s = session(&fixture());
    for bad in [
        StrokeStyle {
            width: Some(-1.0),
            ..StrokeStyle::default()
        },
        StrokeStyle {
            stroke_alpha: Some(1.01),
            ..StrokeStyle::default()
        },
        StrokeStyle {
            dash: Some(Dash::new(vec![0.0], 0.0)),
            ..StrokeStyle::default()
        },
        StrokeStyle {
            dash: Some(Dash::new(vec![1.0, -1.0], 0.0)),
            ..StrokeStyle::default()
        },
    ] {
        let err = s.set_object_stroke_style(0, &[0], &bad).unwrap_err();
        assert!(
            matches!(
                err,
                EditError::VectorEdit(VectorEditError::InvalidStrokeStyle { .. })
            ),
            "{err:?}"
        );
    }
    assert!(s.set_object_stroke_style(0, &[99], &full_style()).is_err());
    assert_eq!(s.undo_depth(), 0);
}
