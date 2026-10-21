//! Binding a format plan's fill-opacity `/ExtGState` resources into the same
//! command as the restyle, so one undo removes both.

use super::{EditSession, ObjectWrite};
use crate::object::{Dict, ObjId, Object};
use crate::text_edit::opacity::{CreatedAlpha, SHARED_NOTE, merge_alphas};

impl EditSession {
    /// Bind `created` into `owner_id`'s resources over the command's other
    /// writes in `prior`, against the session-current graph. Pushes the
    /// shared-resources disclosure when the patched dictionary may be shared.
    pub(super) fn bind_alpha_writes(
        &self,
        prior: &mut Vec<ObjectWrite>,
        owner_id: ObjId,
        may_inherit: bool,
        created: &[CreatedAlpha],
        disclosures: &mut Vec<String>,
    ) {
        if created.is_empty() {
            return;
        }
        let mut writes: Vec<(ObjId, Object)> = prior
            .iter()
            .filter_map(|w| w.after.clone().map(|a| (w.id, a)))
            .collect();
        let shared = merge_alphas(&self.graph(), &mut writes, owner_id, may_inherit, created);
        for (id, value) in writes {
            match prior.iter_mut().find(|w| w.id == id) {
                Some(w) => w.after = Some(value),
                None => prior.push(ObjectWrite {
                    id,
                    before: self.state.get(&id).cloned(),
                    after: Some(value),
                }),
            }
        }
        if shared {
            disclosures.push(SHARED_NOTE.to_owned());
        }
    }

    /// [`Self::bind_alpha_writes`] for a form XObject owner: its own
    /// `/Resources` (no inheritance, §8.10.1), with a patched form dictionary
    /// returned through `form_dict` rather than as a write, because the form
    /// is a stream the command rebuilds from that dictionary.
    pub(super) fn bind_form_alpha_writes(
        &self,
        extra: &mut Vec<ObjectWrite>,
        form_id: ObjId,
        form_dict: &mut Dict,
        created: &[CreatedAlpha],
        disclosures: &mut Vec<String>,
    ) {
        if created.is_empty() {
            return;
        }
        extra.push(ObjectWrite {
            id: form_id,
            before: None,
            after: Some(Object::Dict(form_dict.clone())),
        });
        self.bind_alpha_writes(extra, form_id, false, created, disclosures);
        if let Some(pos) = extra.iter().position(|w| w.id == form_id)
            && let Some(Object::Dict(d)) = extra.remove(pos).after
        {
            *form_dict = d;
        }
    }
}
