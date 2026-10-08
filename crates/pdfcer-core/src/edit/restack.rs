//! Changing objects' place in paint order (pdfcer-gui request G157).

use std::collections::{BTreeMap, HashSet};

use super::{CommandKind, EditError, EditSession, ObjectWrite, PendingGraph, PermissionBit};
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::page_tree::Page;
use crate::vector::{GsResources, RestackOutcome, StackMove, plan_restack};

/// The `/ExtGState` resource-name prefix restack resets are bound under.
const RESET_PREFIX: &str = "pdfcerRS";

impl EditSession {
    /// Move page objects **to the front, to the back, or one step forward or
    /// backward** in paint order — one command, one undo entry
    /// ([`CommandKind::RestackObjects`]). Indices are into
    /// [`Self::page_objects`]`(page_index).objects`.
    ///
    /// Each moved object's bytes are spliced to the new position wrapped in
    /// `q … Q` that rebuilds the graphics state it was painted under (CTM,
    /// colours, line and text parameters, `/ExtGState`s), and replaced at
    /// the old position by the state changes they made, so neither it nor
    /// anything else renders differently except for what now covers what.
    /// Several objects keep their order relative to each other.
    ///
    /// [`StackMove::Forward`] and [`StackMove::Backward`] step past the
    /// nearest unselected object whose bounding box overlaps the moved one —
    /// the next one that changes what is visible; with none, the object
    /// stays.
    ///
    /// An object cannot be moved out from under its own clip or marked-content
    /// section (an `/OC` layer, a tagged-structure element), nor out of state
    /// a wrapper cannot rebuild. A destination under `gs` operations the
    /// object was not painted under gets a new `/ExtGState` restoring the
    /// object's values (bound under a free `pdfcerRS<n>` name). It then goes to the nearest position that
    /// can hold it, or stays, and [`RestackOutcome::limited`] names it by
    /// index with the reason. [`RestackOutcome::indices`] gives each
    /// requested object's new index, valid for the page's next
    /// `page_objects`.
    ///
    /// Nothing moving (everything already in place, or every object limited
    /// to staying) commits nothing and adds no undo entry.
    ///
    /// # Errors
    ///
    /// - [`crate::vector::VectorEditError::ObjectOutOfRange`] for a bad index.
    /// - [`crate::vector::VectorEditError::OverlappingObjectSpans`] for a page
    ///   whose objects' bytes overlap.
    /// - [`EditError::PageOutOfRange`], [`EditError::VectorEditNoContents`],
    ///   and the encryption and certification guards.
    pub fn restack_objects(
        &mut self,
        page_index: usize,
        object_indices: &[usize],
        how: StackMove,
    ) -> Result<RestackOutcome, EditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification()?;
        let pages = self.pages()?;
        let count = pages.len();
        let page = pages.get(page_index).ok_or(EditError::PageOutOfRange {
            index: page_index,
            count,
        })?;
        let content_id = *page
            .contents
            .first()
            .ok_or(EditError::VectorEditNoContents { page_index })?;
        let (stream, model) = self.page_content_and_objects(page)?;
        let free_names = self.free_names(&page.resources, object_indices.len());
        let graph = self.graph();
        let lookup = |name: &[u8]| ext_gstate(&graph, &page.resources, name);
        let res = GsResources {
            ext_gstate: &lookup,
            free_names: &free_names,
        };
        let planned = plan_restack(&stream, &model.objects, object_indices, how, &res)?;
        if planned.outcome.moved.is_empty() {
            return Ok(planned.outcome);
        }
        let prior = self.bind_resets(page, planned.bind)?;
        let mut disclosures = Vec::new();
        let (command, _) = self.text_edit_command(
            CommandKind::RestackObjects,
            content_id,
            page,
            planned.edit.content,
            prior,
            &mut disclosures,
        )?;
        self.commit(command);
        Ok(planned.outcome)
    }

    /// `n` distinct `/ExtGState` names unused on the page.
    fn free_names(&self, resources: &Dict, n: usize) -> Vec<Vec<u8>> {
        let graph = self.graph();
        let present = resources
            .get(b"ExtGState")
            .map(|o| graph.resolve(o))
            .and_then(|o| o.as_dict().cloned())
            .unwrap_or_default();
        (1u32..)
            .map(|i| format!("{RESET_PREFIX}{i}").into_bytes())
            .filter(|c| present.get(c).is_none())
            .take(n)
            .collect()
    }

    /// Writes creating each reset `/ExtGState` and binding it on the page.
    fn bind_resets(
        &mut self,
        page: &Page,
        bind: Vec<(Vec<u8>, Dict)>,
    ) -> Result<Vec<ObjectWrite>, EditError> {
        let mut scratch: BTreeMap<ObjId, Object> = BTreeMap::new();
        let mut created = Vec::new();
        for (name, dict) in bind {
            let id = ObjId::new(self.alloc_number()?, 0);
            created.push(id);
            scratch.insert(id, Object::Dict(dict));
            let removed = HashSet::new();
            let pending = PendingGraph {
                session: self,
                scratch: &scratch,
                removed: &removed,
            };
            let (writes, _) = crate::text_edit::addtext::bind_resource(
                &pending,
                page.id,
                true,
                b"ExtGState",
                &name,
                Object::Reference(id),
            );
            scratch.extend(writes);
        }
        Ok(scratch
            .into_iter()
            .map(|(id, after)| ObjectWrite {
                id,
                before: if created.contains(&id) {
                    None
                } else {
                    self.state.get(&id).cloned()
                },
                after: Some(after),
            })
            .collect())
    }
}

/// The `/ExtGState` named `name` in a page's resources.
fn ext_gstate<G: ObjectGraph + ?Sized>(graph: &G, resources: &Dict, name: &[u8]) -> Option<Dict> {
    let table = graph
        .resolve(resources.get(b"ExtGState")?)
        .as_dict()?
        .clone();
    graph.resolve(table.get(name)?).as_dict().cloned()
}
