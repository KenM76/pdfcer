//! Telling a block's wrapped line ends from its typed breaks.
//!
//! A greedy line breaker (pdfcer's, and the usual word processor's) moves a
//! word down only when it does not fit. So when the next line's first word
//! WOULD have fitted after a line, inside the block's wrap width, the line
//! was ended by hand. The wrap width is taken as the block's own width (a
//! cell block's [`cell_wrap_width`]), which is never wider than the width
//! the producer wrapped at, so the test never calls a real wrap a break.

use super::{Block, EditableTextModel, Line, glyph_text};
use crate::linebreak::FIT_TOLERANCE;
use crate::text_edit::reflow_fit::cell_wrap_width;

/// How a recognised line ends, as [`EditableTextModel::line_ends`] reads it.
///
/// Inferred from the layout, never from a mark in the file (rule 4: a shell
/// shows a block re-opened with these as inferred).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LineEnd {
    /// The next line's first word would have fitted on this line, so a
    /// wrapping producer would not have broken here: a break the author
    /// typed. A producer that balances its lines rather than filling them
    /// can also break early; that is the uncertainty.
    Break,
    /// The next line's first word did not fit: a wrap. A break typed at a
    /// line already too full for the next word looks the same on the page
    /// and is read as a wrap; that is the uncertainty.
    Wrap,
    /// The block's last line.
    Last,
}

impl LineEnd {
    /// The lower-case name: `break`, `wrap` or `last`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Break => "break",
            Self::Wrap => "wrap",
            Self::Last => "last",
        }
    }
}

/// Positions along a line's writing direction, in points.
#[derive(Debug, Clone, Copy)]
struct Extent {
    start: f64,
    end: f64,
    /// The end of the line's first word.
    first_word_end: f64,
}

/// A gap wider than this many ems between two glyphs ends a word even with
/// no space glyph (a producer that positions words with `TJ` or `Td`).
const WORD_GAP_EM: f64 = 0.15;
/// The space width assumed when the block shows no space and no word gap.
const FALLBACK_SPACE_EM: f64 = 0.33;

impl EditableTextModel<'_> {
    /// How each of `block`'s lines ends, in [`Block::line_indices`] order:
    /// [`LineEnd::Break`] when the next line's first word would have fitted
    /// on it within the block's wrap width, else [`LineEnd::Wrap`]; the last
    /// line is [`LineEnd::Last`]. Each variant states its uncertainty.
    ///
    /// The wrap width is the block's width along its writing direction (a
    /// table cell block's inner width, as `edit_block_text` wraps it). A
    /// line is measured from the block's leftmost line start, so a centred
    /// or right-aligned short line reads as [`LineEnd::Wrap`] unless it is
    /// short enough for a left-aligned reading too: the test errs toward
    /// wrap. The space is the block's median space glyph, else its median
    /// word gap, else a third of an em.
    #[must_use]
    pub fn line_ends(&self, block: &Block) -> Vec<LineEnd> {
        let lines: Vec<&Line> = block
            .line_indices
            .iter()
            .filter_map(|&li| self.lines.get(li))
            .collect();
        let Some(first) = lines.first() else {
            return Vec::new();
        };
        let dir = (f64::from(first.direction.0), f64::from(first.direction.1));
        let size = f64::from(first.size).max(1.0);
        let extents: Vec<Option<Extent>> =
            lines.iter().map(|l| self.extent(l, dir, size)).collect();
        let start = extents
            .iter()
            .flatten()
            .map(|e| e.start)
            .fold(f64::INFINITY, f64::min);
        let widest = extents
            .iter()
            .flatten()
            .map(|e| e.end - start)
            .fold(0.0, f64::max);
        let width = cell_wrap_width(block).unwrap_or(widest);
        let space = self.space_width(&lines, dir, size);
        let last = lines.len() - 1;
        (0..lines.len())
            .map(|i| {
                if i == last {
                    return LineEnd::Last;
                }
                let (Some(Some(this)), Some(Some(next))) = (extents.get(i), extents.get(i + 1))
                else {
                    return LineEnd::Wrap;
                };
                let word = next.first_word_end - next.start;
                if this.end - start + space + word <= width + FIT_TOLERANCE {
                    LineEnd::Break
                } else {
                    LineEnd::Wrap
                }
            })
            .collect()
    }

    /// `block`'s text with [`Self::line_ends`] applied: a wrap joins two
    /// lines with a space, a break with `\n`. The spelling
    /// [`EditSession::edit_block_text`](crate::edit::EditSession::edit_block_text)
    /// takes, so a block re-opened this way and written back keeps both its
    /// wraps and its breaks.
    #[must_use]
    pub fn block_text_with_breaks(&self, block: &Block) -> String {
        let ends = self.line_ends(block);
        let mut out = String::new();
        for (i, &li) in block.line_indices.iter().enumerate() {
            if let Some(line) = self.lines.get(li) {
                out.push_str(self.line_text(line).trim_end());
            }
            match ends.get(i) {
                Some(LineEnd::Break) => out.push('\n'),
                Some(LineEnd::Wrap) => out.push(' '),
                _ => {}
            }
        }
        out
    }

    /// The line's start, end and first-word end along `dir`, ignoring
    /// space glyphs; `None` for a line of spaces.
    fn extent(&self, line: &Line, dir: (f64, f64), size: f64) -> Option<Extent> {
        let mut out: Option<Extent> = None;
        let mut in_first_word = true;
        for &gref in &line.glyphs {
            let g = self.glyph(gref)?;
            let pos = f64::from(g.x) * dir.0 + f64::from(g.y) * dir.1;
            let end = pos + f64::from(g.advance);
            if glyph_text(self.page, gref).trim().is_empty() {
                in_first_word = out.is_none();
                continue;
            }
            match &mut out {
                None => {
                    out = Some(Extent {
                        start: pos,
                        end,
                        first_word_end: end,
                    });
                }
                Some(e) => {
                    if pos - e.end > WORD_GAP_EM * size {
                        in_first_word = false;
                    }
                    if in_first_word {
                        e.first_word_end = end;
                    }
                    e.end = e.end.max(end);
                }
            }
        }
        out
    }

    /// The median space glyph advance in `lines`, else the median gap
    /// between words, else [`FALLBACK_SPACE_EM`] ems.
    fn space_width(&self, lines: &[&Line], dir: (f64, f64), size: f64) -> f64 {
        let mut spaces = Vec::new();
        let mut gaps = Vec::new();
        for line in lines {
            let mut prev_end: Option<f64> = None;
            for &gref in &line.glyphs {
                let Some(g) = self.glyph(gref) else { continue };
                if glyph_text(self.page, gref) == " " {
                    spaces.push(f64::from(g.advance));
                    continue;
                }
                let pos = f64::from(g.x) * dir.0 + f64::from(g.y) * dir.1;
                if let Some(p) = prev_end
                    && pos - p > WORD_GAP_EM * size
                {
                    gaps.push(pos - p);
                }
                prev_end = Some(pos + f64::from(g.advance));
            }
        }
        median(spaces)
            .or_else(|| median(gaps))
            .unwrap_or(FALLBACK_SPACE_EM * size)
    }
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    v.retain(|x| x.is_finite() && *x > 0.0);
    v.sort_by(f64::total_cmp);
    v.get(v.len() / 2).copied()
}
