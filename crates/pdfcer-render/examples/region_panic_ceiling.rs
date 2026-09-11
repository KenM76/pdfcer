//! Where does a region render stop REFUSING and start PANICKING? — the
//! experiment behind [`pdfcer_render::MAX_REGION_DEVICE_EXTENT`].
//!
//! ```text
//! cargo run --release -p pdfcer-render --example region_panic_ceiling
//! ```
//!
//! # The question
//!
//! The consuming shell reported a **thread panic inside `tiny-skia`** at a
//! region render with `scale = 509_703.84`:
//!
//! ```text
//! panicked at tiny-skia-0.11.4/src/pipeline/mod.rs:188:9:
//!   range start index 442613758592 out of range for slice of length 1088737
//! ```
//!
//! `1_088_737` is the requested pixmap — window-sized, bounded, exactly as
//! designed. The out-of-range index is a *row* offset, so the rasteriser was
//! asked to blit a scanline far outside a pixmap whose own size was never in
//! question. `MAX_PIXMAP_EDGE` cannot catch it: that constant guards the
//! allocation, and the allocation was fine.
//!
//! ★ The same report contains the reason a number cannot simply be guessed:
//! the shell's own check renders CORRECTLY at 3,099,514 % (≈ 30,995×) and only
//! fails somewhere past ≈ 509,703×. Any ceiling picked as a round number lands
//! either inside working territory — breaking deep zoom that demonstrably
//! works — or above the panic, which is no ceiling at all. So it gets
//! measured.
//!
//! # Method
//!
//! Reproduce the reported shape: an A1-landscape page whose content paints
//! across the WHOLE sheet, and a viewport-sized region around a point well
//! away from the origin. Escalate `scale` and record, for each, whether the
//! call returned, refused, or panicked — `catch_unwind` with the panic hook
//! silenced, so a run prints a table instead of a stack trace.
//!
//! Then bisect the first failing decade to find the boundary.
//!
//! # ★★ What it found, and why the answer was a refusal rather than a constant
//!
//! Six geometries, bisected (2026-09-11, before the guard existed):
//!
//! ```text
//! E-size (3370 x 2384 pt)        284,964
//! A3 landscape (1190 x 842 pt) 2,147,482
//! A4 portrait  (595 x 842 pt)  2,147,482
//! A1 landscape (2384 x 1684)   8,053,069
//! A6 portrait  (298 x 420 pt)  8,053,069
//! business card (144 x 252 pt) 8,053,069
//! ```
//!
//! **Three distinct values, ordering with nothing.** Not page width, not area,
//! not `page_edge × scale`. The LARGEST sheet is the most fragile; an A1 sheet
//! and a business card share a boundary A4 never reaches. The limit belongs to
//! `tiny_skia`'s fixed-point scan conversion interacting with the particular
//! geometry being painted, so it is content-dependent too.
//!
//! That table is the argument for
//! [`pdfcer_render::RenderError::RasterizerLimit`] over a published ceiling: a
//! constant fitted to it would be an invented number, and the request that
//! prompted this work asked, in as many words, not to receive one.
//! [`pdfcer_render::MAX_GUARANTEED_REGION_SCALE`] is set below the lowest row
//! and claims only what the lowest row supports.
//!
//! # Running it AFTER the guard
//!
//! Every `PANIC` above now prints `refused`, which is the whole change. The
//! probe still bisects — the boundary has not moved, only its outcome — so it
//! doubles as the measurement that would notice the boundary shifting under a
//! `tiny_skia` upgrade.

use pdfcer_core::document::Document;
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_render::{RenderOptions, render_page_region};
use std::panic::{AssertUnwindSafe, catch_unwind};

/// A one-page document painting a filled rectangle across the whole sheet,
/// plus a thin bar — content whose device-space extent is the page's own.
fn doc_covering(page_w: f64, page_h: f64) -> Vec<u8> {
    let content = format!("0.8 0.8 0.8 rg 0 0 {page_w} {page_h} re f 0 0 0 rg 300 500 1 200 re f");
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

/// `Ok(true)` rendered, `Ok(false)` refused, `Err(())` panicked.
fn probe(bytes: &[u8], scale: f32) -> Result<bool, ()> {
    let doc = Document::from_bytes(bytes.to_vec()).expect("synthetic doc loads");
    let pages = page_tree::pages(&doc).expect("pages");
    let page = &pages[0];
    let opts = RenderOptions::default();
    // A viewport-sized region: ~1100 x 990 device px however deep the zoom,
    // which is the shape the shell actually asks for.
    let span = 1100.0 / f64::from(scale);
    let region = Rect::from_corners(300.0, 500.0, 300.0 + span, 500.0 + span * 0.9);
    catch_unwind(AssertUnwindSafe(|| {
        render_page_region(&doc.view(), page, scale, region, &opts).is_ok()
    }))
    .map_err(|_| ())
}

fn verdict(bytes: &[u8], scale: f32) -> &'static str {
    match probe(bytes, scale) {
        Ok(true) => "rendered",
        Ok(false) => "refused",
        Err(()) => "PANIC",
    }
}

fn main() {
    // Silence the default hook: a probe that expects panics should print a
    // table, not eighty stack traces.
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    for (label, w, h) in [
        ("E-size", 3370.0_f64, 2384.0_f64),
        ("A1 landscape", 2383.937, 1683.780),
        ("A3 landscape", 1190.551, 841.890),
        ("A4 portrait", 595.276, 841.890),
        ("A6 portrait", 297.638, 419.528),
        ("business card", 144.0, 252.0),
    ] {
        let bytes = doc_covering(w, h);
        println!("\n=== {label}  ({w} x {h} pt) ===");
        println!(
            "{:>14}  {:>18}  {:>10}",
            "scale", "page_w x scale", "outcome"
        );

        let mut last_ok = 0.0_f64;
        let mut first_bad = 0.0_f64;
        let mut scale = 1.0e3_f64;
        while scale <= 1.0e9 {
            #[allow(clippy::cast_possible_truncation)]
            let v = verdict(&bytes, scale as f32);
            println!("{scale:>14.0}  {:>18.3e}  {v:>10}", w * scale);
            // A refusal counts as the boundary too: since the guard landed
            // it is what a panic BECAME, and a probe that only looked for
            // panics would report "no boundary found" on a crate that refuses
            // correctly -- the most misleading possible reading.
            if v != "rendered" {
                first_bad = scale;
                break;
            }
            last_ok = scale;
            scale *= 10.0;
        }

        if first_bad > 0.0 {
            // Bisect the decade that flipped.
            let (mut lo, mut hi) = (last_ok, first_bad);
            for _ in 0..40 {
                let mid = f64::midpoint(lo, hi);
                #[allow(clippy::cast_possible_truncation)]
                let v = verdict(&bytes, mid as f32);
                if v == "rendered" { lo = mid } else { hi = mid }
            }
            println!("\n  last scale that did NOT panic : {lo:.1}");
            println!("  first scale that DID panic    : {hi:.1}");
            println!("  device extent at the boundary : {:.6e} px", w * lo);
        } else {
            println!("\n  no panic found up to 1e9");
        }
    }

    std::panic::set_hook(prev);
}
