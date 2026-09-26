//! Tests for `pdfcer_core::text_edit::model`, run against its public API.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::text_edit::model::*;
use pdfcer_core::text_extract::{ExtractedGlyph, PageText, TextOrigin, TextRun};

/// A `Glyphs` run built from `(char, x, y, advance, size)` tuples, at a
/// given starting run text — enough to drive recognition without the
/// whole extraction pipeline.
fn glyph_run(chars: &[(&str, f32, f32, f32, f32)]) -> TextRun {
    let mut text = String::new();
    let mut glyphs = Vec::new();
    for &(c, x, y, adv, size) in chars {
        let start = text.len() as u32;
        text.push_str(c);
        glyphs.push(ExtractedGlyph {
            code: 0,
            rung: pdfcer_core::text_extract::LadderRung::ToUnicode,
            text_start: start,
            text_len: c.len() as u32,
            x,
            y,
            advance: adv,
            size,
            direction: (1.0, 0.0),
            invisible: false,
            provenance: None,
        });
    }
    TextRun {
        text,
        origin: TextOrigin::Glyphs,
        glyphs,
        artifact: None,
        mcid: None,
        bbox: None,
    }
}

fn line_break() -> TextRun {
    TextRun {
        text: "\n".to_string(),
        origin: TextOrigin::DerivedLineBreak,
        glyphs: Vec::new(),
        artifact: None,
        mcid: None,
        bbox: None,
    }
}

fn page(runs: Vec<TextRun>) -> PageText {
    // `PageText` has a private field, so build it through `Default` and
    // set the public `runs` field rather than a struct literal.
    let mut p = PageText::default();
    p.runs = runs;
    p
}

#[test]
fn one_line_is_one_block_one_column() {
    let p = page(vec![glyph_run(&[
        ("H", 72.0, 700.0, 6.0, 10.0),
        ("i", 78.0, 700.0, 4.0, 10.0),
    ])]);
    let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
    assert_eq!(m.lines().len(), 1);
    assert_eq!(m.columns(), 1);
    assert_eq!(m.blocks().len(), 1);
    assert_eq!(m.diagnostics().glyphs_clustered, 2);
    assert_eq!(m.block_text(&m.blocks()[0]), "Hi");
}

#[test]
fn a_leading_gap_splits_two_paragraphs() {
    // Three lines at 14-unit leading, then a 28-unit gap, then two more.
    let runs = vec![
        glyph_run(&[("a", 72.0, 740.0, 6.0, 10.0)]),
        line_break(),
        glyph_run(&[("b", 72.0, 726.0, 6.0, 10.0)]),
        line_break(),
        glyph_run(&[("c", 72.0, 712.0, 6.0, 10.0)]),
        line_break(),
        glyph_run(&[("d", 72.0, 684.0, 6.0, 10.0)]), // 28-unit gap
        line_break(),
        glyph_run(&[("e", 72.0, 670.0, 6.0, 10.0)]),
    ];
    let p = page(runs);
    let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
    assert_eq!(m.lines().len(), 5);
    assert_eq!(m.columns(), 1);
    assert_eq!(m.blocks().len(), 2, "the 2x leading gap starts a paragraph");
    assert_eq!(m.diagnostics().paragraph_breaks_by_leading, 1);
}

#[test]
fn two_x_bands_are_two_columns_ordered_left_to_right() {
    // Right column emitted FIRST in content order; recognition must
    // still order the bands left-to-right.
    let runs = vec![
        glyph_run(&[("R", 322.0, 740.0, 6.0, 10.0)]),
        line_break(),
        glyph_run(&[("L", 72.0, 740.0, 6.0, 10.0)]),
    ];
    let p = page(runs);
    let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
    assert_eq!(m.columns(), 2);
    // The line at x=72 must be column 0, the one at x=322 column 1.
    let left = m.lines().iter().find(|l| l.bbox.llx < 100.0).unwrap();
    let right = m.lines().iter().find(|l| l.bbox.llx > 300.0).unwrap();
    assert_eq!(left.column, 0);
    assert_eq!(right.column, 1);
    assert!(m.diagnostics().is_multi_column());
}

#[test]
fn hit_test_answers_none_beyond_one_line_height_of_every_line() {
    // pdfcer-gui, 2026-09-05: `None` was unreachable on any page with
    // text, because the fallback took the nearest line at ANY distance.
    // "Hi" at (72, 700), 10 pt: the line box is roughly x 72..82,
    // y 700..710, so the reach is one line-height (10 pt) around it.
    let p = page(vec![glyph_run(&[
        ("H", 72.0, 700.0, 6.0, 10.0),
        ("i", 78.0, 700.0, 4.0, 10.0),
    ])]);
    let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
    // Inside, just past the end, and a little above/below: still a caret.
    assert!(m.hit_test(75.0, 705.0).is_some(), "inside the box");
    assert!(
        m.hit_test(88.0, 705.0).is_some(),
        "6 pt past the last glyph"
    );
    assert!(m.hit_test(75.0, 716.0).is_some(), "6 pt above the line");
    assert!(m.hit_test(75.0, 694.0).is_some(), "6 pt below the line");
    // Beyond one line-height in any direction: blank paper.
    assert_eq!(m.hit_test(300.0, 705.0), None, "215 pt to the right");
    assert_eq!(m.hit_test(75.0, 500.0), None, "200 pt below");
    assert_eq!(m.hit_test(75.0, 740.0), None, "30 pt above");
    assert_eq!(m.hit_test(1.0e9, 1.0e9), None, "a billion points away");
    assert_eq!(m.hit_test(-1.0e4, -1.0e4), None, "off the sheet");
}

#[test]
fn hit_test_lands_on_a_glyph_boundary() {
    let p = page(vec![glyph_run(&[
        ("H", 72.0, 700.0, 6.0, 10.0),
        ("i", 78.0, 700.0, 4.0, 10.0),
    ])]);
    let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
    // A point in the left third of "H" resolves to its leading edge.
    let pos = m.hit_test(73.0, 702.0).unwrap();
    assert_eq!(pos, TextPosition::new(0, 0));
    // A point past "i" resolves to the trailing edge (offset 2 bytes).
    let end = m.hit_test(90.0, 702.0).unwrap();
    assert_eq!(end, TextPosition::new(0, 2));
}

#[test]
fn resolve_range_covers_the_selected_glyphs() {
    let p = page(vec![glyph_run(&[
        ("H", 72.0, 700.0, 6.0, 10.0),
        ("e", 78.0, 700.0, 6.0, 10.0),
        ("y", 84.0, 700.0, 6.0, 10.0),
    ])]);
    let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
    // Select bytes [0, 2): the first two glyphs.
    let covered = m.resolve_range(TextPosition::new(0, 0), TextPosition::new(0, 2));
    assert_eq!(covered, vec![GlyphRef::new(0, 0), GlyphRef::new(0, 1)]);
    // Order-insensitive.
    let rev = m.resolve_range(TextPosition::new(0, 2), TextPosition::new(0, 0));
    assert_eq!(rev, covered);
}

#[test]
fn artifact_runs_are_excluded_and_counted() {
    let mut art = glyph_run(&[("1", 300.0, 40.0, 6.0, 10.0)]);
    art.artifact = Some(pdfcer_core::text_extract::ArtifactKind::Pagination);
    let p = page(vec![glyph_run(&[("A", 72.0, 700.0, 6.0, 10.0)]), art]);
    let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
    assert_eq!(m.diagnostics().artifact_runs_skipped, 1);
    // Only the body glyph was clustered.
    assert_eq!(m.diagnostics().glyphs_clustered, 1);
}

#[test]
fn sourced_view_is_the_untouched_page() {
    let p = page(vec![glyph_run(&[("A", 72.0, 700.0, 6.0, 10.0)])]);
    let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
    assert_eq!(m.sourced_view().sourced_text(), "A");
}

// -- boundary accessors (Pass 14.3, UI spec §4.3/§4.5) --------------

#[test]
fn line_at_maps_a_caret_back_to_its_line() {
    // Two lines; a caret in either must resolve to that line's index.
    let runs = vec![
        glyph_run(&[("a", 72.0, 740.0, 6.0, 10.0), ("b", 78.0, 740.0, 6.0, 10.0)]),
        line_break(),
        glyph_run(&[("c", 72.0, 726.0, 6.0, 10.0)]),
    ];
    let p = page(runs);
    let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
    assert_eq!(m.lines().len(), 2);
    // Caret at the start of run 0 ("a") -> line 0.
    assert_eq!(m.line_at(TextPosition::new(0, 0)), Some(0));
    // Caret on the trailing boundary of "b" (offset 2) -> still line 0.
    assert_eq!(m.line_at(TextPosition::new(0, 2)), Some(0));
    // Caret in run 2 ("c") -> line 1.
    assert_eq!(m.line_at(TextPosition::new(2, 0)), Some(1));
    // A run that carries no clustered glyph resolves to nothing.
    assert_eq!(m.line_at(TextPosition::new(99, 0)), None);
}

#[test]
fn line_range_at_spans_first_to_last_glyph_boundary() {
    let p = page(vec![glyph_run(&[
        ("H", 72.0, 700.0, 6.0, 10.0),
        ("i", 78.0, 700.0, 4.0, 10.0),
    ])]);
    let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
    let (start, end) = m.line_range_at(TextPosition::new(0, 1)).unwrap();
    assert_eq!(start, TextPosition::new(0, 0)); // leading edge of "H"
    assert_eq!(end, TextPosition::new(0, 2)); // trailing edge of "i"
}

#[test]
fn word_range_at_splits_on_whitespace_within_the_run() {
    // One run holding two words separated by a space.
    let p = page(vec![glyph_run(&[
        ("t", 72.0, 700.0, 6.0, 10.0),
        ("h", 78.0, 700.0, 6.0, 10.0),
        ("e", 84.0, 700.0, 6.0, 10.0),
        (" ", 90.0, 700.0, 4.0, 10.0),
        ("c", 94.0, 700.0, 6.0, 10.0),
        ("a", 100.0, 700.0, 6.0, 10.0),
        ("t", 106.0, 700.0, 6.0, 10.0),
    ])]);
    let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
    // A caret inside "the" (byte 1) selects [0, 3).
    let (a, b) = m.word_range_at(TextPosition::new(0, 1));
    assert_eq!((a.byte_offset, b.byte_offset), (0, 3));
    // A caret inside "cat" (byte 5) selects [4, 7).
    let (a, b) = m.word_range_at(TextPosition::new(0, 5));
    assert_eq!((a.byte_offset, b.byte_offset), (4, 7));
    // Both ends stay in the same run — always an editable single-run span.
    assert_eq!(a.run, 0);
    assert_eq!(b.run, 0);
}

#[test]
fn word_range_at_is_panic_free_for_a_stale_run() {
    let p = page(vec![glyph_run(&[("A", 72.0, 700.0, 6.0, 10.0)])]);
    let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
    let pos = TextPosition::new(99, 3);
    assert_eq!(m.word_range_at(pos), (pos, pos));
}

#[test]
fn block_at_maps_caret_to_its_paragraph() {
    // Two paragraphs separated by a blank line (a leading gap), so the
    // default recogniser makes two blocks. A caret in each resolves to a
    // distinct block index; `block_at` == `lines()[line_at].block`.
    // Each paragraph is two closely-spaced lines (14pt leading); a wide
    // blank gap separates them so the leading-gap rule splits the blocks.
    let p = page(vec![
        glyph_run(&[("A", 72.0, 700.0, 6.0, 10.0), ("b", 78.0, 700.0, 6.0, 10.0)]),
        line_break(),
        glyph_run(&[("A", 72.0, 686.0, 6.0, 10.0), ("b", 78.0, 686.0, 6.0, 10.0)]),
        line_break(),
        glyph_run(&[("C", 72.0, 620.0, 6.0, 10.0), ("d", 78.0, 620.0, 6.0, 10.0)]),
        line_break(),
        glyph_run(&[("C", 72.0, 606.0, 6.0, 10.0), ("d", 78.0, 606.0, 6.0, 10.0)]),
    ]);
    let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
    assert!(
        m.blocks().len() >= 2,
        "two paragraphs -> two blocks: {}",
        m.blocks().len()
    );
    let top = m.hit_test(74.0, 700.0).expect("hit top");
    let bot = m.hit_test(74.0, 620.0).expect("hit bottom");
    let bt = m.block_at(top).expect("block for top caret");
    let bb = m.block_at(bot).expect("block for bottom caret");
    assert_ne!(bt, bb, "carets in different paragraphs -> different blocks");
    // Agrees with the hand composition it is sugar over.
    let li = m.line_at(top).unwrap();
    assert_eq!(bt, m.lines()[li].block);
    // An out-of-range run resolves to None, never panics.
    assert_eq!(m.block_at(TextPosition::new(9999, 0)), None);
}

// -- caret navigation (Pass 14.4, UI spec §4.5) --------------------

/// Two lines, each two glyphs, stacked in one column: line 0 ("Hi") at
/// baseline 700, line 1 ("yo") at baseline 686. Enough to exercise
/// Left/Right across the run/line boundary and Up/Down nearest-x.
fn two_line_model_page() -> PageText {
    page(vec![
        glyph_run(&[("H", 72.0, 700.0, 6.0, 10.0), ("i", 78.0, 700.0, 4.0, 10.0)]),
        line_break(),
        glyph_run(&[("y", 72.0, 686.0, 6.0, 10.0), ("o", 78.0, 686.0, 6.0, 10.0)]),
    ])
}

#[test]
fn caret_left_right_step_glyph_boundaries_and_cross_the_line_break() {
    let p = two_line_model_page();
    let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
    // Right from the start of "H" walks the two boundaries of run 0…
    let p0 = TextPosition::new(0, 0);
    let p1 = m.caret_right(p0);
    assert_eq!(p1, TextPosition::new(0, 1)); // between H and i
    let p2 = m.caret_right(p1);
    assert_eq!(p2, TextPosition::new(0, 2)); // trailing edge of "i"
    // …then crosses the DerivedLineBreak (run 1, empty) to run 2's start.
    let p3 = m.caret_right(p2);
    assert_eq!(p3, TextPosition::new(2, 0)); // leading edge of "y"
    // Left is the exact inverse, crossing back over the break.
    assert_eq!(m.caret_left(p3), p2);
    assert_eq!(m.caret_left(p2), p1);
    // Clamp: Left at the very first slot stays put, never wraps/panics.
    assert_eq!(m.caret_left(p0), p0);
    // Clamp: Right at the very last slot stays put.
    let last = TextPosition::new(2, 2);
    assert_eq!(m.caret_right(last), last);
}

#[test]
fn caret_up_down_land_on_the_nearest_x_of_the_adjacent_line() {
    let p = two_line_model_page();
    let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
    // Caret on the trailing edge of "i" (line 0), x ≈ 82. Its desired-x
    // carried DOWN lands on the nearest slot of line 1 — the trailing edge
    // of "o" (x ≈ 84), i.e. offset 2 in run 2.
    let start = TextPosition::new(0, 2);
    let x = m.caret_x(start).expect("caret_x for a real boundary");
    let down = m.caret_down(start, x);
    assert_eq!(down, TextPosition::new(2, 2));
    // Back UP from there returns to line 0's nearest slot (trailing "i").
    let x2 = m.caret_x(down).expect("caret_x");
    assert_eq!(m.caret_up(down, x2), TextPosition::new(0, 2));
    // Up from the TOP line has nowhere to go → stays put (no panic).
    assert_eq!(m.caret_up(start, x), start);
    // Down from the BOTTOM line likewise stays put.
    let bot = TextPosition::new(2, 0);
    let xb = m.caret_x(bot).expect("caret_x");
    assert_eq!(m.caret_down(bot, xb), bot);
}

#[test]
fn caret_x_reads_leading_and_trailing_glyph_edges() {
    let p = two_line_model_page();
    let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
    // Leading edge of "H" is its origin x; trailing edge of "i" is x+adv.
    assert_eq!(m.caret_x(TextPosition::new(0, 0)), Some(72.0));
    assert_eq!(m.caret_x(TextPosition::new(0, 2)), Some(82.0));
    // A stale run / a non-boundary offset yields None, never panics.
    assert_eq!(m.caret_x(TextPosition::new(99, 0)), None);
}

#[test]
fn caret_on_line_nearest_x_clamps_to_a_line_and_is_bounds_checked() {
    let p = two_line_model_page();
    let m = EditableTextModel::recognize(&p, &BlockRecognitionOptions::default());
    // A far-left x on line 0 clamps to that line's leading edge.
    assert_eq!(
        m.caret_on_line_nearest_x(0, 0.0),
        Some(TextPosition::new(0, 0))
    );
    // A far-right x clamps to the trailing edge.
    assert_eq!(
        m.caret_on_line_nearest_x(0, 9999.0),
        Some(TextPosition::new(0, 2))
    );
    // An out-of-range line index resolves to None, never panics.
    assert_eq!(m.caret_on_line_nearest_x(999, 50.0), None);
}
