//! Byte-identical images in an optimize plan are planned as one: each copy
//! is measured over every copy's placements, so all are written the same
//! and the structure rebuild's `merge_duplicates` can keep one. Without
//! that, copies drawn at different sizes would be resampled to different
//! sizes and could no longer merge.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};

use super::EditSession;
use crate::object::{ObjId, Object};
use crate::optimize::OptimizePlan;
use crate::space_audit::ObjectSpace;

impl EditSession {
    /// Each image among `ids` that has a byte-identical twin (same
    /// dictionary, same stored data), mapped to its other copies.
    pub(super) fn image_copies(&self, ids: &BTreeSet<ObjId>) -> BTreeMap<ObjId, Vec<ObjId>> {
        let view = self.view();
        let mut buckets: BTreeMap<u64, Vec<Vec<ObjId>>> = BTreeMap::new();
        for &id in ids {
            let Some(Object::Stream(s)) = self.value(id) else {
                continue;
            };
            if !matches!(s.dict.get(b"Subtype"), Some(Object::Name(n)) if n.as_bytes() == b"Image")
            {
                continue;
            }
            let Some(raw) = view.slice(s.data_span) else {
                continue;
            };
            let mut h = DefaultHasher::new();
            raw.hash(&mut h);
            let groups = buckets.entry(h.finish()).or_default();
            let same = |other: &ObjId| match self.value(*other) {
                Some(Object::Stream(o)) => o.dict == s.dict && view.slice(o.data_span) == Some(raw),
                _ => false,
            };
            match groups.iter_mut().find(|g| g.first().is_some_and(same)) {
                Some(g) => g.push(id),
                None => groups.push(vec![id]),
            }
        }
        let mut copies = BTreeMap::new();
        for group in buckets.into_values().flatten().filter(|g| g.len() > 1) {
            for &id in &group {
                copies.insert(id, group.iter().copied().filter(|o| *o != id).collect());
            }
        }
        copies
    }
}

/// `rows` with each copy's image placements widened to every copy's.
pub(super) fn widened(
    rows: &BTreeMap<ObjId, &ObjectSpace>,
    copies: &BTreeMap<ObjId, Vec<ObjId>>,
) -> BTreeMap<ObjId, ObjectSpace> {
    let mut out = BTreeMap::new();
    for (&id, others) in copies {
        let Some(mut row) = rows.get(&id).map(|r| (*r).clone()) else {
            continue;
        };
        if let Some(space) = row.image.as_mut() {
            for other in others {
                if let Some(theirs) = rows.get(other).and_then(|r| r.image.as_ref()) {
                    space.placements.extend(theirs.placements.iter().cloned());
                }
            }
        }
        out.insert(id, row);
    }
    out
}

/// Name each copy's twins on its planned image change.
pub(super) fn disclose(plan: &mut OptimizePlan, copies: &BTreeMap<ObjId, Vec<ObjId>>) {
    for c in &mut plan.changes {
        if let (Some(img), Some(others)) = (c.image.as_mut(), copies.get(&c.id)) {
            img.copies.clone_from(others);
        }
    }
}
