//! Edit a recognised (OCR-layer) paragraph word by word without moving its
//! words off the scan: [`EditSession::edit_recognised_block_text`], its
//! preview, and the point-to-block lookup.

use super::{CommandKind, EditSession, PermissionBit};
use crate::text_edit::recognised_block::{plan_recognised, recognised_hit};
use crate::text_edit::{
    BlockEditError, EditOptions, RecognisedBlockEditPreview, RecognisedBlockEditReport,
    RecognisedBlockHit, ReflowApplyError, UnsupportedCause,
};

impl EditSession {
    /// The recognised block under page point `(x, y)` (user space, points):
    /// a paragraph of the page's **invisible** text (render mode 3 or 7, an
    /// OCR layer, ISO 32000-2 §9.3.6), its words and their boxes; `None`
    /// when no invisible text is near the point.
    ///
    /// Only invisible text is read, so visible text never joins a recognised
    /// block. Blocks are numbered among that text, as
    /// [`Self::edit_recognised_block_text`] takes them, not as
    /// [`Self::block_at_point`] numbers them.
    ///
    /// # Errors
    ///
    /// [`BlockEditError::Block`] for a bad page index or an unreadable page.
    pub fn recognised_block_at_point(
        &self,
        page_index: usize,
        x: f64,
        y: f64,
    ) -> Result<Option<RecognisedBlockHit>, BlockEditError> {
        recognised_hit(&self.view(), page_index, x, y)
    }

    /// **Edit recognised block `block_index`'s words in place** — one undo
    /// entry ([`CommandKind::EditRecognisedText`]).
    ///
    /// `text` is split on whitespace (line breaks carry no meaning) and
    /// diffed by word against the block's words
    /// ([`RecognisedBlockHit::words`]):
    ///
    /// - a **kept** word keeps its codes and position, byte for byte;
    /// - a **replaced** word's new text starts at the old word's first glyph
    ///   origin, in the old word's font and size, with horizontal scaling
    ///   (`Tz`, §9.3.4) fitted so it spans the old word's width (within 25%
    ///   to 400% of the original scaling; a clamp is disclosed);
    /// - a **removed** word's glyphs go;
    /// - a new word with no old word to take its place joins its neighbour's
    ///   replacement (counted in [`RecognisedBlockEditReport::words_merged`]).
    ///
    /// Nothing is re-wrapped. Only the show operators holding a changed word
    /// are rewritten (§9.4.3), each ending with the text matrix, font and
    /// scaling the original left, so no other glyph moves. Characters are
    /// encoded as [`Self::edit_text`] would (codes allocated, glyphs added as
    /// `opts` allows). The edit stays in the content stream it is in: an OCR
    /// layer stream stays its own stream.
    ///
    /// Unchanged text commits nothing and pushes no undo entry. All or
    /// nothing: on any error the session is unchanged.
    ///
    /// # Errors
    ///
    /// - [`BlockEditError::EmptyText`] for text with no word.
    /// - [`BlockEditError::Text`] naming every character the font refuses.
    /// - [`BlockEditError::Block`]: a bad index, an encrypted document, a
    ///   changed word in a form XObject, a `'`/`"` operator or vertical
    ///   text, or an edit spanning two `/Contents` streams
    ///   ([`UnsupportedCause::EditSpansStreams`]).
    pub fn edit_recognised_block_text(
        &mut self,
        page_index: usize,
        block_index: usize,
        text: &str,
        opts: &EditOptions,
    ) -> Result<RecognisedBlockEditReport, BlockEditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(ReflowApplyError::Encrypted.into());
        }
        let pages = self.pages().map_err(ReflowApplyError::from)?;
        let page = pages
            .get(page_index)
            .ok_or(ReflowApplyError::PageIndex(page_index))?;
        let content_id = *page
            .contents
            .first()
            .ok_or(BlockEditError::unsupported(UnsupportedCause::NoContents))?;
        let plan = plan_recognised(&self.view(), page_index, block_index, text, opts)?;
        let mut report = plan.preview.report;
        let Some(new_content) = plan.new_content else {
            return Ok(report);
        };
        let kind = CommandKind::EditRecognisedText {
            words_changed: report.words_replaced + report.words_deleted,
        };
        let prior = self.revisions(plan.font_writes)?;
        let (command, decoupled) = self
            .text_edit_command(
                kind,
                content_id,
                page,
                new_content,
                prior,
                &mut report.disclosures,
            )
            .map_err(|e| {
                BlockEditError::unsupported(UnsupportedCause::CommitFailed {
                    detail: e.to_string(),
                })
            })?;
        report.content_object = decoupled
            .map(|d| d.content_object)
            .or(report.content_object);
        self.commit(command);
        Ok(report)
    }

    /// The layout [`Self::edit_recognised_block_text`] with the same
    /// arguments would leave, and the report it would return, from the same
    /// plan; the session is not touched. Unchanged `text` lays out the block
    /// as it is, so a shell can draw the paragraph the moment it opens.
    ///
    /// # Errors
    ///
    /// Exactly the refusals [`Self::edit_recognised_block_text`] would raise.
    pub fn edit_recognised_block_text_preview(
        &self,
        page_index: usize,
        block_index: usize,
        text: &str,
        opts: &EditOptions,
    ) -> Result<RecognisedBlockEditPreview, BlockEditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(ReflowApplyError::Encrypted.into());
        }
        Ok(plan_recognised(&self.view(), page_index, block_index, text, opts)?.preview)
    }
}
