//! The marks `edit_block_text` writes so its own line ends read back exactly.
//!
//! A text object pdfcer writes for a block opens with a `/pdfc_TextBlock MP`
//! marked-content point, and each line ended by a typed break is followed by a
//! `/pdfc_Break MP` (ISO 32000-1 §14.6, Table 320; Figure 9 allows
//! marked-content operators inside a text object). The `pdfc_` prefix makes
//! them second-class names (Annex E), which other readers ignore. Inside such
//! a text object a line end is exact: a break where a `pdfc_Break` sits
//! between the two lines' show operators, else a wrap.

use std::ops::Range;

use super::{Block, EditableTextModel, LineEndSource};
use crate::content::ContentStream;
use crate::object::Object;

/// The tag of the point that opens a text object pdfcer wrote for a block.
pub const TEXT_BLOCK_TAG: &[u8] = b"pdfc_TextBlock";
/// The tag of the point after a line that a typed break ended.
pub const BREAK_TAG: &[u8] = b"pdfc_Break";

/// The line-end marks of a page's own content (not its form XObjects), as
/// byte offsets into the decoded buffer [`ContentStream::from_page`] builds,
/// the buffer [`GlyphProvenance::operator_span`](crate::text_extract::GlyphProvenance::operator_span)
/// indexes for page glyphs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineMarks {
    /// `BT … ET` byte ranges whose text object carries a `pdfc_TextBlock`.
    blocks: Vec<Range<usize>>,
    /// The offsets of `pdfc_Break` points, ascending.
    breaks: Vec<usize>,
}

impl LineMarks {
    /// Scan a page's content for the marks.
    #[must_use]
    pub fn scan(stream: &ContentStream) -> Self {
        let mut out = Self::default();
        let mut open: Option<(usize, bool)> = None;
        for op in stream.operations() {
            let at = op.operator.span.start;
            match op.operator_name(&stream.buf) {
                Some(b"BT") => open = Some((at, false)),
                Some(b"ET") => {
                    if let Some((start, true)) = open.take() {
                        out.blocks.push(start..op.operator.span.end());
                    }
                }
                Some(b"MP") => match point_tag(op.operands) {
                    Some(t) if t == TEXT_BLOCK_TAG => {
                        if let Some((_, marked)) = open.as_mut() {
                            *marked = true;
                        }
                    }
                    Some(t) if t == BREAK_TAG && open.is_some() => out.breaks.push(at),
                    _ => {}
                },
                _ => {}
            }
        }
        out
    }

    /// True when no mark was found.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// The marked text object holding both byte offsets, if any.
    pub(super) fn block_holding(&self, a: usize, b: usize) -> Option<&Range<usize>> {
        self.blocks
            .iter()
            .find(|r| r.contains(&a) && r.contains(&b))
    }

    /// How many break points lie in `range`.
    pub(super) fn breaks_in(&self, range: Range<usize>) -> usize {
        let lo = self.breaks.partition_point(|&b| b < range.start);
        let hi = self.breaks.partition_point(|&b| b < range.end);
        hi.saturating_sub(lo)
    }
}

impl EditableTextModel<'_> {
    /// Attach the page's line-end marks, so [`Self::line_ends`] reads
    /// pdfcer's own blocks exactly. The model must have been recognized from
    /// an extraction with provenance (the marks are matched by byte offset).
    #[must_use]
    pub fn with_line_marks(mut self, marks: LineMarks) -> Self {
        self.marks = marks;
        self
    }

    /// Whether [`Self::line_ends`] reads `block` from marks or infers it.
    #[must_use]
    pub fn line_end_source(&self, block: &Block) -> LineEndSource {
        if self.marked_gaps(block).is_some() {
            LineEndSource::Marked
        } else {
            LineEndSource::Inferred
        }
    }

    /// For a block wholly inside one marked text object: the break points
    /// between each line and the next. `None` otherwise.
    pub(super) fn marked_gaps(&self, block: &Block) -> Option<Vec<usize>> {
        if self.marks.is_empty() {
            return None;
        }
        let mut spans = Vec::with_capacity(block.line_indices.len());
        for &li in &block.line_indices {
            let line = self.lines.get(li)?;
            let mut span: Option<(usize, usize)> = None;
            for &g in &line.glyphs {
                let p = self.provenance(g)?;
                if !p.content_stream.is_page() {
                    return None;
                }
                let (s, e) = (p.operator_span.start, p.operator_span.end());
                span = Some(span.map_or((s, e), |(a, b)| (a.min(s), b.max(e))));
            }
            spans.push(span?);
        }
        let first = spans.iter().map(|s| s.0).min()?;
        let last = spans.iter().map(|s| s.1).max()?;
        self.marks.block_holding(first, last.saturating_sub(1))?;
        Some(
            spans
                .iter()
                .zip(spans.iter().skip(1))
                .map(|(this, next)| self.marks.breaks_in(this.1..next.0))
                .collect(),
        )
    }
}

/// The name operand of an `MP`.
fn point_tag(operands: &[crate::content::ContentToken]) -> Option<&[u8]> {
    match operands {
        [t] => match &t.kind {
            crate::content::ContentTokenKind::Operand(Object::Name(n)) => Some(n.0.as_slice()),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)] // test assertions
    use super::*;

    fn scan(content: &str) -> LineMarks {
        LineMarks::scan(&ContentStream::parse(content.as_bytes().to_vec()).unwrap())
    }

    #[test]
    fn only_marked_text_objects_and_their_breaks_count() {
        let m = scan(
            "BT /pdfc_TextBlock MP (a) Tj /pdfc_Break MP (b) Tj ET \
             BT (c) Tj /pdfc_Break MP (d) Tj ET /pdfc_Break MP",
        );
        assert_eq!(m.blocks.len(), 1);
        assert_eq!(
            m.breaks.len(),
            2,
            "a break outside BT … ET is not a line end"
        );
        let r = m.blocks[0].clone();
        assert_eq!(m.breaks_in(r), 1);
    }

    #[test]
    fn other_tags_are_not_marks() {
        assert!(scan("BT /Span MP /pdfc_OCR MP (a) Tj ET").is_empty());
    }
}
