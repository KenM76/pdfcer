//! End-to-end OCR smoke harness: PDF -> raster -> recognise -> text layer ->
//! extract. Proves the PLUMBING, and is deliberately honest about what it does
//! not prove.
//!
//! ```text
//! cargo run --release -p pdfcer-render --example ocr_smoke -- <models-dir> <file.pdf> [scale]
//! ```
//!
//! # ★ READ THE `recognised N words` LIST, NOT THE `EXTRACTED BACK` LINE
//!
//! This harness prints both, and they answer different questions. The
//! extraction runs over the SAVED document, so on any PDF that already has a
//! text layer it returns **the document's own text mixed with the OCR
//! layer's** — and the document's own text is always the better-looking half.
//!
//! That is not hypothetical. On first use this harness was read as showing
//! ocrs recognising Lithuanian words (`pandusas`, `šiukšl`, `mūr`) off a CAD
//! drawing. It was not: those were the drawing's existing text layer. What
//! ocrs actually returned for the same page was `HiTN`, `03POK`, `trasir`,
//! `japsaos`. The giveaway was arithmetic, not reading — ocrs's default
//! alphabet is **ASCII only**, so it cannot emit `š` or `ū` at all, and any
//! diacritic in the output is by definition not from OCR.
//!
//! # What this harness DOES prove
//!
//! Models load, the engine runs, words come back with boxes, the y-flip
//! applies, the invisible layer is written, the disclosures fire, the saved
//! file reloads, and the text extracts. That is the integration.
//!
//! # What it does NOT prove, and cannot
//!
//! **Recognition quality**, because both documents available here are the
//! wrong input: vector PDFs that already contain text. OCR exists for pages
//! that are pictures of text. Feeding it crisp small vector glyphs is
//! out-of-distribution in the opposite direction from a bad scan, and the
//! measured output on both was poor. A real quality claim needs a genuine
//! scanned page, and this project has no rights-cleared one — see
//! `docs/LEGAL.md` §5 before adding any.
use pdfcer_core::document::Document;
use pdfcer_core::ocr::engine_ocrs::OcrsEngine;
use pdfcer_core::ocr::{OcrEngine, OcrPage, layer, words_to_page_space};
use pdfcer_core::page_tree;
use pdfcer_core::text_extract::{self, ExtractOptions};
use pdfcer_render::render_page_view;

fn main() {
    let mut a = std::env::args().skip(1);
    let models = a.next().expect("usage: ocr_smoke <models-dir> <pdf>");
    let pdf = a.next().expect("usage: ocr_smoke <models-dir> <pdf>");

    let doc = Document::load(std::path::Path::new(&pdf)).expect("load pdf");
    let pages = page_tree::pages(&doc).expect("pages");
    let page = &pages[0];

    // Rasterise at 2x and convert to 8-bit greyscale, which is what the trait takes.
    let scale: f32 = a.next().and_then(|v| v.parse().ok()).unwrap_or(2.0);
    let r = render_page_view(&doc.view(), page, scale).expect("rasterise");
    let (w, h) = (r.pixmap.width(), r.pixmap.height());
    let grey: Vec<u8> = r
        .pixmap
        .data()
        .chunks_exact(4)
        .map(|p| {
            // Rec.601 luma; the engine wants intensity, not colour.
            ((0.299 * f32::from(p[0])) + (0.587 * f32::from(p[1])) + (0.114 * f32::from(p[2])))
                as u8
        })
        .collect();
    println!("rasterised {w}x{h}, {} grey bytes", grey.len());

    let t = std::time::Instant::now();
    let engine = OcrsEngine::from_model_dir(std::path::Path::new(&models)).expect("load models");
    println!("models loaded in {:?}", t.elapsed());

    let t = std::time::Instant::now();
    let words = engine.recognize(w, h, &grey).expect("recognise");
    println!("recognised {} words in {:?}", words.len(), t.elapsed());
    println!("reports_confidence = {}", engine.reports_confidence());
    for word in words.iter().take(12) {
        println!("   {:?}  {:?}", word.text, word.rect);
    }
    if words.is_empty() {
        println!("NO WORDS -- stopping");
        return;
    }

    let ocr_page = OcrPage {
        words: words_to_page_space(&words, w, h, page.crop_box),
        confidence_available: engine.reports_confidence(),
    };
    let out = layer::add_ocr_layer(&doc, 0, &ocr_page, &layer::OcrLayerOptions::new())
        .expect("write layer");
    println!("\nlayer written: {:?}", out.report);
    for d in out.report.disclosures() {
        println!("  DISCLOSE: {d}");
    }

    let saved = Document::from_bytes(out.bytes).expect("reload");
    let sp = page_tree::pages(&saved).expect("pages");
    let ex =
        text_extract::extract_page(&saved, &sp[0], 0, &ExtractOptions::default()).expect("extract");
    let text: String = ex.runs.iter().map(|r| r.text.as_str()).collect();
    println!(
        "\nEXTRACTED BACK: {:?}",
        text.chars().take(300).collect::<String>()
    );
}
