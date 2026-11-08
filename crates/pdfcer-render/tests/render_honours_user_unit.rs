//! A page's `/UserUnit` (ISO 32000-1 §14.11.2 Table 30) scales its render.
//!
//! The boxes are in default user space units of `UserUnit / 72` inch, so a
//! 100 × 100 box with `/UserUnit 2` is 200 × 200 points physically, and a
//! render at `scale = dpi / 72` must size it that way on every route.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree::{self, Page, Rect};
use pdfcer_render::display_list::record_page;
use pdfcer_render::{
    MAX_PIXMAP_EDGE, RenderError, RenderOptions, RenderedPage, render_page_region, render_page_with,
};

/// A one-page file: a 100 × 100 crop box, a red square from (10, 10) to
/// (20, 20), and `user_unit` written verbatim as the `/UserUnit` value.
fn page_file(user_unit: &str) -> Vec<u8> {
    let content = "1 0 0 rg 10 10 10 10 re f\n";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Resources << >> \
             /Contents 4 0 R /UserUnit {user_unit} >>"
        ),
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    buf
}

fn open(user_unit: &str) -> (Document, Page) {
    let doc = Document::from_bytes(page_file(user_unit)).expect("fixture parses");
    let page = page_tree::pages(&doc).expect("page tree").remove(0);
    (doc, page)
}

fn size(r: &RenderedPage) -> (u32, u32) {
    (r.pixmap.width(), r.pixmap.height())
}

fn is_red(r: &RenderedPage, x: u32, y: u32) -> bool {
    let p = r.pixmap.pixel(x, y).expect("in bounds").demultiply();
    p.red() > 200 && p.green() < 60 && p.blue() < 60
}

#[test]
fn a_user_unit_of_two_doubles_the_raster() {
    let (doc, page) = open("2");
    assert!((page.user_unit - 2.0).abs() < f64::EPSILON);
    let options = RenderOptions::default();
    let full = render_page_with(&doc, &page, 1.0, &options).expect("render");
    assert_eq!(size(&full), (200, 200));
    let region = render_page_region(&doc.view(), &page, 1.0, page.crop_box, &options)
        .expect("region render");
    assert_eq!(size(&region), (200, 200));
}

#[test]
fn content_lands_at_twice_its_user_space_offset() {
    let (doc, page) = open("2");
    let r = render_page_with(&doc, &page, 1.0, &RenderOptions::default()).expect("render");
    // User (10..20, 10..20) → device x 20..40, y (100 - 20)·2 = 160..180.
    assert!(
        is_red(&r, 30, 170),
        "the square's centre at twice its offset"
    );
    assert!(!is_red(&r, 15, 170), "user x 7.5 is left of the square");
    assert!(!is_red(&r, 30, 185), "user y 7.5 is below the square");
}

#[test]
fn a_region_of_a_user_unit_page_is_the_same_pixels() {
    let (doc, page) = open("2");
    let region = Rect::from_corners(10.0, 10.0, 20.0, 20.0);
    let options = RenderOptions::default();
    let r = render_page_region(&doc.view(), &page, 1.0, region, &options).expect("region");
    assert_eq!(size(&r), (20, 20));
    assert!(is_red(&r, 10, 10));
    let list = record_page(&doc.view(), &page, 1.0, 1, &options).expect("record");
    let replayed = list.replay_region(list.key(), region).expect("replay");
    assert_eq!(
        size(&replayed),
        (20, 20),
        "a replay uses the recorded UserUnit"
    );
}

#[test]
fn an_unusable_user_unit_reads_as_one() {
    for written in ["0", "-1", "(2)", "/Two"] {
        let (doc, page) = open(written);
        assert!((page.user_unit - 1.0).abs() < f64::EPSILON, "{written}");
        let r = render_page_with(&doc, &page, 1.0, &RenderOptions::default()).expect("render");
        assert_eq!(size(&r), (100, 100), "/UserUnit {written}");
    }
}

#[test]
fn the_edge_cap_applies_after_the_user_unit() {
    // 100 units × 200 = 20,000 px: inside the cap at UserUnit 1, past it here.
    let (doc, page) = open("200");
    const { assert!(100 * 200 > MAX_PIXMAP_EDGE) };
    let err = render_page_with(&doc, &page, 1.0, &RenderOptions::default()).unwrap_err();
    assert!(matches!(err, RenderError::BadRasterSize { .. }), "{err}");
}
