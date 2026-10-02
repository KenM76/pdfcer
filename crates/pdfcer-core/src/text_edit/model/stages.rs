//! The three recognition stages: lines, column bands, blocks.

use super::cells::{CellRegion, cell_index_at};
use super::{
    Block, BlockDiagnostics, BlockKind, BlockRecognitionOptions, EditableTextModel, GlyphRef, Line,
};
use crate::page_tree::Rect;
use crate::text_extract::{ExtractedGlyph, PageText, TextOrigin};

/// A line under construction in Stage 1, before column/block assignment.
pub(super) struct RawLine {
    pub(super) glyphs: Vec<GlyphRef>,
    baseline_y: f32,
    pub(super) size: f32,
    /// The line's writing direction, from its first glyph.
    pub(super) direction: (f32, f32),
    /// The first glyph's origin, which the perpendicular split measures from.
    origin: (f32, f32),
    /// The [`CellRegion`] index every glyph of this line falls in.
    pub(super) cell: Option<usize>,
    llx: f32,
    lly: f32,
    urx: f32,
    ury: f32,
}

impl RawLine {
    /// A line holding `g` alone, in cell `cell`.
    pub(super) fn new(gref: GlyphRef, g: &ExtractedGlyph, cell: Option<usize>) -> Self {
        let mut line = Self {
            glyphs: Vec::new(),
            baseline_y: g.y,
            size: g.size,
            direction: g.direction,
            origin: (g.x, g.y),
            cell,
            llx: f32::MAX,
            lly: f32::MAX,
            urx: f32::MIN,
            ury: f32::MIN,
        };
        line.push(gref, g);
        line
    }

    /// Append `g`, growing the line's size and box to cover it.
    pub(super) fn push(&mut self, gref: GlyphRef, g: &ExtractedGlyph) {
        self.glyphs.push(gref);
        self.size = self.size.max(g.size);
        let cell = crate::text_extract::glyph_cell(g.x, g.y, g.advance, g.size, g.direction);
        self.llx = self.llx.min(cell.llx as f32);
        self.urx = self.urx.max(cell.urx as f32);
        self.lly = self.lly.min(cell.lly as f32);
        self.ury = self.ury.max(cell.ury as f32);
    }

    fn bbox(&self) -> Rect {
        Rect::from_corners(
            f64::from(self.llx),
            f64::from(self.lly),
            f64::from(self.urx),
            f64::from(self.ury),
        )
    }

    /// Whether `g` (in cell `cell`) must start a new line rather than
    /// extend this one, and why.
    fn split_before(
        &self,
        g: &ExtractedGlyph,
        cell: Option<usize>,
        options: &BlockRecognitionOptions,
    ) -> Option<Split> {
        let size = self.size.max(g.size).max(1e-6);
        let (dx, dy) = self.direction;
        if dx * g.direction.0 + dy * g.direction.1 < options.same_direction_cos {
            return Some(Split::Baseline);
        }
        // Perpendicular to the line's own direction, so a rotated line is
        // not split between every letter.
        let (ox, oy) = self.origin;
        let perp = (g.x - ox) * dy - (g.y - oy) * dx;
        if perp.abs() > options.line_baseline_ratio * size {
            return Some(Split::Baseline);
        }
        (cell != self.cell).then_some(Split::Cell)
    }
}

/// Why Stage 1 closed a line Pass 4 had not already closed.
enum Split {
    Baseline,
    Cell,
}

/// A column band under construction in Stage 2.
struct ColumnAgg {
    llx: f32,
    urx: f32,
    lines: Vec<usize>,
}

impl EditableTextModel<'_> {
    /// Stage 1: split `page.runs` into raw lines at Pass 4's derived line
    /// breaks, at a defensive baseline jump or direction change, and at a
    /// table-cell boundary.
    pub(super) fn cluster_lines(
        page: &PageText,
        options: &BlockRecognitionOptions,
        cells: &[CellRegion],
        diagnostics: &mut BlockDiagnostics,
    ) -> Vec<RawLine> {
        let mut lines: Vec<RawLine> = Vec::new();
        let mut current: Option<RawLine> = None;
        for (ri, run) in page.runs.iter().enumerate() {
            match run.origin {
                TextOrigin::DerivedLineBreak => lines.extend(current.take()),
                TextOrigin::DerivedWordSpace => {}
                // §14.9.4 N4: no glyphs to cluster; counted, left atomic.
                TextOrigin::ActualText => diagnostics.atomic_runs += 1,
                // Body-text-adjacent, not body text (§14.8.2.2).
                TextOrigin::Glyphs if run.artifact.is_some() => {
                    diagnostics.artifact_runs_skipped += 1;
                }
                TextOrigin::Glyphs => {
                    for (gi, g) in run.glyphs.iter().enumerate() {
                        let cell = cell_index_at(cells, g);
                        if let Some(split) = current
                            .as_ref()
                            .and_then(|l| l.split_before(g, cell, options))
                        {
                            match split {
                                Split::Baseline => diagnostics.lines_split_by_baseline += 1,
                                Split::Cell => diagnostics.lines_split_by_cell += 1,
                            }
                            lines.extend(current.take());
                        }
                        diagnostics.glyphs_clustered += 1;
                        let gref = GlyphRef::new(ri, gi);
                        match current.as_mut() {
                            Some(line) => line.push(gref, g),
                            None => current = Some(RawLine::new(gref, g, cell)),
                        }
                    }
                }
            }
        }
        lines.extend(current.take());
        lines
    }

    /// Stage 2: cluster raw lines into left-to-right column bands by
    /// horizontal overlap and finalize each into a [`Line`]. Body lines are
    /// banded first, so a table row spanning two text columns joins one of
    /// them rather than founding a band both would then join.
    pub(super) fn cluster_columns(
        raw: Vec<RawLine>,
        options: &BlockRecognitionOptions,
    ) -> (Vec<Line>, usize) {
        let mut columns: Vec<ColumnAgg> = Vec::new();
        let body = raw.iter().enumerate().filter(|(_, l)| l.cell.is_none());
        let in_cells = raw.iter().enumerate().filter(|(_, l)| l.cell.is_some());
        for (li, line) in body.chain(in_cells) {
            match best_band(&columns, line.llx, line.urx, options.column_overlap_ratio) {
                Some(ci) => {
                    if let Some(col) = columns.get_mut(ci) {
                        col.llx = col.llx.min(line.llx);
                        col.urx = col.urx.max(line.urx);
                        col.lines.push(li);
                    }
                }
                None => columns.push(ColumnAgg {
                    llx: line.llx,
                    urx: line.urx,
                    lines: vec![li],
                }),
            }
        }
        let column_of = left_to_right(&columns);
        let mut lines: Vec<Line> = Vec::with_capacity(raw.len());
        for (ci, col) in columns.iter_mut().enumerate() {
            let column = column_of.get(ci).copied().unwrap_or(0);
            col.lines.sort_unstable();
            for &li in &col.lines {
                if let Some(src) = raw.get(li) {
                    lines.push(Line {
                        glyphs: src.glyphs.clone(),
                        baseline_y: src.baseline_y,
                        direction: src.direction,
                        size: src.size,
                        bbox: src.bbox(),
                        column,
                        block: 0,
                        cell: src.cell,
                    });
                }
            }
        }
        (lines, columns.len())
    }

    /// Stage 3: segment each column's body lines into paragraphs by leading
    /// gap and first-line indent, then make one block per table cell, and
    /// stamp every line's `block`.
    pub(super) fn segment_blocks(
        lines: &mut [Line],
        columns: usize,
        cells: &[CellRegion],
        options: &BlockRecognitionOptions,
        diagnostics: &mut BlockDiagnostics,
    ) -> Vec<Block> {
        let mut blocks: Vec<Block> = Vec::new();
        for column in 0..columns {
            for paragraph in paragraphs_in_column(lines, column, options, diagnostics) {
                blocks.push(Block {
                    kind: BlockKind::Paragraph,
                    column,
                    bbox: union_bbox(lines, &paragraph),
                    line_indices: paragraph,
                    cell_rect: None,
                });
            }
        }
        let cell_blocks = super::cells::cell_blocks(lines, cells);
        diagnostics.table_cell_blocks = cell_blocks.len() as u64;
        blocks.extend(cell_blocks);
        for (bi, block) in blocks.iter().enumerate() {
            for &li in &block.line_indices {
                if let Some(line) = lines.get_mut(li) {
                    line.block = bi;
                }
            }
        }
        blocks
    }
}

/// The band with the greatest overlap of `[llx, urx]` that is at least
/// `ratio` of the narrower span.
fn best_band(columns: &[ColumnAgg], llx: f32, urx: f32, ratio: f32) -> Option<usize> {
    let line_w = (urx - llx).max(0.0);
    let mut best: Option<(usize, f32)> = None;
    for (ci, col) in columns.iter().enumerate() {
        let overlap = (urx.min(col.urx) - llx.max(col.llx)).max(0.0);
        let narrower = line_w.min((col.urx - col.llx).max(0.0)).max(1e-6);
        if overlap >= ratio * narrower && best.is_none_or(|(_, b)| overlap > b) {
            best = Some((ci, overlap));
        }
    }
    best.map(|(ci, _)| ci)
}

/// Maps each band's index to its left-to-right rank: the derived reading
/// order of an untagged multi-column page (§14.8.2.3.1).
fn left_to_right(columns: &[ColumnAgg]) -> Vec<usize> {
    let mut ranked: Vec<(f32, usize)> = columns
        .iter()
        .enumerate()
        .map(|(ci, col)| (col.llx, ci))
        .collect();
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut column_of = vec![0usize; columns.len()];
    for (new_ci, &(_, old_ci)) in ranked.iter().enumerate() {
        if let Some(slot) = column_of.get_mut(old_ci) {
            *slot = new_ci;
        }
    }
    column_of
}

/// One column's body lines (not in a cell), top to bottom, split into
/// paragraphs at a leading gap or a first-line indent.
fn paragraphs_in_column(
    lines: &[Line],
    column: usize,
    options: &BlockRecognitionOptions,
    diagnostics: &mut BlockDiagnostics,
) -> Vec<Vec<usize>> {
    let mut col_lines: Vec<(usize, f32, f64, f32)> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.column == column && l.cell.is_none())
        .map(|(i, l)| (i, l.baseline_y, l.bbox.llx, l.size))
        .collect();
    col_lines.sort_by(|a, b| b.1.total_cmp(&a.1));
    let margin = col_lines
        .iter()
        .map(|&(_, _, llx, _)| llx)
        .fold(f64::MAX, f64::min);
    // The median leading is robust to one outsized paragraph gap.
    let mut gaps: Vec<f32> = col_lines
        .windows(2)
        .filter_map(|w| match w {
            [a, b] => Some(a.1 - b.1),
            _ => None,
        })
        .collect();
    let typical = median(&mut gaps);

    let mut paragraphs: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut prev_baseline: Option<f32> = None;
    for &(i, baseline_y, llx, size) in &col_lines {
        if let Some(prev_y) = prev_baseline {
            let gap = prev_y - baseline_y;
            let leading_break =
                typical > 0.0 && gap > (1.0 + options.paragraph_leading_ratio) * typical;
            let indent_break = llx - margin > f64::from(options.indent_ratio * size);
            if leading_break {
                diagnostics.paragraph_breaks_by_leading += 1;
            } else if indent_break {
                // Counted only when it is the reason, so the two counters
                // partition the paragraph starts.
                diagnostics.paragraph_breaks_by_indent += 1;
            }
            if (leading_break || indent_break) && !current.is_empty() {
                paragraphs.push(std::mem::take(&mut current));
            }
        }
        current.push(i);
        prev_baseline = Some(baseline_y);
    }
    if !current.is_empty() {
        paragraphs.push(current);
    }
    paragraphs
}

/// The union of the boxes of `lines[indices]`; a zero rect when empty.
pub(super) fn union_bbox(lines: &[Line], indices: &[usize]) -> Rect {
    let mut bbox: Option<Rect> = None;
    for b in indices
        .iter()
        .filter_map(|&li| lines.get(li))
        .map(|l| l.bbox)
    {
        bbox = Some(match bbox {
            None => b,
            Some(acc) => Rect {
                llx: acc.llx.min(b.llx),
                lly: acc.lly.min(b.lly),
                urx: acc.urx.max(b.urx),
                ury: acc.ury.max(b.ury),
            },
        });
    }
    bbox.unwrap_or_else(|| Rect::from_corners(0.0, 0.0, 0.0, 0.0))
}

/// The median of `gaps` (sorts in place); `0.0` when empty, so a one-line
/// column yields one block.
fn median(gaps: &mut [f32]) -> f32 {
    if gaps.is_empty() {
        return 0.0;
    }
    gaps.sort_by(f32::total_cmp);
    let mid = gaps.len() / 2;
    if gaps.len() % 2 == 1 {
        gaps.get(mid).copied().unwrap_or(0.0)
    } else {
        let lo = gaps.get(mid.wrapping_sub(1)).copied().unwrap_or(0.0);
        let hi = gaps.get(mid).copied().unwrap_or(0.0);
        (lo + hi) / 2.0
    }
}
