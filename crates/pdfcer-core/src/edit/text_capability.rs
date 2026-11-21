//! Caret-time edit capability for a run in the page's content or inside a
//! form XObject the page paints.

use super::EditSession;
use crate::span::ByteSpan;
use crate::text_edit::edit::{EditPlanTarget, PlanMode, plan_edit_with_records, walk_records};
use crate::text_edit::{EditError, EditOptions, EditRequest, EditTarget, forms};

impl EditSession {
    /// [`Self::edit_capability`] for the buffer `target` names: the page's
    /// own content, or the form XObject a run's provenance places it in.
    ///
    /// `span` is a byte range in that buffer (the form's decoded content for
    /// [`EditTarget::Form`]). The answer is the run-level refusal
    /// [`Self::edit_text`] would give for the same [`EditRequest`] with the
    /// same `target`, including the form-only refusals. [`EditTarget::Auto`]
    /// asks about the page's content, as [`Self::edit_capability`] does.
    ///
    /// # Errors
    ///
    /// The [`EditError`] an edit of this run would raise;
    /// [`EditError::PinnedSpanNotFound`] when `span` names no show operator
    /// in the buffer; [`EditError::NoMatch`] when the page paints no form
    /// with that object number.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use pdfcer_core::{document::Document, edit::EditSession, span::ByteSpan};
    /// use pdfcer_core::text_edit::EditTarget;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let session = EditSession::new(Document::load(std::path::Path::new("drawing.pdf"))?);
    /// let span = ByteSpan { start: 40, len: 12 };
    /// match session.edit_capability_for(0, span, EditTarget::Form { object: 12 }) {
    ///     Ok(()) => println!("editable"),
    ///     Err(e) => println!("{e}"),
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn edit_capability_for(
        &self,
        page_index: usize,
        span: ByteSpan,
        target: EditTarget,
    ) -> Result<(), EditError> {
        let EditTarget::Form { object } = target else {
            return self.edit_capability(page_index, span);
        };
        let mut req = EditRequest::whole_operator(page_index, span, "");
        req.target = target;
        self.form_preview_plan(&req, object).map(|_| ())
    }

    /// A preview plan of `req` against form `object`'s own content.
    fn form_preview_plan(
        &self,
        req: &EditRequest,
        object: u32,
    ) -> Result<crate::text_edit::edit::EditPlan, EditError> {
        if crate::encryption_gate::forbids(
            &self.base,
            &[crate::crypto::PermissionBit::ModifyContents],
        ) {
            return Err(EditError::Encrypted);
        }
        let pages = self.pages()?;
        let page = pages
            .get(req.page_index)
            .ok_or(EditError::PageIndex(req.page_index))?;
        let view = self.view();
        let form = forms::scan_page_forms(&view, page)
            .forms
            .into_iter()
            .find(|f| f.id.num == object)
            .ok_or_else(|| EditError::no_match(req.find.clone()))?;
        let stream = crate::content::ContentStream::from_form(&view, form.id)
            .map_err(|_| EditError::no_match(req.find.clone()))?;
        let invocations = forms::invocation_map(&view)
            .remove(&object)
            .unwrap_or_default();
        let target = EditPlanTarget::form(form, invocations);
        let recs = walk_records(&view, &target.resources, &stream);
        let opts = EditOptions::default();
        plan_edit_with_records(
            &view,
            &target,
            &stream,
            &recs,
            req,
            &opts,
            PlanMode::Preview,
        )
    }
}
