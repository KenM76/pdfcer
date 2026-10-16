//! `EditSession::edit_recognised_block_text`: an OCR-layer paragraph edited
//! word by word, every untouched word staying on its spot of the scan and
//! the layer staying its own stream (G203).
//!
//! Fixture: `fixtures/synthetic/addtext/plain.pdf` plus an OCR layer written
//! by `add_ocr_layer`, shaped like the operator's drawing title block.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::path::Path;

use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditSession, OcrPageLayer};
use pdfcer_core::ocr::layer::OcrLayerOptions;
use pdfcer_core::ocr::{OcrPage, RecognizedWord};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::text_edit::{EditOptions, RecognisedBlockHit};
use pdfcer_core::writer::SaveOptions;

/// `(text, llx, urx, baseline)`; `SHEET` and `2` are 0.3 pt apart.
const WORDS: &[(&str, f64, f64, f64)] = &[
    ("SITE", 72.0, 106.0, 700.0),
    ("PLAN", 112.0, 154.0, 700.0),
    ("REVISION", 160.0, 246.0, 700.0),
    ("C", 252.0, 261.0, 700.0),
    ("SCANNED", 72.0, 140.0, 688.0),
    ("DRAWING", 145.0, 220.0, 688.0),
    ("SHEET", 225.0, 271.7, 688.0),
    ("2", 272.0, 280.0, 688.0),
];

const ORIGINAL: &str = "SITE PLAN REVISION C\nSCANNED DRAWING SHEET 2";

fn session() -> EditSession {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/addtext/plain.pdf");
    let mut s = EditSession::new(Document::load(&path).unwrap());
    let recognised = OcrPage {
        words: WORDS
            .iter()
            .map(|&(t, l, r, y)| RecognizedWord {
                text: t.to_owned(),
                rect: Rect::from_corners(l, y - 2.0, r, y + 9.0),
                confidence: Some(0.9),
            })
            .collect(),
        confidence_available: true,
        ..OcrPage::default()
    };
    let page = OcrPageLayer {
        page_index: 0,
        recognised: &recognised,
    };
    s.add_ocr_layer(&[page], &OcrLayerOptions::new()).unwrap();
    s
}

fn hit(s: &EditSession) -> RecognisedBlockHit {
    s.recognised_block_at_point(0, 80.0, 703.0)
        .unwrap()
        .expect("the layer's block is under the point")
}

fn bytes(s: &EditSession) -> Vec<u8> {
    s.to_full_bytes(&SaveOptions::default()).unwrap().0
}

/// Each word's `(text, llx)`.
fn starts(h: &RecognisedBlockHit) -> Vec<(String, f64)> {
    h.words
        .iter()
        .map(|w| (w.text.clone(), w.bbox.llx))
        .collect()
}

#[test]
fn the_lookup_reads_the_layer_words_and_nothing_visible() {
    let s = session();
    let h = hit(&s);
    assert_eq!(h.text, ORIGINAL, "{h:?}");
    let texts: Vec<&str> = h.words.iter().map(|w| w.text.as_str()).collect();
    assert!(
        texts.contains(&"SHEET") && texts.contains(&"2"),
        "{texts:?}"
    );
}

#[test]
fn unchanged_text_commits_nothing() {
    let mut s = session();
    let depth = s.undo_depth();
    let r = s
        .edit_recognised_block_text(0, hit(&s).block_index, ORIGINAL, &EditOptions::default())
        .unwrap();
    assert_eq!((r.words_replaced, r.words_deleted), (0, 0));
    assert_eq!(s.undo_depth(), depth);
}

#[test]
fn one_changed_word_moves_nothing_else() {
    let mut s = session();
    let before = hit(&s);
    let text = ORIGINAL.replace("REVISION C", "REVISION D");
    let r = s
        .edit_recognised_block_text(0, before.block_index, &text, &EditOptions::default())
        .unwrap();
    assert_eq!(r.words_replaced, 1, "{r:?}");
    assert_eq!(r.words_kept, 7);
    let after = hit(&s);
    assert_eq!(after.text, text.as_str());
    for (b, a) in starts(&before).iter().zip(starts(&after)) {
        assert!((b.1 - a.1).abs() < 0.05, "{b:?} moved to {a:?}");
    }
    assert_eq!(s.find_ocr_layers().unwrap().len(), 1, "layer stays a layer");
    assert!(
        r.disclosures.iter().all(|d| !d.starts_with("multi-stream")),
        "only the layer stream is written, nothing folded: {r:?}"
    );
}

#[test]
fn a_shorter_word_is_fitted_to_the_old_width() {
    let mut s = session();
    let before = hit(&s);
    let text = ORIGINAL.replace("REVISION", "REV");
    let r = s
        .edit_recognised_block_text(0, before.block_index, &text, &EditOptions::default())
        .unwrap();
    let fit = &r.fits[0];
    assert_eq!(
        (fit.old_text.as_str(), fit.new_text.as_str()),
        ("REVISION", "REV")
    );
    assert!(!fit.clamped, "{fit:?}");
    let old = &before.words[2].bbox;
    let new = &hit(&s).words[2].bbox;
    assert!((old.llx - new.llx).abs() < 0.05, "{old:?} vs {new:?}");
    assert!((old.urx - new.urx).abs() < 1.0, "{old:?} vs {new:?}");
}

#[test]
fn a_deleted_and_a_merged_word_are_reported() {
    let mut s = session();
    let b = hit(&s).block_index;
    let r = s
        .edit_recognised_block_text(
            0,
            b,
            "SITE PLAN REVISION C\nDRAWING SHEET 2 FINAL",
            &EditOptions::default(),
        )
        .unwrap();
    assert_eq!(r.words_deleted, 1, "{r:?}");
    assert_eq!(r.words_merged, 1, "{r:?}");
    assert!(!r.disclosures.is_empty());
    let h = hit(&s);
    assert_eq!(h.text, "SITE PLAN REVISION C\nDRAWING SHEET 2 FINAL");
}

#[test]
fn the_preview_changes_nothing_and_matches_the_commit() {
    let mut s = session();
    let b = hit(&s).block_index;
    let saved = bytes(&s);
    let text = ORIGINAL.replace("PLAN", "PLANS");
    let preview = s
        .edit_recognised_block_text_preview(0, b, &text, &EditOptions::default())
        .unwrap();
    assert_eq!(bytes(&s), saved);
    let mut done = s
        .edit_recognised_block_text(0, b, &text, &EditOptions::default())
        .unwrap();
    done.content_object = preview.report.content_object;
    assert_eq!(done, preview.report);
}

/// A glyph's page-space origin from its preview matrix.
fn origin(m: [f64; 6]) -> (f64, f64) {
    (m[4], m[5])
}

#[test]
fn unchanged_text_lays_out_the_block_as_it_is() {
    let s = session();
    let h = hit(&s);
    let p = s
        .edit_recognised_block_text_preview(0, h.block_index, ORIGINAL, &EditOptions::default())
        .unwrap();
    assert_eq!(p.lines.len(), 2, "{:?}", p.lines);
    assert_eq!(p.lines[0].text, "SITE PLAN REVISION C");
    assert_eq!(p.lines[1].text, "SCANNED DRAWING SHEET 2");
    assert!(
        // The layer sets its baseline a descent above the word box's bottom.
        (p.lines[0].baseline_y - 700.0).abs() < 1.0,
        "{:?}",
        p.lines[0]
    );
    assert!(
        (p.lines[0].origin_x - 72.0).abs() < 0.01,
        "{:?}",
        p.lines[0]
    );
    assert!(
        (p.lines[1].baseline_y - 688.0).abs() < 1.0,
        "{:?}",
        p.lines[1]
    );
    // The line runs from SITE's origin to C's end.
    assert!(
        p.lines[0].width > 150.0 && p.lines[0].width < 200.0,
        "{:?}",
        p.lines[0]
    );
    assert_eq!(p.runs.len(), 1, "one font resource");
    let run = &p.runs[0];
    assert_eq!(run.render_mode, 3);
    let glyphs: usize = ORIGINAL.split_whitespace().map(str::len).sum();
    assert_eq!(run.glyphs.len(), glyphs);
    let first = origin(run.glyphs[0].matrix);
    assert!(
        (first.0 - 72.0).abs() < 0.01 && (first.1 - 700.0).abs() < 1.0,
        "{first:?}"
    );
    assert_eq!(p.old_bbox, h.bbox);
    assert!(
        p.new_bbox.llx <= 72.01 && p.new_bbox.ury >= 700.0,
        "{:?}",
        p.new_bbox
    );
    assert_eq!(p.report.words_kept, 8);
    assert_eq!(p.report.content_object, None);
}

#[test]
fn a_replaced_word_is_laid_out_in_the_old_words_box() {
    let s = session();
    let h = hit(&s);
    let text = ORIGINAL.replace("PLAN", "PLANS");
    let p = s
        .edit_recognised_block_text_preview(0, h.block_index, &text, &EditOptions::default())
        .unwrap();
    assert_eq!(p.lines[0].text, "SITE PLANS REVISION C");
    assert_eq!(
        p.runs.len(),
        1,
        "the replacement is set in the layer's own font"
    );
    let run = &p.runs[0];
    let total = text.split_whitespace().map(str::len).sum::<usize>();
    assert_eq!(run.glyphs.len(), total);
    // SITE is 4 glyphs; PLANS follows, starting where PLAN did.
    let start = origin(run.glyphs[4].matrix);
    assert!((start.0 - 112.0).abs() < 0.01, "{start:?}");
    // The glyph after PLANS (REVISION's R) still starts where it did.
    let after = origin(run.glyphs[9].matrix);
    assert!((after.0 - 160.0).abs() < 0.01, "{after:?}");
    // PLANS is set at its fitted Tz: five glyphs inside PLAN's box, each
    // narrower than PLAN's own were, all at the one scaling the report
    // states.
    let was = s
        .edit_recognised_block_text_preview(0, h.block_index, ORIGINAL, &EditOptions::default())
        .unwrap();
    let plan_scale = was.runs[0].glyphs[4].matrix[0];
    let fitted: Vec<f64> = run.glyphs[4..9].iter().map(|g| g.matrix[0]).collect();
    assert!(
        fitted.iter().all(|&f| (f - fitted[0]).abs() < 1e-9),
        "{fitted:?}"
    );
    assert!(fitted[0] < plan_scale, "{fitted:?} vs {plan_scale}");
    let xs: Vec<f64> = run.glyphs[4..9].iter().map(|g| g.matrix[4]).collect();
    assert!(xs.windows(2).all(|w| w[0] < w[1]), "{xs:?}");
    assert!(xs[4] > 112.0 && xs[4] < 154.0, "{xs:?}");
    let fit = &p.report.fits[0];
    assert_eq!(fit.new_text, "PLANS");
    assert!(
        (p.new_bbox.llx - p.old_bbox.llx).abs() < 0.5,
        "{:?}",
        p.new_bbox
    );
    assert!(
        (p.new_bbox.urx - p.old_bbox.urx).abs() < 2.0,
        "{:?} vs {:?}",
        p.new_bbox,
        p.old_bbox
    );
}

#[test]
fn undo_restores_the_page() {
    let mut s = session();
    let saved = bytes(&s);
    let b = hit(&s).block_index;
    s.edit_recognised_block_text(
        0,
        b,
        &ORIGINAL.replace("SITE", "PARK"),
        &EditOptions::default(),
    )
    .unwrap();
    assert_ne!(bytes(&s), saved);
    assert!(matches!(
        s.undo(),
        Some(CommandKind::EditRecognisedText { words_changed: 1 })
    ));
    assert_eq!(bytes(&s), saved);
}
