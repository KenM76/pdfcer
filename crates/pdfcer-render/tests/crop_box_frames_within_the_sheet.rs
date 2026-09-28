//! A crop box larger than the media box frames the media box
//! (ISO 32000-2 §14.11.2.1: "a processor shall treat the box as its
//! intersection with the media box").

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree::{self, BoxResolution};
use pdfcer_render::render_page;

fn doc(page: &str, content: &str) -> Document {
    let mut buf = b"%PDF-1.7\n".to_vec();
    let stream = format!(
        "<< /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    );
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /Resources << >> >>",
        page,
        &stream,
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
fn an_oversized_crop_box_renders_the_sheet_not_beyond_it() {
    // Black fill over the whole oversized crop box.
    let d = doc(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 50] \
         /CropBox [-20 -20 300 300] /Contents 4 0 R >>",
        "0 g -20 -20 320 320 re f",
    );
    let pages = page_tree::pages(&d).unwrap();
    assert_eq!(pages[0].crop_box_resolution, BoxResolution::Clipped);
    let out = render_page(&d, &pages[0], 1.0).unwrap();
    assert_eq!((out.pixmap.width(), out.pixmap.height()), (100, 50));
    assert_eq!(out.diagnostics.page_crop_box, BoxResolution::Clipped);
}
