//! `EditSession::merge_text_runs` — merge consecutive show operators of one
//! text object into one (`G035`).
//!
//! What these tests hold the verb to:
//!
//! - the merged run shows every run's characters, in order, with the
//!   separator between them;
//! - under the default span fit it reaches from the first run's origin to the
//!   last run's end; under the natural fit it keeps the first run's scale;
//! - nothing after the merge moves, and an OCR word stays invisible;
//! - one undo entry takes it back;
//! - the refusals are named, record nothing, and the preflight agrees.
//!
//! Every glyph is 500 units wide, so at 10 pt each is exactly 5 pt.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, OcrPageLayer};
use pdfcer_core::ocr::layer::OcrLayerOptions;
use pdfcer_core::ocr::{OcrPage, RecognizedWord};
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_core::text_edit::{FormatError, MergeFit, MergeOptions, MergeSeparator};
use pdfcer_core::text_extract::{self, ExtractOptions};
use pdfcer_core::vector::{
    Bounds, Matrix, TextObject, VectorEditError, VectorObject, decompose_page, text_merge_refusal,
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

/// The page's extracted text, runs joined with `|`.
fn words(bytes: &[u8]) -> String {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    let text = text_extract::extract_page(&doc, &pages[0], 0, &ExtractOptions::default()).unwrap();
    text.runs
        .iter()
        .map(|r| r.text.as_str())
        .collect::<Vec<_>>()
        .join("|")
}

fn close(label: &str, got: f64, want: f64) {
    assert!((got - want).abs() <= EPS, "{label}: want {want}, got {got}");
}

fn merge(s: &mut EditSession, runs: &[usize], opts: &MergeOptions) {
    s.merge_text_runs(0, 0, runs, opts)
        .unwrap_or_else(|e| panic!("merge {runs:?}: {e}"));
}

/// `hel` at x=72, `lo` at x=100 on the same baseline, then `next` on a line
/// of its own.
const SPLIT: &str = "BT /F1 10 Tf 1 0 0 1 72 700 Tm (hel) Tj 1 0 0 1 100 700 Tm (lo) Tj \
                     1 0 0 1 72 680 Tm (next) Tj ET";

/// `b` is placed; `c` inherits its origin from `b`'s end.
const INHERITED_NEXT: &str =
    "BT /F1 10 Tf 1 0 0 1 72 700 Tm (a) Tj 1 0 0 1 90 700 Tm (b) Tj (c) Tj ET";

// ===========================================================================
// What a merge produces
// ===========================================================================

#[test]
fn the_span_fit_reaches_from_the_first_origin_to_the_last_end() {
    let mut s = session(SPLIT);
    let before = boxes(&save(&s));
    let r = s
        .merge_text_runs(0, 0, &[0, 1], &MergeOptions::default())
        .unwrap();
    assert_eq!(r.text, "hello");
    assert_eq!(r.runs_merged, 2);
    // 5 glyphs at 5 pt = 25 pt natural; the span is 72..110 = 38 pt.
    let (_, pct) = r.h_scale_change.expect("the span fit sets Tz");
    close("Tz", pct, 152.0);
    let saved = save(&s);
    let after = boxes(&saved);
    assert_eq!(after.len(), 2, "two runs became one");
    close("merged min.x", after[0].min.x, 72.0);
    close("merged max.x", after[0].max.x, 110.0);
    close("next min.x", after[1].min.x, before[2].min.x);
    close("next min.y", after[1].min.y, before[2].min.y);
    close("next width", after[1].max.x - after[1].min.x, 20.0);
    let w = words(&saved);
    assert!(w.starts_with("hello|") && w.ends_with("|next"), "{w}");
}

#[test]
fn the_natural_fit_keeps_the_first_runs_scale() {
    let mut s = session(SPLIT);
    let r = s
        .merge_text_runs(
            0,
            0,
            &[0, 1],
            &MergeOptions::default().fit(MergeFit::Natural),
        )
        .unwrap();
    assert_eq!(r.h_scale_change, None);
    let after = boxes(&save(&s));
    close("merged width", after[0].max.x - after[0].min.x, 25.0);
}

#[test]
fn a_separator_is_encoded_in_the_runs_font() {
    let mut s = session(SPLIT);
    let opts = MergeOptions::default()
        .separator(MergeSeparator::Space)
        .fit(MergeFit::Natural);
    let r = s.merge_text_runs(0, 0, &[0, 1], &opts).unwrap();
    assert_eq!(r.text, "hel lo");
    assert!(
        words(&save(&s)).starts_with("hel lo"),
        "{}",
        words(&save(&s))
    );

    let mut s = session(SPLIT);
    let opts = MergeOptions::default().separator(MergeSeparator::Text("-".to_owned()));
    let r = s.merge_text_runs(0, 0, &[0, 1], &opts).unwrap();
    assert_eq!(r.text, "hel-lo");
    // Six glyphs now span the same 38 pt.
    let (_, pct) = r.h_scale_change.unwrap();
    close("Tz", pct, 38.0 / 30.0 * 100.0);
}

#[test]
fn a_tj_split_word_with_inherited_positions_merges() {
    // `(Inv) Tj (oice) Tj`: the second run inherits its origin, so the span
    // already equals the natural width and no Tz is written.
    let mut s = session("BT /F1 10 Tf 1 0 0 1 72 700 Tm (Inv) Tj (oice) Tj ET");
    let r = s
        .merge_text_runs(0, 0, &[0, 1], &MergeOptions::default())
        .unwrap();
    assert_eq!(
        r.h_scale_change, None,
        "the span already equals the natural width"
    );
    let saved = save(&s);
    assert_eq!(words(&saved), "Invoice");
    let b = boxes(&saved);
    assert_eq!(b.len(), 1);
    close("width", b[0].max.x - b[0].min.x, 35.0);
}

#[test]
fn kerning_inside_a_tj_is_kept() {
    let mut s =
        session("BT /F1 10 Tf 1 0 0 1 72 700 Tm [(A) -1000 (B)] TJ 1 0 0 1 120 700 Tm (C) Tj ET");
    let r = s
        .merge_text_runs(0, 0, &[0, 1], &MergeOptions::default())
        .unwrap();
    // Natural: A(5) + kern(10) + B(5) + C(5) = 25; the span is 72..125 = 53.
    let (_, pct) = r.h_scale_change.unwrap();
    close("Tz", pct, 212.0);
    let b = boxes(&save(&s));
    close("merged max.x", b[0].max.x, 125.0);
}

#[test]
fn a_differing_horizontal_scale_is_not_a_refusal() {
    let mut s =
        session("BT /F1 10 Tf 1 0 0 1 72 700 Tm (hel) Tj 1 0 0 1 100 700 Tm 50 Tz (lo) Tj ET");
    merge(&mut s, &[0, 1], &MergeOptions::default());
    let b = boxes(&save(&s));
    // `lo` at 50% is 5 pt wide, so the span is 72..105.
    assert_eq!(b.len(), 1);
    close("merged max.x", b[0].max.x, 105.0);
}

#[test]
fn undo_restores_the_original() {
    let mut s = session(SPLIT);
    let original = save(&s);
    merge(&mut s, &[0, 1], &MergeOptions::default());
    assert_eq!(s.undo_depth(), 1, "one undo entry");
    assert!(s.undo().is_some());
    let back = save(&s);
    assert_eq!(boxes(&back), boxes(&original));
    assert_eq!(words(&back), words(&original));
}

// ===========================================================================
// OCR: the case the request is for
// ===========================================================================

#[test]
fn two_ocr_words_merge_and_stay_invisible() {
    let mut s = session("q Q");
    let word = |text: &str, x0: f64, x1: f64| RecognizedWord {
        text: text.to_owned(),
        rect: Rect::from_corners(x0, 700.0, x1, 712.0),
        confidence: Some(0.9),
    };
    let recognised = OcrPage {
        words: vec![word("Inv", 72.0, 95.0), word("oice", 96.0, 130.0)],
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
    let first = texts(&laid)[object].runs[0].bounds;
    let last = texts(&laid)[object].runs[1].bounds;

    let r = s
        .merge_text_runs(0, object, &[0, 1], &MergeOptions::default())
        .unwrap_or_else(|e| panic!("merge the OCR words: {e}"));
    assert_eq!(r.text, "Invoice");
    let merged = save(&s);
    assert_eq!(census(&merged), (7, 7), "every OCR glyph is still mode 3");
    let runs = &texts(&merged)[object].runs;
    assert_eq!(runs.len(), 1);
    close("merged min.x", runs[0].bounds.min.x, first.min.x);
    close("merged max.x", runs[0].bounds.max.x, last.max.x);
    assert!(words(&merged).contains("Invoice"), "{}", words(&merged));
}

// ===========================================================================
// Refusals: named, record nothing, and the preflight agrees
// ===========================================================================

fn refused(content: &str, runs: &[usize]) -> FormatError {
    let mut s = session(content);
    let e = s
        .merge_text_runs(0, 0, runs, &MergeOptions::default())
        .expect_err("the merge is refused");
    assert_eq!(s.undo_depth(), 0, "a refusal records nothing: {e}");
    e
}

fn preflight(content: &str, runs: &[usize]) -> Option<VectorEditError> {
    text_merge_refusal(&texts(&save(&session(content)))[0], runs)
}

#[test]
fn structural_refusals_match_the_preflight() {
    let cases: [(&str, &[usize]); 4] = [
        (SPLIT, &[0]),
        (SPLIT, &[0, 5]),
        (SPLIT, &[0, 2]),
        (INHERITED_NEXT, &[0, 1]),
    ];
    for (content, runs) in cases {
        let e = refused(content, runs);
        let pre = preflight(content, runs).expect("the preflight refuses too");
        let FormatError::TextRun(v) = e else {
            panic!("{runs:?}: {e:?}")
        };
        assert_eq!(v.to_string(), pre.to_string(), "{runs:?}");
    }
    assert!(matches!(
        preflight(SPLIT, &[0]),
        Some(VectorEditError::MergeNeedsTwoRuns { count: 1 })
    ));
    assert!(matches!(
        preflight(SPLIT, &[0, 5]),
        Some(VectorEditError::TextRunOutOfRange { index: 5, count: 3 })
    ));
    assert!(matches!(
        preflight(SPLIT, &[0, 2]),
        Some(VectorEditError::MergeRunsNotContiguous { after: 0, next: 2 })
    ));
    assert!(matches!(
        preflight(INHERITED_NEXT, &[0, 1]),
        Some(VectorEditError::MergeWouldMoveNextRun { index: 2 })
    ));
    assert!(preflight(SPLIT, &[0, 1]).is_none());
    assert!(preflight(SPLIT, &[1, 2]).is_none());
    assert!(preflight(INHERITED_NEXT, &[0, 1, 2]).is_none());
}

#[test]
fn a_differing_text_state_is_refused_by_name() {
    let cases = [
        ("12 Tf", "font size"),
        ("2 Tc", "character spacing (Tc)"),
        ("3 Tw", "word spacing (Tw)"),
        ("1 Tr", "render mode (Tr)"),
        ("1 0 0 rg", "fill colour"),
        ("3 Ts", "rise (Ts)"),
    ];
    for (between, want) in cases {
        let content = format!(
            "BT /F1 10 Tf 1 0 0 1 72 700 Tm (hel) Tj 1 0 0 1 100 700 Tm {between} (lo) Tj ET"
        );
        let e = refused(&content, &[0, 1]);
        assert!(
            matches!(e, FormatError::MergeStateDiffers { run: 1, parameter } if parameter == want),
            "{between}: {e:?}"
        );
    }
}

#[test]
fn marked_content_between_the_runs_is_refused() {
    let e = refused(
        "BT /F1 10 Tf 1 0 0 1 72 700 Tm (hel) Tj /Span BMC 1 0 0 1 100 700 Tm (lo) Tj EMC ET",
        &[0, 1],
    );
    assert!(matches!(e, FormatError::MergeCrossesMarkedContent), "{e:?}");
}

#[test]
fn a_quote_show_is_refused() {
    let e = refused(
        "BT /F1 10 Tf 12 TL 1 0 0 1 72 700 Tm (hel) Tj (lo) ' ET",
        &[0, 1],
    );
    assert!(
        matches!(e, FormatError::MergeLineShowOperator { run: 1 }),
        "{e:?}"
    );
}

#[test]
fn runs_out_of_order_along_the_baseline_refuse_the_span_fit() {
    let content = "BT /F1 10 Tf 1 0 0 1 200 700 Tm (hel) Tj 1 0 0 1 72 700 Tm (lo) Tj ET";
    let e = refused(content, &[0, 1]);
    assert!(matches!(e, FormatError::MergeRunsOutOfOrder), "{e:?}");
    // The natural fit has no span to compute, so it proceeds.
    let mut s = session(content);
    s.merge_text_runs(
        0,
        0,
        &[0, 1],
        &MergeOptions::default().fit(MergeFit::Natural),
    )
    .unwrap();
}
