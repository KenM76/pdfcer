//! Aliasing one ink to another: rename the colorant in every `Separation`
//! and `DeviceN` space that names it (ISO 32000-2 §8.6.6.4, §8.6.6.5), the
//! `DeviceN` attributes keyed by it (Table 72, Table 73) and a page's
//! `/SeparationInfo /DeviceColorant` (§14.11.4).
//!
//! A reference that names the ink is replaced by the new name, so no
//! indirect name object is ever rewritten for one space and seen changed by
//! another. A refused `DeviceN` protects every object it reaches; the alias
//! is refused when it would write one, and when the walk was cut short.

use std::collections::{BTreeMap, HashSet};

use crate::graph::ObjectGraph;
use crate::object::{Name, ObjId, Object};
use crate::preflight::walk::MAX_COLORANTS;
use crate::view::DocumentView;

use super::inks::{InkPlan, InkUse, Planned, ink_space, inks, live_ids, text};

/// Nesting followed inside one object.
const MAX_NESTING: usize = 24;

/// Values one alias looks at in total, across every object.
const MAX_VISITS: usize = 1 << 22;

/// Work out renaming `from` to `to` everywhere. Names are not validated
/// here; the session does that.
///
/// `Err` (one sentence) when the walk was cut short by nesting, the visit
/// allowance or a `DeviceN` space with more than [`MAX_COLORANTS`] names,
/// or when a write would change an object a refused `DeviceN` reaches.
/// Both are checked after the whole walk, so the answer does not depend on
/// the order objects are visited in.
pub(crate) fn plan_alias(
    view: &DocumentView<'_>,
    from: &[u8],
    to: &[u8],
) -> Result<InkPlan, String> {
    let listed = inks(view).inks;
    let mut r = Renamer {
        view,
        from,
        to,
        at: None,
        work: BTreeMap::new(),
        plan: InkPlan::default(),
        visits: MAX_VISITS,
        complete: true,
        protected: HashSet::new(),
    };
    for id in live_ids(view) {
        r.at = Some(id);
        let Some(mut obj) = r.current(id) else {
            continue;
        };
        if r.rename(&mut obj, 0) {
            r.work.insert(id, obj);
        }
    }
    if !r.complete {
        return Err(format!(
            "the search for \"{}\" stopped short (an object nested deeper than {MAX_NESTING} levels, more than {MAX_VISITS} values, or a DeviceN space naming more than {MAX_COLORANTS} colorants), so a space naming it could be missed; nothing was changed",
            text(from)
        ));
    }
    if r.work.keys().any(|id| r.protected.contains(id)) {
        return Err("a DeviceN space naming both inks shares objects with a space being renamed, so renaming would change it; nothing was changed".to_owned());
    }
    let mut plan = r.plan;
    plan.writes = r
        .work
        .into_iter()
        .map(|(id, o)| (id, Planned::Object(o)))
        .collect();
    disclose_merge(&listed, from, to, &mut plan);
    Ok(plan)
}

/// Say when the two names' definitions disagree, since each space keeps
/// its own alternate and tint transform.
fn disclose_merge(listed: &[InkUse], from: &[u8], to: &[u8], plan: &mut InkPlan) {
    let find = |n: &[u8]| {
        let spelled = text(n);
        listed.iter().find(|u| u.name == spelled)
    };
    if plan.writes.is_empty() {
        plan.report
            .disclose(format!("No colour space names \"{}\".", text(from)));
        return;
    }
    if let (Some(a), Some(b)) = (find(from), find(to))
        && a.alternates != b.alternates
    {
        plan.report.disclose(format!(
            "\"{}\" and \"{}\" are defined with different alternate spaces ({} and {}); each space keeps its own alternate and tint transform.",
            text(from),
            text(to),
            a.alternates.join(", "),
            b.alternates.join(", "),
        ));
    }
}

struct Renamer<'v, 'd> {
    view: &'v DocumentView<'d>,
    from: &'v [u8],
    to: &'v [u8],
    /// The top-level object being visited.
    at: Option<ObjId>,
    /// Objects rewritten so far.
    work: BTreeMap<ObjId, Object>,
    plan: InkPlan,
    /// Values still allowed to be looked at.
    visits: usize,
    /// False once a limit left something unvisited.
    complete: bool,
    /// Objects a refused `DeviceN` reaches through its names or attributes.
    protected: HashSet<ObjId>,
}

fn is_container(obj: &Object) -> bool {
    matches!(obj, Object::Array(_) | Object::Dict(_) | Object::Stream(_))
}

impl Renamer<'_, '_> {
    /// An object's value with this plan's earlier writes applied.
    fn current(&self, id: ObjId) -> Option<Object> {
        self.work
            .get(&id)
            .cloned()
            .or_else(|| self.view.graph().value(id).cloned())
    }

    /// Take one visit from the allowance; false (and incomplete) when none
    /// is left.
    fn spend(&mut self) -> bool {
        let Some(left) = self.visits.checked_sub(1) else {
            self.complete = false;
            return false;
        };
        self.visits = left;
        true
    }

    /// Whether a container at `depth` may be entered; clears `complete`
    /// when it may not.
    fn enter(&mut self, obj: &Object, depth: usize) -> bool {
        if !self.spend() || !is_container(obj) {
            return false;
        }
        if depth > MAX_NESTING {
            self.complete = false;
            return false;
        }
        true
    }

    /// Apply `f` to `obj`, or, when it is a reference, to the object it
    /// names (recorded as a write). True when `obj` itself changed.
    fn through(
        &mut self,
        obj: &mut Object,
        f: &mut dyn FnMut(&mut Self, &mut Object) -> bool,
    ) -> bool {
        let Object::Reference(id) = obj else {
            return f(self, obj);
        };
        let id = *id;
        if self.spend()
            && let Some(mut target) = self.current(id)
            && f(self, &mut target)
        {
            self.work.insert(id, target);
        }
        false
    }

    /// Rename inside `obj`; true when it changed.
    fn rename(&mut self, obj: &mut Object, depth: usize) -> bool {
        if !self.enter(obj, depth) {
            return false;
        }
        match obj {
            Object::Array(items) => self.array(items, depth),
            Object::Dict(d) => self.dict(&mut d.0, depth),
            Object::Stream(s) => self.dict(&mut s.dict.0, depth),
            _ => false,
        }
    }

    fn dict(&mut self, entries: &mut [(Name, Object)], depth: usize) -> bool {
        let mut changed = false;
        for (key, value) in entries.iter_mut() {
            if key.as_bytes() == b"DeviceColorant" && self.is_from(value) {
                *value = Object::Name(Name::from(self.to));
                self.plan.report.spaces = self.plan.report.spaces.saturating_add(1);
                changed = true;
            } else {
                changed |= self.rename(value, depth + 1);
            }
        }
        changed
    }

    fn is_from(&self, obj: &Object) -> bool {
        self.view
            .resolve(obj)
            .as_name()
            .is_some_and(|n| n.as_bytes() == self.from)
    }

    fn array(&mut self, items: &mut [Object], depth: usize) -> bool {
        let Some(space) = ink_space(self.view, items) else {
            let mut changed = false;
            for item in items.iter_mut() {
                changed |= self.rename(item, depth + 1);
            }
            return changed;
        };
        if space.capped {
            self.complete = false;
        }
        let names_from = space.names.iter().any(|n| n == self.from);
        let both = !space.separation && space.names.iter().any(|n| n == self.to);
        if !names_from {
            return items
                .get_mut(4)
                .is_some_and(|attrs| self.rename(attrs, depth + 1));
        }
        if both {
            self.refuse_both(items, depth);
            return false;
        }
        self.plan.report.spaces = self.plan.report.spaces.saturating_add(1);
        let mut changed = match items.get_mut(1) {
            Some(slot) if self.view.resolve(slot).as_name().is_some() => self.entry(slot),
            Some(names) => self.through(names, &mut |r, o| r.entries(o)),
            None => false,
        };
        if let Some(attrs) = items.get_mut(4) {
            changed |= self.through(attrs, &mut |r, o| r.attributes(o));
            changed |= self.rename(attrs, depth + 1);
        }
        changed
    }

    /// Refuse a `DeviceN` naming both inks, and protect what it reaches
    /// through its names (index 1) and attributes (index 4).
    fn refuse_both(&mut self, items: &[Object], depth: usize) {
        let reason = format!(
            "this DeviceN space already names \"{}\"; renaming would give it two components of one ink",
            text(self.to)
        );
        self.plan.report.refuse(self.at, self.from, reason);
        for slot in [items.get(1), items.get(4)].into_iter().flatten() {
            self.protect(slot, depth + 1);
        }
    }

    /// Record every object `o` reaches as one no write may change.
    fn protect(&mut self, o: &Object, depth: usize) {
        if !self.spend() {
            return;
        }
        if depth > MAX_NESTING {
            self.complete = false;
            return;
        }
        let deeper = depth + 1;
        match o {
            Object::Reference(id) => {
                if self.protected.insert(*id)
                    && let Some(target) = self.view.graph().value(*id).cloned()
                {
                    self.protect(&target, deeper);
                }
            }
            Object::Array(items) => items.iter().for_each(|i| self.protect(i, deeper)),
            Object::Dict(d) => d.iter().for_each(|(_, v)| self.protect(v, deeper)),
            Object::Stream(s) => s.dict.iter().for_each(|(_, v)| self.protect(v, deeper)),
            _ => {}
        }
    }

    /// Each entry of a names array, or a single name.
    fn entries(&mut self, obj: &mut Object) -> bool {
        match obj {
            Object::Array(entries) => entries
                .iter_mut()
                .fold(false, |changed, e| self.entry(e) | changed),
            other => self.entry(other),
        }
    }

    /// One ink name, direct or through a reference; either is replaced by
    /// the new name itself.
    fn entry(&self, e: &mut Object) -> bool {
        if !self.is_from(e) {
            return false;
        }
        *e = Object::Name(Name::from(self.to));
        true
    }

    /// A DeviceN attributes dictionary: `/Colorants` keys and
    /// `/MixingHints` `/Solidities`, `/DotGain` and `/PrintingOrder`.
    fn attributes(&mut self, obj: &mut Object) -> bool {
        let Object::Dict(attrs) = obj else {
            return false;
        };
        let mut changed = false;
        for (key, value) in &mut attrs.0 {
            changed |= match key.as_bytes() {
                b"Colorants" => self.through(value, &mut |r, o| r.rekey(o)),
                b"MixingHints" => self.through(value, &mut |r, o| r.mixing_hints(o)),
                _ => false,
            };
        }
        changed
    }

    fn mixing_hints(&mut self, obj: &mut Object) -> bool {
        let Object::Dict(hints) = obj else {
            return false;
        };
        let mut changed = false;
        for (key, value) in &mut hints.0 {
            changed |= match key.as_bytes() {
                b"Solidities" | b"DotGain" => self.through(value, &mut |r, o| r.rekey(o)),
                b"PrintingOrder" => self.through(value, &mut |r, o| r.reorder(o)),
                _ => false,
            };
        }
        changed
    }

    /// Rename the key `from`; drop it when `to` is already a key.
    fn rekey(&mut self, obj: &mut Object) -> bool {
        let Object::Dict(d) = obj else {
            return false;
        };
        if !d.contains_key(self.from) {
            return false;
        }
        if d.contains_key(self.to) {
            d.remove(self.from);
        } else if let Some(slot) = d.0.iter_mut().find(|(k, _)| k.as_bytes() == self.from) {
            slot.0 = Name::from(self.to);
        }
        true
    }

    /// Rename `from` in a `/PrintingOrder` array, dropping it when `to` is
    /// already listed.
    fn reorder(&mut self, obj: &mut Object) -> bool {
        let Object::Array(order) = obj else {
            return false;
        };
        if !order.iter().any(|o| self.is_from(o)) {
            return false;
        }
        let is_to = |o: &Object| {
            self.view
                .resolve(o)
                .as_name()
                .is_some_and(|n| n.as_bytes() == self.to)
        };
        if order.iter().any(is_to) {
            order.retain(|o| !self.is_from(o));
        } else {
            self.entries(obj);
        }
        true
    }
}
