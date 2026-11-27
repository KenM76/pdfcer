//! A render cancelled while a large image decodes stops within a row, not
//! after the whole image (request G337).
//!
//! Ratio-based so it holds in debug and release alike: the latency from
//! `cancel()` to the worker's return must be a small fraction of the
//! uncancelled render's own time on the same page.

use std::io::Write as _;
use std::time::{Duration, Instant};

use flate2::Compression;
use flate2::write::ZlibEncoder;
use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::cancel::RenderCancel;
use pdfcer_render::{RenderError, RenderOptions, render_page_with_view};

const SIDE: u32 = 2000;

/// One page drawing one `SIDE` x `SIDE` Flate-compressed DeviceGray image
/// over the whole MediaBox. Synthetic; a few KB on disk.
fn scan_like_page() -> Vec<u8> {
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::fast());
    let row: Vec<u8> = (0..SIDE).map(|x| (x % 251) as u8).collect();
    for _ in 0..SIDE {
        enc.write_all(&row).expect("zlib");
    }
    let data = enc.finish().expect("zlib");
    let content = b"100 0 0 100 0 0 cm /Im0 Do";
    let mut image = format!(
        "<< /Type /XObject /Subtype /Image /Width {SIDE} /Height {SIDE} \
         /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode \
         /Length {} >>\nstream\n",
        data.len()
    )
    .into_bytes();
    image.extend_from_slice(&data);
    image.extend_from_slice(b"\nendstream");
    let mut contents = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    contents.extend_from_slice(content);
    contents.extend_from_slice(b"\nendstream");
    let objects: [Vec<u8>; 5] = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R \
          /Resources << /XObject << /Im0 5 0 R >> >> >>"
            .to_vec(),
        contents,
        image,
    ];
    let mut buf = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 6\n0000000000 65535 f\r\n");
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    buf
}

/// Render at a scale that keeps the device raster small, so the image
/// DECODE dominates the time and the test measures what G337 is about.
const SCALE: f32 = 1.0;

#[test]
fn a_cancel_during_a_large_image_decode_returns_within_a_fraction_of_the_render() {
    let doc = Document::from_bytes(scan_like_page()).expect("fixture parses");
    let view = doc.view();
    let page = page_tree::pages(&doc).expect("page tree").remove(0);

    let start = Instant::now();
    let rendered = render_page_with_view(&view, &page, SCALE, &RenderOptions::default())
        .expect("the uncancelled render succeeds");
    let whole = start.elapsed();
    assert_eq!(
        rendered.diagnostics.images_unsupported, 0,
        "the fixture image must actually decode, or the cancel has nothing to interrupt"
    );

    let token = RenderCancel::new();
    let options = RenderOptions::default().with_cancel(token.clone());
    let delay = whole / 5;
    let (outcome, latency) = std::thread::scope(|scope| {
        let worker = scope.spawn(|| render_page_with_view(&view, &page, SCALE, &options));
        std::thread::sleep(delay);
        let cancelled_at = Instant::now();
        token.cancel();
        let outcome = worker.join().expect("worker does not panic");
        (outcome, cancelled_at.elapsed())
    });

    assert!(
        matches!(outcome, Err(RenderError::Cancelled)),
        "a cancel 1/5 into the render must return Cancelled, got {:?}",
        outcome.map(|_| "a page")
    );
    let bound = (whole / 2).max(Duration::from_millis(20));
    assert!(
        latency < bound,
        "cancel took {latency:?} to return against a {whole:?} render (bound {bound:?}): \
         the image decode is not polling the token per row"
    );
}
