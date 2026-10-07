//! `EditSession::set_object_paint_in_form` and
//! `set_object_stroke_style_in_form` (pdfcer-gui request G142): colour, width,
//! dash and opacity for paths inside a form XObject, addressed by leaf index.

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, PaintRefusalReason};
use pdfcer_core::vector::{PathObject, PathPaint, Rgb, StrokeStyle, VectorObject};
use pdfcer_core::writer::SaveOptions;

/// Leaves 0 and 1: stroked paths in `Fm0`, which has DIRECT `/Resources`.
/// Leaf 2: text in `Fm0`. Leaf 3: a path in `Fm1`, which has no `/Resources`.
fn fixture() -> Vec<u8> {
    let page = "q 1 0 0 1 10 10 cm /Fm0 Do Q\nq 1 0 0 1 50 50 cm /Fm1 Do Q\n";
    let fm0 = "1 0 0 RG 0 0 10 10 re S\n20 0 m 30 10 l S\nBT /F1 12 Tf 0 20 Td (A) Tj ET\n";
    let fm1 = "0 0 5 5 re S\n";
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
         << /Fm0 5 0 R /Fm1 6 0 R >> >> /Contents 4 0 R >>"
            .to_owned(),
        stream("", page),
        stream(
            "/Type /XObject /Subtype /Form /BBox [0 0 40 40] /Resources << /Font << /F1 7 0 R \
             >> >>",
            fm0,
        ),
        stream("/Type /XObject /Subtype /Form /BBox [0 0 10 10]", fm1),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
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

fn reopened(s: &EditSession) -> EditSession {
    session(&s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0)
}

fn leaf(s: &mut EditSession, i: usize) -> PathObject {
    match &s.page_objects(0).unwrap().leaves[i].object {
        VectorObject::Path(p) => p.clone(),
        other => panic!("leaf {i} is not a path: {other:?}"),
    }
}

const BLUE: Rgb = Rgb {
    r: 0.0,
    g: 0.0,
    b: 1.0,
};

fn is_blue(p: &PathPaint) -> bool {
    matches!(p, PathPaint::Device { rgb, .. } if *rgb == BLUE)
}

#[test]
fn paint_recolours_form_paths_and_refuses_the_text_by_leaf_index() {
    let mut s = session(&fixture());
    let out = s
        .set_object_paint_in_form(0, &[0, 1, 2], None, Some(BLUE))
        .unwrap();
    assert_eq!(out.paint.changed, vec![0, 1]);
    assert_eq!(out.paint.refused.len(), 1);
    assert_eq!(out.paint.refused[0].object, 2, "a LEAF index, as passed in");
    assert_eq!(out.paint.refused[0].reason, PaintRefusalReason::NotAPath);
    assert_eq!(out.reach.as_ref().unwrap().invocations, 1);

    assert!(is_blue(&leaf(&mut s, 0).stroke_paint));
    assert!(is_blue(&leaf(&mut s, 1).stroke_paint));
    assert!(!is_blue(&leaf(&mut s, 3).stroke_paint), "the other form");
    assert!(is_blue(&leaf(&mut reopened(&s), 1).stroke_paint), "saved");

    s.undo().unwrap();
    assert!(!is_blue(&leaf(&mut s, 0).stroke_paint), "one undo step");
}

#[test]
fn stroke_style_with_alpha_binds_in_the_forms_direct_resources() {
    let mut s = session(&fixture());
    let style = StrokeStyle {
        width: Some(3.0),
        stroke_alpha: Some(0.5),
        ..StrokeStyle::default()
    };
    let out = s.set_object_stroke_style_in_form(0, &[1], &style).unwrap();
    assert_eq!(out.paint.changed, vec![1]);

    // Survives a save: the /ExtGState must be reachable from the form, so
    // the direct /Resources went into the form's own dictionary.
    let mut r = reopened(&s);
    let p = leaf(&mut r, 1);
    assert!((p.line_width - 3.0).abs() < 1e-9, "{}", p.line_width);
    assert_eq!(p.stroke_alpha, 0.5);
    let untouched = leaf(&mut r, 0);
    assert!((untouched.line_width - 1.0).abs() < 1e-9);
    assert_eq!(untouched.stroke_alpha, 1.0);

    s.undo().unwrap();
    assert_eq!(leaf(&mut s, 1).stroke_alpha, 1.0);
}

#[test]
fn alpha_in_a_form_without_resources_is_refused_but_width_works() {
    let mut s = session(&fixture());
    let alpha = StrokeStyle {
        fill_alpha: Some(0.25),
        ..StrokeStyle::default()
    };
    let err = s
        .set_object_stroke_style_in_form(0, &[3], &alpha)
        .unwrap_err();
    assert!(
        matches!(err, EditError::FormInheritsResources { .. }),
        "{err:?}"
    );
    assert!(s.undo_kind().is_none(), "nothing committed");

    let width = StrokeStyle {
        width: Some(2.0),
        ..StrokeStyle::default()
    };
    s.set_object_stroke_style_in_form(0, &[3], &width).unwrap();
    assert!((leaf(&mut s, 3).line_width - 2.0).abs() < 1e-9);
}

#[test]
fn a_selection_across_two_forms_is_refused_whole() {
    let mut s = session(&fixture());
    let err = s
        .set_object_paint_in_form(0, &[0, 3], Some(BLUE), None)
        .unwrap_err();
    assert!(
        matches!(err, EditError::FormLeafSelectionSpansForms { .. }),
        "{err:?}"
    );
    let err = s
        .set_object_paint_in_form(0, &[9], Some(BLUE), None)
        .unwrap_err();
    assert!(
        matches!(err, EditError::FormLeafOutOfRange { .. }),
        "{err:?}"
    );
    assert!(s.undo_kind().is_none());
}

#[test]
fn nothing_to_set_changes_nothing() {
    let mut s = session(&fixture());
    let out = s.set_object_paint_in_form(0, &[0], None, None).unwrap();
    assert!(out.paint.changed.is_empty() && out.reach.is_none());
    let out = s
        .set_object_stroke_style_in_form(0, &[2], &StrokeStyle::default())
        .unwrap();
    assert_eq!(out.paint.refused.len(), 1);
    assert!(s.undo_kind().is_none());
}

/// The request's own case: a Line markup flattened into the page becomes a
/// leaf of a form, and its colour and opacity must still be settable.
#[test]
fn a_flattened_line_markup_can_be_recoloured_and_made_translucent() {
    use pdfcer_core::annot_author::{Color, LineEnding, MarkupSpec};
    let mut s = session(&fixture());
    let id = s
        .add_markup(
            0,
            &MarkupSpec::Line {
                start: (20.0, 20.0),
                end: (80.0, 70.0),
                color: Color::Gray(0.0),
                width: 1.0,
                endings: (LineEnding::None, LineEnding::None),
            },
        )
        .unwrap();
    s.flatten_annotations(0, Some(&[id])).unwrap();
    let last = s.page_objects(0).unwrap().leaves.len() - 1;

    let out = s
        .set_object_paint_in_form(0, &[last], None, Some(BLUE))
        .unwrap();
    assert_eq!(out.paint.changed, vec![last], "{:?}", out.paint.refused);
    let style = StrokeStyle {
        stroke_alpha: Some(0.5),
        ..StrokeStyle::default()
    };
    s.set_object_stroke_style_in_form(0, &[last], &style)
        .unwrap();
    let p = leaf(&mut reopened(&s), last);
    assert!(is_blue(&p.stroke_paint));
    assert_eq!(p.stroke_alpha, 0.5);
}
