//! # `Pass 71.0` regression gate — an OCR text layer changes NO PIXELS
//!
//! This is the operator's own acceptance criterion, made executable. In his
//! words (2026-08-13, the statement behind decision 059):
//!
//! > *"I want OCRed stuff to look normal when the command is executed too."*
//!
//! "Looks normal" is not a feeling here — it is a **byte-for-byte pixel
//! identity** between the page before OCR and the page after it, and that is
//! what this file asserts. `pdfcer-core`'s `tests/ocr_layer.rs` proves the text
//! goes in and comes back out; this proves the other half, that putting it in
//! cost nothing visible.
//!
//! ## Why it has to live in `pdfcer-render` and not beside the writer
//!
//! `pdfcer-core` cannot rasterise — it has no renderer and, by the GUI-core
//! separation invariant, will never grow one. The writer's own tests can
//! therefore only inspect bytes, and **bytes cannot answer "does it look the
//! same"**. That question needs a raster, so the test that asks it lives with
//! the rasteriser. The two halves of `Pass 71.0`'s correctness are split
//! across two crates for that reason, not by accident.
//!
//! ## The failure this actually guards against
//!
//! Text rendering mode 3 (ISO 32000-1 §9.3.6, Table 106 — *"neither fill nor
//! stroke text (invisible)"*) is what makes the sandwich work, and it depends
//! on **two** independent things being right at once:
//!
//! 1. the **writer** emitting `3 Tr` before any `Tj`, and
//! 2. the **renderer** honouring mode 3 rather than painting the glyphs.
//!
//! Either one alone failing produces the same, very loud symptom: a page of
//! stretched Helvetica smeared across the scan. The spec corpus warns
//! renderers about exactly this. A differential pixel test is the only check
//! that covers both halves at once, and it covers them for every future change
//! to either crate — which is the point, because the two are edited by
//! different Passes and nothing else connects them.
//!
//! A second, quieter failure it also catches: the `q … Q` wrapper going
//! missing. Without it, `3 Tr` leaks past the appended stream into whatever
//! comes next in the `/Contents` array, and the page's **pre-existing** text
//! disappears. That reads to an operator as *"running OCR deleted my
//! document's text"* — a far harder defect to trace back to OCR than to
//! prevent here.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::ocr::layer::{OcrLayerOptions, add_ocr_layer};
use pdfcer_core::ocr::{OcrPage, RecognizedWord};
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_render::render_page_view;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/addtext")
        .join(name)
}

fn word(text: &str, llx: f64, lly: f64, urx: f64, ury: f64) -> RecognizedWord {
    RecognizedWord {
        text: text.to_owned(),
        rect: Rect::from_corners(llx, lly, urx, ury),
        confidence: Some(0.87),
    }
}

/// A dense, deliberately awkward recognised page.
///
/// Chosen to maximise the damage a mode-3 failure would do: many words, large
/// sizes, heavy horizontal stretching, and one word laid directly over the
/// fixture's own existing text. If the glyphs were painted, none of this could
/// hide.
fn busy_page() -> OcrPage {
    OcrPage {
        words: vec![
            word("INVOICE", 72.0, 700.0, 300.0, 736.0),
            word("Number", 72.0, 650.0, 400.0, 690.0),
            word("40129", 72.0, 600.0, 500.0, 640.0),
            word("overlapping", 72.0, 720.0, 540.0, 760.0),
            word("stretched", 40.0, 400.0, 560.0, 460.0),
        ],
        confidence_available: true,
    }
}

/// ★ The whole feature, in one assertion: adding an OCR layer changes NOTHING
/// on screen.
///
/// Rendered at a deliberately high scale (2.0) so a sub-pixel difference has
/// somewhere to show up rather than being swallowed by rounding at 1:1.
#[test]
fn adding_an_ocr_layer_changes_not_one_pixel() {
    let doc = Document::load(&fixture("plain.pdf")).expect("fixture loads");
    let pages = page_tree::pages(&doc).expect("page tree walks");
    let before = render_page_view(&doc.view(), &pages[0], 2.0).expect("base rasterises");

    let out = add_ocr_layer(&doc, 0, &busy_page(), &OcrLayerOptions::new()).expect("layer written");
    assert_eq!(out.report.words_written, 5, "all five words were written");

    let saved = Document::from_bytes(out.bytes).expect("output reloads");
    let saved_pages = page_tree::pages(&saved).expect("page tree walks");
    let after =
        render_page_view(&saved.view(), &saved_pages[0], 2.0).expect("OCR'd page rasterises");

    assert_eq!(
        before.pixmap.width(),
        after.pixmap.width(),
        "the page size must not change"
    );
    assert_eq!(before.pixmap.height(), after.pixmap.height());

    let (a, b) = (before.pixmap.data(), after.pixmap.data());
    let differing = a.iter().zip(b.iter()).filter(|(x, y)| x != y).count();
    assert_eq!(
        differing,
        0,
        "an OCR text layer must be INVISIBLE: {differing} of {} bytes differ. \
         Either the writer stopped emitting `3 Tr` before its `Tj`s, or the \
         renderer stopped honouring text rendering mode 3 (§9.3.6 Table 106). \
         Both produce a page of stretched Helvetica over the scan.",
        a.len()
    );
}

/// ★ The invisible mode does not leak into the page's EXISTING content.
///
/// `Tf`/`Tr`/`Tz` are graphics state and a `/Contents` array concatenates, so
/// an unwrapped layer would set `3 Tr` and leave it set. The fixture's own
/// visible text would then vanish — which is precisely why the previous test
/// cannot stand alone: **a page whose original text also disappeared would
/// still differ from the baseline**, so that test would fail, but for a reason
/// its message would misattribute. This one names the failure directly by
/// checking the page is not blank.
#[test]
fn the_pages_existing_text_still_renders_after_ocr() {
    let doc = Document::load(&fixture("plain.pdf")).expect("fixture loads");
    let pages = page_tree::pages(&doc).expect("page tree walks");
    let before = render_page_view(&doc.view(), &pages[0], 2.0).expect("base rasterises");

    // How much ink the untouched page has, as a baseline. A page that renders
    // completely blank would make the pixel-identity test above vacuous.
    let ink_of = |data: &[u8]| {
        data.chunks_exact(4)
            .filter(|px| px[3] > 0 && px[0] < 250)
            .count()
    };
    let ink_before = ink_of(before.pixmap.data());
    assert!(
        ink_before > 0,
        "the fixture must actually draw something, or the identity test proves \
         nothing"
    );

    let out = add_ocr_layer(&doc, 0, &busy_page(), &OcrLayerOptions::new()).expect("layer written");
    let saved = Document::from_bytes(out.bytes).expect("output reloads");
    let saved_pages = page_tree::pages(&saved).expect("page tree walks");
    let after =
        render_page_view(&saved.view(), &saved_pages[0], 2.0).expect("OCR'd page rasterises");

    assert_eq!(
        ink_of(after.pixmap.data()),
        ink_before,
        "the page's pre-existing ink must be untouched — losing it means `3 Tr` \
         escaped the appended stream's `q … Q` and made everything after it \
         invisible"
    );
}
