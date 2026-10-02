//! Replace a recognised block's whole text: [`EditSession::edit_block_text`],
//! its preview, and the point-to-block lookup a shell starts from.

use super::{CommandKind, EditSession, PermissionBit};
use crate::text_edit::block_text::plan_block_text;
use crate::text_edit::{
    BlockEditError, BlockEditOptions, BlockEditPreview, BlockEditReport, BlockHit,
    EditableTextModel, ReflowApplyError, UnsupportedCause, detect_cell_regions,
    reflow_recognition_options,
};
use crate::text_extract::{self, ExtractOptions};

impl EditSession {
    /// **Replace block `block_index`'s whole text with `text` and re-wrap
    /// it** — one undo entry ([`CommandKind::EditBlockText`]).
    ///
    /// Blocks are numbered as [`Self::reflow_block`] numbers them; use
    /// [`Self::block_at_point`] to find one under a click. `\n` in `text` is
    /// a paragraph break; other whitespace runs become single word gaps.
    ///
    /// The text is set in the block's **first** run's font, size, text state
    /// and colours, encoded as an [`Self::edit_text`] of that run would be
    /// (codes allocated and glyphs added as needed, a same-face sibling under
    /// [`EditOptions::with_sibling_fonts`](crate::text_edit::EditOptions::with_sibling_fonts)),
    /// and packed greedily at the block's width (or
    /// [`BlockEditOptions::wrap_width`]) under its detected alignment and
    /// leading. The first line keeps its origin; later lines step down by the
    /// leading. The block's text objects are replaced by one `BT … ET` with a
    /// `Tm` per line (ISO 32000-2 §9.4.2) and a `TJ` per line (§9.4.3); the
    /// §9.3 text state is restored before `ET` so nothing after the block
    /// sees a change. `q`/`Q` and marked-content sequences (§14.6) inside the
    /// region must balance, and those inside it go with the old text
    /// (counted in [`BlockEditReport::marked_content_removed`]).
    ///
    /// All-or-nothing: on any error the session is unchanged.
    ///
    /// # Errors
    ///
    /// - [`BlockEditError::Text`] with [`EditError::Unencodable`](crate::text_edit::EditError)
    ///   naming **every** character the first run's font cannot take.
    /// - [`BlockEditError::Block`] when the block cannot be rewritten (bad
    ///   index, rotated or shared text, a form XObject, an unbalanced
    ///   region, an encrypted document).
    /// - [`BlockEditError::EmptyText`] for text with no visible character.
    pub fn edit_block_text(
        &mut self,
        page_index: usize,
        block_index: usize,
        text: &str,
        opts: &BlockEditOptions,
    ) -> Result<BlockEditReport, BlockEditError> {
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
        let plan = plan_block_text(&self.view(), page_index, block_index, text, opts)?;
        let mut report = plan.preview.report;
        let kind = CommandKind::EditBlockText {
            lines_before: report.lines_before,
            lines_after: report.lines_after,
        };
        let prior = self.revisions(plan.font_writes)?;
        let (command, decoupled) = self
            .text_edit_command(
                kind,
                content_id,
                page,
                plan.new_content,
                prior,
                &mut report.disclosures,
            )
            .map_err(|e| {
                BlockEditError::unsupported(UnsupportedCause::CommitFailed {
                    detail: e.to_string(),
                })
            })?;
        if let Some(d) = decoupled {
            report.content_object = d.content_object;
            report.extra_objects_emptied = d.emptied;
        }
        self.commit(command);
        Ok(report)
    }

    /// The layout [`Self::edit_block_text`] with the same arguments would
    /// commit: lines, origins, boxes, positioned glyphs and the report. Runs
    /// the same plan, so breaks and origins are identical; the session is
    /// not touched.
    ///
    /// # Errors
    ///
    /// Exactly the refusals [`Self::edit_block_text`] would raise.
    pub fn edit_block_text_preview(
        &self,
        page_index: usize,
        block_index: usize,
        text: &str,
        opts: &BlockEditOptions,
    ) -> Result<BlockEditPreview, BlockEditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(ReflowApplyError::Encrypted.into());
        }
        Ok(plan_block_text(&self.view(), page_index, block_index, text, opts)?.preview)
    }

    /// The block under page point `(x, y)` (user space, points) and its text,
    /// numbered as [`Self::edit_block_text`] takes it; `None` when no text
    /// line is near the point.
    ///
    /// This is [`EditableTextModel::hit_test`] then
    /// [`EditableTextModel::block_at`] on the cell-aware model
    /// [`reflow_recognition_options`] builds, packaged for one call.
    ///
    /// # Errors
    ///
    /// [`BlockEditError::Block`] for a bad page index or an unreadable page.
    pub fn block_at_point(
        &self,
        page_index: usize,
        x: f64,
        y: f64,
    ) -> Result<Option<BlockHit>, BlockEditError> {
        let view = self.view();
        let pages = crate::page_tree::pages_in(&view).map_err(ReflowApplyError::from)?;
        let page = pages
            .get(page_index)
            .ok_or(ReflowApplyError::PageIndex(page_index))?;
        let options = ExtractOptions::default().with_provenance(true);
        let extracted = text_extract::extract_page_view(&view, page, page_index, &options)
            .map_err(ReflowApplyError::from)?;
        let cells = detect_cell_regions(&view, page_index)
            .map_err(crate::text_edit::reflow_apply::table_error)?;
        let model = EditableTextModel::recognize_with_cells(
            &extracted,
            &reflow_recognition_options(),
            &cells,
        );
        let Some(block_index) = model.hit_test(x, y).and_then(|p| model.block_at(p)) else {
            return Ok(None);
        };
        Ok(model.blocks().get(block_index).map(|b| BlockHit {
            block_index,
            text: model.block_text(b).replace('\n', " "),
            bbox: b.bbox,
        }))
    }
}
