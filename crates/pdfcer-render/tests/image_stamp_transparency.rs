//! An image stamp keeps its PNG's alpha: author → save → reload → paint, and
//! the stamp's clear half shows the page, not a box.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, MarkupOptions};
use pdfcer_core::image_import;
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_core::writer::SaveOptions;
use pdfcer_render::{RenderOptions, render_page_with};

fn blank_page_doc() -> Document {
    let mut buf = b"%PDF-1.7\n".to_vec();
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 120] /Resources << >> >>",
        "<< /Type /Page /Parent 2 0 R >>",
    ];
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    Document::from_bytes(buf).unwrap()
}

#[test]
fn an_image_stamp_is_transparent_where_its_png_was() {
    let png = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/synthetic/images/rgba-half-clear.png"),
    )
    .unwrap();
    let image = image_import::import(&png).unwrap();
    let rect = Rect {
        llx: 20.0,
        lly: 20.0,
        urx: 68.0,
        ury: 44.0,
    };
    let mut session = EditSession::new(blank_page_doc());
    session
        .add_image_stamp(0, rect, &image, &MarkupOptions::default())
        .unwrap();
    let (bytes, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap();
    let doc = Document::from_bytes(bytes).unwrap();
    let page = page_tree::pages(&doc).unwrap().remove(0);
    let pm = render_page_with(
        &doc,
        &page,
        2.0,
        &RenderOptions::default().with_annotations(true),
    )
    .unwrap()
    .pixmap;
    // Device pixel of a page point at scale 2 on a 120 pt tall page.
    let at = |x: f32, y: f32| {
        let px = pm
            .pixel((x * 2.0) as u32, ((120.0 - y) * 2.0) as u32)
            .unwrap();
        (px.red(), px.green(), px.blue())
    };
    for y in [24.0, 32.0, 40.0] {
        let (r, g, b) = at(30.0, y);
        assert!(
            r > 0xF0 && g < 0x10 && b < 0x10,
            "opaque half is red: {r} {g} {b}"
        );
        assert_eq!(at(58.0, y), (0xFF, 0xFF, 0xFF), "clear half shows the page");
    }
}
