//! The document-object half of the optimize discard (Pass 616.3), through
//! the same staged key deletion as [`EditSession::remove_metadata`].

use std::collections::{BTreeSet, HashSet};

use super::EditSession;
use super::metadata_remove::Staged;
use super::optimize_cleanup::Fold;
use crate::doc_metadata::scan::value_bytes;
use crate::object::{ObjId, Object};
use crate::optimize::actions::{Rewrite, apply_rewrite};
use crate::optimize::objects::{self, DiscardObject, ObjectDiscard, Strip};
use crate::optimize::{DiscardOptions, OptimizeError};

/// What each class's discard costs the reader, in plan-note order.
const NOTES: &[(DiscardObject, &str)] = &[
    (
        DiscardObject::Tags,
        "the structure tree goes: the file is no longer tagged, so screen readers and reflow lose it, and a PDF/UA or PDF/A level A claim no longer holds",
    ),
    (
        DiscardObject::PrintSettings,
        "embedded print settings go: page scaling, duplex, tray, page range and copies fall back to the printer dialog's defaults",
    ),
    (
        DiscardObject::FormActions,
        "form submit, import and reset actions go: buttons that sent, loaded or cleared the form do nothing when clicked, and actions chained after them still run",
    ),
    (
        DiscardObject::ExternalReferences,
        "actions that open another file go: those links stay on the page but do nothing when clicked, and web links stay",
    ),
    (
        DiscardObject::AlternateImages,
        "alternate images go: printing uses the on-screen image, and images under optional content are kept",
    ),
    (
        DiscardObject::ObjectData,
        "object data goes: the properties a CAD or diagram program attached to drawn objects (part numbers, shape data) are no longer listed; the drawing is unchanged",
    ),
];

impl EditSession {
    /// Every carrier of an object class `options` discards, each measured.
    /// A certification forbidding deletions is noted by `plan_discard`.
    pub(super) fn plan_discard_objects(
        &self,
        options: DiscardOptions,
        notes: &mut Vec<String>,
    ) -> Vec<ObjectDiscard> {
        if options.objects().next().is_none()
            || crate::signature::census(self).forbids_structural_change()
        {
            return Vec::new();
        }
        let view = self.view();
        let roots: Vec<ObjId> = [b"Root".as_slice(), b"Info"]
            .iter()
            .filter_map(|k| self.trailer.get(k).and_then(Object::as_reference))
            .collect();
        let live: BTreeSet<ObjId> = super::reachable(&view, &roots, &HashSet::new())
            .into_iter()
            .collect();
        let mut items = objects::carriers(&view, &live, |o| options.contains_object(o));
        for item in &mut items {
            let mut st = Staged::default();
            if self.stage_item(item, &mut st).is_ok() {
                let freed: u64 = self
                    .orphaned_by(&st)
                    .iter()
                    .filter_map(|id| self.value(*id))
                    .map(value_bytes)
                    .sum();
                let added: u64 = item
                    .rewrites
                    .iter()
                    .filter_map(|r| r.value.as_ref())
                    .map(value_bytes)
                    .sum();
                item.bytes = (st.detached.iter().map(value_bytes).sum::<u64>() + freed)
                    .saturating_sub(added);
            }
        }
        for (object, note) in NOTES {
            if items.iter().any(|i| i.object == *object) {
                notes.push((*note).to_owned());
            }
        }
        items
    }

    /// Delete `items` inside the run `fold` is collecting, with every object
    /// only they reached. On a failure the whole run is undone.
    pub(super) fn apply_discard_objects(
        &mut self,
        items: &[ObjectDiscard],
        fold: &mut Fold,
    ) -> Result<usize, OptimizeError> {
        if items.is_empty() {
            return Ok(0);
        }
        let mut st = Staged::default();
        for item in items {
            if let Err(reason) = self.stage_item(item, &mut st) {
                fold.roll_back(self);
                return Err(OptimizeError::ObjectDiscardFailed {
                    object: item.object,
                    owner: item.owner,
                    reason,
                });
            }
        }
        self.commit_metadata_edits(st);
        fold.absorb(self);
        Ok(items.len())
    }

    fn stage_item(&self, item: &ObjectDiscard, st: &mut Staged) -> Result<(), String> {
        self.stage_strips(&item.strips, st)?;
        self.stage_rewrites(&item.rewrites, st)
    }

    /// Stage each rewrite whose entry still holds what the plan saw.
    fn stage_rewrites(&self, rewrites: &[Rewrite], st: &mut Staged) -> Result<(), String> {
        for r in rewrites {
            let mut obj = st
                .scratch
                .get(&r.owner)
                .or_else(|| self.value(r.owner))
                .cloned()
                .ok_or("the object no longer exists")?;
            let Object::Dict(dict) = &mut obj else {
                return Err("not a dictionary".to_owned());
            };
            if let Some(old) = apply_rewrite(dict, r) {
                st.detached.push(old);
                st.scratch.insert(r.owner, obj);
            }
        }
        Ok(())
    }

    /// Stage each strip whose entry is still there; an earlier step of the
    /// run (the bookmark clean-up) may already have removed one.
    fn stage_strips(&self, strips: &[Strip], st: &mut Staged) -> Result<(), String> {
        for s in strips {
            if self.has_path(st, s) {
                self.strip_metadata_key(st, s.owner, &s.path)?;
            }
        }
        Ok(())
    }

    fn has_path(&self, st: &Staged, s: &Strip) -> bool {
        let dict = match st.scratch.get(&s.owner).or_else(|| self.value(s.owner)) {
            Some(Object::Dict(d)) => d,
            Some(Object::Stream(x)) => &x.dict,
            _ => return false,
        };
        let [first, rest @ ..] = s.path.as_slice() else {
            return false;
        };
        let Some(value) = dict.get(first) else {
            return false;
        };
        let Some(last) = rest.first() else {
            return true;
        };
        let inner = match value {
            Object::Reference(r) => st.scratch.get(r).or_else(|| self.value(*r)),
            other => Some(other),
        };
        inner
            .and_then(Object::as_dict)
            .is_some_and(|d| d.get(last).is_some())
    }
}
