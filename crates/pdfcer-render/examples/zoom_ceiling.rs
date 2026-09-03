//! Where does deep zoom stop being meaningful? — the experiment behind
//! `MAX_ZOOM`, answering question 3 of `request_region_rasterisation.md`.
//!
//! ```text
//! cargo run --release -p pdfcer-render --example zoom_ceiling
//! ```
//!
//! # The question, and why it needs an experiment rather than a round number
//!
//! `render_page_region` removed the memory ceiling on magnification, so the
//! remaining limit is **numerical**, not spatial: at some scale the
//! rasteriser's `f32` transform arithmetic stops resolving the difference
//! between adjacent user-space coordinates, and the picture silently stops
//! being a faithful magnification of the page. A `MAX_ZOOM` picked as a round
//! number would be exactly the "beyond any plausible use" error that
//! `MAX_PIXMAP_EDGE` already made once.
//!
//! # Method
//!
//! A 1-point-wide black bar at a known page position, FAR from the origin
//! (position matters: `f32` has ~7 significant decimal digits, so absolute
//! magnitude is what consumes precision, and a test at the origin would
//! measure nothing). At each scale, a region is rendered tightly around the
//! bar and the ink's measured left edge is compared against where the
//! arithmetic says it must land.
//!
//! The reported error is in **device pixels**. The ceiling is the scale at
//! which it exceeds one pixel — beyond that the operator is looking at a
//! picture that no longer corresponds to the page.
//!
//! # ★ Both the position and the scale are deliberately NON-ROUND
//!
//! The first version of this harness used `x = 3000.0` and scales that were
//! exact powers of two. It reported **error 0.000 at every scale up to
//! 2048x**, which looks like a definitive all-clear and measures nothing:
//! `3000 x 2^n` is **exactly representable in `f32`**, so the arithmetic
//! never has to round and the experiment cannot observe the thing it exists
//! to observe.
//!
//! `2999.7373` and a `x1.3137` scale factor force a non-terminating binary
//! mantissa at every step. Anyone re-running or extending this must keep
//! both non-round, for the same reason.

use pdfcer_core::document::Document;
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_render::{RenderOptions, render_page_region};

/// Build a one-page document with a 1pt bar whose left edge sits at `x`.
fn doc_with_bar(x: f64, page_edge: f64) -> Vec<u8> {
    let content = format!("0 0 0 rg {x} 100 1 200 re f");
    let mut pdf = String::from("%PDF-1.7\n");
    let mut offsets = Vec::new();
    let objects: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {page_edge} {page_edge}] \
             /Contents 4 0 R /Resources << >> >>"
        ),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
    ];
    for (i, o) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{o}\nendobj\n", i + 1));
    }
    let xref = pdf.len();
    pdf.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for off in &offsets {
        pdf.push_str(&format!("{off:010} 00000 n \n"));
    }
    pdf.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    ));
    pdf.into_bytes()
}

fn main() {
    // A large sheet, and a bar far from the origin — both maximise the
    // absolute coordinate magnitude that f32 has to resolve.
    let page_edge = 3370.0; // A0 long edge, pt
    let bar_x = 2999.7373;
    let bytes = doc_with_bar(bar_x, page_edge);
    let doc = Document::from_bytes(bytes).expect("synthetic doc loads");
    let pages = page_tree::pages(&doc).expect("pages");
    let page = &pages[0];
    let opts = RenderOptions::default();

    println!("bar left edge at x={bar_x} pt on a {page_edge}pt page\n");
    println!(
        "{:>12}  {:>10}  {:>12}  {:>10}",
        "scale", "region px", "ink left px", "error px"
    );

    for exp in 0..18 {
        let scale = f32::powi(2.0, exp) * 1.3137;
        // A region 2pt wide around the bar's left edge, so the pixmap stays
        // small however deep the zoom goes.
        let region = Rect::from_corners(bar_x - 0.02, 150.0, bar_x + 0.06, 150.04);
        match render_page_region(&doc.view(), page, scale, region, &opts) {
            Ok(r) => {
                // Device x of the region's left edge, and of the bar's edge.
                let region_left_dev = ((bar_x - 0.02) * f64::from(scale)).floor();
                let expected = (bar_x * f64::from(scale)) - region_left_dev;
                // First column containing ink.
                let w = r.pixmap.width();
                let mut found = None;
                'outer: for x in 0..w {
                    for y in 0..r.pixmap.height() {
                        let p = r.pixmap.pixel(x, y).expect("in bounds");
                        if p.red() < 200 {
                            found = Some(x);
                            break 'outer;
                        }
                    }
                }
                match found {
                    Some(ink) => println!(
                        "{scale:>12}  {:>10}  {ink:>12}  {:>10.3}",
                        format!("{}x{}", r.pixmap.width(), r.pixmap.height()),
                        (f64::from(ink) - expected).abs()
                    ),
                    None => println!(
                        "{scale:>12}  {:>10}  {:>12}  {:>10}",
                        format!("{}x{}", r.pixmap.width(), r.pixmap.height()),
                        "NO INK",
                        "-"
                    ),
                }
            }
            Err(e) => println!("{scale:>12}  ERR {e}"),
        }
    }
}
