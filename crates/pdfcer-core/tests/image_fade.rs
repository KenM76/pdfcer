//! Fading images and forms (pdfcer-gui request G155): `set_object_stroke_style`
//! and its in-form twin wrap an image or form's `Do` (or inline image) in
//! `q /GS gs … Q` carrying the alpha, and never the width or dash.

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, PaintRefusalReason};
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::vector::{
    Dash, ImageObject, ImageSource, Matrix, StrokeStyle, VectorObject, decompose_page,
};
use pdfcer_core::writer::SaveOptions;

/// Objects: 0 image XObject, 1 form (a filled square and image `/Im2`),
/// 2 inline image, 3 a stroked path.
fn fixture() -> Vec<u8> {
    let page = "q 50 0 0 50 0 0 cm /Im1 Do Q\nq 1 0 0 1 60 0 cm /Fm1 Do Q\n\
                q 10 0 0 10 0 60 cm BI /W 1 /H 1 /BPC 8 /CS /G ID A EI Q\n\
                0 0 m 10 10 l S\n";
    let form = "0 0 10 10 re f\nq 5 0 0 5 0 0 cm /Im2 Do Q\n";
    let image = "<< /Type /XObject /Subtype /Image /Width 1 /Height 1 /BitsPerComponent 8 \
                 /ColorSpace /DeviceGray /Length 1 >>\nstream\nA\nendstream";
    let objs = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << /XObject \
         << /Im1 5 0 R /Fm1 6 0 R >> >> /Contents 4 0 R >>"
            .to_owned(),
        format!("<< /Length {} >>\nstream\n{page}endstream", page.len()),
        image.to_owned(),
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 10 10] /Resources << /XObject \
             << /Im2 7 0 R >> >> /Length {} >>\nstream\n{form}endstream",
            form.len()
        ),
        image.to_owned(),
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

fn reopened_objects(s: &EditSession) -> Vec<VectorObject> {
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    decompose_page(&doc.view(), &pages[0], Matrix::IDENTITY)
        .unwrap()
        .objects
}

fn image(objs: &[VectorObject], i: usize) -> ImageObject {
    match &objs[i] {
        VectorObject::Image(img) => *img,
        other => panic!("object {i} is not an image: {other:?}"),
    }
}

fn page_content(s: &EditSession) -> String {
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    let content_ref = Object::Reference(pages[0].contents[0]);
    let Object::Stream(st) = doc.resolve(&content_ref) else {
        panic!("no content stream");
    };
    let raw = st.data_span.slice(doc.bytes()).unwrap();
    String::from_utf8_lossy(&pdfcer_core::filters::decode_stream(&st.dict, raw).unwrap())
        .into_owned()
}

fn fade(a: f64) -> StrokeStyle {
    StrokeStyle {
        fill_alpha: Some(a),
        ..StrokeStyle::default()
    }
}

#[test]
fn the_fixture_decomposes_as_documented() {
    let objs = session().page_objects(0).unwrap().objects.clone();
    assert_eq!(image(&objs, 0).source, ImageSource::XObject);
    assert_eq!(image(&objs, 1).source, ImageSource::Form);
    assert_eq!(image(&objs, 2).source, ImageSource::Inline);
    assert!(matches!(objs[3], VectorObject::Path(_)));
    assert_eq!(image(&objs, 0).fill_alpha, 1.0);
}

#[test]
fn a_fill_alpha_fades_images_forms_and_paths_and_reads_back() {
    let mut s = session();
    let out = s
        .set_object_stroke_style(0, &[0, 1, 2, 3], &fade(0.4))
        .unwrap();
    assert_eq!(out.changed, vec![0, 1, 2, 3]);
    assert!(out.refused.is_empty());
    let objs = reopened_objects(&s);
    for i in 0..3 {
        let img = image(&objs, i);
        assert!((img.fill_alpha - 0.4).abs() < 1e-9, "{i}: {img:?}");
        assert_eq!(img.stroke_alpha, 1.0, "{i}");
    }
    let content = page_content(&s);
    assert!(
        content.contains("cm q /pdfcerGS1 gs /Im1 Do Q Q"),
        "{content}"
    );
}

#[test]
fn width_and_dash_never_reach_an_image_or_form() {
    let mut s = session();
    let style = StrokeStyle {
        width: Some(3.0),
        dash: Some(Dash::new(vec![2.0, 1.0], 0.0)),
        fill_alpha: Some(0.5),
        ..StrokeStyle::default()
    };
    s.set_object_stroke_style(0, &[0, 1, 3], &style).unwrap();
    let content = page_content(&s);
    assert_eq!(
        content.matches("3 w").count(),
        1,
        "the path only: {content}"
    );
    assert_eq!(content.matches("[2 1] 0 d").count(), 1, "{content}");
}

#[test]
fn a_style_an_image_cannot_read_is_refused_by_index() {
    let mut s = session();
    let width = StrokeStyle {
        width: Some(2.0),
        ..StrokeStyle::default()
    };
    let out = s.set_object_stroke_style(0, &[0, 1, 3], &width).unwrap();
    assert_eq!(out.changed, vec![3]);
    assert_eq!(out.refused.len(), 2);
    assert!(
        out.refused
            .iter()
            .all(|r| r.reason == PaintRefusalReason::NotAPath)
    );

    // A stroke alpha reaches a form's content, not an image's samples.
    let mut s = session();
    let stroke = StrokeStyle {
        stroke_alpha: Some(0.5),
        ..StrokeStyle::default()
    };
    let out = s.set_object_stroke_style(0, &[0, 1, 2], &stroke).unwrap();
    assert_eq!(out.changed, vec![1]);
    let refused: Vec<usize> = out.refused.iter().map(|r| r.object).collect();
    assert_eq!(refused, vec![0, 2]);
    assert!((image(&reopened_objects(&s), 1).stroke_alpha - 0.5).abs() < 1e-9);
}

#[test]
fn one_undo_restores_the_page() {
    let mut s = session();
    let before = page_content(&s);
    s.set_object_stroke_style(0, &[0, 2], &fade(0.3)).unwrap();
    assert_ne!(page_content(&s), before);
    s.undo().unwrap();
    assert_eq!(page_content(&s), before);
}

#[test]
fn an_image_inside_a_form_fades_through_the_in_form_twin() {
    let mut s = session();
    let im2 = ObjId::new(7, 0);
    let leaves = s.page_objects(0).unwrap().leaves.clone();
    let leaf = leaves
        .iter()
        .position(|l| matches!(&l.object, VectorObject::Image(i) if i.xobject == Some(im2)))
        .expect("the image inside the form is a leaf");
    let out = s
        .set_object_stroke_style_in_form(0, &[leaf], &fade(0.25))
        .unwrap();
    assert_eq!(out.paint.changed, vec![leaf]);
    let after = s.page_objects(0).unwrap().leaves.clone();
    let VectorObject::Image(img) = &after[leaf].object else {
        panic!("leaf {leaf} is no longer an image");
    };
    assert!((img.fill_alpha - 0.25).abs() < 1e-9, "{img:?}");
}
