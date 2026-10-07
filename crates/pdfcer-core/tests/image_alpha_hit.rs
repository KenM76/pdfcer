//! A click on a transparent part of an image falls through to what is drawn
//! underneath, and a rotated image is hit only inside its own parallelogram.
//!
//! Each fixture is a 100 x 100 pt page: a stroked line across y = 50, then a
//! 2 x 2 grey image over x, y 30..70 whose LEFT column is transparent and
//! RIGHT column opaque, by whichever mask the test names.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)] // Tests fail loudly.

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_core::vector::{
    DocumentImageAlpha, HitTarget, Matrix, Point, decompose_page, hit_test_point,
    hit_test_point_all_with, hit_test_point_deep_with, hit_test_point_with,
};

const LINE_THEN_IMAGE: &[u8] = b"0 50 m 100 50 l S q 40 0 0 40 30 30 cm /Im0 Do Q";
const GREY: &str = "/ColorSpace /DeviceGray /BitsPerComponent 8";

/// Object 5 is the image (`image_dict` + `samples`); object 6, if given, is
/// a mask stream `(dict, data)` the image dictionary may reference.
fn page(content: &[u8], image_dict: &str, samples: &[u8], mask: Option<(&str, &[u8])>) -> Vec<u8> {
    let mut objects: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R \
          /Resources << /XObject << /Im0 5 0 R >> >> >>"
            .to_vec(),
        stream("", content),
        stream(
            &format!("/Type /XObject /Subtype /Image {image_dict}"),
            samples,
        ),
    ];
    if let Some((dict, data)) = mask {
        objects.push(stream(
            &format!("/Type /XObject /Subtype /Image {dict}"),
            data,
        ));
    }
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        pdf.extend_from_slice(o);
        pdf.extend_from_slice(b"\nendobj\n");
    }
    let xref = pdf.len();
    let n = objects.len() + 1;
    pdf.extend_from_slice(format!("xref\n0 {n}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        pdf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    pdf
}

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut s = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    s.extend_from_slice(data);
    s.extend_from_slice(b"\nendstream");
    s
}

/// The topmost object index at each point, with masks decoded.
fn topmost(bytes: Vec<u8>, points: &[(f64, f64)]) -> Vec<Option<usize>> {
    let doc = Document::from_bytes(bytes).expect("synthetic page loads");
    let page = &page_tree::pages(&doc).expect("page tree")[0];
    let view = doc.view();
    let model = decompose_page(&view, page, Matrix::IDENTITY).expect("decomposes");
    let alpha = DocumentImageAlpha::new(&view);
    points
        .iter()
        .map(|&(x, y)| hit_test_point_with(&model, Point::new(x, y), 0.5, &alpha))
        .collect()
}

/// Line is object 0, image object 1: through the clear column the line wins,
/// on the opaque column the image does.
fn assert_left_clear(bytes: Vec<u8>) {
    assert_eq!(
        topmost(bytes, &[(40.0, 50.0), (60.0, 50.0)]),
        [Some(0), Some(1)]
    );
}

#[test]
fn a_zero_soft_mask_sample_lets_the_click_through() {
    let sm = format!("/Width 2 /Height 2 {GREY}");
    assert_left_clear(page(
        LINE_THEN_IMAGE,
        &format!("/Width 2 /Height 2 {GREY} /SMask 6 0 R"),
        &[90, 90, 90, 90],
        Some((&sm, &[0, 255, 0, 255])),
    ));
}

#[test]
fn a_soft_mask_decode_array_inverts_the_alpha() {
    let sm = format!("/Width 2 /Height 2 {GREY} /Decode [1 0]");
    assert_left_clear(page(
        LINE_THEN_IMAGE,
        &format!("/Width 2 /Height 2 {GREY} /SMask 6 0 R"),
        &[90, 90, 90, 90],
        Some((&sm, &[255, 0, 255, 0])),
    ));
}

#[test]
fn a_stencil_mask_one_sample_is_masked_out() {
    // 2 x 1, 1 bpc: left sample 1 (masked), right 0 (painted) — §8.9.6.2.
    assert_left_clear(page(
        LINE_THEN_IMAGE,
        &format!("/Width 2 /Height 2 {GREY} /Mask 6 0 R"),
        &[90, 90, 90, 90],
        Some(("/Width 2 /Height 1 /ImageMask true", &[0x80])),
    ));
}

#[test]
fn a_stencil_mask_decode_array_reverses_its_polarity() {
    assert_left_clear(page(
        LINE_THEN_IMAGE,
        &format!("/Width 2 /Height 2 {GREY} /Mask 6 0 R"),
        &[90, 90, 90, 90],
        Some(("/Width 2 /Height 1 /ImageMask true /Decode [1 0]", &[0x40])),
    ));
}

#[test]
fn a_colour_key_range_masks_its_samples() {
    assert_left_clear(page(
        LINE_THEN_IMAGE,
        &format!("/Width 2 /Height 2 {GREY} /Mask [0 10]"),
        &[0, 200, 5, 200],
        None,
    ));
}

#[test]
fn an_image_mask_is_hit_only_where_it_paints() {
    assert_left_clear(page(
        LINE_THEN_IMAGE,
        "/Width 2 /Height 1 /ImageMask true",
        &[0x80],
        None,
    ));
}

#[test]
fn an_undecodable_mask_keeps_the_image_clickable() {
    let sm = format!("/Width 2 /Height 2 {GREY} /Filter /FlateDecode");
    let bytes = page(
        LINE_THEN_IMAGE,
        &format!("/Width 2 /Height 2 {GREY} /SMask 6 0 R"),
        &[90, 90, 90, 90],
        Some((&sm, b"not a zlib stream")),
    );
    assert_eq!(topmost(bytes, &[(40.0, 50.0)]), [Some(1)]);
}

#[test]
fn the_geometry_only_queries_ignore_the_mask() {
    let sm = format!("/Width 2 /Height 2 {GREY}");
    let bytes = page(
        LINE_THEN_IMAGE,
        &format!("/Width 2 /Height 2 {GREY} /SMask 6 0 R"),
        &[90, 90, 90, 90],
        Some((&sm, &[0, 255, 0, 255])),
    );
    let doc = Document::from_bytes(bytes).expect("loads");
    let page = &page_tree::pages(&doc).expect("pages")[0];
    let view = doc.view();
    let model = decompose_page(&view, page, Matrix::IDENTITY).expect("decomposes");
    let at = Point::new(40.0, 50.0);
    assert_eq!(hit_test_point(&model, at, 0.5), Some(1));
    let alpha = DocumentImageAlpha::new(&view);
    assert_eq!(hit_test_point_all_with(&model, at, 0.5, &alpha), [0]);
    assert_eq!(
        hit_test_point_deep_with(&model, at, 0.5, &alpha),
        [HitTarget::Object(0)]
    );
}

#[test]
fn a_rotated_image_misses_at_its_bounding_box_corner() {
    // The unit square turned 45 degrees: a diamond with its bottom vertex at
    // (50, 10); its axis-aligned bbox spans x 21.7..78.3, y 10..56.6.
    let bytes = page(
        b"q 28.2843 28.2843 -28.2843 28.2843 50 10 cm /Im0 Do Q",
        &format!("/Width 1 /Height 1 {GREY}"),
        &[90],
        None,
    );
    assert_eq!(
        topmost(bytes, &[(23.0, 12.0), (50.0, 33.0), (50.0, 9.8)]),
        [None, Some(0), Some(0)]
    );
}

#[test]
fn the_tolerance_widens_the_image_edge() {
    let bytes = page(
        LINE_THEN_IMAGE,
        &format!("/Width 1 /Height 1 {GREY}"),
        &[90],
        None,
    );
    // 0.4 pt outside the right edge is within 0.5; 2 pt is not.
    assert_eq!(
        topmost(bytes, &[(70.4, 60.0), (72.0, 60.0)]),
        [Some(1), None]
    );
}
