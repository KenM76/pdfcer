//! Optimize on a session: [`EditSession::plan_optimize`],
//! [`EditSession::apply_optimize`] and [`EditSession::save_optimized`].

use std::collections::{BTreeMap, BTreeSet};

use super::{Command, CommandKind, EditSession, ObjectWrite};
use crate::document::Document;
use crate::object::{ObjId, Object, Stream};
use crate::optimize::select::ObjectFacts;
use crate::optimize::streams::{self, Rewrite};
use crate::optimize::{
    OptimizeError, OptimizePlan, OptimizeSettings, OptimizedFile, PlannedChange, Refusal, Skipped,
    StreamAction,
};
use crate::space_audit::{self, ObjectSpace, SpaceCategory};
use crate::writer::{self, RebuildOptions, RebuildReport, SaveOptions, WriteError};

impl EditSession {
    /// Plan an optimize run over the session as it stands, performing every
    /// encoding in memory (see [`crate::optimize`]). Changes nothing.
    ///
    /// Categories and page uses come from a space audit of the file as
    /// opened; an object created in this session has neither, so only id,
    /// size and filter constraints select it.
    ///
    /// # Errors
    ///
    /// [`OptimizeError::Refused`] for an encrypted document, or a signed one
    /// without [`OptimizeSettings::allow_signed`].
    ///
    /// # Example
    ///
    /// ```
    /// # use pdfcer_core::{document::Document, edit::EditSession, optimize::Preset};
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// # let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/synthetic/hello.pdf"))?;
    /// let session = EditSession::new(Document::from_bytes(bytes)?);
    /// let plan = session.plan_optimize(&Preset::Standard.settings())?;
    /// println!("{} streams, {} B saved", plan.changes.len(), plan.stream_saving());
    /// # Ok(()) }
    /// ```
    pub fn plan_optimize(
        &self,
        settings: &OptimizeSettings,
    ) -> Result<OptimizePlan, OptimizeError> {
        let mut notes = self.optimize_gate(settings)?;
        let audit = space_audit::audit(&self.base);
        let rows: BTreeMap<ObjId, &ObjectSpace> = audit.objects.iter().map(|o| (o.id, o)).collect();
        let ids: BTreeSet<ObjId> = rows
            .keys()
            .chain(self.state.keys())
            .copied()
            .filter(|id| !self.deleted.contains(id))
            .collect();
        let mut plan = OptimizePlan {
            file_bytes: self.base.bytes().len() as u64,
            changes: Vec::new(),
            skipped: Vec::new(),
            notes: Vec::new(),
            cleanup: crate::optimize::CleanupPlan::default(),
            discard: Vec::new(),
            discard_objects: Vec::new(),
            writes: Vec::new(),
        };
        let view = self.view();
        for id in ids {
            let Some(Object::Stream(stream)) = self.value(id) else {
                continue;
            };
            let Some(raw) = view.slice(stream.data_span) else {
                continue;
            };
            let row = rows.get(&id).copied();
            plan_stream(&mut plan, settings, id, stream, raw, row);
        }
        if plan.changes.iter().any(|c| c.lossy) {
            notes.push("some changes are lossy; each is listed".to_owned());
        }
        plan.cleanup = self.plan_cleanup(&settings.cleanup, &mut notes);
        plan.discard = self.plan_discard(settings.discard, &mut notes);
        plan.discard_objects = self.plan_discard_objects(settings.discard, &mut notes);
        if plan
            .discard_objects
            .iter()
            .any(|o| o.object == crate::optimize::DiscardObject::Bookmarks)
        {
            // Every bookmark goes anyway; listing the broken ones too would
            // count them twice.
            plan.cleanup.bookmarks.clear();
        }
        plan.notes = notes;
        Ok(plan)
    }

    /// The refusals, or the disclosures for overridden ones.
    fn optimize_gate(&self, settings: &OptimizeSettings) -> Result<Vec<String>, OptimizeError> {
        let mut refusals = Vec::new();
        let mut notes = Vec::new();
        if self.base.encryption().is_some() {
            refusals.push(Refusal::Encrypted);
        }
        let census = crate::signature::census(self);
        if census.signatures > 0 {
            if settings.allow_signed {
                notes.push(format!(
                    "{} signature(s) will be invalidated by the rewrite",
                    census.signatures
                ));
            } else {
                refusals.push(Refusal::Signed {
                    signatures: census.signatures,
                    certifications: census.certifications,
                });
            }
        }
        if refusals.is_empty() {
            Ok(notes)
        } else {
            Err(OptimizeError::Refused(refusals))
        }
    }

    /// Commit `plan` as ONE undoable command and return how many objects it
    /// rewrote or removed. Pair with [`Self::save_optimized`].
    ///
    /// # Errors
    ///
    /// [`OptimizeError::StalePlan`] when an object the plan rewrites has
    /// changed since planning, and [`OptimizeError::CleanupFailed`] when a
    /// clean-up deletion fails, [`OptimizeError::DiscardFailed`] or
    /// [`OptimizeError::ObjectDiscardFailed`] when a discarded item cannot
    /// be removed. Nothing is committed in any case.
    pub fn apply_optimize(&mut self, plan: &OptimizePlan) -> Result<usize, OptimizeError> {
        if let Some(w) = plan
            .writes
            .iter()
            .find(|w| self.value(w.id) != Some(&w.before) || self.deleted.contains(&w.id))
        {
            return Err(OptimizeError::StalePlan { id: w.id });
        }
        let mut fold = super::optimize_cleanup::Fold::new(self, CommandKind::Optimize { count: 0 });
        let removed = self.apply_cleanup(&plan.cleanup, &mut fold)?
            + self.apply_discard(&plan.discard, &mut fold)?
            + self.apply_discard_objects(&plan.discard_objects, &mut fold)?;
        if plan.writes.is_empty() {
            fold.finish(self, removed);
            return Ok(removed);
        }
        let mut objects = Vec::with_capacity(plan.writes.len());
        for w in &plan.writes {
            let data_span = self.stage_bytes(&w.data);
            objects.push(ObjectWrite {
                id: w.id,
                before: self.state.get(&w.id).cloned(),
                after: Some(Object::Stream(Stream {
                    dict: w.dict.clone(),
                    data_span,
                })),
            });
        }
        let count = objects.len();
        self.commit(Command {
            kind: CommandKind::Optimize { count },
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        fold.finish(self, count + removed);
        Ok(count + removed)
    }

    /// The full rewrite of this session, rebuilt under `structure`, **only if
    /// it is strictly smaller** than the file as opened (decision 191's
    /// whole-file rule). The report is `None` when `structure` is all off and
    /// the file's structure was kept.
    ///
    /// # Errors
    ///
    /// As [`Self::to_full_bytes`], and [`WriteError::RewriteReload`] if the
    /// rewrite cannot be reloaded for the rebuild.
    pub fn save_optimized(
        &self,
        options: &SaveOptions,
        structure: &RebuildOptions,
    ) -> Result<(OptimizedFile, Option<RebuildReport>), WriteError> {
        let (bytes, _) = self.to_full_bytes(options)?;
        let (bytes, report) = if structure.is_none() {
            (bytes, None)
        } else {
            let doc =
                Document::from_bytes(bytes).map_err(|e| WriteError::RewriteReload(Box::new(e)))?;
            let (rebuilt, report) = writer::rebuild(&doc, structure, options)?;
            (rebuilt, Some(report))
        };
        let original = self.base.bytes().len() as u64;
        Ok((crate::optimize::keep_if_smaller(bytes, original), report))
    }
}

/// Decide and perform one stream's action, recording it in `plan`.
fn plan_stream(
    plan: &mut OptimizePlan,
    settings: &OptimizeSettings,
    id: ObjId,
    stream: &Stream,
    raw: &[u8],
    row: Option<&ObjectSpace>,
) {
    let filters = streams::chain(&stream.dict);
    let facts = ObjectFacts {
        audit: row,
        stored_bytes: raw.len() as u64,
        filters: &filters,
    };
    let action = settings.action_for(id, &facts);
    if action == StreamAction::Leave {
        return;
    }
    let skip = |reason| Skipped { id, action, reason };
    if let Some(what) = streams::protected(&stream.dict) {
        plan.skipped
            .push(skip(crate::optimize::SkipReason::Protected(what)));
        return;
    }
    match streams::apply(action, &stream.dict, raw) {
        Ok(None) => {}
        Ok(Some((dict, data))) => {
            plan.changes.push(PlannedChange {
                id,
                category: row.map_or(SpaceCategory::OtherReferenced, |r| r.category),
                action,
                filters_before: filters,
                before_bytes: raw.len() as u64,
                after_bytes: data.len() as u64,
                lossy: action.is_lossy(),
            });
            plan.writes.push(Rewrite {
                id,
                before: Object::Stream(stream.clone()),
                dict,
                data,
            });
        }
        Err(reason) => plan.skipped.push(skip(reason)),
    }
}
