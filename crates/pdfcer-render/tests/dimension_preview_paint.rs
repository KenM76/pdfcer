//! `paint_dimension_preview` paints the pixels the committed ce dimension
//! paints: preview a placement, commit it, render the saved page, compare.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::dimension::{DEFAULT_GROUP_ID, DimensionKind};
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::vector::{AxisConstraint, Point};
use pdfcer_core::writer::SaveOptions;
use pdfcer_render::edit_preview::paint_dimension_preview;
use pdfcer_render::tiny_skia::{Color, Pixmap};
use pdfcer_render::{RenderOptions, page_device_geometry, render_page_with};

fn blank_page_doc() -> Document {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 300] /Resources << >> >>",
    ];
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 4\n0000000000 65535 f \n");
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    Document::from_bytes(buf).unwrap()
}

fn linear(offset: f64, text_along: f64) -> DimensionKind {
    DimensionKind::Linear {
        a: Point::new(100.0, 150.0),
        b: Point::new(300.0, 150.0),
        constraint: AxisConstraint::Horizontal,
        offset,
        text_along,
    }
}

#[test]
fn a_ce_dimension_preview_paints_what_the_commit_renders() {
    let mut s = EditSession::new(blank_page_doc());
    let (_, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, linear(0.0, 0.0))
        .unwrap();
    let scale = 2.0;
    let options = RenderOptions::default();

    let preview = s.dimension_preview(id, &linear(25.0, 35.0)).unwrap();
    let page = s.pages().unwrap().remove(0);
    let (w, h, to_device) = page_device_geometry(&page, scale);
    let mut painted = Pixmap::new(w, h).unwrap();
    painted.fill(Color::WHITE);
    let diag = paint_dimension_preview(&s.view(), &preview, &options, to_device, &mut painted);
    assert!(diag.sample_ops.is_empty(), "{:?}", diag.sample_ops);

    s.place_dimension(id, 25.0, 35.0).unwrap();
    let saved = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let doc = Document::from_bytes(saved).unwrap();
    let page = pdfcer_core::page_tree::pages(&doc).unwrap().remove(0);
    let rendered = render_page_with(&doc, &page, scale, &options).unwrap();

    let ink = |p: &[u8]| p.chunks_exact(4).filter(|px| px[0] < 0xF0).count();
    assert!(ink(painted.data()) > 200, "the preview painted nothing");
    let differing = painted
        .data()
        .chunks_exact(4)
        .zip(rendered.pixmap.data().chunks_exact(4))
        .filter(|(a, b)| a.iter().zip(*b).any(|(x, y)| x.abs_diff(*y) > 2))
        .count();
    assert_eq!(differing, 0, "preview and committed render differ");
}
