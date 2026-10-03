//! Replace a recognised block's whole text and re-wrap it: the verb behind
//! [`EditSession::edit_block_text`](crate::edit::EditSession::edit_block_text)
//! and its preview.
//!
//! The block is the one [`ReflowEngine`](super::ReflowEngine) numbers (the
//! cell-aware model under [`reflow_recognition_options`](super::reflow_recognition_options)).
//! The new text is set in the block's FIRST run's font, size, text state and
//! colours, encoded exactly as an `edit_text` of that run would encode it
//! (code allocation, subset extension, same-face sibling), measured with that
//! font's widths and packed with the shared greedy breaker
//! ([`crate::linebreak::greedy_pack`]). The block's `BT … ET` region is
//! replaced by one new text object: one `Tm` per line (§9.4.2), one `Tj`/`TJ`
//! per line (§9.4.3), justify slack as negative `TJ` numbers after each word
//! gap, and the R88 restore before `ET`. A preview and a commit run the same
//! `plan_block_text`, so their breaks and origins are identical by
//! construction.

mod emit;
mod layout;

use crate::page_tree::Rect;
use crate::text_edit::cause::UnsupportedCause;

pub(crate) use layout::{block_looks, plan_block_text};

use super::edit::{EditError, EditOptions, TextEditPreview};
use super::reflow::{BlockAlignment, PageOverflow};
use super::reflow_apply::ReflowApplyError;

/// How [`edit_block_text`](crate::edit::EditSession::edit_block_text) lays the
/// new text out.
///
/// # Examples
///
/// ```
/// use pdfcer_core::text_edit::BlockEditOptions;
///
/// let opts = BlockEditOptions::new().with_wrap_width(240.0);
/// assert_eq!(opts.wrap_width, Some(240.0));
/// assert_eq!(BlockEditOptions::default().wrap_width, None);
/// ```
#[derive(Debug, Clone, Copy, Default)]
#[non_exhaustive]
pub struct BlockEditOptions {
    /// Wrap width in points; `None` wraps at the block's current width (a
    /// table cell's inner width for a cell block).
    pub wrap_width: Option<f64>,
    /// The encoding options an `edit_text` of the first run would take:
    /// embedded glyphs, subset augmentation, same-face siblings.
    pub edit: EditOptions,
}

impl BlockEditOptions {
    /// Every default: the block's own width, default [`EditOptions`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Wrap at `width` points.
    #[must_use]
    pub const fn with_wrap_width(mut self, width: f64) -> Self {
        self.wrap_width = Some(width);
        self
    }

    /// Wrap at `width` when `Some`, else at the block's own width.
    #[must_use]
    pub const fn with_wrap_width_opt(mut self, width: Option<f64>) -> Self {
        self.wrap_width = width;
        self
    }

    /// Encode under `edit` (see [`EditOptions`]).
    #[must_use]
    pub const fn with_edit_options(mut self, edit: EditOptions) -> Self {
        self.edit = edit;
        self
    }
}

/// One laid-out line of the new text, in page user space (points, y up).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct BlockEditLine {
    /// The line's characters, words joined by single spaces; empty for a
    /// blank paragraph.
    pub text: String,
    /// The x of the line's first glyph origin.
    pub origin_x: f64,
    /// The line's baseline y.
    pub baseline_y: f64,
    /// The line's natural width (before any justify slack), points.
    pub width: f64,
}

/// What an `edit_block_text` did and inferred. Every field is disclosure
/// (rule 4); [`Self::disclosures`] carries the operator-facing sentences.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct BlockEditReport {
    /// The block replaced.
    pub block_index: usize,
    /// Lines the block had.
    pub lines_before: usize,
    /// Lines the new text occupies.
    pub lines_after: usize,
    /// The width the text was wrapped at, points.
    pub wrap_width: f64,
    /// The block's alignment (detected; the new lines use it).
    pub alignment: BlockAlignment,
    /// Distinct looks among the block's runs: font, size, fill, stroke and
    /// every text-state parameter except leading. `1` is a uniform block;
    /// above `1` the new text flattens them to the first run's look.
    pub looks: usize,
    /// New block height minus old, points (positive = taller).
    pub height_delta: f64,
    /// How far the last new baseline falls below the block's original last
    /// baseline, points; `None` when the text fits the original height.
    pub overflow_pt: Option<f64>,
    /// The new block past the page cropbox, when it is. Content is written
    /// at its true position regardless.
    pub page_overflow: Option<PageOverflow>,
    /// `/BaseFont` of the font the text is set in (subset tag included).
    pub base_font: String,
    /// `/BaseFont` of the first run's own font when a same-face sibling
    /// carries the text instead ([`EditOptions::with_sibling_fonts`]).
    pub font_substituted_from: Option<String>,
    /// Characters the font gained a glyph, code or `/ToUnicode` entry for.
    pub glyphs_added: Vec<char>,
    /// The `/MCID` of the sequence enclosing the block, if tagged.
    pub tagged_mcid: Option<i64>,
    /// Marked-content sequences (`BDC`/`BMC` … `EMC`) that sat between the
    /// block's text objects and were removed with them.
    pub marked_content_removed: usize,
    /// The content-stream object rewritten.
    pub content_object: u32,
    /// Extra `/Contents` streams folded into the first and emptied.
    pub extra_objects_emptied: u64,
    /// Every operator-facing disclosure, verbatim.
    pub disclosures: Vec<String>,
}

/// The layout an `edit_block_text` with the same arguments would commit.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct BlockEditPreview {
    /// The new lines, top to bottom.
    pub lines: Vec<BlockEditLine>,
    /// The baseline-to-baseline distance used, points.
    pub leading: f64,
    /// The block's box before the edit.
    pub old_bbox: Rect,
    /// The box the new lines occupy: the wrap width wide, top-anchored.
    pub new_bbox: Rect,
    /// Every glyph, positioned, for drawing (`pdfcer_render::edit_preview`):
    /// font, colours, render mode and one matrix per code.
    pub glyphs: TextEditPreview,
    /// What the commit would report (`content_object` and
    /// `extra_objects_emptied` as planned, before any decoupling).
    pub report: BlockEditReport,
}

/// Why a block's text could not be replaced. The session is unchanged.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BlockEditError {
    /// The block cannot be located, rewritten or re-wrapped: a bad page or
    /// block index, a rotated, shared or non-contiguous block, a form
    /// XObject, an encrypted document, …
    #[error(transparent)]
    Block(#[from] ReflowApplyError),
    /// The first run's font cannot take the new text: every character it
    /// refuses is named, as `edit_text` names them.
    #[error(transparent)]
    Text(#[from] EditError),
    /// The new text has no non-whitespace character. Deleting a block is a
    /// different operation.
    #[error("the new block text is empty")]
    EmptyText,
}

impl BlockEditError {
    /// A block refusal by cause, without spelling out the wrapping.
    pub(crate) fn unsupported(cause: UnsupportedCause) -> Self {
        Self::Block(ReflowApplyError::Unsupported(cause))
    }
}

/// The block under a page point, and its text.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct BlockHit {
    /// The index `edit_block_text` and `reflow_block` take.
    pub block_index: usize,
    /// The block's text, its lines joined by single spaces: a starting value
    /// for `edit_block_text`, where `\n` means a paragraph break.
    pub text: String,
    /// The block's box.
    pub bbox: Rect,
    /// [`BlockEditReport::looks`] for this block; `None` when
    /// `edit_block_text` would refuse the block (its runs cannot be located).
    pub looks: Option<usize>,
}
