//! `white_margin_crop`: the box around what a page paints, from outside the
//! crate, as a shell calls it.

use pdfcer_core::document::Document;
use pdfcer_core::page_tree::{self, Page, Rect};
use pdfcer_render::{WhiteMarginError, white_margin_crop};

/// One 200×100 page with `content` and extra page attributes.
fn one_page(content: &str, page_extra: &str) -> (Document, Page) {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R {page_extra} >>"
        ),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len() + 1
        ),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f\r\n");
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    let doc = Document::from_bytes(buf).expect("parse");
    let page = page_tree::pages(&doc).expect("pages").remove(0);
    (doc, page)
}

fn close(a: Rect, b: [f64; 4], tol: f64) -> bool {
    [a.llx - b[0], a.lly - b[1], a.urx - b[2], a.ury - b[3]]
        .iter()
        .all(|d| d.abs() <= tol)
}

const INK: &str = "0 g 50 20 100 60 re f";

#[test]
fn the_box_fits_the_ink_within_a_pixel() {
    let (doc, page) = one_page(INK, "");
    let found = white_margin_crop(&doc.view(), &page, 0.0)
        .expect("render")
        .expect("ink found");
    assert!(
        close(found.content, [50.0, 20.0, 150.0, 80.0], 0.5),
        "{:?}",
        found.content
    );
    assert_eq!(found.crop, found.content);
}

#[test]
fn the_margin_grows_the_crop_and_stops_at_the_sheet() {
    let (doc, page) = one_page(INK, "");
    let found = white_margin_crop(&doc.view(), &page, 30.0)
        .expect("render")
        .expect("ink found");
    assert!(
        close(found.crop, [20.0, 0.0, 180.0, 100.0], 0.5),
        "{:?}",
        found.crop
    );
}

#[test]
fn a_rotated_page_maps_back_to_user_space() {
    for rotate in [90, 180, 270] {
        let (doc, page) = one_page(INK, &format!("/Rotate {rotate}"));
        let found = white_margin_crop(&doc.view(), &page, 0.0)
            .expect("render")
            .expect("ink found");
        assert!(
            close(found.content, [50.0, 20.0, 150.0, 80.0], 0.5),
            "/Rotate {rotate}: {:?}",
            found.content
        );
    }
}

#[test]
fn a_blank_page_has_no_box() {
    let (doc, page) = one_page("", "");
    assert_eq!(
        white_margin_crop(&doc.view(), &page, 6.0).expect("render"),
        None
    );
}

#[test]
fn near_white_counts_as_paper_and_light_grey_as_content() {
    let (doc, page) = one_page("0.99 g 0 0 200 100 re f", "");
    assert_eq!(
        white_margin_crop(&doc.view(), &page, 0.0).expect("render"),
        None
    );
    let (doc, page) = one_page("0.9 g 0 0 200 100 re f", "");
    let found = white_margin_crop(&doc.view(), &page, 0.0)
        .expect("render")
        .expect("grey is content");
    assert!(
        close(found.content, [0.0, 0.0, 200.0, 100.0], 0.5),
        "{:?}",
        found.content
    );
}

#[test]
fn a_negative_or_non_finite_margin_is_refused() {
    let (doc, page) = one_page(INK, "");
    for bad in [-1.0, f64::NAN, f64::INFINITY] {
        let got = white_margin_crop(&doc.view(), &page, bad);
        assert!(matches!(got, Err(WhiteMarginError::BadMargin(_))), "{bad}");
    }
}
