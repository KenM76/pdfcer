//! # After `apply_redactions`, the region renders WHITE (ISO 32000-1 §12.5.6.23)
//!
//! The one test in the workspace that checks redaction the way an auditor
//! would: not by reading the content stream, but by **rasterising the
//! output and looking at the pixels under the mark**. Every kind of content
//! pdfcer removes is placed so that it crosses the region — a thick diagonal
//! stroke, a wide Bézier, a filled rectangle, a fill-and-stroke, a filled
//! shape wholly inside, an even-odd ring, an image, text — and the mark
//! carries no `/IC`, so any ink that survives inside the region is a leak
//! the renderer will show and this test will catch.
//!
//! The complement is asserted too: the survivors outside the region still
//! paint, so a redaction that "passes" by blanking the whole page cannot.
//!
//! This test lives in `pdfcer-render`'s suite because `pdfcer-core` cannot
//! rasterise; it is the only place both halves — the surgery and the proof
//! by pixels — are reachable.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::annot_author::{Quad, RedactSpec};
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_core::redact;
use pdfcer_core::vartext::Quadding;
use pdfcer_core::writer::SaveOptions;
use pdfcer_render::{RenderOptions, RenderedPage, render_page_with};

/// Assemble a classic one-page PDF from binary bodies (objects `1..=n`).
fn assemble(bodies: &[Vec<u8>]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = buf.len();
    let n = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {n}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    buf
}

fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut b = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    b.extend_from_slice(data);
    b.extend_from_slice(b"\nendstream");
    b
}

/// The region every piece of content is arranged to cross.
const REGION: [f64; 4] = [100.0, 80.0, 200.0, 140.0];

/// A 300×200 page whose every object crosses `REGION`.
fn page() -> Vec<u8> {
    let content = b"\
0 g 0 G\n\
8 w 20 20 m 280 190 l S\n\
6 w 20 170 m 120 20 250 200 280 60 c S\n\
60 60 80 60 re f\n\
4 w 180 120 60 40 re B\n\
120 100 30 20 re f\n\
90 70 120 90 re 110 90 80 50 re f*\n\
q 90 0 0 60 160 100 cm /Im1 Do Q\n\
BT /F1 28 Tf 80 100 Td (SECRET) Tj ET\n";
    assemble(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] \
           /Resources << /XObject << /Im1 5 0 R >> /Font << /F1 6 0 R >> >> \
           /Contents 4 0 R >>"
            .to_vec(),
        stream("", content),
        stream(
            "/Type /XObject /Subtype /Image /Width 4 /Height 4 /BitsPerComponent 8 \
             /ColorSpace /DeviceGray",
            &[0x20; 16],
        ),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ])
}

fn render(bytes: Vec<u8>) -> RenderedPage {
    let doc = Document::from_bytes(bytes).expect("parses");
    let p = page_tree::pages(&doc).expect("pages").remove(0);
    render_page_with(&doc, &p, 2.0, &RenderOptions::default()).expect("render")
}

/// Count the pixels darker than near-white in a page-space rectangle.
fn ink_in(r: &RenderedPage, x0: f64, y0: f64, x1: f64, y1: f64) -> usize {
    let pm = &r.pixmap;
    let s = 2.0; // render scale
    let h = f64::from(pm.height());
    let (px0, px1) = ((x0 * s) as u32, (x1 * s) as u32);
    // Page y-up → pixel y-down.
    let (py0, py1) = ((h - y1 * s) as u32, (h - y0 * s) as u32);
    let mut n = 0;
    for y in py0..py1.min(pm.height()) {
        for x in px0..px1.min(pm.width()) {
            let p = pm.pixel(x, y).unwrap().demultiply();
            if p.red() < 250 || p.green() < 250 || p.blue() < 250 {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn the_redacted_region_renders_white_and_the_rest_still_paints() {
    // Before: the region is full of ink.
    let before = render(page());
    let [rx0, ry0, rx1, ry1] = REGION;
    let ink_before = ink_in(&before, rx0, ry0, rx1, ry1);
    assert!(
        ink_before > 1000,
        "the fixture must put ink in the region: {ink_before}"
    );

    // Mark and apply, with no /IC so nothing is painted over the region.
    let doc = Document::from_bytes(page()).unwrap();
    let mut session = EditSession::new(doc);
    session
        .add_redaction(
            0,
            &RedactSpec {
                quads: vec![Quad::from_rect(Rect::from_corners(rx0, ry0, rx1, ry1))],
                fill: None,
                overlay_text: None,
                quadding: Quadding::Left,
            },
        )
        .unwrap();
    let (marked, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap();
    let marked_doc = Document::from_bytes(marked).unwrap();
    let (out, report) = redact::apply_redactions(&marked_doc, &SaveOptions::identity()).unwrap();

    assert_eq!(report.marks_retained, 0, "{report:?}");
    assert_eq!(report.vector_paths_intersecting, 0, "{report:?}");
    assert!(report.vector_paths_cut >= 5, "{report:?}");
    assert_eq!(report.images_cleared, 1, "{report:?}");
    assert!(report.glyphs_removed > 0, "{report:?}");
    assert!(!report.has_disclosed_residuals(), "{:?}", report.carriers);

    // After: not one inked pixel strictly inside the region. One pixel of
    // slack on each edge covers the renderer's anti-aliasing of survivors
    // that end exactly on the boundary.
    if let Ok(dir) = std::env::var("REDACT_DEBUG_DIR") {
        std::fs::write(format!("{dir}/after.pdf"), &out).unwrap();
        let d = Document::from_bytes(out.clone()).unwrap();
        let p = page_tree::pages(&d).unwrap().remove(0);
        let cs = pdfcer_core::content::ContentStream::from_page(&d.view(), &p).unwrap();
        std::fs::write(format!("{dir}/after_content.txt"), &cs.buf).unwrap();
        let r = render(out.clone());
        r.pixmap.save_png(format!("{dir}/after.png")).unwrap();
    }
    let after = render(out);
    let inset = 0.5;
    let leaked = ink_in(&after, rx0 + inset, ry0 + inset, rx1 - inset, ry1 - inset);
    assert_eq!(leaked, 0, "ink survived inside the redaction region");

    // And the survivors outside still paint — the diagonal's tail, the
    // Bézier's ends, the rectangle's outer parts, the ring's outer band.
    let outside = ink_in(&after, 0.0, 0.0, 90.0, 200.0) + ink_in(&after, 210.0, 0.0, 300.0, 200.0);
    assert!(
        outside > 500,
        "the content outside the region must survive: {outside}"
    );
}
