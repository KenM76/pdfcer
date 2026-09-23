//! `G034` — text rendering mode (`Tr`, ISO 32000-1 §9.3.6 Table 106) as an
//! editing control, and the text-state reset every added run now starts with.
//!
//! Visibility is measured by extraction's `invisible_glyphs` counter, which
//! counts glyphs shown in mode 3 or 7 — the one question this control changes.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, OcrPageLayer};
use pdfcer_core::ocr::layer::OcrLayerOptions;
use pdfcer_core::ocr::{OcrPage, RecognizedWord};
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_core::text_edit::{
    AddTextError, AddTextRequest, EditOptions, EditRequest, FormatError, FormatOptions,
    FormatRequest, StyleSynthesis, TextRenderMode, add_text,
};
use pdfcer_core::text_extract::{self, ExtractOptions};
use pdfcer_core::writer::SaveOptions;

/// A one-page PDF from numbered objects, with a correct xref.
fn pdf(objects: &[(u32, String)]) -> Vec<u8> {
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0usize; objects.len() + 1];
    for (n, body) in objects {
        offsets[*n as usize] = out.len();
        out.extend_from_slice(format!("{n} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for off in &offsets[1..] {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

/// A page whose content stream is `content`, with Helvetica as `/F1`.
fn page(content: &str) -> Document {
    let widths = (0..95).map(|_| "500").collect::<Vec<_>>().join(" ");
    let bytes = pdf(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
             /Resources << /Font << /F1 5 0 R >> >> >>"
                .to_owned(),
        ),
        (
            4,
            format!(
                "<< /Length {} >>\nstream\n{content}\nendstream",
                content.len()
            ),
        ),
        (
            5,
            format!(
                "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
                 /Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 126 /Widths [{widths}] >>"
            ),
        ),
    ]);
    Document::from_bytes(bytes).unwrap()
}

/// `(glyphs, invisible glyphs)` on page 0 of `bytes`.
fn census(bytes: &[u8]) -> (usize, u64) {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    let text = text_extract::extract_page(&doc, &pages[0], 0, &ExtractOptions::default()).unwrap();
    let glyphs = text.runs.iter().map(|r| r.glyphs.len()).sum();
    (glyphs, text.diagnostics.invisible_glyphs)
}

/// An OCR-style layer: the producer sets `3 Tr` and never puts it back.
const OCR_LAYER: &str = "BT /F1 12 Tf 3 Tr 72 700 Td (hello) Tj 0 -20 Td (world) Tj ET";

#[test]
fn format_makes_one_ocr_run_visible_and_the_next_stays_invisible() {
    let mut s = EditSession::new(page(OCR_LAYER));
    let r = s
        .format_text(
            &FormatRequest::new(0, "hello").render_mode(0),
            &FormatOptions::default(),
        )
        .unwrap();
    assert_eq!(r.render_mode_change, Some((3.0, 0)));
    let (bytes, _) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();
    // `hello` painted, `world` still invisible: the ambient 3 came back.
    assert_eq!(census(&bytes), (10, 5));
}

#[test]
fn format_can_hide_a_run_and_restores_the_painted_ambient() {
    let mut s = EditSession::new(page(
        "BT /F1 12 Tf 72 700 Td (hello) Tj 0 -20 Td (world) Tj ET",
    ));
    let r = s
        .format_text(
            &FormatRequest::new(0, "hello").render_mode(TextRenderMode::Invisible),
            &FormatOptions::default(),
        )
        .unwrap();
    assert!(
        r.disclosures.iter().any(|d| d.contains("INVISIBLE")),
        "{:?}",
        r.disclosures
    );
    let (bytes, _) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();
    assert_eq!(census(&bytes), (10, 5));
}

#[test]
fn format_refuses_a_mode_that_does_not_exist() {
    let mut s = EditSession::new(page(OCR_LAYER));
    let err = s
        .format_text(
            &FormatRequest::new(0, "hello").render_mode(8),
            &FormatOptions::default(),
        )
        .unwrap_err();
    assert!(
        matches!(err, FormatError::InvalidRenderMode { mode: 8 }),
        "{err}"
    );
}

#[test]
fn format_refuses_a_mode_with_synthetic_bold() {
    let mut s = EditSession::new(page(OCR_LAYER));
    let err = s
        .format_text(
            &FormatRequest::new(0, "hello")
                .render_mode(3)
                .synthetic(StyleSynthesis::Bold),
            &FormatOptions::default(),
        )
        .unwrap_err();
    assert!(matches!(err, FormatError::ConflictingRenderMode), "{err}");
}

#[test]
fn a_mode_with_bold_is_accepted_when_a_real_bold_face_binds() {
    // Bold through the ladder binds Helvetica-Bold, so nothing writes `2 Tr`
    // and the requested mode stands.
    let mut s = EditSession::new(page(OCR_LAYER));
    let r = s
        .format_text(
            &FormatRequest::new(0, "hello")
                .render_mode(0)
                .style(StyleSynthesis::Bold),
            &FormatOptions::default(),
        )
        .unwrap();
    assert_eq!(r.base_font, "Helvetica-Bold");
    assert_eq!(r.render_mode_change, Some((3.0, 0)));
}

#[test]
fn added_text_is_visible_after_a_producer_left_3_tr_in_force() {
    let doc = page(OCR_LAYER);
    let out = add_text(&doc, &AddTextRequest::new(0, (72.0, 600.0), "Hi")).unwrap();
    // 12 glyphs, and only the OCR layer's 10 are invisible.
    assert_eq!(census(&out.bytes), (12, 10));
}

#[test]
fn added_text_in_mode_3_is_invisible_and_disclosed() {
    let doc = page("BT /F1 12 Tf 72 700 Td (hello) Tj ET");
    let out = add_text(
        &doc,
        &AddTextRequest::new(0, (72.0, 600.0), "Hi").with_render_mode(3),
    )
    .unwrap();
    assert_eq!(census(&out.bytes), (7, 2));
    assert!(
        out.report
            .disclosures
            .iter()
            .any(|d| d.contains("INVISIBLE")),
        "{:?}",
        out.report.disclosures
    );
}

#[test]
fn added_text_refuses_a_mode_that_does_not_exist() {
    let doc = page("BT /F1 12 Tf 72 700 Td (hello) Tj ET");
    let err = add_text(
        &doc,
        &AddTextRequest::new(0, (72.0, 600.0), "Hi").with_render_mode(9),
    )
    .unwrap_err();
    assert!(
        matches!(err, AddTextError::InvalidRenderMode { mode: 9 }),
        "{err}"
    );
}

/// `G034`'s second ask: edit an OCR layer written by the real writer, with
/// `edit_text` on one word and `format_text` on another, save, re-extract, and
/// every glyph must still be invisible.
#[test]
fn editing_a_real_ocr_layer_leaves_every_glyph_invisible() {
    let mut s = EditSession::new(page("q Q"));
    let recognised = OcrPage {
        words: vec![
            RecognizedWord {
                text: "1NVOICE".to_owned(),
                rect: Rect::from_corners(72.0, 700.0, 200.0, 712.0),
                confidence: Some(0.9),
            },
            RecognizedWord {
                text: "TOTAL".to_owned(),
                rect: Rect::from_corners(72.0, 600.0, 160.0, 612.0),
                confidence: Some(0.9),
            },
        ],
        confidence_available: true,
    };
    s.add_ocr_layer(
        &[OcrPageLayer {
            page_index: 0,
            recognised: &recognised,
        }],
        &OcrLayerOptions::new(),
    )
    .unwrap();
    s.edit_text(
        &EditRequest::find_replace(0, "1NVOICE", "INVOICE"),
        &EditOptions::default(),
    )
    .unwrap();
    s.format_text(
        &FormatRequest::new(0, "TOTAL").size(14.0),
        &FormatOptions::default(),
    )
    .unwrap();
    let (bytes, _) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();
    let (glyphs, invisible) = census(&bytes);
    assert_eq!(glyphs, 12, "INVOICE + TOTAL");
    assert_eq!(invisible, 12, "every OCR glyph must still be mode 3");
}

#[test]
fn the_typed_mode_round_trips_through_u8_and_rejects_8() {
    for m in 0..=7u8 {
        assert_eq!(u8::from(TextRenderMode::try_from(m).unwrap()), m);
    }
    assert_eq!(TextRenderMode::try_from(8), Err(8));
    let invisible: Vec<u8> = (0..=7)
        .filter(|m| TextRenderMode::try_from(*m).unwrap().is_invisible())
        .collect();
    assert_eq!(invisible, [3, 7]);
}
