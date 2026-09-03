//! # `Pass 71.0` integration test — the OCR sandwich, end to end
//!
//! Drives [`pdfcer_core::ocr::layer::add_ocr_layer`] over the committed
//! `fixtures/synthetic/addtext/` fixtures (provenance: that directory's
//! `PROVENANCE.md`) and asserts the four properties that make an OCR text
//! layer *correct* rather than merely *present*:
//!
//! | Property | Asserted by |
//! |---|---|
//! | the words come back out — the stream parses, the font resource resolves, `Tm`/`Tz` are honoured | `the_recognised_words_extract_from_the_saved_file` |
//! | the original file is untouched — rule 3, and the scan is never re-encoded | `the_original_bytes_are_a_prefix_of_the_output` |
//! | the geometry survives the round trip — extracted positions land on the reported boxes | `extracted_word_positions_land_on_the_reported_boxes` |
//! | the §7.7.3.4 inheritance trap is handled | `an_inherited_resources_page_gets_its_own_without_touching_the_ancestor` |
//!
//! ## Why the extraction test is the load-bearing one
//!
//! Everything this feature does is **invisible by construction**. There is no
//! visual symptom of a broken OCR layer: the page looks correct whether the
//! layer is perfect, mis-scaled, mirrored, or absent entirely. The only
//! observable is extraction, so an end-to-end extract is not a nicety here —
//! it is the *only* oracle that distinguishes "wrote a text layer" from
//! "wrote 900 bytes that happen to parse".
//!
//! ## The absence-assertion trap, avoided deliberately
//!
//! `Pass 69.0` filed the lesson that **an absence assertion on PDF bytes is
//! vacuous under an incremental save**, because a superseded object is still
//! physically in the file. Nothing here asserts an absence of bytes. The
//! round-trip test asserts a **prefix**, which is the positive form of the
//! same claim and is exactly what `ARCHITECTURE.md`'s save-mode table
//! guarantees for a non-empty dirty set (`output.starts_with(input)`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::ocr::layer::{OcrLayerError, OcrLayerOptions, add_ocr_layer};
use pdfcer_core::ocr::{OcrPage, RecognizedWord};
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_core::text_extract::{self, ExtractOptions};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/addtext")
        .join(name)
}

fn load(name: &str) -> Document {
    Document::load(&fixture(name)).expect("fixture loads")
}

/// A recognised word with a confidence, positioned in PAGE space (y-up).
///
/// Deliberately *not* run through `words_to_page_space`: that conversion has
/// its own unit tests, and threading it through here would make a failure
/// ambiguous between the flip and the writer.
fn word(text: &str, llx: f64, lly: f64, urx: f64, ury: f64) -> RecognizedWord {
    RecognizedWord {
        text: text.to_owned(),
        rect: Rect::from_corners(llx, lly, urx, ury),
        confidence: Some(0.87),
    }
}

/// Three words laid out as a plausible scanned line.
fn sample_page() -> OcrPage {
    OcrPage {
        words: vec![
            word("INVOICE", 72.0, 700.0, 148.0, 712.0),
            word("Number", 160.0, 700.0, 220.0, 712.0),
            word("40129", 228.0, 700.0, 272.0, 712.0),
        ],
        confidence_available: true,
    }
}

/// Every text run extracted from page 0 of `bytes`, concatenated.
fn page0_text(bytes: &[u8]) -> String {
    let doc = Document::from_bytes(bytes.to_vec()).expect("output reloads");
    let pages = page_tree::pages(&doc).expect("page tree walks");
    let page = text_extract::extract_page(&doc, &pages[0], 0, &ExtractOptions::default())
        .expect("page 0 extracts");
    page.runs
        .iter()
        .map(|r| r.text.as_str())
        .collect::<String>()
}

/// ★ A certified document that forbids change does not get an OCR layer.
///
/// Writing the layer creates a content stream and a font and rewrites the
/// page dict's `/Contents` and `/Resources` — a structural page change.
/// §12.8.4 Table 258 requires a consumer to enforce the permissions a
/// `/Perms` → `/DocMDP` certification carries, and Table 254's
/// permitted-change lists contain no operation pdfcer can perform, so the only
/// correct answer is a refusal.
///
/// WHY THIS TEST EXISTS RATHER THAN BEING ASSUMED: `add_ocr_layer` shipped
/// (commit 49af8fb) with no certification check at all, while its structural
/// twin `add_text` had refused since it was written. The two paths do the
/// same thing to a page and disagreed about whether a signature could stop
/// them. `tools/check-bypass-paths.sh` surfaced it — the function is an
/// `EditSession` bypass, and the exception list's warrant for sanctioning
/// that shape is precisely that such a function "honours the certification
/// gate". It did not.
#[test]
fn a_certified_document_refuses_the_ocr_layer() {
    let doc = load("certified-locked.pdf");
    let err = add_ocr_layer(&doc, 0, &sample_page(), &OcrLayerOptions::new())
        .expect_err("a certified, change-forbidding document must refuse");

    assert!(
        matches!(
            err,
            OcrLayerError::CertificationForbidsChange { permission: 1 }
        ),
        "expected a certification refusal carrying the DocMDP /P value, got {err:?} \
         — a refusal that does not name the permission leaves the operator \
         unable to tell a locked document from a broken one",
    );
}

/// ★ The words go in and the words come out.
///
/// If this fails, nothing else about the feature matters — the layer is
/// invisible, so extraction is the only way anyone will ever observe it.
#[test]
fn the_recognised_words_extract_from_the_saved_file() {
    let doc = load("plain.pdf");
    let out = add_ocr_layer(&doc, 0, &sample_page(), &OcrLayerOptions::new())
        .expect("the layer is written");

    assert_eq!(out.report.words_written, 3);
    assert_eq!(out.report.words_skipped, 0);
    assert_eq!(out.report.words_substituted, 0);

    let text = page0_text(&out.bytes);
    for expected in ["INVOICE", "Number", "40129"] {
        assert!(
            text.contains(expected),
            "the OCR word {expected:?} must extract from the saved file; got {text:?}"
        );
    }
}

/// ★ Round-trip / minimal diff (project rule 3): nothing that existed is
/// rewritten.
///
/// The incremental save's contract for a non-empty dirty set is that the input
/// is a byte-prefix of the output. For OCR specifically this is the guarantee
/// that **the scan itself was never decoded and re-encoded** — the promise the
/// module header makes about not costing generation loss on an image whose
/// provenance the operator may need to defend. A prefix assertion proves it
/// for every object at once, including ones this test never names.
#[test]
fn the_original_bytes_are_a_prefix_of_the_output() {
    let doc = load("plain.pdf");
    let original = doc.bytes().to_vec();
    let out = add_ocr_layer(&doc, 0, &sample_page(), &OcrLayerOptions::new())
        .expect("the layer is written");

    assert!(
        out.bytes.starts_with(&original),
        "an incremental save must append: the original {} bytes must be an \
         untouched prefix of the {} written",
        original.len(),
        out.bytes.len()
    );
    assert!(out.bytes.len() > original.len(), "something was appended");
}

/// ★ The geometry survives the whole pipeline.
///
/// The unit tests solve the fit arithmetically; this one proves the solved
/// numbers actually reach the extractor through the emitted `Tm`/`Tf`/`Tz`. It
/// is the test that catches a y-flip, a percentage/ratio confusion in `Tz`, or
/// a baseline placed at the box bottom instead of a descender above it — every
/// one of which leaves a page that looks perfect and selects wrong.
///
/// # The two tolerances differ on purpose, and both were MEASURED
///
/// A first draft used a loose 6 pt on both axes, chosen by eye. Measuring the
/// real error showed that to be wrong in *both* directions:
///
/// - **`dx` is exactly 0.** The horizontal fit *solves* for `Tz`, so the
///   advance equals the reported box width to the last bit. A 6 pt tolerance
///   would have accepted a completely broken fit while looking rigorous.
/// - **`dy` is a constant 0.558 pt**, and chasing that constant turned up
///   something worth recording rather than papering over: **the two sides use
///   different box conventions and both are correct.** `TextRun::bbox` is the
///   *block model's* box — a nominal 0.75/0.25 em ascent/descent, shared with
///   the `Pass 15.x` reflow engine so a run's box and a reflowed line's box
///   agree. The OCR layer instead fits Helvetica's *real* AFM metrics
///   (0.718/0.207, from `fontdata`'s descriptor table, and rung 3 of the
///   extractor's own vertical ladder). The gap is `(0.25 − 0.207) × size`,
///   which at this size is 0.558 pt.
///
/// So `dy < 1.0` is not slack — it is that convention gap plus a little. It
/// stays tight enough to catch what actually goes wrong: a baseline placed at
/// the box bottom rather than a descender above it gives `dy ≈ 2.7`, and a
/// y-flip gives hundreds. Both were made to happen and seen to fail this
/// assertion before it was trusted.
#[test]
fn extracted_word_positions_land_on_the_reported_boxes() {
    let doc = load("plain.pdf");
    let ocr = sample_page();
    let out = add_ocr_layer(&doc, 0, &ocr, &OcrLayerOptions::new()).expect("written");

    let saved = Document::from_bytes(out.bytes.clone()).expect("reloads");
    let pages = page_tree::pages(&saved).expect("page tree");
    let page = text_extract::extract_page(&saved, &pages[0], 0, &ExtractOptions::default())
        .expect("extracts");

    for expected in &ocr.words {
        let found = page
            .runs
            .iter()
            .find(|r| r.text.contains(expected.text.as_str()))
            .unwrap_or_else(|| panic!("run for {:?} not found", expected.text));

        // `bbox` is `Option` because a run of zero-advance glyphs has no
        // extent; an OCR run always has one, so its absence is itself a defect
        // worth failing on rather than skipping past.
        let bbox = found
            .bbox
            .unwrap_or_else(|| panic!("the run for {:?} has no bbox", expected.text));
        let dx = (bbox.llx - expected.rect.llx).abs();
        let dy = (bbox.lly - expected.rect.lly).abs();
        assert!(
            dx < 0.01 && dy < 1.0,
            "{:?} was written at ({}, {}) but extracts at ({}, {}) — dx={dx:.4} \
             dy={dy:.4}; a large dy is the y-flip or a baseline placed at the \
             box bottom",
            expected.text,
            expected.rect.llx,
            expected.rect.lly,
            bbox.llx,
            bbox.lly
        );

        // ★ The WIDTH is what tests the `Tz` fit, and the origin alone is not.
        //
        // The first version of this test asserted only on `llx`/`lly` and
        // claimed in its own message that a large `dx` meant a broken `Tz`. The
        // sabotage run disproved that: emitting `Tz` as the raw ratio instead
        // of the percentage (a 100x error, and the exact confusion the spec
        // corpus flags) left `dx` at 0.0000 and the test PASSED. `Tm` sets the
        // left edge, so the origin is right no matter how wrong the scaling is
        // — the error is entirely in the extent.
        //
        // This assertion is the one that fails under that sabotage. It is the
        // reason the sabotage check is mandatory rather than a formality: the
        // test looked thorough, had a message describing this very defect, and
        // could not detect it.
        let dw = ((bbox.urx - bbox.llx) - (expected.rect.urx - expected.rect.llx)).abs();
        assert!(
            dw < 0.01,
            "{:?} was written {} pt wide but extracts {} pt wide — dw={dw:.4}; \
             this is the Tz fit, and a ~100x error here is the percentage/ratio \
             confusion (§9.3.4: Th = Tz/100)",
            expected.text,
            expected.rect.urx - expected.rect.llx,
            bbox.urx - bbox.llx
        );
    }
}

/// The §7.7.3.4 inheritance trap: the page gets its OWN `/Resources`, and the
/// shared ancestor's is not disturbed.
///
/// Writing an own `/Resources` holding only the new OCR font would **shadow**
/// the inherited one and silently break every other resource the page uses —
/// the page would lose its existing fonts and images while gaining searchable
/// text. This is the same trap `add_text` documents; the OCR path builds its
/// page dict from the effective (own-or-inherited) resources for exactly this
/// reason, and this test is what proves it rather than the comment.
#[test]
fn an_inherited_resources_page_gets_its_own_without_touching_the_ancestor() {
    let doc = load("inherited-resources.pdf");
    let before = page0_text(doc.bytes());
    let out = add_ocr_layer(&doc, 0, &sample_page(), &OcrLayerOptions::new()).expect("written");

    let after = page0_text(&out.bytes);
    assert!(
        after.contains("INVOICE"),
        "the OCR layer must be present: {after:?}"
    );
    for existing in before.split_whitespace().take(3) {
        assert!(
            after.contains(existing),
            "the page's PRE-EXISTING text {existing:?} must survive — losing it \
             means the new /Resources shadowed the inherited one; got {after:?}"
        );
    }
}

/// A page whose words are all unplaceable refuses by name rather than writing
/// an empty stream and reporting success.
#[test]
fn a_page_with_nothing_placeable_refuses_by_name() {
    let doc = load("plain.pdf");
    let empty = OcrPage {
        words: vec![word("", 0.0, 0.0, 10.0, 10.0)],
        confidence_available: false,
    };
    match add_ocr_layer(&doc, 0, &empty, &OcrLayerOptions::new()) {
        Err(OcrLayerError::NothingToWrite) => {}
        other => panic!("expected NothingToWrite, got {other:?}"),
    }
}

/// An out-of-range page is a named refusal, made before anything is allocated.
#[test]
fn an_out_of_range_page_refuses_by_name() {
    let doc = load("plain.pdf");
    match add_ocr_layer(&doc, 99, &sample_page(), &OcrLayerOptions::new()) {
        Err(OcrLayerError::PageIndex(99)) => {}
        other => panic!("expected PageIndex(99), got {other:?}"),
    }
}

/// The rule-4 disclosure is populated and names the confidence.
///
/// Decision 059 moved rule 4's obligation off the canvas and onto exactly this
/// report. A layer written with an empty disclosure list would be pdfcer being
/// silent about a page of guesses — which is the one thing the amended rule
/// still forbids outright.
#[test]
fn the_disclosure_report_is_populated() {
    let doc = load("plain.pdf");
    let out = add_ocr_layer(&doc, 0, &sample_page(), &OcrLayerOptions::new()).expect("written");
    let lines = out.report.disclosures();
    assert!(!lines.is_empty(), "rule 4: the report must say something");
    let joined = lines.join(" ");
    assert!(
        joined.contains("invisible"),
        "the disclosure must state that the layer is invisible: {joined}"
    );
    assert!(
        joined.contains("confidence"),
        "the disclosure must state the confidence: {joined}"
    );
    assert!(out.report.content_object > 0 && out.report.font_object > 0);
}
