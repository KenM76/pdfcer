//! Width, merge and split for a text object inside a form XObject — the
//! `_in_form` twins of the page verbs (pdfcer-gui request G158).

use super::{CommandKind, EditError, EditSession, FormSurgeryOutcome, PermissionBit};
use crate::object::{ObjId, Object};
use crate::text_edit::{FormatError as FmtError, FormatReport, MergeOptions, MergeReport};

/// A text verb's report, plus the reach of the shared form it changed.
///
/// A form's content stream is shared by every place it is drawn, so the
/// change shows at `invocations` places on `pages` pages; a shell should
/// say so when either is above 1.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FormTextOutcome<R> {
    /// What the page verb would report for the same change.
    pub report: R,
    /// The form XObject whose stream was rewritten.
    pub form: ObjId,
    /// How many `Do` invocations draw the form, across the document.
    pub invocations: usize,
    /// How many pages draw it at least once.
    pub pages: usize,
}

/// A form leaf resolved to the form's current stream and its model.
struct FormTextLeaf {
    form: ObjId,
    form_dict: crate::object::Dict,
    stream: crate::content::ContentStream,
    model: crate::vector::PageObjects,
    object_index: usize,
}

/// The `FormatError` a text verb reports for a session-level refusal.
pub(super) fn to_format_error(e: EditError) -> FmtError {
    match e {
        EditError::PageOutOfRange { index, .. } => FmtError::PageIndex(index),
        EditError::DocumentEncrypted => FmtError::Encrypted,
        EditError::VectorEditContent(c) => FmtError::Content(c),
        EditError::PageTree(t) => FmtError::PageTree(t),
        EditError::VectorEdit(v) => FmtError::TextRun(v),
        EditError::FormLeafOutOfRange { index, count } => {
            FmtError::FormLeafOutOfRange { index, count }
        }
        other => FmtError::Unsupported(other.to_string()),
    }
}

impl EditSession {
    /// [`Self::set_text_run_width`] for a text object inside a form XObject
    /// (`leaf_index` indexes [`crate::vector::PageObjects::leaves`]).
    ///
    /// `width_pts` is page points along the run's baseline **at this
    /// placement**; the form's stream is shared, so every other place it
    /// is drawn changes by the same horizontal scaling. One undo entry,
    /// `CommandKind::FormatText`.
    ///
    /// # Errors
    ///
    /// As [`Self::set_text_run_width`], plus
    /// [`FormatError::FormLeafOutOfRange`](crate::text_edit::FormatError::FormLeafOutOfRange)
    /// and the certification refusal (as
    /// [`FormatError::Unsupported`](crate::text_edit::FormatError::Unsupported)).
    pub fn set_text_run_width_in_form(
        &mut self,
        page_index: usize,
        leaf_index: usize,
        run_index: usize,
        width_pts: f64,
    ) -> Result<FormTextOutcome<FormatReport>, FmtError> {
        if !(width_pts.is_finite() && width_pts > 0.0) {
            return Err(FmtError::BadTargetWidth(width_pts));
        }
        let leaf = self.form_text_leaf(page_index, leaf_index)?;
        let text = Self::leaf_as_text(&leaf.model, leaf.object_index)?;
        let scale = crate::vector::edit::text_run_width_scale(text, run_index)?;
        let span = super::run_span(text, run_index)?;
        let target = crate::text_edit::EditTarget::Form {
            object: leaf.form.num,
        };
        let report = self.fit_run_to_width(page_index, target, span, scale, width_pts)?;
        Ok(self.form_text_outcome(report, leaf.form))
    }

    /// [`Self::merge_text_runs`] for a text object inside a form XObject
    /// (`leaf_index` indexes [`crate::vector::PageObjects::leaves`]). The
    /// merge shows wherever the form is drawn. One undo entry,
    /// `CommandKind::MergeTextRuns`.
    ///
    /// # Errors
    ///
    /// As [`Self::merge_text_runs`], plus
    /// [`FormatError::FormLeafOutOfRange`](crate::text_edit::FormatError::FormLeafOutOfRange)
    /// and the certification refusal.
    pub fn merge_text_runs_in_form(
        &mut self,
        page_index: usize,
        leaf_index: usize,
        runs: &[usize],
        opts: &MergeOptions,
    ) -> Result<FormTextOutcome<MergeReport>, FmtError> {
        let leaf = self.form_text_leaf(page_index, leaf_index)?;
        let text = Self::leaf_as_text(&leaf.model, leaf.object_index)?;
        if let Some(refusal) = crate::vector::text_merge_refusal(text, runs) {
            return Err(refusal.into());
        }
        let spans: Vec<crate::span::ByteSpan> = runs
            .iter()
            .filter_map(|&i| text.runs.get(i).map(|r| r.bytes))
            .collect();
        let target = self
            .form_plan_target(page_index, leaf.form)
            .ok_or_else(|| FmtError::Unsupported("the form is not drawn on this page".into()))?;
        let plan =
            crate::text_edit::merge::plan_merge(&self.view(), &target, &leaf.stream, &spans, opts)?;
        let command = self.form_edit_command_kind(
            CommandKind::MergeTextRuns,
            leaf.form,
            &leaf.form_dict,
            plan.new_content,
        );
        self.commit(command);
        Ok(self.form_text_outcome(plan.report, leaf.form))
    }

    /// [`Self::split_text_object`] for a text object inside a form XObject
    /// (`leaf_index` indexes [`crate::vector::PageObjects::leaves`]). One
    /// undo entry, `CommandKind::SplitTextObject`.
    ///
    /// # Errors
    ///
    /// As [`Self::split_text_object`] and [`Self::move_node_in_form`].
    pub fn split_text_object_in_form(
        &mut self,
        page_index: usize,
        leaf_index: usize,
        before_runs: &[usize],
    ) -> Result<FormSurgeryOutcome, EditError> {
        self.form_surgery_inner(
            CommandKind::SplitTextObject,
            page_index,
            leaf_index,
            |stream, model, object_index| {
                super::plan_text_split(stream, model, object_index, before_runs)
            },
        )
    }

    /// The guards the form text verbs share, then the leaf's form, its
    /// current stream and the model decomposed from the leaf's placement.
    fn form_text_leaf(
        &mut self,
        page_index: usize,
        leaf_index: usize,
    ) -> Result<FormTextLeaf, FmtError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(FmtError::Encrypted);
        }
        self.check_certification().map_err(to_format_error)?;
        let (leaf, form) = self
            .form_leaf_at(page_index, leaf_index)
            .map_err(to_format_error)?;
        let stream = crate::content::ContentStream::from_form(&self.view(), form)
            .map_err(FmtError::Content)?;
        let Some(Object::Stream(form_stream)) = self.value(form) else {
            return Err(FmtError::Unsupported(format!(
                "object {} is not a form XObject stream",
                form.num
            )));
        };
        let form_dict = form_stream.dict.clone();
        let model = self.form_model(page_index, &stream, &form_dict, &leaf);
        Ok(FormTextLeaf {
            form,
            form_dict,
            stream,
            model,
            object_index: leaf.form_object_index,
        })
    }

    /// The text planner's target for `form` as drawn from `page_index`.
    fn form_plan_target(
        &self,
        page_index: usize,
        form: ObjId,
    ) -> Option<crate::text_edit::edit::EditPlanTarget> {
        use crate::text_edit::forms;
        let pages = self.pages().ok()?;
        let page = pages.get(page_index)?;
        let view = self.view();
        let form_ref = forms::scan_page_forms(&view, page)
            .forms
            .into_iter()
            .find(|f| f.id == form)?;
        let invocations = forms::invocation_map(&view)
            .remove(&form.num)
            .unwrap_or_default();
        Some(crate::text_edit::edit::EditPlanTarget::form(
            form_ref,
            invocations,
        ))
    }

    fn form_text_outcome<R>(&self, report: R, form: ObjId) -> FormTextOutcome<R> {
        let reach = self.form_invocation_reach(form);
        FormTextOutcome {
            report,
            form,
            invocations: reach.invocations,
            pages: reach.pages,
        }
    }
}
