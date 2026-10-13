//! `StrokeUnits::Page` (pdfcer-gui request G174): a stroke width and dash
//! given in points land as the same page length on paths drawn under
//! different CTM scales, on the page and inside a form, and read back
//! through `PathObject::line_width_pt` / `dash_pt` (Pass 576.0).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree;
use pdfcer_core::vector::{
    Dash, Matrix, PathObject, StrokeStyle, StrokeUnits, VectorObject, decompose_page,
};
use pdfcer_core::writer::SaveOptions;

/// Page objects: 0 at scale 1, 1 at scale 2, 2 rotated 90° at scale 2,
/// 3 stretched 3 by 1. Leaf 0 (after them) is a path in `Fm0`, drawn at 4.
fn fixture() -> Vec<u8> {
    let page = "0 0 20 20 re S\n\
                q 2 0 0 2 30 0 cm 0 0 5 5 re S Q\n\
                q 0 2 -2 0 90 0 cm 0 0 5 5 re S Q\n\
                q 3 0 0 1 60 30 cm 0 0 5 5 re S Q\n\
                q 4 0 0 4 10 50 cm /Fm0 Do Q\n";
    let fm0 = "0 0 5 5 re S\n";
    let stream = |dict: &str, body: &str| {
        format!(
            "<< {dict} /Length {} >>\nstream\n{body}endstream",
            body.len()
        )
    };
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /XObject \
         << /Fm0 5 0 R >> >> /Contents 4 0 R >>"
            .to_owned(),
        stream("", page),
        stream("/Type /XObject /Subtype /Form /BBox [0 0 10 10]", fm0),
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

fn path(s: &mut EditSession, i: usize) -> PathObject {
    match &s.page_objects(0).unwrap().objects[i] {
        VectorObject::Path(p) => p.clone(),
        other => panic!("object {i} is not a path: {other:?}"),
    }
}

fn leaf(s: &mut EditSession, i: usize) -> PathObject {
    match &s.page_objects(0).unwrap().leaves[i].object {
        VectorObject::Path(p) => p.clone(),
        other => panic!("leaf {i} is not a path: {other:?}"),
    }
}

/// Decompose page 0 of `s` saved and reopened.
fn reopened(s: &EditSession, i: usize) -> PathObject {
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    let model = decompose_page(&doc.view(), &pages[0], Matrix::IDENTITY).unwrap();
    match &model.objects[i] {
        VectorObject::Path(p) => p.clone(),
        other => panic!("object {i} is not a path: {other:?}"),
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-4
}

fn in_points(width: f64, dash: Option<Dash>) -> StrokeStyle {
    StrokeStyle {
        width: Some(width),
        dash,
        units: StrokeUnits::Page,
        ..StrokeStyle::default()
    }
}

#[test]
fn the_readers_measure_each_paths_scale() {
    let mut s = session();
    let scales: Vec<f64> = (0..4).map(|i| path(&mut s, i).stroke_scale()).collect();
    assert!(close(scales[0], 1.0) && close(scales[1], 2.0) && close(scales[2], 2.0));
    assert!(close(scales[3], 3f64.sqrt()), "geometric mean: {scales:?}");
    let uniform: Vec<bool> = (0..4)
        .map(|i| path(&mut s, i).stroke_scale_is_uniform())
        .collect();
    assert_eq!(uniform, [true, true, true, false], "rotation is uniform");
    assert!(
        close(path(&mut s, 1).line_width_pt(), 2.0),
        "1 unit at scale 2"
    );
}

#[test]
fn one_width_in_points_strokes_every_scale_alike() {
    let mut s = session();
    let dash = Dash::new(vec![4.0, 2.0], 1.0);
    let out = s
        .set_object_stroke_style(0, &[0, 1, 2], &in_points(5.0, Some(dash.clone())))
        .unwrap();
    assert_eq!(out.changed, vec![0, 1, 2]);
    assert!(out.disclosures.is_empty(), "{:?}", out.disclosures);
    for i in 0..3 {
        let p = path(&mut s, i);
        assert!(close(p.line_width_pt(), 5.0), "object {i}: {p:?}");
        let pt = p.dash_pt();
        assert!(
            pt.array.len() == 2 && close(pt.array[0], 4.0) && close(pt.array[1], 2.0),
            "object {i}: {pt:?}"
        );
        assert!(close(pt.phase, 1.0));
    }
    assert!(
        close(path(&mut s, 1).line_width, 2.5),
        "written in user space"
    );
    assert!(close(reopened(&s, 2).line_width_pt(), 5.0), "saved");
}

#[test]
fn a_stretched_path_gets_the_mean_and_a_disclosure() {
    let mut s = session();
    let out = s
        .set_object_stroke_style(0, &[0, 3], &in_points(6.0, None))
        .unwrap();
    assert_eq!(out.disclosures.len(), 1, "{:?}", out.disclosures);
    assert!(out.disclosures[0].contains("1 path(s)"));
    assert!(close(path(&mut s, 3).line_width_pt(), 6.0));
}

#[test]
fn user_space_stays_the_default() {
    let mut s = session();
    let style = StrokeStyle {
        width: Some(5.0),
        ..StrokeStyle::default()
    };
    s.set_object_stroke_style(0, &[1], &style).unwrap();
    let p = path(&mut s, 1);
    assert!(close(p.line_width, 5.0) && close(p.line_width_pt(), 10.0));
}

#[test]
fn points_inside_a_form_use_the_placements_scale() {
    let mut s = session();
    let out = s
        .set_object_stroke_style_in_form(0, &[0], &in_points(2.0, None))
        .unwrap();
    assert_eq!(out.paint.changed, vec![0]);
    let p = leaf(&mut s, 0);
    assert!(close(p.stroke_scale(), 4.0), "{p:?}");
    assert!(close(p.line_width_pt(), 2.0), "{p:?}");
    assert!(close(p.line_width, 0.5), "form units");
}
