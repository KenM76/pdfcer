//! A deep region render draws the right pixels (`Pass 296.0`).
//!
//! A page-wide fill at extreme magnification reaches device coordinates past
//! `tiny_skia`'s fixed-point range. Unguarded, an anti-aliased fill there
//! panics (a dead worker behind a live window) and a sheet a little short of
//! that paints **wrong pixels and returns `Ok`**. The renderer pre-clips such
//! geometry to the target; these tests pin both failure shapes at scales that
//! produced them, on the geometries that produced them.
//!
//! Each render is a viewport-sized region centred on the right edge of a thin
//! black bar over a grey (0.8) page-wide fill, so a correct raster is black
//! up to the middle column and grey after it.

use pdfcer_core::document::Document;
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_render::{RenderError, RenderOptions, render_page_region};

const E_SIZE: (f64, f64) = (3370.0, 2384.0);
const A4: (f64, f64) = (595.276, 841.890);

/// One page of `size` painted grey edge to edge, with a black bar whose
/// right edge is at `edge_x`.
fn sheet((w, h): (f64, f64), edge_x: f64) -> Vec<u8> {
    let bar = edge_x - 1.0;
    let content = format!("0.8 0.8 0.8 rg 0 0 {w} {h} re f 0 0 0 rg {bar} 500 1 200 re f");
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] \
             /Contents 4 0 R /Resources << >> >>"
        ),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
    ];
    let mut pdf = String::from("%PDF-1.7\n");
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{o}\nendobj\n", i + 1));
    }
    let xref = pdf.len();
    pdf.push_str("xref\n0 5\n0000000000 65535 f \n");
    for off in &offsets {
        pdf.push_str(&format!("{off:010} 00000 n \n"));
    }
    pdf.push_str(&format!(
        "trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
    ));
    pdf.into_bytes()
}

/// Renders the 1100-pixel region centred on (`edge_x`, 600) at `scale` and
/// returns the first column that is not black, and the pixmap width. Panics
/// on a refusal: every scale here must render.
fn edge_column(size: (f64, f64), edge_x: f64, scale: f32) -> (u32, u32) {
    let doc = Document::from_bytes(sheet(size, edge_x)).expect("synthetic sheet loads");
    let pages = page_tree::pages(&doc).expect("pages");
    let half = 550.0 / f64::from(scale);
    let region = Rect::from_corners(edge_x - half, 600.0 - half, edge_x + half, 600.0 + half);
    let r = render_page_region(
        &doc.view(),
        &pages[0],
        scale,
        region,
        &RenderOptions::default(),
    )
    .unwrap_or_else(|e| panic!("{scale}x must render, got: {e}"));
    let (w, h) = (r.pixmap.width(), r.pixmap.height());
    let red = |x: u32| r.pixmap.pixel(x, h / 2).map_or(0, |c| c.red());
    // Both halves, not only the edge: the silent failure turned the grey
    // fill white while leaving the bar black.
    assert!(red(w / 4) < 20, "{scale}x: left half must be the black bar");
    assert!(
        (190..=215).contains(&red(3 * w / 4)),
        "{scale}x: right half must be the 0.8 grey fill, got {}",
        red(3 * w / 4)
    );
    ((0..w).find(|&x| red(x) > 100).unwrap_or(w), w)
}

fn assert_edge_centred(size: (f64, f64), edge_x: f64, scale: f32) {
    let (edge, w) = edge_column(size, edge_x, scale);
    assert!(
        edge.abs_diff(w / 2) <= 1,
        "{scale}x: edge at column {edge}, expected {}",
        w / 2
    );
}

#[test]
fn an_e_size_sheet_renders_past_the_old_panic_boundary() {
    // Unguarded, 1,000,000x on E-size panicked in the rasteriser.
    assert_edge_centred(E_SIZE, 301.0, 1_000_000.0);
}

#[test]
fn an_a4_sheet_is_not_silently_wrong_past_its_old_boundary() {
    // Unguarded, A4 returned Ok with the grey fill gone from ~900,000x.
    assert_edge_centred(A4, 301.0, 1_500_000.0);
}

#[test]
fn the_published_floor_is_exact_far_from_the_origin() {
    // A non-integer edge far from the origin is where precision runs out
    // first (measured exact to ~33.5 million).
    assert_edge_centred(E_SIZE, 3301.37, pdfcer_render::MAX_GUARANTEED_REGION_SCALE);
}

/// The floor must sit under the lowest measured pixel-exact boundary
/// (E-size, edge at 3301.37 pt: 33,554,982). A `const` item, so raising the
/// constant past the measurement is a compile error rather than a test
/// someone could ignore.
const _: () = assert!(
    pdfcer_render::MAX_GUARANTEED_REGION_SCALE < 33_554_982.0,
    "the published floor must be below the lowest measured exact boundary"
);

#[test]
fn a_rasterizer_refusal_names_the_scale_and_hides_the_panic_text() {
    // A consuming shell shows an error's Display on the page; a third
    // party's panic text must not reach it, the scale must.
    let panic_message =
        "range start index 442613758592 out of range for slice of length 1088737".to_string();
    let shown = RenderError::RasterizerLimit {
        scale: 1_000_000.0,
        panic_message: panic_message.clone(),
    }
    .to_string();
    assert!(!shown.contains(&panic_message), "{shown}");
    assert!(shown.contains("1000000"), "{shown}");
}
