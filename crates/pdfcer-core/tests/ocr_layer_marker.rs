//! OCR layers pdfcer wrote can be found, replaced and removed (G036).
//!
//! Each layer is wrapped in a `/pdfc_OCR` marked-content sequence
//! (`ocr::marker`). The properties:
//!
//! | Property | Test |
//! |---|---|
//! | a written layer is found, with its engine | `a_written_layer_is_found_with_its_engine` |
//! | the marker survives save and reload | `the_marker_survives_a_save` |
//! | re-running OCR replaces the old layer by default | `a_rerun_replaces_the_old_layer` |
//! | `Refuse` refuses and leaves the session alone | `refuse_policy_refuses_and_changes_nothing` |
//! | `Stack` keeps both | `stack_policy_keeps_both` |
//! | removal takes the text, the font entry and the objects out; undo puts them back | `removing_a_layer_takes_its_text_and_font_out` |
//! | a stale reference is refused | `a_stale_reference_is_refused` |
//! | invisible text pdfcer did not write as OCR is never touched | `unmarked_invisible_text_is_not_a_layer` |
//! | the one-shot also replaces | `the_one_shot_replaces_too` |
//! | the tag, not just the producer entry, identifies a layer | `a_different_tag_is_not_a_layer` |
//!
//! The layer is invisible, so every text assertion goes through extraction of
//! saved bytes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditSession, OcrPageLayer};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::ocr::layer::{self, ExistingLayers, OcrLayerError, OcrLayerOptions};
use pdfcer_core::ocr::marker;
use pdfcer_core::ocr::{OcrPage, RecognizedWord};
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_core::text_edit::{AddTextRequest, TextRenderMode};
use pdfcer_core::text_extract::{self, ExtractOptions};
use pdfcer_core::writer::SaveOptions;

fn plain() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/addtext/plain.pdf")
}

fn session() -> EditSession {
    EditSession::new(Document::load(&plain()).expect("fixture loads"))
}

fn one_word(text: &str) -> OcrPage {
    OcrPage {
        words: vec![RecognizedWord {
            text: text.to_owned(),
            rect: Rect::from_corners(72.0, 700.0, 200.0, 712.0),
            confidence: Some(0.9),
        }],
        confidence_available: true,
    }
}

fn ocr(
    s: &mut EditSession,
    text: &str,
    opts: &OcrLayerOptions,
) -> Result<Vec<layer::OcrLayerReport>, OcrLayerError> {
    let page = one_word(text);
    s.add_ocr_layer(
        &[OcrPageLayer {
            page_index: 0,
            recognised: &page,
        }],
        opts,
    )
}

fn engine_opts() -> OcrLayerOptions {
    OcrLayerOptions::new().with_engine("test-engine")
}

fn page_text(bytes: &[u8]) -> String {
    let doc = Document::from_bytes(bytes.to_vec()).expect("output reloads");
    let pages = page_tree::pages(&doc).expect("page tree walks");
    text_extract::extract_page(&doc, &pages[0], 0, &ExtractOptions::default())
        .expect("page extracts")
        .runs
        .iter()
        .map(|r| r.text.as_str())
        .collect()
}

fn save(s: &EditSession) -> Vec<u8> {
    s.to_incremental_bytes(&SaveOptions::identity())
        .expect("session saves")
        .0
}

#[test]
fn a_written_layer_is_found_with_its_engine() {
    let mut s = session();
    assert!(s.find_ocr_layers().expect("walks").is_empty());
    ocr(&mut s, "FIRST", &engine_opts()).expect("written");
    let found = s.find_ocr_layers().expect("walks");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].page_index, 0);
    assert_eq!(found[0].engine.as_deref(), Some("test-engine"));
    assert_eq!(found[0].version, marker::LAYER_VERSION);
    assert_eq!(found[0].font_names.len(), 1);
}

#[test]
fn the_marker_survives_a_save() {
    let mut s = session();
    ocr(&mut s, "FIRST", &engine_opts()).expect("written");
    let doc = Document::from_bytes(save(&s)).expect("reloads");
    let found = marker::find_ocr_layers(&doc.view()).expect("walks");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].engine.as_deref(), Some("test-engine"));
}

#[test]
fn a_rerun_replaces_the_old_layer() {
    let mut s = session();
    ocr(&mut s, "FIRSTRUN", &engine_opts()).expect("first");
    let reports = ocr(&mut s, "SECONDRUN", &engine_opts()).expect("second");
    assert_eq!(reports[0].layers_replaced, 1);
    assert!(
        reports[0]
            .disclosures()
            .iter()
            .any(|l| l.contains("Replaced 1 earlier OCR layer")),
        "the replacement must be disclosed"
    );
    assert_eq!(s.find_ocr_layers().expect("walks").len(), 1);
    let text = page_text(&save(&s));
    assert!(text.contains("SECONDRUN"), "{text:?}");
    assert!(
        !text.contains("FIRSTRUN"),
        "old layer still there: {text:?}"
    );
    // The replacement is one undo: undoing it brings the first layer back.
    assert_eq!(s.undo(), Some(CommandKind::AddOcrLayer));
    let text = page_text(&save(&s));
    assert!(
        text.contains("FIRSTRUN") && !text.contains("SECONDRUN"),
        "{text:?}"
    );
}

#[test]
fn refuse_policy_refuses_and_changes_nothing() {
    let mut s = session();
    ocr(&mut s, "FIRST", &engine_opts()).expect("first");
    let before = save(&s);
    let err = ocr(
        &mut s,
        "SECOND",
        &engine_opts().with_existing(ExistingLayers::Refuse),
    )
    .expect_err("refused");
    assert!(
        matches!(
            err,
            OcrLayerError::LayerPresent {
                page_index: 0,
                count: 1
            }
        ),
        "{err:?}"
    );
    assert_eq!(save(&s), before, "a refusal must not change the session");
}

#[test]
fn stack_policy_keeps_both() {
    let mut s = session();
    ocr(&mut s, "FIRST", &engine_opts()).expect("first");
    let reports = ocr(
        &mut s,
        "SECOND",
        &engine_opts().with_existing(ExistingLayers::Stack),
    )
    .expect("second");
    assert_eq!(reports[0].layers_replaced, 0);
    assert_eq!(s.find_ocr_layers().expect("walks").len(), 2);
    let text = page_text(&save(&s));
    assert!(
        text.contains("FIRST") && text.contains("SECOND"),
        "{text:?}"
    );
}

#[test]
fn removing_a_layer_takes_its_text_and_font_out() {
    let mut s = session();
    let original = page_text(&save(&s));
    ocr(&mut s, "REMOVEME", &engine_opts()).expect("written");
    let found = s.find_ocr_layers().expect("walks");
    let layer = &found[0];
    let font_name = layer.font_names[0].clone();

    s.remove_ocr_layer(layer).expect("removed");
    assert_eq!(s.undo_kind(), Some(CommandKind::RemoveOcrLayer));
    assert!(s.find_ocr_layers().expect("walks").is_empty());

    let bytes = save(&s);
    assert_eq!(
        page_text(&bytes),
        original,
        "the page's own text is untouched"
    );

    let doc = Document::from_bytes(bytes).expect("reloads");
    let pages = page_tree::pages(&doc).expect("walks");
    let fonts = pages[0]
        .resources
        .get(b"Font")
        .and_then(|o| doc.resolve(o).as_dict());
    assert!(
        fonts.is_none_or(|d| d.get(&font_name).is_none()),
        "the layer's font entry must leave the page"
    );
    assert!(
        doc.value(layer.content).is_none(),
        "the layer's content stream must be freed"
    );

    assert_eq!(s.undo(), Some(CommandKind::RemoveOcrLayer));
    assert!(
        page_text(&save(&s)).contains("REMOVEME"),
        "undo restores it"
    );
}

#[test]
fn a_stale_reference_is_refused() {
    let mut s = session();
    ocr(&mut s, "FIRST", &engine_opts()).expect("written");
    let stale = s.find_ocr_layers().expect("walks").remove(0);
    ocr(&mut s, "SECOND", &engine_opts()).expect("replaced");
    let err = s.remove_ocr_layer(&stale).expect_err("stale");
    assert!(
        matches!(err, OcrLayerError::LayerNotFound { .. }),
        "{err:?}"
    );
    assert_eq!(s.undo_kind(), Some(CommandKind::AddOcrLayer));
}

#[test]
fn unmarked_invisible_text_is_not_a_layer() {
    let mut s = session();
    let req = AddTextRequest::new(0, (72.0, 600.0), "HIDDENTEXT")
        .with_render_mode(TextRenderMode::Invisible);
    s.add_text(&req).expect("text added");
    assert!(
        s.find_ocr_layers().expect("walks").is_empty(),
        "invisible text without the marker is not pdfcer's OCR"
    );
    let reports = ocr(&mut s, "OCRWORD", &engine_opts()).expect("written");
    assert_eq!(reports[0].layers_replaced, 0);
    let text = page_text(&save(&s));
    assert!(
        text.contains("HIDDENTEXT") && text.contains("OCRWORD"),
        "{text:?}"
    );
}

#[test]
fn the_one_shot_replaces_too() {
    let doc = Document::load(&plain()).expect("fixture loads");
    let first =
        layer::add_ocr_layer(&doc, 0, &one_word("ONESHOTA"), &engine_opts()).expect("first");
    let doc = Document::from_bytes(first.bytes).expect("reloads");
    let second =
        layer::add_ocr_layer(&doc, 0, &one_word("ONESHOTB"), &engine_opts()).expect("second");
    assert_eq!(second.report.layers_replaced, 1);
    let text = page_text(&second.bytes);
    assert!(
        text.contains("ONESHOTB") && !text.contains("ONESHOTA"),
        "{text:?}"
    );
}
#[test]
fn a_different_tag_is_not_a_layer() {
    let mut s = session();
    ocr(&mut s, "FIRST", &engine_opts()).expect("written");
    let bytes = save(&s);
    let (from, to) = (&b"/pdfc_OCR"[..], &b"/pdfc_OCX"[..]);
    let at = bytes
        .windows(from.len())
        .position(|w| w == from)
        .expect("the layer stream is stored uncompressed");
    let mut edited = bytes.clone();
    edited[at..at + from.len()].copy_from_slice(to);
    let doc = Document::from_bytes(edited).expect("reloads");
    assert!(
        marker::find_ocr_layers(&doc.view())
            .expect("walks")
            .is_empty(),
        "the producer entry alone must not identify a layer"
    );
}
