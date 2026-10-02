//! A re-wrap keeps the source's `TJ` kerning inside a word, so a kerned word
//! a narrower wrap does not move renders pixel-identical (Pass 432.0).
//!
//! Page 3 of `fixtures/synthetic/reflow/fidelity.pdf` opens with
//! `[(A) 80 (V) 80 (A) 60 (T) 40 (AR)] TJ`; re-wrapped at 120 pt its first
//! line keeps "AVATAR leads a line" at the same origin.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_core::text_edit::{ReflowRequest, apply_reflow};
use pdfcer_render::render_page;

const PAGE: usize = 2;
const SCALE: f32 = 3.0;
/// The first line's box in page space: x, then y from the bottom.
const X: (f32, f32) = (60.0, 170.0);
const Y: (f32, f32) = (736.0, 752.0);

fn crop(bytes: &[u8]) -> Vec<u8> {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    let r = render_page(&doc, &pages[PAGE], SCALE).unwrap();
    let px = &r.pixmap;
    let h = 792.0 * SCALE;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // in-page pixel bounds
    let (x0, x1, y0, y1) = (
        (X.0 * SCALE) as u32,
        (X.1 * SCALE) as u32,
        (h - Y.1 * SCALE) as u32,
        (h - Y.0 * SCALE) as u32,
    );
    let mut out = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            let p = px.pixel(x, y).unwrap();
            out.extend_from_slice(&[p.red(), p.green(), p.blue(), p.alpha()]);
        }
    }
    out
}

#[test]
fn a_kerned_word_that_does_not_move_renders_pixel_identical() {
    let src = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/reflow/fidelity.pdf"),
    )
    .expect("fidelity.pdf; run tools/gen-reflow-fidelity-fixtures.py");
    let doc = Document::from_bytes(src.clone()).unwrap();
    let out = apply_reflow(&doc, PAGE, 0, &ReflowRequest::new().with_wrap_width(120.0)).unwrap();
    let before = crop(&src);
    assert!(
        before.chunks(4).any(|p| p[0] < 128),
        "the crop must hold ink"
    );
    assert!(
        crop(&out.bytes) == before,
        "the first line's pixels changed"
    );
}
