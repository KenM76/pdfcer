//! Fit checks on a re-wrapped block: the page cropbox (§3.5 / R76) and, for
//! a table-cell block, the cell it sits in. Both are disclosed, never
//! applied: the block is not clipped and nothing around it moves.

use crate::page_tree::Rect;

use super::model::Block;
use super::reflow::{PageOverflow, ReflowDiagnostics, ReflowLine};

const EPS: f64 = 1e-6;

/// A disclosed table-cell overflow: the re-wrapped text of a
/// [`BlockKind::TableCell`](super::BlockKind::TableCell) block does not fit
/// inside [`Block::cell_rect`]. Computed only; the cell, its row and the
/// rows below are not resized or moved.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct CellOverflow {
    /// How far the new text falls below the cell's bottom edge, points.
    pub past_bottom_pt: f64,
    /// How far the new text runs past the cell's right edge, points (an
    /// unbreakable word, or a wrap width wider than the cell).
    pub past_right_pt: f64,
    /// How many re-wrapped lines are not wholly inside the cell.
    pub lines_outside: usize,
}

/// The wrap width that keeps a cell block inside its cell: from the block's
/// left edge to the cell's right edge less the cell's padding. The padding
/// is taken as the smaller of the block's current left and right insets, so
/// a centred or right-aligned cell keeps its margin; it is zero when the
/// text already touches or crosses an edge. `None` for a non-cell block, or
/// when the block starts at or past the cell's inner right edge.
pub(crate) fn cell_wrap_width(block: &Block) -> Option<f64> {
    let cell = block.cell_rect?;
    let left = block.bbox.llx - cell.llx;
    let right = cell.urx - block.bbox.urx;
    let padding = left.min(right).max(0.0);
    let width = cell.urx - padding - block.bbox.llx;
    (width > EPS).then_some(width)
}

/// The right edge of the widest re-wrapped line, or of `new_bbox`.
fn right_extent(new_bbox: Rect, lines: &[ReflowLine]) -> f64 {
    lines
        .iter()
        .map(|l| l.origin_x + l.natural_width)
        .fold(new_bbox.urx, f64::max)
}

/// The page-cropbox overflow of a re-wrap on both axes, disclosed into
/// `diagnostics`; `None` when the new box fits. The right edge matters
/// because an unoverridden wrap width is measured from the block's own box,
/// which an earlier edit may have widened past the margin (R148).
pub(crate) fn page_overflow(
    crop: Rect,
    new_bbox: Rect,
    lines: &[ReflowLine],
    descent: f64,
    diagnostics: &mut ReflowDiagnostics,
) -> Option<PageOverflow> {
    let past_bottom = (crop.lly - new_bbox.lly).max(0.0);
    let past_right = (new_bbox.urx - crop.urx).max(0.0);
    if past_bottom <= EPS && past_right <= EPS {
        return None;
    }
    let lines_outside = lines
        .iter()
        .filter(|l| l.baseline_y - descent < crop.lly - EPS)
        .count();
    if past_bottom > EPS {
        diagnostics.disclose(format!(
            "reflow: re-wrap grows the block {past_bottom:.1}pt past the page bottom \
             (cropbox); {lines_outside} line(s) fall outside the visible page — \
             DISCLOSED, not applied (decision 015 §3.5, R76)"
        ));
    }
    if past_right > EPS {
        diagnostics.disclose(format!(
            "reflow: the wrap width puts the block {past_right:.1}pt past the page right \
             edge (cropbox), so the re-wrapped text runs off the page. That width was \
             measured from the block's own box, which an earlier edit may have widened \
             past the margin — pass an explicit width to wrap to the original margin \
             — DISCLOSED, not applied (R148, R76)"
        ));
    }
    Some(PageOverflow {
        past_bottom_pt: past_bottom,
        lines_outside,
        past_right_pt: past_right,
    })
}

/// The cell overflow of a re-wrapped cell block, disclosed into
/// `diagnostics`; `None` when every line fits inside `cell`.
pub(crate) fn cell_overflow(
    cell: Rect,
    new_bbox: Rect,
    lines: &[ReflowLine],
    descent: f64,
    diagnostics: &mut ReflowDiagnostics,
) -> Option<CellOverflow> {
    let past_bottom = (cell.lly - new_bbox.lly).max(0.0);
    let past_right = (right_extent(new_bbox, lines) - cell.urx).max(0.0);
    if past_bottom <= EPS && past_right <= EPS {
        return None;
    }
    let lines_outside = lines
        .iter()
        .filter(|l| {
            l.baseline_y - descent < cell.lly - EPS || l.origin_x + l.natural_width > cell.urx + EPS
        })
        .count();
    diagnostics.disclose(format!(
        "reflow: the re-wrapped text does not fit its table cell — {past_bottom:.1}pt past the \
         cell bottom, {past_right:.1}pt past its right edge, {lines_outside} line(s) not wholly \
         inside. pdfcer reports this and leaves the cell, its row and the rows below where \
         they are; shorten the text or pass a narrower width"
    ));
    Some(CellOverflow {
        past_bottom_pt: past_bottom,
        past_right_pt: past_right,
        lines_outside,
    })
}
