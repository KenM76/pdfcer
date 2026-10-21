//! Optimize on a session: [`EditSession::plan_optimize`],
//! [`EditSession::apply_optimize`] and [`EditSession::save_optimized`].

use std::collections::{BTreeMap, BTreeSet};

use super::{Command, CommandKind, EditSession, ObjectWrite};
use crate::document::Document;
use crate::object::{ObjId, Object, Stream};
use crate::optimize::image::{self, pipeline::Outcome};
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
            smooth_tolerance: settings.smooth_tolerance,
        };
        self.plan_streams(&mut plan, settings, &ids, &rows);
        if plan.changes.iter().any(|c| c.lossy) {
            notes.push("some changes are lossy; each is listed".to_owned());
        }
        plan.cleanup = self.plan_cleanup(&settings.cleanup, &mut notes);
        plan.discard = self.plan_discard(settings.discard, &mut notes);
        plan.discard_objects = self.plan_discard_objects(settings, &mut notes);
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

    /// Plan every stream among `ids`; `/Matte` soft masks after their
    /// parents, so they can follow a parent's resample. Under
    /// `merge_duplicates`, byte-identical images are measured over every
    /// copy's placements (see `optimize_copies`).
    fn plan_streams(
        &self,
        plan: &mut OptimizePlan,
        settings: &OptimizeSettings,
        ids: &BTreeSet<ObjId>,
        rows: &BTreeMap<ObjId, &ObjectSpace>,
    ) {
        let view = self.view();
        let soft_masks = self.soft_mask_targets(ids);
        let pairs = self.matte_pairs(ids);
        let copies = if settings.structure.merge_duplicates {
            self.image_copies(ids)
        } else {
            BTreeMap::new()
        };
        let wide = super::optimize_copies::widened(rows, &copies);
        let rows: &BTreeMap<ObjId, &ObjectSpace> = &rows
            .iter()
            .map(|(id, r)| (*id, wide.get(id).unwrap_or(r)))
            .collect();
        for &id in ids {
            if pairs.contains_key(&id) {
                continue;
            }
            let Some(Object::Stream(stream)) = self.value(id) else {
                continue;
            };
            let Some(raw) = view.slice(stream.data_span) else {
                continue;
            };
            let row = rows.get(&id).copied();
            let smask = match stream.dict.get(b"SMask") {
                Some(Object::Reference(r)) => match self.value(*r) {
                    Some(Object::Stream(s)) => Some(&s.dict),
                    _ => None,
                },
                _ => None,
            };
            let ctx = image::pipeline::Context {
                view: &view,
                blocker: image::pipeline::mask_blocker(
                    &stream.dict,
                    smask,
                    pairs.values().any(|p| *p == id),
                ),
                convert_blocker: image::pipeline::convert_blocker(
                    &stream.dict,
                    smask,
                    soft_masks.contains(&id),
                ),
                lockstep: None,
            };
            plan_stream(plan, settings, (id, stream, raw), row, &ctx);
        }
        for (&mask, &parent) in &pairs {
            self.plan_matte_mask(plan, settings, (mask, parent), rows, &view);
        }
        super::optimize_copies::disclose(plan, &copies);
    }

    /// Every object some stream among `ids` names as its `/SMask`.
    fn soft_mask_targets(&self, ids: &BTreeSet<ObjId>) -> BTreeSet<ObjId> {
        ids.iter()
            .filter_map(|id| match self.value(*id) {
                Some(Object::Stream(s)) => match s.dict.get(b"SMask") {
                    Some(Object::Reference(r)) => Some(*r),
                    _ => None,
                },
                _ => None,
            })
            .collect()
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
            + self.apply_discard_objects(
                &plan.discard_objects,
                plan.smooth_tolerance,
                &mut fold,
            )?;
        let objects = self.stream_writes(&plan.writes);
        if objects.is_empty() {
            fold.finish(self, removed);
            return Ok(removed);
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

    /// The planned stream rewrites as writes against the session now. A
    /// stream an earlier step of the run rewrote (a layer flattened out of
    /// a page's content) gets its action redone on the new value, so the
    /// planned bytes never overwrite that step; one it removed is skipped.
    fn stream_writes(&mut self, writes: &[Rewrite]) -> Vec<ObjectWrite> {
        let mut objects = Vec::with_capacity(writes.len());
        for w in writes {
            let (dict, data) = if self.value(w.id) == Some(&w.before) {
                (w.dict.clone(), w.data.clone())
            } else {
                let view = self.view();
                let redone = match self.value(w.id) {
                    Some(Object::Stream(s))
                        if !self.deleted.contains(&w.id)
                            && streams::protected(&s.dict).is_none() =>
                    {
                        view.slice(s.data_span).and_then(|raw| match &w.image {
                            Some(job) => image::encode::run(job, &view, &s.dict, raw)
                                .ok()
                                .map(|w| (w.dict, w.data)),
                            None => streams::apply(w.action, &s.dict, raw).ok().flatten(),
                        })
                    }
                    _ => None,
                };
                let Some(redone) = redone else {
                    continue;
                };
                redone
            };
            let data_span = self.stage_bytes(&data);
            objects.push(ObjectWrite {
                id: w.id,
                before: self.state.get(&w.id).cloned(),
                after: Some(Object::Stream(Stream { dict, data_span })),
            });
        }
        let written: BTreeSet<ObjId> = objects.iter().map(|o| o.id).collect();
        let orphaned: BTreeSet<ObjId> = writes
            .iter()
            .filter(|w| w.partner.is_some_and(|p| !written.contains(&p)))
            .map(|w| w.id)
            .collect();
        objects.retain(|o| !orphaned.contains(&o.id));
        objects
    }

    /// The full rewrite of this session, rebuilt under `structure`, written
    /// **only if it is strictly smaller** than the file as opened (decision
    /// 191's whole-file rule) **or `plan` removes content**
    /// ([`OptimizePlan::removes_content`]): a removal the operator asked for
    /// is not dropped because it did not save bytes. `plan` is the one
    /// passed to [`Self::apply_optimize`]. The report is `None` when
    /// `structure` is all off and the file's structure was kept.
    ///
    /// # Errors
    ///
    /// As [`Self::to_full_bytes`], and [`WriteError::RewriteReload`] if the
    /// rewrite cannot be reloaded for the rebuild.
    pub fn save_optimized(
        &self,
        plan: &OptimizePlan,
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
        Ok((
            crate::optimize::keep_if_smaller(bytes, original, plan.removes_content()),
            report,
        ))
    }
}

/// Decide and perform one stream's action, recording it in `plan`.
pub(super) fn plan_stream(
    plan: &mut OptimizePlan,
    settings: &OptimizeSettings,
    (id, stream, raw): (ObjId, &Stream, &[u8]),
    row: Option<&ObjectSpace>,
    ctx: &image::pipeline::Context<'_, '_>,
) {
    let filters = streams::chain(&stream.dict);
    let facts = ObjectFacts {
        audit: row,
        stored_bytes: raw.len() as u64,
        filters: &filters,
    };
    let resolved = settings.resolve(id, &facts);
    let is_image =
        matches!(stream.dict.get(b"Subtype"), Some(Object::Name(n)) if n.as_bytes() == b"Image");
    let (action, job) = match ctx.lockstep.map_or(resolved.image, |l| Some(l.settings)) {
        Some(img) if is_image => {
            let space = row.and_then(|r| r.image.as_ref());
            match image::pipeline::decide(&img, &stream.dict, raw, space, ctx) {
                Ok(Outcome::Unchanged) => return,
                Ok(Outcome::Lossless(level)) => (StreamAction::Recompress { level }, None),
                Ok(Outcome::Rewrite(job, change)) => {
                    (StreamAction::Image(img.to_patch()), Some((job, change)))
                }
                Err(reason) => {
                    let action = StreamAction::Image(img.to_patch());
                    plan.skipped.push(Skipped { id, action, reason });
                    return;
                }
            }
        }
        _ => (resolved.base, None),
    };
    if action == StreamAction::Leave {
        return;
    }
    let skip = |reason| Skipped { id, action, reason };
    if let Some(what) = streams::protected(&stream.dict) {
        plan.skipped
            .push(skip(crate::optimize::SkipReason::Protected(what)));
        return;
    }
    let mut job = job;
    let result = match job.as_mut() {
        Some((job, change)) => {
            image::encode::run(job, ctx.view, &stream.dict, raw).map(|w| Some(change.record(w)))
        }
        None => streams::apply(action, &stream.dict, raw),
    };
    match result {
        Ok(None) => {}
        Ok(Some((dict, data))) => {
            let (job, change) = job.map_or((None, None), |(j, c)| (Some(j), Some(*c)));
            plan.changes.push(PlannedChange {
                id,
                category: row.map_or(SpaceCategory::OtherReferenced, |r| r.category),
                action,
                filters_before: filters,
                before_bytes: raw.len() as u64,
                after_bytes: data.len() as u64,
                lossy: change
                    .as_ref()
                    .is_some_and(image::pipeline::ImageChange::is_lossy),
                image: change,
            });
            plan.writes.push(Rewrite {
                id,
                before: Object::Stream(stream.clone()),
                action,
                image: job,
                partner: ctx.lockstep.map(|l| l.parent),
                dict,
                data,
            });
        }
        Err(reason) => plan.skipped.push(skip(reason)),
    }
}
