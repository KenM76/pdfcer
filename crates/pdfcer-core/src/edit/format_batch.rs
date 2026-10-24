//! [`EditSession::format_texts`]: many format requests as one command
//! (request G227).

use super::{CommandKind, EditSession, MAX_UNDO_DEPTH};
use crate::content::ContentStream;
use crate::crypto::PermissionBit;
use crate::text_edit::edit::EditPlanTarget;
use crate::text_edit::format::{
    PlannedFormat, is_locational_format_error, plan_format_target, walk_target,
};
use crate::text_edit::format_plan::splice_batch;
use crate::text_edit::{
    EditTarget, FormatBatch, FormatBatchError, FormatError, FormatOptions, FormatReport,
    FormatRequest,
};

/// `(request indices, plans)` in application order.
type OncePlans = (Vec<usize>, Vec<PlannedFormat>);

fn at(index: usize, error: FormatError) -> FormatBatchError {
    FormatBatchError::Request {
        index,
        error: Box::new(error),
    }
}

impl EditSession {
    /// Apply many [`Self::format_text`] requests as **one command and one
    /// undo entry**: the result is the same as calling `format_text` for each
    /// request in descending pinned-span order.
    ///
    /// When every request is pinned (`FormatRequest::whole_operator` or a
    /// pinned span) to the same page's own `/Contents`, with spans that do
    /// not overlap, the stream is walked once, every request is planned
    /// against that walk, and the page is rewritten once — so the cost is
    /// close to one `format_text` call, not one per request
    /// ([`FormatBatch::planned_once`]). A request that adds a font or an
    /// opacity resource, draws a decoration, synthesises or removes an
    /// italic slant, lands in a form XObject, or is unpinned makes the batch
    /// apply the requests one at a time instead, folded into one undo entry.
    ///
    /// An empty slice changes nothing and returns an empty batch.
    ///
    /// # Errors
    ///
    /// [`FormatBatchError::Request`] names the first refused request by its
    /// index in `reqs`, with the [`FormatError`] `format_text` would have
    /// returned; [`FormatBatchError::TooManyForOneUndo`] when a batch that
    /// must be applied one at a time is longer than
    /// [`MAX_UNDO_DEPTH`]. On any error **nothing is applied**.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use pdfcer_core::{document::Document, edit::EditSession, span::ByteSpan};
    /// # use pdfcer_core::text_edit::{FormatOptions, FormatRequest};
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut session = EditSession::new(Document::load(std::path::Path::new("in.pdf"))?);
    /// let spans = [ByteSpan { start: 120, len: 14 }, ByteSpan { start: 40, len: 9 }];
    /// let reqs: Vec<_> = spans
    ///     .iter()
    ///     .map(|s| FormatRequest::whole_operator(0, *s).drawn_size(6.0))
    ///     .collect();
    /// let batch = session.format_texts(&reqs, &FormatOptions::default())?;
    /// assert_eq!(batch.reports.len(), 2);
    /// session.undo(); // one step restores the page
    /// # Ok(())
    /// # }
    /// ```
    pub fn format_texts(
        &mut self,
        reqs: &[FormatRequest],
        opts: &FormatOptions,
    ) -> Result<FormatBatch, FormatBatchError> {
        if let Some(i) = reqs.iter().position(FormatRequest::is_empty) {
            return Err(at(i, FormatError::NoOp));
        }
        if reqs.is_empty() {
            return Ok(FormatBatch {
                reports: Vec::new(),
                disclosures: Vec::new(),
                planned_once: true,
            });
        }
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(at(0, FormatError::Encrypted));
        }
        match self.format_texts_once(reqs, opts)? {
            Some(batch) => Ok(batch),
            None => self.format_texts_each(reqs, opts),
        }
    }

    /// The one-walk path; `Ok(None)` when the batch is not eligible for it.
    fn format_texts_once(
        &mut self,
        reqs: &[FormatRequest],
        opts: &FormatOptions,
    ) -> Result<Option<FormatBatch>, FormatBatchError> {
        let Some(order) = once_order(reqs) else {
            return Ok(None);
        };
        let page_index = reqs.first().map_or(0, |r| r.page_index);
        let pages = self.pages().map_err(|e| at(0, FormatError::from(e)))?;
        let page = pages
            .get(page_index)
            .ok_or(at(0, FormatError::PageIndex(page_index)))?
            .clone();
        let Some(content_id) = page.contents.first().copied() else {
            return Ok(None);
        };
        let stream = self
            .current_page_content(&page)
            .map_err(|e| at(0, FormatError::Content(e)))?;
        let target = EditPlanTarget::page(&page).map_err(|e| at(0, FormatError::from_edit(e)))?;
        let Some((indices, mut planned)) = self.plan_once(&target, &stream, reqs, &order, opts)?
        else {
            return Ok(None);
        };
        let Some(new_content) = splice_batch(&stream.buf, &mut planned) else {
            return Ok(None);
        };
        let mut disclosures = Vec::new();
        let (command, decoupled) = self
            .text_edit_command(
                CommandKind::FormatText,
                content_id,
                &page,
                new_content,
                Vec::new(),
                &mut disclosures,
            )
            .map_err(|e| at(0, FormatError::Unsupported(e.to_string())))?;
        self.commit(command);
        self.take_structure_notes(&mut disclosures);
        let mut reports: Vec<Option<FormatReport>> = vec![None; reqs.len()];
        for (i, p) in indices.into_iter().zip(planned) {
            let mut report = p.report;
            if let Some(d) = &decoupled {
                report.content_object = d.content_object;
                report.extra_objects_emptied = d.emptied;
            }
            if let Some(slot) = reports.get_mut(i) {
                *slot = Some(report);
            }
        }
        Ok(Some(FormatBatch {
            reports: reports.into_iter().flatten().collect(),
            disclosures,
            planned_once: true,
        }))
    }

    /// Plan every request, in `order`, against one walk of `stream`:
    /// `(request indices, plans)` in application order, or `Ok(None)` when a
    /// plan is not local to its own edits or the batch belongs in a form.
    fn plan_once(
        &self,
        target: &EditPlanTarget,
        stream: &ContentStream,
        reqs: &[FormatRequest],
        order: &[usize],
        opts: &FormatOptions,
    ) -> Result<Option<OncePlans>, FormatBatchError> {
        let view = self.view();
        let recs = walk_target(&view, target, stream).map_err(|e| at(0, e))?;
        let mut indices = Vec::with_capacity(order.len());
        let mut plans = Vec::with_capacity(order.len());
        for &i in order {
            let Some(req) = reqs.get(i) else { continue };
            match plan_format_target(&view, target, stream, &recs, req, opts) {
                Ok(p) if p.local && p.created_font.is_none() && p.created_alphas.is_empty() => {
                    indices.push(i);
                    plans.push(p);
                }
                Ok(_) => return Ok(None),
                Err(e)
                    if matches!(req.target, EditTarget::Auto) && is_locational_format_error(&e) =>
                {
                    return Ok(None);
                }
                Err(e) => return Err(at(i, e)),
            }
        }
        Ok(Some((indices, plans)))
    }

    /// One `format_text` per request, in descending pinned order, folded
    /// into one undo entry; rolled back on the first refusal.
    fn format_texts_each(
        &mut self,
        reqs: &[FormatRequest],
        opts: &FormatOptions,
    ) -> Result<FormatBatch, FormatBatchError> {
        if reqs.len() > MAX_UNDO_DEPTH {
            return Err(FormatBatchError::TooManyForOneUndo {
                requests: reqs.len(),
                limit: MAX_UNDO_DEPTH,
            });
        }
        let before = self.checkpoint();
        let mut reports: Vec<Option<FormatReport>> = vec![None; reqs.len()];
        for i in descending(reqs) {
            let Some(req) = reqs.get(i) else { continue };
            match self.format_text(req, opts) {
                Ok(report) => {
                    if let Some(slot) = reports.get_mut(i) {
                        *slot = Some(report);
                    }
                }
                Err(e) => {
                    self.abandon(before);
                    return Err(at(i, e));
                }
            }
        }
        let pushed = self.pushed_since(&before);
        let _ = self.coalesce_last(pushed, CommandKind::FormatText);
        Ok(FormatBatch {
            reports: reports.into_iter().flatten().collect(),
            disclosures: Vec::new(),
            planned_once: false,
        })
    }
}

/// Request indices by descending pinned start (unpinned last, in order).
fn descending(reqs: &[FormatRequest]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..reqs.len()).collect();
    order.sort_by_key(|&i| {
        std::cmp::Reverse(reqs.get(i).and_then(|r| r.pinned_span).map(|s| s.start))
    });
    order
}

/// The application order when the batch can be planned in one walk: every
/// request pinned, on one page, in the page's own content, spans disjoint.
fn once_order(reqs: &[FormatRequest]) -> Option<Vec<usize>> {
    let page = reqs.first()?.page_index;
    let eligible = reqs.iter().all(|r| {
        r.page_index == page
            && r.pinned_span.is_some()
            && matches!(r.target, EditTarget::Auto | EditTarget::PageContents)
    });
    if !eligible {
        return None;
    }
    let order = descending(reqs);
    let spans: Vec<_> = order
        .iter()
        .filter_map(|&i| reqs.get(i).and_then(|r| r.pinned_span))
        .collect();
    let disjoint = spans.windows(2).all(|w| match w {
        [later, earlier] => earlier.start + earlier.len <= later.start,
        _ => true,
    });
    disjoint.then_some(order)
}
