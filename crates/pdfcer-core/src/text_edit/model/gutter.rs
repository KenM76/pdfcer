//! The column-gutter rule. Pass 4 breaks a line only on a baseline move or
//! a backward jump, so a producer that writes a two-column page row by row
//! (left half, right half, next baseline) yields lines that run across the
//! gutter. A wide forward gap that lines up with the same gap on other lines
//! is a gutter, and the line is cut there.

use super::stages::RawLine;
use super::{BlockDiagnostics, BlockRecognitionOptions, GlyphRef};
use crate::text_extract::{ExtractedGlyph, PageText};

/// A horizontal line's direction cosine must exceed this for its gaps to be
/// measured along page x.
const HORIZONTAL_COS: f32 = 0.999;

/// One wide gap inside a line: glyph `at` is the first glyph right of it.
struct Gap {
    line: usize,
    at: usize,
    x0: f32,
    x1: f32,
    size: f32,
}

/// Cut every non-cell horizontal line at each gap at least
/// `gutter_min_em` em wide that overlaps (by half an em) such a gap on at
/// least `gutter_min_lines` lines, counting itself.
pub(super) fn split_gutters(
    page: &PageText,
    raw: Vec<RawLine>,
    options: &BlockRecognitionOptions,
    diagnostics: &mut BlockDiagnostics,
) -> Vec<RawLine> {
    if !options.gutter_min_em.is_finite() {
        return raw;
    }
    let gaps = candidate_gaps(page, &raw, options.gutter_min_em);
    let confirmed: Vec<&Gap> = gaps
        .iter()
        .filter(|g| aligned_lines(&gaps, g) >= options.gutter_min_lines)
        .collect();
    if confirmed.is_empty() {
        return raw;
    }
    let mut out = Vec::with_capacity(raw.len() + confirmed.len());
    for (li, line) in raw.into_iter().enumerate() {
        let mut cuts: Vec<usize> = confirmed
            .iter()
            .filter(|g| g.line == li)
            .map(|g| g.at)
            .collect();
        if cuts.is_empty() {
            out.push(line);
            continue;
        }
        cuts.sort_unstable();
        diagnostics.lines_split_by_gutter += cuts.len() as u64;
        out.extend(cut_line(page, &line, &cuts));
    }
    out
}

fn glyph(page: &PageText, r: GlyphRef) -> Option<&ExtractedGlyph> {
    page.runs.get(r.run)?.glyphs.get(r.glyph)
}

fn candidate_gaps(page: &PageText, raw: &[RawLine], min_em: f32) -> Vec<Gap> {
    let mut gaps = Vec::new();
    for (li, line) in raw.iter().enumerate() {
        if line.cell.is_some() || line.direction.0 < HORIZONTAL_COS {
            continue;
        }
        for (k, pair) in line.glyphs.windows(2).enumerate() {
            let [a, b] = pair else { continue };
            let (Some(ga), Some(gb)) = (glyph(page, *a), glyph(page, *b)) else {
                continue;
            };
            let (x0, x1) = (ga.advance_end().0, gb.x);
            if x1 - x0 >= min_em * line.size {
                gaps.push(Gap {
                    line: li,
                    at: k + 1,
                    x0,
                    x1,
                    size: line.size,
                });
            }
        }
    }
    gaps
}

/// How many distinct lines carry a gap overlapping `gap` by half an em.
fn aligned_lines(gaps: &[Gap], gap: &Gap) -> usize {
    let need = 0.5 * gap.size;
    let mut lines: Vec<usize> = gaps
        .iter()
        .filter(|o| o.x1.min(gap.x1) - o.x0.max(gap.x0) >= need)
        .map(|o| o.line)
        .collect();
    lines.dedup();
    lines.len()
}

/// `line` cut before each glyph index in `cuts` (ascending).
fn cut_line(page: &PageText, line: &RawLine, cuts: &[usize]) -> Vec<RawLine> {
    let mut pieces: Vec<RawLine> = Vec::with_capacity(cuts.len() + 1);
    let mut current: Option<RawLine> = None;
    for (k, &gref) in line.glyphs.iter().enumerate() {
        let Some(g) = glyph(page, gref) else { continue };
        if cuts.contains(&k) {
            pieces.extend(current.take());
        }
        match current.as_mut() {
            Some(piece) => piece.push(gref, g),
            None => current = Some(RawLine::new(gref, g, None)),
        }
    }
    pieces.extend(current);
    pieces
}
