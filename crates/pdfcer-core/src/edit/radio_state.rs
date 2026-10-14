//! Radio on-state names for a widget joining a group (§12.7.4.2.3: a radio
//! member's export value is the non-`Off` key of its `/AP /N`).

use super::{EditError, EditSession, ObjectWrite};
use crate::forms;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};

impl EditSession {
    /// The on-state a widget being added to radio field `field_id` will
    /// carry, or `None` when the field is not a radio group.
    ///
    /// A copied member's state is a member's already, and two members
    /// sharing one select together, so a taken state is renamed to a fresh
    /// one throughout the widget's appearances and disclosed. With `/Opt`
    /// the state indexes an entry this verb does not author, so a taken
    /// state is refused with [`EditError::RadioExportValueTaken`].
    pub(super) fn claim_radio_state(
        &self,
        field_id: ObjId,
        fqn: &str,
        widget: &mut Dict,
        pending: &mut [ObjectWrite],
        disclosures: &mut Vec<String>,
    ) -> Result<Option<String>, EditError> {
        let lossy = |s: &[u8]| String::from_utf8_lossy(s).into_owned();
        let Some(taken) = self.radio_export_collision(field_id, widget, pending) else {
            return Ok(self
                .field_is_radio(field_id)
                .then(|| {
                    self.pending_on_states(widget, pending)
                        .first()
                        .map(|s| lossy(s))
                })
                .flatten());
        };
        if self.field_has_opt(field_id) {
            return Err(EditError::RadioExportValueTaken {
                fqn: fqn.to_owned(),
                state: lossy(&taken),
            });
        }
        let fresh = self.fresh_radio_state(field_id, &taken);
        rename_appearance_state(widget, pending, &taken, &fresh);
        let (was, now) = (lossy(&taken), lossy(&fresh));
        disclosures.push(format!(
            "paste: the copied button's export value \"{was}\" is already a member of this group, so the new button exports \"{now}\" and starts unselected."
        ));
        Ok(Some(now))
    }

    /// An on-state name no member of `field_id` carries: `wanted` itself if
    /// free, else `wanted` with any trailing digits replaced by the lowest
    /// free number (`A` -> `A1`, `A1` -> `A2`).
    pub(super) fn fresh_radio_state(&self, field_id: ObjId, wanted: &[u8]) -> Vec<u8> {
        let taken: Vec<Vec<u8>> = forms::parse_acroform(&self.graph())
            .and_then(|form| {
                let field = form.fields.into_iter().find(|f| f.id == field_id)?;
                Some(
                    field
                        .widgets
                        .into_iter()
                        .flat_map(|w| w.on_states)
                        .collect(),
                )
            })
            .unwrap_or_default();
        let free = |s: &[u8]| s != b"Off" && !taken.iter().any(|t| t == s);
        if free(wanted) {
            return wanted.to_vec();
        }
        let stem_len = wanted
            .iter()
            .rposition(|b| !b.is_ascii_digit())
            .map_or(0, |i| i + 1);
        let stem = wanted.get(..stem_len).unwrap_or_default();
        (1u32..)
            .map(|n| [stem, n.to_string().as_bytes()].concat())
            .find(|c| free(c))
            .unwrap_or_else(|| wanted.to_vec())
    }

    /// The non-`Off` keys of a widget's `/AP /N`, where `/AP` or `/N` may be
    /// inline, staged in `pending` (not yet visible through the graph), or
    /// committed.
    pub(super) fn pending_on_states(&self, widget: &Dict, pending: &[ObjectWrite]) -> Vec<Vec<u8>> {
        let graph = self.graph();
        let dict_of = |o: &Object| -> Option<Dict> {
            if let Some(r) = o.as_reference()
                && let Some(w) = pending.iter().find(|w| w.id == r)
            {
                return w.after.as_ref()?.as_dict().cloned();
            }
            graph.resolve(o).as_dict().cloned()
        };
        widget
            .get(b"AP")
            .and_then(dict_of)
            .and_then(|ap| ap.get(b"N").and_then(dict_of))
            .map(|n| {
                n.iter()
                    .map(|(k, _)| k.as_bytes().to_vec())
                    .filter(|k| k != b"Off")
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether `field_id` is a radio-button field.
    pub(super) fn field_is_radio(&self, field_id: ObjId) -> bool {
        forms::parse_acroform(&self.graph()).is_some_and(|form| {
            form.fields
                .iter()
                .any(|f| f.id == field_id && f.button_kind == Some(forms::ButtonKind::Radio))
        })
    }

    /// Whether the field carries `/Opt`, whose entries a radio member's
    /// on-state indexes (§12.7.4.2.4) — renaming a state there would also
    /// need an `/Opt` entry this module does not author.
    pub(super) fn field_has_opt(&self, field_id: ObjId) -> bool {
        matches!(self.value(field_id), Some(Object::Dict(d)) if d.get(b"Opt").is_some())
    }
}

/// Rename appearance state `from` to `to` in the widget's `/AP` `/N`, `/D`
/// and `/R`, and turn the widget off if it showed `from`. Each of `/AP` and
/// its sub-dictionaries may be inline or a reference to an object staged in
/// `pending`; a reference to anything else is left alone.
pub(super) fn rename_appearance_state(
    widget: &mut Dict,
    pending: &mut [ObjectWrite],
    from: &[u8],
    to: &[u8],
) {
    if matches!(widget.get(b"AS"), Some(Object::Name(n)) if n.0 == from) {
        widget.insert(Name::from(b"AS"), Object::Name(Name::from(b"Off")));
    }
    let ap_obj = widget.get(b"AP").cloned();
    let ap_ref = ap_obj.as_ref().and_then(Object::as_reference);
    let mut ap = match ap_obj {
        Some(Object::Dict(d)) => d,
        Some(Object::Reference(r)) => match staged_dict(pending, r) {
            Some(d) => d.clone(),
            None => return,
        },
        _ => return,
    };
    let mut sub_refs = Vec::new();
    for key in [&b"N"[..], b"D", b"R"] {
        match ap.get(key).cloned() {
            Some(Object::Dict(mut d)) => {
                rename_key(&mut d, from, to);
                ap.insert(Name::from(key), Object::Dict(d));
            }
            Some(Object::Reference(r)) => sub_refs.push(r),
            _ => {}
        }
    }
    match ap_ref {
        Some(r) => {
            if let Some(d) = staged_dict(pending, r) {
                *d = ap;
            }
        }
        None => widget.insert(Name::from(b"AP"), Object::Dict(ap)),
    }
    for r in sub_refs {
        if let Some(d) = staged_dict(pending, r) {
            rename_key(d, from, to);
        }
    }
}

fn staged_dict(pending: &mut [ObjectWrite], id: ObjId) -> Option<&mut Dict> {
    match pending.iter_mut().find(|w| w.id == id)?.after.as_mut()? {
        Object::Dict(d) => Some(d),
        _ => None,
    }
}

fn rename_key(d: &mut Dict, from: &[u8], to: &[u8]) {
    if let Some(v) = d.remove(from) {
        d.insert(Name::from(to), v);
    }
}
