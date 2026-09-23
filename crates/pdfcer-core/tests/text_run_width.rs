//! `EditSession::set_text_run_width` — fit ONE show operator to a page width
//! by setting its horizontal scaling (`G038`, ISO 32000-1 §9.3.4, `Tz`).
//!
//! What these tests hold the verb to:
//!
//! - the run's advance on the PAGE equals the width asked for, measured along
//!   its own baseline (so a rotated or scaled `Tm` is converted, not ignored);
//! - the scale is absolute: a run that already carries a `Tz`, or is fitted
//!   twice, gets the same result, never a compounded one;
//! - nothing after the run moves, including a run whose origin is inherited
//!   from this one's advance;
//! - an invisible OCR word (`3 Tr`) stays invisible;
//! - the refusals are named, and the free preflight agrees with the verb.
//!
//! Every document here gives each glyph a 500-unit width, so a five-glyph
//! word at 10 pt is exactly 25 pt wide and every expected number is exact.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, OcrPageLayer};
use pdfcer_core::ocr::layer::OcrLayerOptions;
use pdfcer_core::ocr::{OcrPage, RecognizedWord};
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_core::text_edit::{FormatError, FormatReport};
use pdfcer_core::text_extract::{self, ExtractOptions};
use pdfcer_core::vector::edit::text_run_width_refusal;
use pdfcer_core::vector::{
    Bounds, Matrix, TextObject, VectorEditError, VectorObject, decompose_page,
};
use pdfcer_core::writer::SaveOptions;

const EPS: f64 = 1e-3;

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

/// A page whose content stream is `content`, with `/F1` = Helvetica at a
/// uniform 500-unit width.
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

fn session(content: &str) -> EditSession {
    EditSession::new(page(content))
}

fn save(s: &EditSession) -> Vec<u8> {
    s.to_incremental_bytes(&SaveOptions::identity())
        .expect("session saves")
        .0
}

/// Every text object on page 0 of `bytes`, re-opened from the saved file.
fn texts(bytes: &[u8]) -> Vec<TextObject> {
    let doc = Document::from_bytes(bytes.to_vec()).expect("the saved file re-opens");
    let pages = page_tree::pages(&doc).expect("pages");
    let model = decompose_page(&doc.view(), &pages[0], Matrix::IDENTITY).expect("decomposes");
    model
        .objects
        .iter()
        .filter_map(|o| match o {
            VectorObject::Text(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

/// The boxes of every run of the first text object of `bytes`.
fn boxes(bytes: &[u8]) -> Vec<Bounds> {
    texts(bytes)[0].runs.iter().map(|r| r.bounds).collect()
}

/// `(glyphs, invisible glyphs)` on page 0 of `bytes`.
fn census(bytes: &[u8]) -> (usize, u64) {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    let text = text_extract::extract_page(&doc, &pages[0], 0, &ExtractOptions::default()).unwrap();
    let glyphs = text.runs.iter().map(|r| r.glyphs.len()).sum();
    (glyphs, text.diagnostics.invisible_glyphs)
}

fn fit(s: &mut EditSession, run: usize, width: f64) -> FormatReport {
    s.set_text_run_width(0, 0, run, width)
        .unwrap_or_else(|e| panic!("fit run {run} to {width}: {e}"))
}

fn close(label: &str, got: f64, want: f64) {
    assert!((got - want).abs() <= EPS, "{label}: want {want}, got {got}");
}

const TWO_ABSOLUTE: &str =
    "BT /F1 10 Tf 1 0 0 1 72 700 Tm (hello) Tj 1 0 0 1 72 680 Tm (world) Tj ET";

// ===========================================================================
// The width lands where asked
// ===========================================================================

/// The whole claim: the run is as wide on the page as asked, its origin did
/// not move, and the other run is untouched.
#[test]
fn a_run_is_fitted_to_the_width_asked_and_keeps_its_origin() {
    let before = boxes(&save(&session(TWO_ABSOLUTE)));
    close("natural width", before[0].max.x - before[0].min.x, 25.0);

    let mut s = session(TWO_ABSOLUTE);
    let r = fit(&mut s, 0, 50.0);
    assert_eq!(r.h_scale_change.map(|c| c.1), Some(200.0));
    assert!(
        r.disclosures.iter().any(|d| d.contains("50.00 pt")),
        "the fit is disclosed: {:?}",
        r.disclosures
    );
    assert!(
        !r.disclosures.iter().any(|d| d.contains("relaid out")),
        "nothing was relaid out, so no disclosure may say so: {:?}",
        r.disclosures
    );

    let after = boxes(&save(&s));
    assert_eq!(after.len(), 2);
    close("fitted width", after[0].max.x - after[0].min.x, 50.0);
    close("origin x", after[0].min.x, before[0].min.x);
    close("origin y", after[0].min.y, before[0].min.y);
    close("the other run min.x", after[1].min.x, before[1].min.x);
    close("the other run max.x", after[1].max.x, before[1].max.x);
}

/// A `Tm` that scales by 2 makes the natural width 50 pt on the page; asking
/// for 50 pt must give `100 Tz`, so the width is in PAGE points, not text
/// space.
#[test]
fn the_width_is_in_page_points_through_a_scaling_text_matrix() {
    let mut s = session("BT /F1 10 Tf 2 0 0 2 72 700 Tm (hello) Tj ET");
    let r = fit(&mut s, 0, 75.0);
    close("Tz", r.h_scale_change.unwrap().1, 150.0);
    let b = boxes(&save(&s))[0];
    close("fitted width", b.max.x - b.min.x, 75.0);
}

/// A run rotated a quarter turn is measured along its baseline, which is the
/// page's y axis here.
#[test]
fn a_rotated_run_is_measured_along_its_baseline() {
    let mut s = session("BT /F1 10 Tf 0 1 -1 0 300 300 Tm (hello) Tj ET");
    let r = fit(&mut s, 0, 40.0);
    close("Tz", r.h_scale_change.unwrap().1, 160.0);
    let b = boxes(&save(&s))[0];
    close("length along the baseline", b.max.y - b.min.y, 40.0);
    close("baseline origin", b.min.y, 300.0);
}

// ===========================================================================
// Absolute, never compounded
// ===========================================================================

/// A run that already carries `50 Tz` is fitted from its natural width, not
/// from its current one: the new scale replaces the old.
#[test]
fn an_existing_tz_is_replaced_not_compounded() {
    let mut s = session("BT /F1 10 Tf 50 Tz 1 0 0 1 72 700 Tm (hello) Tj ET");
    let r = fit(&mut s, 0, 50.0);
    assert_eq!(r.h_scale_change, Some((50.0, 200.0)));
    let b = boxes(&save(&s))[0];
    close("fitted width", b.max.x - b.min.x, 50.0);
}

/// Fitting the same run to the same width twice is a fixed point.
#[test]
fn fitting_twice_gives_the_same_scale() {
    let mut s = session(TWO_ABSOLUTE);
    fit(&mut s, 0, 40.0);
    let second = fit(&mut s, 0, 40.0);
    close(
        "second Tz",
        second.h_scale_change.map_or(160.0, |c| c.1),
        160.0,
    );
    let b = boxes(&save(&s))[0];
    close("fitted width", b.max.x - b.min.x, 40.0);
}

// ===========================================================================
// Nothing after it moves
// ===========================================================================

/// `world` has no coordinate of its own: it starts where `hello`'s advance
/// ends. Widening `hello` would push it right unless the verb pins it.
#[test]
fn a_run_that_inherits_its_origin_does_not_move() {
    let content = "BT /F1 10 Tf 72 700 Td (hello) Tj (world) Tj ET";
    let before = boxes(&save(&session(content)));
    close("successor starts at the advance", before[1].min.x, 97.0);

    let mut s = session(content);
    fit(&mut s, 0, 60.0);
    let after = boxes(&save(&s));
    close("fitted width", after[0].max.x - after[0].min.x, 60.0);
    close("successor min.x", after[1].min.x, before[1].min.x);
    close("successor max.x", after[1].max.x, before[1].max.x);
}

// ===========================================================================
// OCR: invisible stays invisible
// ===========================================================================

#[test]
fn a_fitted_ocr_word_stays_invisible() {
    let mut s = session("q Q");
    let recognised = OcrPage {
        words: vec![RecognizedWord {
            text: "INVOICE".to_owned(),
            rect: Rect::from_corners(72.0, 700.0, 200.0, 712.0),
            confidence: Some(0.9),
        }],
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
    let laid = save(&s);
    assert_eq!(census(&laid), (7, 7), "the layer starts invisible");
    let object = texts(&laid).len() - 1;

    let r = s
        .set_text_run_width(0, object, 0, 90.0)
        .unwrap_or_else(|e| panic!("fit the OCR word: {e}"));
    assert!(r.h_scale_change.is_some());
    let fitted = save(&s);
    assert_eq!(census(&fitted), (7, 7), "every OCR glyph is still mode 3");
    let b = texts(&fitted)[object].runs[0].bounds;
    close("fitted OCR width", b.max.x - b.min.x, 90.0);
}

// ===========================================================================
// Refusals, and the preflight agrees
// ===========================================================================

#[test]
fn a_width_that_is_not_positive_and_finite_is_refused() {
    for w in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut s = session(TWO_ABSOLUTE);
        let e = s.set_text_run_width(0, 0, 0, w).unwrap_err();
        assert!(matches!(e, FormatError::BadTargetWidth(_)), "{w}: {e:?}");
        assert_eq!(s.undo_depth(), 0, "a refusal records nothing");
    }
}

#[test]
fn an_out_of_range_run_or_object_is_refused_and_the_preflight_agrees() {
    let bytes = save(&session(TWO_ABSOLUTE));
    let text = &texts(&bytes)[0];
    assert!(text_run_width_refusal(text, 0).is_none());
    assert!(matches!(
        text_run_width_refusal(text, 2),
        Some(VectorEditError::TextRunOutOfRange { .. })
    ));

    let mut s = session(TWO_ABSOLUTE);
    let e = s.set_text_run_width(0, 0, 2, 10.0).unwrap_err();
    assert!(
        matches!(
            e,
            FormatError::TextRun(VectorEditError::TextRunOutOfRange { .. })
        ),
        "{e:?}"
    );
    let e = s.set_text_run_width(0, 9, 0, 10.0).unwrap_err();
    assert!(
        matches!(
            e,
            FormatError::TextRun(VectorEditError::ObjectOutOfRange { .. })
        ),
        "{e:?}"
    );
}

/// A `TJ` with kerning is refused by name: the format rewrite re-emits the
/// run's codes, so the kerning would be lost and the fitted width wrong.
#[test]
fn a_kerned_tj_is_refused() {
    let mut s = session("BT /F1 10 Tf 1 0 0 1 72 700 Tm [(he) -200 (llo)] TJ ET");
    let e = s.set_text_run_width(0, 0, 0, 40.0).unwrap_err();
    assert!(matches!(e, FormatError::WidthFitKerned), "{e:?}");
}

#[test]
fn undo_restores_the_natural_width() {
    let mut s = session(TWO_ABSOLUTE);
    fit(&mut s, 0, 50.0);
    assert!(s.undo().is_some());
    let b = boxes(&save(&s))[0];
    close("width after undo", b.max.x - b.min.x, 25.0);
}
