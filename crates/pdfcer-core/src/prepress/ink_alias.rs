//! Aliasing one ink to another: rename the colorant in every `Separation`
//! and `DeviceN` space that names it (ISO 32000-2 §8.6.6.4, §8.6.6.5), the
//! `DeviceN` attributes keyed by it (Table 72, Table 73) and a page's
//! `/SeparationInfo /DeviceColorant` (§14.11.4).

use std::collections::BTreeMap;

use crate::graph::ObjectGraph;
use crate::object::{Name, ObjId, Object};
use crate::view::DocumentView;

use super::inks::{InkPlan, InkUse, Planned, ink_space, inks, live_ids, text};

/// Nesting followed inside one object.
const MAX_NESTING: usize = 24;

/// Work out renaming `from` to `to` everywhere. Names are not validated
/// here; the session does that.
pub(crate) fn plan_alias(view: &DocumentView<'_>, from: &[u8], to: &[u8]) -> InkPlan {
    let listed = inks(view);
    let mut r = Renamer {
        view,
        from,
        to,
        at: None,
        work: BTreeMap::new(),
        plan: InkPlan::default(),
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
    let mut plan = r.plan;
    plan.writes = r
        .work
        .into_iter()
        .map(|(id, o)| (id, Planned::Object(o)))
        .collect();
    disclose_merge(&listed, from, to, &mut plan);
    plan
}

/// Say when the two names' definitions disagree, since each space keeps
/// its own alternate and tint transform.
fn disclose_merge(listed: &[InkUse], from: &[u8], to: &[u8], plan: &mut InkPlan) {
    let find = |n: &[u8]| listed.iter().find(|u| u.name.as_bytes() == n);
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
}

impl Renamer<'_, '_> {
    /// An object's value with this plan's earlier writes applied.
    fn current(&self, id: ObjId) -> Option<Object> {
        self.work
            .get(&id)
            .cloned()
            .or_else(|| self.view.graph().value(id).cloned())
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
        if let Some(mut target) = self.current(id)
            && f(self, &mut target)
        {
            self.work.insert(id, target);
        }
        false
    }

    /// Rename inside `obj`; true when it changed.
    fn rename(&mut self, obj: &mut Object, depth: usize) -> bool {
        if depth > MAX_NESTING {
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
        let names_from = space.names.iter().any(|n| n == self.from);
        let separation = space.separation;
        if !names_from {
            return items
                .get_mut(4)
                .is_some_and(|attrs| self.rename(attrs, depth + 1));
        }
        if !separation && space.names.iter().any(|n| n == self.to) {
            let reason = format!(
                "this DeviceN space already names \"{}\"; renaming would give it two components of one ink",
                text(self.to)
            );
            self.plan.report.refuse(self.at, self.from, reason);
            return false;
        }
        self.plan.report.spaces += 1;
        let Some(names) = items.get_mut(1) else {
            return false;
        };
        let mut changed = self.through(names, &mut |r, o| r.rename_entries(o));
        if let Some(attrs) = items.get_mut(4) {
            changed |= self.through(attrs, &mut |r, o| r.attributes(o));
            changed |= self.rename(attrs, depth + 1);
        }
        changed
    }

    /// A Separation's name, or every entry of a DeviceN names array.
    fn rename_entries(&mut self, obj: &mut Object) -> bool {
        match obj {
            Object::Name(n) if n.as_bytes() == self.from => {
                *n = Name::from(self.to);
                true
            }
            Object::Array(entries) => {
                let mut changed = false;
                for entry in entries.iter_mut() {
                    changed |= self.rename_entries(entry);
                }
                changed
            }
            _ => false,
        }
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
        let is = |o: &Object, n: &[u8]| o.as_name().is_some_and(|x| x.as_bytes() == n);
        if !order.iter().any(|o| is(o, self.from)) {
            return false;
        }
        if order.iter().any(|o| is(o, self.to)) {
            order.retain(|o| !is(o, self.from));
        } else {
            self.rename_entries(obj);
        }
        true
    }
}
