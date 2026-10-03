//! `EditSession::edit_block_text`: a block's whole text replaced and
//! re-wrapped in its first run's look, one undo entry, a preview that is the
//! commit's own plan (Pass 433.0).
//!
//! Fixture: `fixtures/synthetic/reflow/block_text.pdf` (generator
//! `tools/gen-block-text-fixtures.py`, provenance beside it).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use pdfcer_core::content::ContentStream;
use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditSession};
use pdfcer_core::page_tree;
use pdfcer_core::text_edit::{BlockEditError, BlockEditOptions, EditableTextModel};
use pdfcer_core::text_extract::{self, ExtractOptions};
use pdfcer_core::writer::SaveOptions;

const TAGGED: usize = 0;
const PER_LINE: usize = 1;

const LONGER: &str = "Synthetic placeholder words now make up a much longer first \
                      paragraph, so the edit must wrap the new text onto more lines \
                      than the three the block had, at the same width as before.";

fn session() -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/reflow/block_text.pdf");
    let bytes = std::fs::read(path).expect("fixture: run tools/gen-block-text-fixtures.py");
    EditSession::new(Document::from_bytes(bytes).expect("parses"))
}

fn full_bytes(s: &EditSession) -> Vec<u8> {
    s.to_full_bytes(&SaveOptions::default()).expect("save").0
}

/// Each line of block 0 on `page` after a save: `(baseline, first x, text)`.
fn lines_of(s: &EditSession, page: usize) -> Vec<(f64, f64, String)> {
    let d = Document::from_bytes(full_bytes(s)).expect("reload");
    let pages = page_tree::pages(&d).expect("pages");
    let opts = ExtractOptions::default().with_provenance(true);
    let p = text_extract::extract_page(&d, &pages[page], page, &opts).expect("extract");
    let m = EditableTextModel::recognize(&p, &pdfcer_core::text_edit::reflow_recognition_options());
    let block = &m.blocks()[0];
    block
        .line_indices
        .iter()
        .map(|&li| {
            let l = &m.lines()[li];
            let x = m.glyph(l.glyphs[0]).map_or(0.0, |g| f64::from(g.x));
            (f64::from(l.baseline_y), x, m.line_text(l))
        })
        .collect()
}

/// `[q, Q, BDC+BMC, EMC, BT, ET]` in the page's content.
fn structure(s: &EditSession, page: usize) -> [usize; 6] {
    let d = Document::from_bytes(full_bytes(s)).expect("reload");
    let pages = page_tree::pages(&d).expect("pages");
    let view = d.view();
    let cs = ContentStream::from_page(&view, &pages[page]).expect("parses");
    let mut n = [0; 6];
    for op in cs.operations() {
        let i = match op.operator_name(&cs.buf) {
            Some(b"q") => 0,
            Some(b"Q") => 1,
            Some(b"BDC" | b"BMC") => 2,
            Some(b"EMC") => 3,
            Some(b"BT") => 4,
            Some(b"ET") => 5,
            _ => continue,
        };
        n[i] += 1;
    }
    n
}

#[test]
fn longer_text_rewraps_at_the_same_width_and_keeps_the_first_origin() {
    let mut s = session();
    let before = lines_of(&s, TAGGED);
    assert_eq!(before.len(), 3, "{before:?}");
    let report = s
        .edit_block_text(TAGGED, 0, LONGER, &BlockEditOptions::new())
        .expect("edit");
    assert_eq!(report.lines_before, 3);
    assert!(report.lines_after > 3, "{report:?}");
    let after = lines_of(&s, TAGGED);
    assert_eq!(after.len(), report.lines_after, "{after:?}");
    assert!(
        (after[0].0 - before[0].0).abs() < 0.01,
        "first baseline kept"
    );
    assert!((after[0].1 - before[0].1).abs() < 0.01, "first x kept");
    let joined: Vec<&str> = after.iter().map(|l| l.2.as_str()).collect();
    assert_eq!(
        joined.join(" "),
        LONGER.split_whitespace().collect::<Vec<_>>().join(" ")
    );
    assert!(
        report.overflow_pt.is_some_and(|o| o > 0.0),
        "taller block disclosed"
    );
    assert_eq!(report.base_font, "Helvetica");
    assert_eq!(report.tagged_mcid, Some(0));
    assert!(
        report
            .disclosures
            .iter()
            .any(|d| d.contains("wrapped from 3 to")),
        "{:?}",
        report.disclosures
    );
    let mut seen = std::collections::HashSet::new();
    assert!(
        report.disclosures.iter().all(|d| seen.insert(d)),
        "each disclosure once: {:?}",
        report.disclosures
    );
}

#[test]
fn one_undo_restores_the_original_bytes() {
    let mut s = session();
    let original = full_bytes(&s);
    s.edit_block_text(TAGGED, 0, LONGER, &BlockEditOptions::new())
        .expect("edit");
    assert_ne!(full_bytes(&s), original);
    assert_eq!(s.undo_kinds().len(), 1, "one undo entry");
    assert!(matches!(
        s.undo(),
        Some(CommandKind::EditBlockText {
            lines_before: 3,
            ..
        })
    ));
    assert_eq!(full_bytes(&s), original);
}

#[test]
fn the_preview_is_the_commit() {
    let mut s = session();
    let opts = BlockEditOptions::new().with_wrap_width(180.0);
    let preview = s
        .edit_block_text_preview(TAGGED, 0, LONGER, &opts)
        .expect("preview");
    let report = s.edit_block_text(TAGGED, 0, LONGER, &opts).expect("edit");
    assert_eq!(preview.report, report);
    let after = lines_of(&s, TAGGED);
    assert_eq!(after.len(), preview.lines.len());
    for (got, want) in after.iter().zip(&preview.lines) {
        assert_eq!(got.2, want.text);
        assert!(
            (got.0 - want.baseline_y).abs() < 0.01,
            "{got:?} vs {want:?}"
        );
        assert!((got.1 - want.origin_x).abs() < 0.01, "{got:?} vs {want:?}");
    }
    let chars: usize = preview.lines.iter().map(|l| l.text.chars().count()).sum();
    assert_eq!(preview.glyphs.glyphs.len(), chars, "a glyph per code");
    let first = &preview.glyphs.glyphs[0];
    assert!((first.matrix[4] - preview.lines[0].origin_x).abs() < 0.01);
    assert!((first.matrix[5] - preview.lines[0].baseline_y).abs() < 0.01);
}

#[test]
fn an_unencodable_character_refuses_the_whole_edit_by_name() {
    let mut s = session();
    let original = full_bytes(&s);
    let err = s
        .edit_block_text(
            TAGGED,
            0,
            "Plain words then \u{3A9} and \u{3042}.",
            &BlockEditOptions::new(),
        )
        .expect_err("WinAnsi Helvetica has neither");
    assert!(matches!(err, BlockEditError::Text(_)), "{err:?}");
    let msg = err.to_string();
    assert!(msg.contains('\u{3A9}') && msg.contains('\u{3042}'), "{msg}");
    assert_eq!(s.undo_kinds().len(), 0);
    assert_eq!(full_bytes(&s), original, "the session is unchanged");
}

#[test]
fn nesting_balances_and_the_next_paragraph_stays() {
    let mut s = session();
    let before = structure(&s, TAGGED);
    let second = {
        let d = Document::from_bytes(full_bytes(&s)).expect("reload");
        let pages = page_tree::pages(&d).expect("pages");
        let p = text_extract::extract_page(&d, &pages[0], 0, &ExtractOptions::default()).unwrap();
        p.runs
            .iter()
            .find(|r| r.text.starts_with("A second"))
            .map(|r| r.glyphs[0].y)
    };
    s.edit_block_text(
        TAGGED,
        0,
        "Short.\nTwo paragraphs.",
        &BlockEditOptions::new(),
    )
    .expect("edit");
    let after = structure(&s, TAGGED);
    assert_eq!(after[0], after[1], "q/Q balance");
    assert_eq!(after[2], after[3], "BDC/EMC balance");
    assert_eq!(after[2], before[2], "the enclosing sequences are kept");
    assert_eq!(after[4], before[4] - 2, "three text objects became one");
    let lines = lines_of(&s, TAGGED);
    assert_eq!(lines.len(), 2, "\\n is a paragraph break: {lines:?}");
    let d = Document::from_bytes(full_bytes(&s)).expect("reload");
    let pages = page_tree::pages(&d).expect("pages");
    let p = text_extract::extract_page(&d, &pages[0], 0, &ExtractOptions::default()).unwrap();
    let moved = p
        .runs
        .iter()
        .find(|r| r.text.starts_with("A second"))
        .map(|r| r.glyphs[0].y);
    assert_eq!(moved, second, "the paragraph after the block does not move");
}

#[test]
fn per_line_marked_content_is_removed_with_its_lines_and_disclosed() {
    let mut s = session();
    let before = structure(&s, PER_LINE);
    let report = s
        .edit_block_text(PER_LINE, 0, "Replaced.", &BlockEditOptions::new())
        .expect("edit");
    assert_eq!(report.marked_content_removed, 2, "the inner two sequences");
    let after = structure(&s, PER_LINE);
    assert_eq!(after[2], after[3]);
    assert_eq!(after[2], before[2] - 2);
    assert!(
        report
            .disclosures
            .iter()
            .any(|d| d.contains("marked-content")),
        "{:?}",
        report.disclosures
    );
}

#[test]
fn a_point_names_its_block_and_text() {
    let s = session();
    let hit = s
        .block_at_point(TAGGED, 100.0, 701.0)
        .expect("ok")
        .expect("a block");
    assert_eq!(hit.block_index, 0);
    assert!(
        hit.text.starts_with("Synthetic placeholder words"),
        "{}",
        hit.text
    );
    assert!(!hit.text.contains('\n'));
    let second = s
        .block_at_point(TAGGED, 100.0, 562.0)
        .expect("ok")
        .expect("a block");
    assert_eq!(second.block_index, 1);
    assert_eq!(s.block_at_point(TAGGED, 500.0, 100.0).expect("ok"), None);
}

/// A three-line paragraph; `mid` is shown between "quick " and "brown".
fn paragraph(mid: &str) -> EditSession {
    let content = format!(
        "BT /F1 12 Tf 14 TL 72 700 Td (The quick ) Tj {mid} (brown fox jumps over) Tj \
         T* (the lazy dog and runs into) Tj T* (the forest at dusk.) Tj ET\n"
    );
    EditSession::new(super::block_layout::doc_from_pages(&[&content], 0))
}

/// `looks` from the hit and from the preview, before anything is written.
fn looks_of(s: &EditSession) -> (Option<usize>, usize, Vec<String>) {
    let hit = s
        .block_at_point(0, 100.0, 701.0)
        .expect("ok")
        .expect("a block");
    let preview = s
        .edit_block_text_preview(
            0,
            hit.block_index,
            "New text.",
            &BlockEditOptions::default(),
        )
        .expect("previews");
    (hit.looks, preview.report.looks, preview.report.disclosures)
}

#[test]
fn a_uniform_paragraph_has_one_look() {
    let (hit, preview, disclosures) = looks_of(&paragraph(""));
    assert_eq!((hit, preview), (Some(1), 1));
    assert!(
        !disclosures.iter().any(|d| d.contains("mixed")),
        "{disclosures:?}"
    );
}

#[test]
fn a_word_in_a_second_colour_is_a_second_look() {
    let (hit, preview, disclosures) = looks_of(&paragraph("1 0 0 rg (red ) Tj 0 g"));
    assert_eq!((hit, preview), (Some(2), 2));
    assert!(
        disclosures.iter().any(|d| d.contains("mixed 2 looks")),
        "{disclosures:?}"
    );
}

#[test]
fn stroke_colour_and_horizontal_scale_alone_count_as_looks() {
    for mid in ["1 0 0 RG (red ) Tj 0 G", "90 Tz (narrow ) Tj 100 Tz"] {
        let (hit, preview, _) = looks_of(&paragraph(mid));
        assert_eq!((hit, preview), (Some(2), 2), "{mid}");
    }
}
