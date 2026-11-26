//! A flattened ce dimension leaves the ce dimension model: its annotation is
//! page content now, so a record still naming it would offer edits that
//! rewrite an orphaned object. Ce dimensions anchored to it are detached.

use super::{AnnotFlattenOutcome, EditError, EditSession, ObjectWrite};
use crate::dimension::DimensionId;
use crate::object::ObjId;

impl EditSession {
    /// Drop every ce dimension whose annotation is in `removing` from the
    /// model, detach anchors to it, and queue the sidecar write; counts and
    /// disclosures go to `outcome`. No ce dimension flattened writes nothing.
    pub(super) fn release_flattened_dimensions(
        &self,
        removing: &[ObjId],
        objects: &mut Vec<ObjectWrite>,
        outcome: &mut AnnotFlattenOutcome,
    ) -> Result<(), EditError> {
        let mut model = self.read_dimension_model();
        let gone: Vec<DimensionId> = model
            .dimensions()
            .iter()
            .filter(|r| r.annot.is_some_and(|a| removing.contains(&a)))
            .map(|r| r.id)
            .collect();
        if gone.is_empty() {
            return Ok(());
        }
        self.check_dimension_sidecar()?;
        let mut detached: Vec<DimensionId> = Vec::new();
        for &id in &gone {
            for r in self.detach_for_delete(&mut model, id) {
                if !gone.contains(&r.id) && !detached.contains(&r.id) {
                    detached.push(r.id);
                }
            }
        }
        for &id in &gone {
            model.remove_dimension(id);
        }
        objects.push(self.catalog_dimension_write(&model)?);
        outcome.ce_dimensions_flattened = gone.len();
        outcome.ce_dimension_anchors_detached = detached.len();
        outcome.disclosures.push(format!(
            "{} ce dimension(s) are page content now and can no longer be edited as ce dimensions",
            gone.len()
        ));
        if !detached.is_empty() {
            let ids: Vec<String> = detached.iter().map(|d| d.0.to_string()).collect();
            outcome.disclosures.push(format!(
                "detached ce dimension(s) {} anchored to a flattened one; they keep their position",
                ids.join(", ")
            ));
        }
        Ok(())
    }
}
