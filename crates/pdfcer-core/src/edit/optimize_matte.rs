//! `/Matte` soft masks in an optimize plan: a mask whose parent is
//! resampled is resampled to the same size with the same kernel, or the
//! parent's rewrite is withdrawn (ISO 32000-2 §11.6.5.3: with `/Matte`
//! the two sizes shall be equal).
//!
//! Resampling both with one linear kernel keeps the pair consistent: the
//! preblended colour `c' = m + α(c − m)` is linear in `α` and `αc`, so the
//! resampled pair un-premultiplies to the opacity-weighted average colour.

use std::collections::{BTreeMap, BTreeSet};

use super::EditSession;
use super::optimize::plan_stream;
use crate::object::{Dict, ObjId, Object};
use crate::optimize::image::pipeline::{self, Lockstep};
use crate::optimize::select::ObjectFacts;
use crate::optimize::streams;
use crate::optimize::{
    OptimizePlan, OptimizeSettings, SettingSource, SkipReason, Skipped, StreamAction,
};
use crate::space_audit::ObjectSpace;
use crate::view::DocumentView;

impl EditSession {
    /// Each `/Matte` soft mask among `ids` named by exactly one parent
    /// image of the same size, mapped to that parent. A mask shared by two
    /// parents could need two sizes, so it is planned on its own and keeps
    /// blocking its parents.
    pub(super) fn matte_pairs(&self, ids: &BTreeSet<ObjId>) -> BTreeMap<ObjId, ObjId> {
        let mut parents: BTreeMap<ObjId, Vec<ObjId>> = BTreeMap::new();
        for &id in ids {
            let Some(Object::Stream(s)) = self.value(id) else {
                continue;
            };
            let Some(Object::Reference(mask)) = s.dict.get(b"SMask") else {
                continue;
            };
            parents.entry(*mask).or_default().push(id);
        }
        parents
            .into_iter()
            .filter_map(|(mask, list)| match list.as_slice() {
                [parent] if ids.contains(&mask) && self.is_matte_mask_of(mask, *parent) => {
                    Some((mask, *parent))
                }
                _ => None,
            })
            .collect()
    }

    fn is_matte_mask_of(&self, mask: ObjId, parent: ObjId) -> bool {
        let dict = |id| match self.value(id) {
            Some(Object::Stream(s)) => Some(&s.dict),
            _ => None,
        };
        let (Some(m), Some(p)) = (dict(mask), dict(parent)) else {
            return false;
        };
        let size = |d: &Dict| (d.get(b"Width").cloned(), d.get(b"Height").cloned());
        m.contains_key(b"Matte") && size(m) == size(p) && size(m).0.is_some()
    }

    /// Plan `mask` after its parent: in lockstep when the parent was
    /// resampled, else on its own settings. When the lockstep rewrite
    /// fails, the parent's rewrite is withdrawn and both are skipped.
    pub(super) fn plan_matte_mask(
        &self,
        plan: &mut OptimizePlan,
        settings: &OptimizeSettings,
        (mask, parent): (ObjId, ObjId),
        rows: &BTreeMap<ObjId, &ObjectSpace>,
        view: &DocumentView<'_>,
    ) {
        let Some(Object::Stream(stream)) = self.value(mask) else {
            return;
        };
        let Some(raw) = view.slice(stream.data_span) else {
            return;
        };
        let row = rows.get(&mask).copied();
        let lockstep = resized_parent(plan, parent).map(|(size, parent_settings)| {
            let facts = ObjectFacts {
                audit: row,
                stored_bytes: raw.len() as u64,
                filters: &streams::chain(&stream.dict),
            };
            let mut own = settings
                .resolve(mask, &facts)
                .image
                .unwrap_or(parent_settings);
            own.method = parent_settings.method;
            own.convert = crate::optimize::image::sourced(
                crate::optimize::ImageConvert::Keep,
                SettingSource::BuiltIn,
            );
            own.min_saving_pct = crate::optimize::image::sourced(0, SettingSource::BuiltIn);
            Lockstep {
                parent,
                size,
                settings: own,
            }
        });
        let ctx = pipeline::Context {
            view,
            blocker: pipeline::mask_blocker(&stream.dict, None, false),
            convert_blocker: pipeline::convert_blocker(&stream.dict, None, true),
            lockstep,
        };
        plan_stream(plan, settings, (mask, stream, raw), row, &ctx);
        if lockstep.is_none() {
            return;
        }
        if plan.writes.iter().any(|w| w.id == mask) {
            if let Some(w) = plan.writes.iter_mut().find(|w| w.id == parent) {
                w.partner = Some(mask);
            }
        } else {
            withdraw(plan, parent, mask);
        }
    }
}

/// The parent's new size and settings, when the plan resamples it.
fn resized_parent(
    plan: &OptimizePlan,
    parent: ObjId,
) -> Option<((u32, u32), crate::optimize::ResolvedImage)> {
    plan.changes
        .iter()
        .find(|c| c.id == parent)
        .and_then(|c| c.image.as_ref())
        .filter(|i| i.size_after != i.size_before)
        .map(|i| (i.size_after, i.settings))
}

/// Drop the parent's planned rewrite because its mask could not follow.
fn withdraw(plan: &mut OptimizePlan, parent: ObjId, mask: ObjId) {
    let action = plan
        .changes
        .iter()
        .find(|c| c.id == parent)
        .map_or(StreamAction::Leave, |c| c.action);
    plan.changes.retain(|c| c.id != parent);
    plan.writes.retain(|w| w.id != parent);
    plan.skipped.push(Skipped {
        id: parent,
        action,
        reason: SkipReason::ImageUnsupported(format!(
            "its /Matte soft mask {} {} R could not be resampled with it",
            mask.num, mask.generation
        )),
    });
}
