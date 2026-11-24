//! The copier's `/SeparationInfo` route (ISO 32000-2 §14.11.4, Table 364):
//! a copied plate's set names only the plates copied in the same call.

use super::{Copier, DocumentView};
use crate::object::{Dict, ObjId, Object};
use crate::pageops::PageOpError;
use crate::pageops::separation::{
    self, CopiedSeparation, SeparationImpact, SeparationPolicy, rebuilt_info, source_info,
};

impl Copier {
    /// The value `page`'s `/SeparationInfo` takes in the output, `None` to
    /// drop the entry, with this page's contribution to the impact.
    ///
    /// The page walk never follows `/SeparationInfo`: it names sibling
    /// plates, and a dictionary several plates share would be copied with
    /// its `/Pages` refused. A set shared through one indirect object stays
    /// one shared object, rebuilt once.
    pub(super) fn copy_separation(
        &mut self,
        view: &DocumentView<'_>,
        source_index: usize,
        page: &Dict,
        map: impl Fn(ObjId) -> Option<u32>,
        policy: SeparationPolicy,
    ) -> Result<(Option<Object>, SeparationImpact), PageOpError> {
        let Some(original) = page.get(b"SeparationInfo") else {
            return Ok((None, SeparationImpact::default()));
        };
        let (plan, impact) = separation::plan_copied(
            view.graph(),
            page,
            |member| map(member).map(|number| ObjId::new(number, 0)),
            policy,
            self.split_sets.entry(source_index).or_default(),
        )?;
        let members = match plan {
            CopiedSeparation::Discard => return Ok((None, impact)),
            CopiedSeparation::Members(members) => members,
            CopiedSeparation::Untouched => {
                let copied = self.copy_value(view, source_index, original, 1)?;
                return Ok((copied, impact));
            }
        };
        let Some((info, shared)) = source_info(view.graph(), page) else {
            return Ok((None, impact));
        };
        if let Some(number) = shared.and_then(|id| self.mapped(source_index, id)) {
            return Ok((Some(Object::Reference(ObjId::new(number, 0))), impact));
        }
        let number = shared.map(|id| self.reserve_for(source_index, id));
        let rebuilt = rebuilt_info(info, &members, |value| {
            self.copy_value(view, source_index, value, 2)
        })?;
        let value = match number {
            Some(number) => {
                self.store(number, Object::Dict(rebuilt));
                Object::Reference(ObjId::new(number, 0))
            }
            None => Object::Dict(rebuilt),
        };
        Ok((Some(value), impact))
    }
}
