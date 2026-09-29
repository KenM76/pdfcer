//! Do deep-zoom region renders that SUCCEED draw the right pixels?
//!
//! ```text
//! cargo run --release -p pdfcer-render --example deep_zoom_pixels
//! ```
//!
//! A region centred on the bar's right edge should come back with its left
//! half black and its right half grey (0.8 -> 204) at every scale. The table
//! reports the pixmap size, the column where black turns to grey, and whether
//! that column is the middle (within one pixel).
//!
//! - `--bisect`: the highest scale that is still correct, per page size.
//! - `--strict`: with `--bisect`, also require the edge column to be exact.
//! - `--far`: put the edge at 3301.37 pt instead of 301 pt; a non-integer far
//!   from the origin is where precision runs out first.
//!
//! The numbers behind `MAX_GUARANTEED_REGION_SCALE` come from
//! `--bisect --strict --far`.

use pdfcer_core::document::Document;
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_render::{RenderOptions, render_page_region};
use std::panic::{AssertUnwindSafe, catch_unwind};

/// A one-page document painting a filled rectangle across the whole sheet,
/// plus a thin bar — content whose device-space extent is the page's own.
fn doc_covering(page_w: f64, page_h: f64) -> Vec<u8> {
    let bx = bar_x() - 1.0;
    let content = format!("0.8 0.8 0.8 rg 0 0 {page_w} {page_h} re f 0 0 0 rg {bx} 500 1 200 re f");
    let mut pdf = String::from("%PDF-1.7\n");
    let mut offsets = Vec::new();
    let objects: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {page_w} {page_h}] \
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

/// `true` when the region centred on the bar edge comes back black | grey.
fn correct(doc: &Document, page: &pdfcer_core::page_tree::Page, scale: f64) -> Option<bool> {
    let span = 1100.0 / scale;
    let region = Rect::from_corners(
        bar_x() - span / 2.0,
        600.0 - span / 2.0,
        bar_x() + span / 2.0,
        600.0 + span / 2.0,
    );
    #[allow(clippy::cast_possible_truncation)] // the scales probed are far inside f32 range
    let r = catch_unwind(AssertUnwindSafe(|| {
        render_page_region(
            &doc.view(),
            page,
            scale as f32,
            region,
            &RenderOptions::default(),
        )
    }))
    .ok()?
    .ok()?;
    let (w, h) = (r.pixmap.width(), r.pixmap.height());
    let px = |x: u32| r.pixmap.pixel(x, h / 2).map_or(0, |c| c.red());
    let edge = (0..w).find(|&x| px(x) > 100).unwrap_or(w);
    let strict = std::env::args().any(|a| a == "--strict");
    let edge_ok = !strict || (i64::from(edge) - i64::from(w / 2)).abs() <= 1;
    Some(px(w / 4) < 20 && (190..=215).contains(&px(3 * w / 4)) && edge_ok)
}

/// The bar's right edge in points: 301 by default, `--far` puts it at
/// 3301.37 (a non-integer far from the origin, where f32 spacing is coarsest).
fn bar_x() -> f64 {
    if std::env::args().any(|a| a == "--far") {
        3301.37
    } else {
        301.0
    }
}

fn bisect_all() {
    for (label, w, h) in [
        ("E-size", 3370.0_f64, 2384.0_f64),
        ("A1 landscape", 2383.937, 1683.780),
        ("A4 portrait", 595.276, 841.890),
        ("business card", 144.0, 252.0),
    ] {
        let doc = Document::from_bytes(doc_covering(w, h)).expect("loads");
        let pages = page_tree::pages(&doc).expect("pages");
        let (mut lo, mut hi) = (1.0e3_f64, 1.0e12_f64);
        for _ in 0..40 {
            let mid = (lo * hi).sqrt();
            if correct(&doc, &pages[0], mid) == Some(true) {
                lo = mid
            } else {
                hi = mid
            }
        }
        println!(
            "{label:>14}: correct up to {lo:.0}, first wrong/refused {hi:.0}  (outcome there: {:?})",
            correct(&doc, &pages[0], hi * 1.01)
        );
    }
}

fn main() {
    std::panic::set_hook(Box::new(|_| {}));
    if std::env::args().any(|a| a == "--bisect") {
        bisect_all();
        return;
    }
    let bytes = doc_covering(2383.937, 1683.780);
    let doc = Document::from_bytes(bytes).expect("loads");
    let pages = page_tree::pages(&doc).expect("pages");
    let page = &pages[0];
    let opts = RenderOptions::default();
    println!(
        "{:>8}  {:>10}  {:>11}  {:>6}  {:>6}  verdict",
        "scale", "span pt", "pixmap", "edge", "mid"
    );
    let mut scale = 1.0e2_f64;
    while scale <= 1.0e12 {
        let span = 1100.0 / scale;
        let region = Rect::from_corners(
            bar_x() - span / 2.0,
            600.0 - span / 2.0,
            bar_x() + span / 2.0,
            600.0 + span / 2.0,
        );
        #[allow(clippy::cast_possible_truncation)] // the scales probed are far inside f32 range
        let r = catch_unwind(AssertUnwindSafe(|| {
            render_page_region(&doc.view(), page, scale as f32, region, &opts)
        }));
        let line = match r {
            Err(_) => "PANIC".to_string(),
            Ok(Err(e)) => format!("refused: {e}"),
            Ok(Ok(p)) => {
                let (w, h) = (p.pixmap.width(), p.pixmap.height());
                let row = h / 2;
                let px = |x: u32| p.pixmap.pixel(x, row).map_or(0, |c| c.red());
                let edge = (0..w).find(|&x| px(x) > 100).unwrap_or(w);
                let blacks = (0..w).filter(|&x| px(x) < 20).count();
                let greys = (0..w).filter(|&x| (190..=215).contains(&px(x))).count();
                let sample = [0, w / 4, w / 2 - 2, w / 2 + 2, 3 * w / 4, w - 1].map(|x| {
                    p.pixmap
                        .pixel(x, row)
                        .map_or((0, 0, 0, 0), |c| (c.red(), c.green(), c.blue(), c.alpha()))
                });
                eprintln!("{scale:e}: {sample:?}");
                let ok = (i64::from(edge) - i64::from(w / 2)).abs() <= 1;
                format!(
                    "{:>11}  {edge:>6}  {:>6}  {} (black {blacks}, grey {greys})",
                    format!("{w}x{h}"),
                    w / 2,
                    if ok { "correct" } else { "WRONG" }
                )
            }
        };
        println!("{scale:>8.0e}  {span:>10.3e}  {line}");
        scale *= 10.0;
    }
}
