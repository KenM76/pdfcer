//! List items: a line that opens with a bullet or enumerator, and the lines
//! after it that start at its hanging indent.

use super::{GlyphRef, Line};
use crate::block_layout::list_marker;
use crate::text_extract::{ExtractedGlyph, PageText};

/// Below this the direction's y component is zero: a list is recognised on
/// horizontal lines only, where a hanging indent is an x position.
const HORIZONTAL_EPS: f32 = 1e-3;

/// A positioned gap at least this many ems wide ends the marker even with
/// no space glyph, as when a producer places the text with `Td` or `TJ`.
const MARKER_GAP_EM: f32 = 0.15;

/// Where a list item's marker ends and its text starts on its first line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Hanging {
    /// Index into the line's glyphs of the marker's last glyph.
    pub(crate) marker_last: usize,
    /// The marker's first glyph origin x, default user space.
    pub(crate) marker_x: f64,
    /// The first text glyph's origin x: the hanging indent.
    pub(crate) indent_x: f64,
}

/// The decoded text of one glyph, its slice of its run's text, or `""` for
/// a stale reference.
pub(crate) fn glyph_text(page: &PageText, gref: GlyphRef) -> &str {
    page.runs
        .get(gref.run)
        .and_then(|run| {
            run.glyphs.get(gref.glyph).and_then(|g| {
                let start = g.text_start as usize;
                let end = start + g.text_len as usize;
                run.text.get(start..end)
            })
        })
        .unwrap_or("")
}

/// The hanging-indent geometry of `line` when it opens a list item: on a
/// horizontal left-to-right line, the first token (ended by a space glyph or
/// a positioned gap) is a bullet or enumerator and text follows it.
pub(crate) fn hanging(page: &PageText, line: &Line) -> Option<Hanging> {
    if line.direction.1.abs() > HORIZONTAL_EPS || line.direction.0 <= 0.0 {
        return None;
    }
    let glyph = |i: usize| -> Option<&ExtractedGlyph> {
        let r = line.glyphs.get(i)?;
        page.runs.get(r.run)?.glyphs.get(r.glyph)
    };
    let text = |i: usize| line.glyphs.get(i).map_or("", |&g| glyph_text(page, g));
    let blank = |i: usize| text(i).trim().is_empty();
    let n = line.glyphs.len();
    let first = (0..n).find(|&i| !blank(i))?;
    let mut token = String::new();
    let mut last = first;
    for i in first..n {
        token.push_str(text(i));
        last = i;
        let gap = match (glyph(i), glyph(i + 1)) {
            (Some(a), Some(b)) => b.x - (a.x + a.advance) > MARKER_GAP_EM * a.size,
            _ => true,
        };
        if gap || blank(i + 1) {
            break;
        }
    }
    let (_, rest) = list_marker(&token)?;
    if !rest.is_empty() {
        return None;
    }
    let text_at = (last + 1..n).find(|&i| !blank(i))?;
    Some(Hanging {
        marker_last: last,
        marker_x: f64::from(glyph(first)?.x),
        indent_x: f64::from(glyph(text_at)?.x),
    })
}
