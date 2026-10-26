//! The three recognition stages: lines, column bands, blocks.

use super::cells::{CellRegion, cell_index_at};
use super::lists;
use super::{
    Block, BlockDiagnostics, BlockKind, BlockRecognitionOptions, EditableTextModel, GlyphRef, Line,
};
use crate::page_tree::Rect;
use crate::text_extract::{ExtractedGlyph, PageText, TextOrigin};

/// A line under construction in Stage 1, before column/block assignment.
pub(super) struct RawLine {
    pub(super) glyphs: Vec<GlyphRef>,
    pub(super) baseline_y: f32,
    pub(super) size: f32,
    /// The line's writing direction, from its first glyph.
    pub(super) direction: (f32, f32),
    /// The first glyph's origin, which the perpendicular split measures from.
    origin: (f32, f32),
    /// The [`CellRegion`] index every glyph of this line falls in.
    pub(super) cell: Option<usize>,
    pub(super) llx: f32,
    lly: f32,
    pub(super) urx: f32,
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
        let ratio = options.column_overlap_ratio;
        let mut columns: Vec<ColumnAgg> = Vec::new();
        for (li, line) in raw.iter().enumerate().filter(|(_, l)| l.cell.is_none()) {
            band(&mut columns, li, line, ratio);
        }
        merge_bands(&mut columns, ratio);
        for (li, line) in raw.iter().enumerate().filter(|(_, l)| l.cell.is_some()) {
            band(&mut columns, li, line, ratio);
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
    /// gap and first-line indent, then make each table cell's paragraphs
    /// blocks, and
    /// stamp every line's `block`.
    pub(super) fn segment_blocks(
        page: &PageText,
        lines: &mut [Line],
        columns: usize,
        cells: &[CellRegion],
        options: &BlockRecognitionOptions,
        diagnostics: &mut BlockDiagnostics,
    ) -> Vec<Block> {
        let mut blocks: Vec<Block> = Vec::new();
        for column in 0..columns {
            for (paragraph, kind) in paragraphs_in_column(page, lines, column, options, diagnostics)
            {
                if kind == BlockKind::ListItem {
                    diagnostics.list_item_blocks += 1;
                }
                blocks.push(Block {
                    kind,
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

/// Add line `li` to its best band, or found one.
fn band(columns: &mut Vec<ColumnAgg>, li: usize, line: &RawLine, ratio: f32) {
    match best_band(columns, line.llx, line.urx, ratio) {
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

/// Merge bands that came to overlap by `ratio` of the narrower as they grew.
/// Banding is greedy in content order, so a heading's second word drawn
/// before the body below it founds a band of its own; once the body widens
/// both, they are one column, and a short line between them must not be
/// split off by whichever it happened to overlap more.
fn merge_bands(columns: &mut Vec<ColumnAgg>, ratio: f32) {
    let mut i = 0;
    while i < columns.len() {
        let Some(a) = columns.get(i) else { break };
        let (llx, urx) = (a.llx, a.urx);
        let other = columns.iter().enumerate().skip(i + 1).find(|(_, c)| {
            let overlap = (urx.min(c.urx) - llx.max(c.llx)).max(0.0);
            let narrower = (urx - llx).min(c.urx - c.llx).max(1e-6);
            overlap >= ratio * narrower
        });
        match other.map(|(j, _)| j) {
            Some(j) => {
                let b = columns.swap_remove(j);
                if let Some(a) = columns.get_mut(i) {
                    a.llx = a.llx.min(b.llx);
                    a.urx = a.urx.max(b.urx);
                    a.lines.extend(b.lines);
                }
            }
            None => i += 1,
        }
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

/// A continuation line joins a list item when its left edge is within this
/// many ems of the item's hanging indent.
const LIST_INDENT_TOLERANCE_EM: f64 = 0.25;

/// One body line of a column: its index, baseline y, left edge and size.
#[derive(Clone, Copy)]
struct ColLine {
    index: usize,
    baseline_y: f32,
    llx: f64,
    size: f32,
}

/// A baseline gap within this range of the line size, in ems, is one line
/// pitch: single to one-and-a-half spacing, excluding a sub/superscript
/// shift and a blank line.
const PITCH_EM: (f32, f32) = (0.6, 1.6);

/// A column's body lines, top to bottom, with the column's left margin and
/// median leading.
fn column_metrics(lines: &[Line], column: usize) -> (Vec<ColLine>, f64, f32) {
    let mut col_lines: Vec<ColLine> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.column == column && l.cell.is_none())
        .map(|(index, l)| ColLine {
            index,
            baseline_y: l.baseline_y,
            llx: l.bbox.llx,
            size: l.size,
        })
        .collect();
    col_lines.sort_by(|a, b| b.baseline_y.total_cmp(&a.baseline_y));
    let margin = col_lines.iter().map(|c| c.llx).fold(f64::MAX, f64::min);
    // The leading is the median of the gaps that look like one line pitch.
    // A page of short paragraphs and headings each set off by a blank line
    // has more blank-line gaps than pitches, so the median of ALL gaps is a
    // blank line and no paragraph would break (G205). The plain median is
    // the fallback for a column with no pitch-sized gap (double spacing).
    let pairs: Vec<(f32, f32)> = col_lines
        .windows(2)
        .filter_map(|w| match w {
            [a, b] => Some((a.baseline_y - b.baseline_y, a.size.max(b.size))),
            _ => None,
        })
        .collect();
    let mut pitches: Vec<f32> = pairs
        .iter()
        .filter(|&&(gap, size)| (PITCH_EM.0 * size..=PITCH_EM.1 * size).contains(&gap))
        .map(|&(gap, _)| gap)
        .collect();
    let typical = if pitches.is_empty() {
        median(&mut pairs.iter().map(|&(gap, _)| gap).collect::<Vec<_>>())
    } else {
        median(&mut pitches)
    };
    (col_lines, margin, typical)
}

/// Whether every glyph of `line` is whitespace: the empty paragraph a word
/// processor writes between two paragraphs.
fn is_blank(page: &PageText, line: &Line) -> bool {
    line.glyphs
        .iter()
        .all(|&g| super::glyph_text(page, g).trim().is_empty())
}

/// Whether a line with left edge `llx` and size `size` sits at `indent_x`.
fn at_indent(llx: f64, size: f32, indent_x: f64) -> bool {
    (llx - indent_x).abs() <= LIST_INDENT_TOLERANCE_EM * f64::from(size)
}

/// One column's body lines (not in a cell), top to bottom, split into
/// blocks at a leading gap or a first-line indent, and into list items.
///
/// A line opening with a bullet or enumerator ([`lists::hanging`]) starts a
/// [`BlockKind::ListItem`] when it would start a block anyway, follows
/// another item, or the next line sits at its hanging indent; later lines
/// stay in the item while they sit at that indent with no leading break.
fn paragraphs_in_column(
    page: &PageText,
    lines: &[Line],
    column: usize,
    options: &BlockRecognitionOptions,
    diagnostics: &mut BlockDiagnostics,
) -> Vec<(Vec<usize>, BlockKind)> {
    let (col_lines, margin, typical) = column_metrics(lines, column);
    let mut blocks: Vec<(Vec<usize>, BlockKind)> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut kind = BlockKind::Paragraph;
    let mut item_indent: Option<f64> = None;
    let mut prev_baseline: Option<f32> = None;
    let mut prev_llx = margin;
    for (k, c) in col_lines.iter().enumerate() {
        let ColLine {
            index: i,
            baseline_y,
            llx,
            size,
        } = *c;
        if lines.get(i).is_some_and(|l| is_blank(page, l)) {
            close_at_blank(&mut blocks, &mut current, kind, i, diagnostics);
            item_indent = None;
            prev_baseline = None;
            continue;
        }
        let leading_break = prev_baseline.is_some_and(|prev_y| {
            typical > 0.0 && prev_y - baseline_y > (1.0 + options.paragraph_leading_ratio) * typical
        });
        // A first-line indent is measured from the line above, not the
        // column margin: a body set in from its headings is not indented
        // on every line.
        // A piece on the previous line's baseline (a heading drawn in two
        // pieces) continues that line's block.
        let same_row = prev_baseline.is_some_and(|y| (y - baseline_y).abs() < 0.5 * size);
        let indent_break = prev_baseline.is_some()
            && !same_row
            && llx - prev_llx.max(margin) > f64::from(options.indent_ratio * size);
        if !same_row {
            prev_llx = llx;
        }
        let opens_item = lines
            .get(i)
            .filter(|_| !same_row)
            .and_then(|l| lists::hanging(page, l))
            .filter(|h| {
                current.is_empty()
                    || leading_break
                    || indent_break
                    || item_indent.is_some()
                    || col_lines
                        .get(k + 1)
                        .is_some_and(|n| at_indent(n.llx, n.size, h.indent_x))
            });
        let break_here = !same_row
            && (opens_item.is_some()
                || match item_indent {
                    Some(x) => leading_break || !at_indent(llx, size, x),
                    None => leading_break || indent_break,
                });
        if break_here && !current.is_empty() {
            let plain = opens_item.is_none() && item_indent.is_none();
            count_break(diagnostics, leading_break, indent_break && plain);
            blocks.push((std::mem::take(&mut current), kind));
        }
        if current.is_empty() {
            item_indent = opens_item.map(|h| h.indent_x);
            kind = item_indent.map_or(BlockKind::Paragraph, |_| BlockKind::ListItem);
        }
        current.push(i);
        prev_baseline = Some(baseline_y);
    }
    if !current.is_empty() {
        blocks.push((current, kind));
    }
    blocks
}

/// A blank line closes the open block and is a paragraph of its own.
fn close_at_blank(
    blocks: &mut Vec<(Vec<usize>, BlockKind)>,
    current: &mut Vec<usize>,
    kind: BlockKind,
    blank: usize,
    diagnostics: &mut BlockDiagnostics,
) {
    if !current.is_empty() {
        diagnostics.paragraph_breaks_by_blank_line += 1;
        blocks.push((std::mem::take(current), kind));
    }
    blocks.push((vec![blank], BlockKind::Paragraph));
}

/// Count a block start under the break that caused it — only when it is the
/// reason, so the counters partition the block starts.
fn count_break(diagnostics: &mut BlockDiagnostics, leading: bool, indent: bool) {
    if leading {
        diagnostics.paragraph_breaks_by_leading += 1;
    } else if indent {
        diagnostics.paragraph_breaks_by_indent += 1;
    }
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
