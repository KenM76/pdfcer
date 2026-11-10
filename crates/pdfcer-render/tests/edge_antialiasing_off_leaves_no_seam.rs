//! Two filled shapes that touch leave a seam when their edges are
//! antialiased, and none with [`EdgeAntialiasing::Off`].
//!
//! Each shape covers half of the shared column, and source-over of two half
//! coverages is `0.5 + 0.5·(1 − 0.5) = 0.75`: the page shows through a join
//! the document does not contain. Seen on paper in conformance pages built
//! from knockout tiles, at print resolution where the smoothing itself is
//! invisible.

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::{EdgeAntialiasing, RenderOptions, render_page_with};

/// Two red rectangles sharing the edge x = 40.2, which lands mid-pixel at
/// scale 2.5 (device x = 100.5) so an integer-aligned join cannot hide the
/// seam.
fn two_abutting_red_rectangles() -> Document {
    let content = "1 0 0 rg 0 0 40.2 100 re f 40.2 0 59.8 100 re f\n";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << >> >>".to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ),
    ];
    let mut out = String::from("%PDF-1.7\n");
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.push_str(&format!("{} 0 obj\n{body}\nendobj\n", i + 1));
    }
    let xref_at = out.len();
    out.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for off in offsets {
        out.push_str(&format!("{off:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
        objects.len() + 1
    ));
    Document::from_bytes(out.into_bytes()).expect("fixture must load")
}

/// The green channel along the middle row: 0 is solid red, 255 white page.
fn join_column_green(edges: EdgeAntialiasing) -> u8 {
    let doc = two_abutting_red_rectangles();
    let page = page_tree::pages(&doc).expect("page tree walks").remove(0);
    let options = RenderOptions::default().with_edge_antialiasing(edges);
    let rendered = render_page_with(&doc, &page, 2.5, &options).expect("the page renders");
    let pm = &rendered.pixmap;
    let y = pm.height() / 2;
    (0..pm.width())
        .filter_map(|x| pm.pixel(x, y))
        .map(|px| px.green())
        .max()
        .expect("the row has pixels")
}

#[test]
fn antialiased_edges_leave_a_seam_where_two_shapes_touch() {
    // The control: without it, a pass of the test below could mean the
    // fixture never produced a seam at all.
    assert!(
        join_column_green(EdgeAntialiasing::On) > 40,
        "the fixture no longer shows the seam it exists to reproduce"
    );
}

#[test]
fn hard_edges_leave_none() {
    assert_eq!(join_column_green(EdgeAntialiasing::Off), 0);
}
