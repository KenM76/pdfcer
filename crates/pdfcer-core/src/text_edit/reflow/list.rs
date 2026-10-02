//! Line packing for a re-wrap, including a list item's hanging indent: the
//! marker keeps its own position and the text wraps at the indent.

use crate::page_tree::Rect;

use super::{
    Block, BlockKind, EditableTextModel, LineFrame, ReflowDiagnostics, ReflowError, ReflowLine,
    WordTok, hanging, line_natural_width, place_lines,
};
use crate::text_edit::model::Hanging;

/// The hanging indent of `block` when it is a list item whose first line
/// still opens with its marker.
pub(super) fn item_hanging(model: &EditableTextModel<'_>, block: &Block) -> Option<Hanging> {
    if block.kind != BlockKind::ListItem {
        return None;
    }
    let line = model.lines().get(*block.line_indices.first()?)?;
    hanging(model.sourced_view(), line)
}

/// Greedily pack `words` into `frame` and place each line, returning the
/// lines and the new block box.
///
/// With `list` (and a word after the marker), word 0 is the marker: it
/// becomes `lines[0]` alone at its source x on the first baseline, and the
/// remaining words wrap in the frame narrowed to start at the hanging
/// indent. [`ReflowDiagnostics::lines_after`] then counts text lines, so
/// it still compares with the block's source line count.
///
/// # Errors
///
/// [`ReflowError::BadWidth`] when the indent leaves no positive text width.
pub(super) fn wrap(
    words: &[WordTok],
    frame: &LineFrame,
    list: Option<Hanging>,
    size: f64,
    diagnostics: &mut ReflowDiagnostics,
) -> Result<(Vec<ReflowLine>, Rect), ReflowError> {
    let widths: Vec<f64> = words.iter().map(|w| w.width).collect();
    let list = list.filter(|_| words.len() >= 2);
    let indent = list.map_or(0.0, |h| h.indent_x - frame.llx);
    let text_frame = LineFrame {
        llx: frame.llx + indent,
        wrap_width: frame.wrap_width - indent,
        ..*frame
    };
    if !(text_frame.wrap_width.is_finite() && text_frame.wrap_width > 0.0) {
        return Err(ReflowError::BadWidth(text_frame.wrap_width));
    }
    let first = usize::from(list.is_some());
    let body = widths.get(first..).unwrap_or(&[]);
    // Greedy re-break through the ONE shared breaker (decision 015 §3.2).
    let ranges = crate::linebreak::greedy_pack(body.len(), text_frame.wrap_width, |s, e| {
        line_natural_width(body, frame.space_width, s, e)
    })
    .into_iter()
    .map(|r| r.start + first..r.end + first)
    .collect();
    let mut lines = place_lines(words, &widths, ranges, &text_frame, diagnostics);
    let new_bbox = frame.block_box(lines.len(), size);
    if let (Some(h), Some(marker)) = (list, words.first()) {
        lines.insert(0, marker_line(marker, h, frame.first_baseline));
        diagnostics.disclose(format!(
            "reflow: list item — the marker '{}' stays at x={:.2} and the text wraps at its \
             hanging indent x={:.2}; the space between them is re-made by positioning, not by \
             re-showing the source space glyph",
            marker.text, h.marker_x, h.indent_x
        ));
    }
    Ok((lines, new_bbox))
}

/// The marker alone, unmoved, on the first baseline.
fn marker_line(marker: &WordTok, h: Hanging, baseline_y: f64) -> ReflowLine {
    ReflowLine {
        words: 0..1,
        text: marker.text.clone(),
        origin_x: h.marker_x,
        baseline_y,
        natural_width: marker.width,
        gap_count: 0,
        is_overflowing_word: false,
        justified_slack: None,
    }
}
