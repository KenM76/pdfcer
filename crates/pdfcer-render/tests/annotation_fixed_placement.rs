//! `NoRotate` and `NoZoom` (§12.5.3), and a `/Text` annotation behaving as
//! if both were set (§12.5.6.4): the appearance keeps `/Rect`'s upper-left
//! corner where the page puts it and turns or scales about that point.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{RenderOptions, render_page_with};

/// One 200 x 100 page turned by `rotate`, carrying one annotation of
/// `subtype` with flags `f` over `/Rect [50 20 90 40]`, whose appearance
/// fills its whole 40 x 20 `/BBox` black.
fn doc(rotate: i32, subtype: &str, f: u32) -> Document {
    let ap = "0 g 0 0 40 20 re f";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Rotate {rotate} /Annots [4 0 R] >>"
        ),
        format!(
            "<< /Type /Annot /Subtype /{subtype} /F {f} /Rect [50 20 90 40] /AP << /N 5 0 R >> >>"
        ),
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 40 20] /Length {} >>\nstream\n{ap}\nendstream",
            ap.len()
        ),
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    let n = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {n}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    Document::from_bytes(out).unwrap()
}

/// The inked pixels' bounds `[left, top, right, bottom]` (exclusive).
fn ink(d: &Document, scale: f32, options: &RenderOptions) -> [u32; 4] {
    let pages = page_tree::pages(d).unwrap();
    let page = render_page_with(d, &pages[0], scale, options).unwrap();
    let pm = &page.pixmap;
    let mut b = [u32::MAX, u32::MAX, 0, 0];
    for y in 0..pm.height() {
        for x in 0..pm.width() {
            let px = pm.pixel(x, y).unwrap();
            if px.red() < 128 {
                b = [b[0].min(x), b[1].min(y), b[2].max(x + 1), b[3].max(y + 1)];
            }
        }
    }
    assert!(b[0] < b[2], "nothing inked");
    b
}

const NO_ZOOM: u32 = 8;
const NO_ROTATE: u32 = 16;

#[test]
fn no_rotate_keeps_the_appearance_upright_on_a_turned_page() {
    let opts = RenderOptions::default();
    let turned = ink(&doc(90, "Square", 0), 1.0, &opts);
    let upright = ink(&doc(90, "Square", NO_ROTATE), 1.0, &opts);
    // Turned with the page: 20 wide, 40 tall.
    assert_eq!([turned[2] - turned[0], turned[3] - turned[1]], [20, 40]);
    // Upright: 40 wide, 20 tall.
    assert_eq!([upright[2] - upright[0], upright[3] - upright[1]], [40, 20]);
    // A clockwise quarter turn carries the /Rect upper-left corner to the
    // turned box's upper-right; NoRotate keeps it there, as the upright
    // box's upper-left.
    assert_eq!((upright[0], upright[1]), (turned[2], turned[1]));
}

#[test]
fn no_rotate_changes_nothing_on_an_unrotated_page() {
    let opts = RenderOptions::default();
    assert_eq!(
        ink(&doc(0, "Square", NO_ROTATE), 2.0, &opts),
        ink(&doc(0, "Square", 0), 2.0, &opts)
    );
}

#[test]
fn a_text_annotation_stays_upright_without_either_flag() {
    let b = ink(&doc(270, "Text", 0), 1.0, &RenderOptions::default());
    assert_eq!([b[2] - b[0], b[3] - b[1]], [40, 20]);
}

#[test]
fn no_zoom_keeps_the_100_percent_size_at_a_viewer_magnification() {
    let viewer = RenderOptions::default().with_view_magnification(3.0);
    let fixed = ink(&doc(0, "Square", NO_ZOOM), 3.0, &viewer);
    let zoomed = ink(&doc(0, "Square", 0), 3.0, &viewer);
    assert_eq!([zoomed[2] - zoomed[0], zoomed[3] - zoomed[1]], [120, 60]);
    assert_eq!([fixed[2] - fixed[0], fixed[3] - fixed[1]], [40, 20]);
    // Anchored at the /Rect upper-left corner.
    assert_eq!((fixed[0], fixed[1]), (zoomed[0], zoomed[1]));
}

#[test]
fn no_zoom_without_a_viewer_magnification_scales_with_the_page() {
    // No magnification is the print answer: 100 %, so the flag has
    // nothing to divide out.
    let opts = RenderOptions::default();
    assert_eq!(
        ink(&doc(0, "Square", NO_ZOOM), 3.0, &opts),
        ink(&doc(0, "Square", 0), 3.0, &opts)
    );
}
