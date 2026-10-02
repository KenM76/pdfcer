//! A re-wrapped paragraph keeps each glyph's own font, size, kerning and
//! colour, re-justifies a justified one, and refuses by name what it cannot
//! carry (Pass 432.0).
//!
//! Every test re-wraps one page of `fixtures/synthetic/reflow/fidelity.pdf`
//! (generator `tools/gen-reflow-fidelity-fixtures.py`, provenance beside
//! it) to a narrower width, re-extracts with provenance, and compares the
//! per-glyph style sequence of the visible glyphs before and after.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_core::text_edit::{
    BlockKind, EditableTextModel, ReflowApplyError, ReflowEngine, ReflowRequest, UnsupportedCause,
    apply_reflow, reflow_recognition_options,
};
use pdfcer_core::text_extract::{self, ExtractOptions, PageText, TextColor};
use pdfcer_core::text_state::TextStateParam;

/// Page indices of the fixture.
const COMPOSITE: usize = 0;
const STYLES: usize = 1;
const KERNING: usize = 2;
const SIZES: usize = 3;
const COLOUR: usize = 4;
const PATH: usize = 5;
const SCALE: usize = 6;
const WORD_JUSTIFIED: usize = 7;
const LETTER_JUSTIFIED: usize = 8;
const BULLETS: usize = 9;
const NUMBERED: usize = 10;

/// What a visible glyph looks like: its text, font resource, `Tf` size and
/// fill colour.
type Look = (String, Vec<u8>, f32, Option<TextColor>);

fn fixture() -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/reflow/fidelity.pdf"),
    )
    .expect("fidelity.pdf; run tools/gen-reflow-fidelity-fixtures.py")
}

fn extract(bytes: &[u8], page: usize) -> PageText {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    let opts = ExtractOptions::default().with_provenance(true);
    text_extract::extract_page(&doc, &pages[page], page, &opts).unwrap()
}

/// Every non-space glyph's look, in content order.
fn looks(page: &PageText) -> Vec<Look> {
    let mut out = Vec::new();
    for run in &page.runs {
        for g in &run.glyphs {
            let start = g.text_start as usize;
            let text = run.text[start..start + g.text_len as usize].to_owned();
            if text.trim().is_empty() {
                continue;
            }
            let p = g.provenance.as_ref().expect("provenance requested");
            out.push((
                text,
                p.font_resource.clone().unwrap_or_default(),
                p.tf_size,
                p.fill_color,
            ));
        }
    }
    out
}

fn words(page: &PageText) -> Vec<String> {
    page.plain_text()
        .split_whitespace()
        .map(str::to_owned)
        .collect()
}

fn reflow(bytes: &[u8], page: usize, width: f64) -> Result<Vec<u8>, ReflowApplyError> {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    apply_reflow(&doc, page, 0, &ReflowRequest::new().with_wrap_width(width)).map(|o| o.bytes)
}

/// Re-wraps `page` narrower and asserts the text and every glyph's look
/// survived; returns the source and re-wrapped extractions.
fn rewrap_keeps_looks(page: usize, width: f64) -> (PageText, PageText) {
    let src = fixture();
    let before = extract(&src, page);
    let after = extract(&reflow(&src, page, width).unwrap(), page);
    assert_eq!(words(&after), words(&before));
    assert_eq!(looks(&after), looks(&before));
    (before, after)
}

fn line_count(page: &PageText) -> usize {
    page.plain_text()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count()
}

#[test]
fn a_composite_paragraph_rewraps_narrower_with_its_text_and_resource() {
    let (before, after) = rewrap_keeps_looks(COMPOSITE, 120.0);
    assert!(line_count(&after) > line_count(&before));
    assert!(looks(&after).iter().all(|l| l.1 == b"F4"));
}

#[test]
fn bold_and_italic_words_keep_their_fonts() {
    let (_, after) = rewrap_keeps_looks(STYLES, 120.0);
    let font_of = |word: &str| {
        let first = word.chars().next().unwrap().to_string();
        let looks = looks(&after);
        let at = looks
            .windows(word.len())
            .position(|w| w.iter().map(|l| l.0.as_str()).collect::<String>() == word)
            .unwrap_or_else(|| panic!("{word} not found"));
        assert_eq!(looks[at].0, first);
        looks[at].1.clone()
    };
    assert_eq!(font_of("bold"), b"F2");
    assert_eq!(font_of("strong"), b"F2");
    assert_eq!(font_of("italic"), b"F3");
    assert_eq!(font_of("Plain"), b"F1");
}

#[test]
fn a_kerned_word_that_does_not_move_keeps_every_glyph_position() {
    let (before, after) = rewrap_keeps_looks(KERNING, 120.0);
    let xs = |p: &PageText| -> Vec<(f32, f32)> {
        p.runs
            .iter()
            .flat_map(|r| &r.glyphs)
            .take(6)
            .map(|g| (g.x, g.y))
            .collect()
    };
    for (a, b) in xs(&after).iter().zip(xs(&before)) {
        assert!(
            (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3,
            "{a:?} vs {b:?}"
        );
    }
}

#[test]
fn a_larger_word_keeps_its_size() {
    let (_, after) = rewrap_keeps_looks(SIZES, 120.0);
    let sizes: Vec<f32> = looks(&after).iter().map(|l| l.2).collect();
    assert!(sizes.contains(&14.0) && sizes.contains(&10.0));
}

#[test]
fn a_coloured_word_keeps_its_colour_and_what_follows_stays_black() {
    let (_, after) = rewrap_keeps_looks(COLOUR, 120.0);
    let looks = looks(&after);
    let red: String = looks
        .iter()
        .filter(|l| l.3 == Some(TextColor::Rgb(1.0, 0.0, 0.0)))
        .map(|l| l.0.as_str())
        .collect();
    // The block ends on a red word, so the paragraph after it stays black
    // only if the re-emitted block restores the colour it found.
    assert_eq!(red, "redred.");
    let after_word = &looks[looks.len() - 5..];
    assert_eq!(
        after_word.iter().map(|l| l.0.as_str()).collect::<String>(),
        "after"
    );
    assert!(
        after_word
            .iter()
            .all(|l| matches!(l.3, None | Some(TextColor::Gray(0.0))))
    );
}

#[test]
fn a_path_inside_the_block_is_refused_by_name() {
    let err = reflow(&fixture(), PATH, 120.0).unwrap_err();
    assert!(
        matches!(
            &err,
            ReflowApplyError::Unsupported(UnsupportedCause::OperatorInBlock { operator })
                if operator == "re"
        ),
        "{err:?}"
    );
}

/// Per visual line, top down: its baseline, the left edge and the right
/// edge of its last visible glyph's advance.
fn line_edges(page: &PageText) -> Vec<(f32, f32, f32)> {
    let mut lines: Vec<(f32, f32, f32)> = Vec::new();
    for run in &page.runs {
        for g in &run.glyphs {
            let start = g.text_start as usize;
            if run.text[start..start + g.text_len as usize]
                .trim()
                .is_empty()
            {
                continue;
            }
            match lines.iter_mut().find(|l| (l.0 - g.y).abs() < 0.5) {
                Some(l) => {
                    l.1 = l.1.min(g.x);
                    l.2 = l.2.max(g.x + g.advance);
                }
                None => lines.push((g.y, g.x, g.x + g.advance)),
            }
        }
    }
    lines.sort_by(|a, b| b.0.total_cmp(&a.0));
    lines
}

/// Re-wraps a justified page at 200 pt and asserts every line but the last
/// is flush to 72..272, the last is ragged, and no glyph carries `param`.
fn rejustifies(page: usize, param: TextStateParam) {
    let (_, after) = rewrap_keeps_looks(page, 200.0);
    let lines = line_edges(&after);
    let (last, full) = lines.split_last().unwrap();
    assert!(full.len() >= 2);
    for l in full {
        assert!(
            (l.1 - 72.0).abs() < 0.05 && (l.2 - 272.0).abs() < 0.05,
            "{l:?}"
        );
    }
    assert!(last.2 < 262.0, "last line stretched: {last:?}");
    for g in after.runs.iter().flat_map(|r| &r.glyphs) {
        let v = g.provenance.as_ref().unwrap().text_state.get(param).value;
        assert!(v == 0.0, "{param:?} {v} survived");
    }
}

#[test]
fn a_word_spaced_justified_paragraph_rejustifies_with_a_ragged_last_line() {
    rejustifies(WORD_JUSTIFIED, TextStateParam::WordSpacing);
}

#[test]
fn a_letter_spaced_justified_paragraph_rejustifies_with_a_ragged_last_line() {
    rejustifies(LETTER_JUSTIFIED, TextStateParam::CharSpacing);
}

#[test]
fn two_text_matrix_scales_are_refused_by_name() {
    let err = reflow(&fixture(), SCALE, 120.0).unwrap_err();
    assert!(
        matches!(
            err,
            ReflowApplyError::Unsupported(UnsupportedCause::MixedScale { .. })
        ),
        "{err:?}"
    );
}

#[test]
fn bulleted_and_numbered_lines_are_recognised_as_list_items() {
    let src = fixture();
    for (page, expected) in [
        (
            BULLETS,
            vec![
                BlockKind::ListItem,
                BlockKind::ListItem,
                BlockKind::Paragraph,
            ],
        ),
        (NUMBERED, vec![BlockKind::ListItem, BlockKind::ListItem]),
    ] {
        let text = extract(&src, page);
        let model = EditableTextModel::recognize(&text, &reflow_recognition_options());
        let kinds: Vec<BlockKind> = model.blocks().iter().map(|b| b.kind).collect();
        assert_eq!(kinds, expected, "page {page}");
        assert_eq!(model.blocks()[0].line_indices.len(), 2, "page {page}");
    }
}

/// The preview splits the marker from the item's first word even with no
/// space glyph between them (the bullet page positions the text by `Td`).
#[test]
fn the_preview_puts_the_marker_alone_on_line_zero() {
    let src = fixture();
    for (page, marker, first) in [(BULLETS, "\u{2022}", "First"), (NUMBERED, "1.", "Numbered")] {
        let text = extract(&src, page);
        let model = EditableTextModel::recognize(&text, &reflow_recognition_options());
        let pv = ReflowEngine::new(&model)
            .preview(0, &ReflowRequest::new().with_wrap_width(150.0))
            .expect("preview");
        assert_eq!(pv.lines[0].text, marker, "page {page}");
        assert!(
            pv.lines[1].text.starts_with(first),
            "page {page}: {:?}",
            pv.lines[1]
        );
        assert_eq!(pv.lines_after, pv.lines.len() - 1, "page {page}");
    }
}

/// Every visible glyph as `(text, x, y)`, in content order.
fn placed(page: &PageText) -> Vec<(String, f32, f32)> {
    let mut out = Vec::new();
    for run in &page.runs {
        for g in &run.glyphs {
            let start = g.text_start as usize;
            let text = &run.text[start..start + g.text_len as usize];
            if !text.trim().is_empty() {
                out.push((text.to_owned(), g.x, g.y));
            }
        }
    }
    out
}

/// Re-wraps list `page` narrower and asserts the first `marker_len` glyphs
/// (the marker) keep their position while every text line of the item
/// starts at the hanging `indent`.
fn rewraps_at_the_hanging_indent(page: usize, marker_len: usize, indent: f32) {
    let (before, after) = rewrap_keeps_looks(page, 150.0);
    let (before, after) = (placed(&before), placed(&after));
    assert_eq!(after[..marker_len], before[..marker_len]);
    let text_start = &after[marker_len];
    assert!((text_start.1 - indent).abs() < 0.05, "{text_start:?}");
    assert!((text_start.2 - after[0].2).abs() < 0.05, "{text_start:?}");
    let item = |p: &[(String, f32, f32)]| -> Vec<(f32, f32)> {
        let mut lines: Vec<(f32, f32)> = Vec::new();
        for (_, x, y) in p.iter().skip(marker_len).filter(|g| g.2 > 690.0) {
            match lines.iter_mut().find(|l| (l.0 - y).abs() < 0.5) {
                Some(l) => l.1 = l.1.min(*x),
                None => lines.push((*y, *x)),
            }
        }
        lines
    };
    let lines = item(&after);
    assert!(lines.len() > item(&before).len(), "{lines:?}");
    for l in &lines {
        assert!((l.1 - indent).abs() < 0.05, "{l:?}");
    }
}

#[test]
fn a_bulleted_item_rewraps_at_its_indent_and_the_bullet_stays() {
    rewraps_at_the_hanging_indent(BULLETS, 1, 84.0);
}

#[test]
fn a_numbered_item_rewraps_at_its_indent_and_the_number_stays() {
    rewraps_at_the_hanging_indent(NUMBERED, 2, 83.12);
}
