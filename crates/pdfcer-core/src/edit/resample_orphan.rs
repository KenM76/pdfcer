//! Dropping the image a deskew or upscale replaced, when nothing draws it
//! any more, so a save does not carry the old scan alongside the new one.

use super::{Command, CommandKind, EditError, EditSession, ObjectWrite, Removal};
use crate::graph::ObjectGraph;
use crate::object::{ObjId, Object};
use crate::vector::{ImageSource, VectorObject};

impl EditSession {
    /// When `old` is referenced only from `page_index`'s own `/Resources
    /// /XObject` (no other page, form, annotation or shared dictionary) and
    /// the page draws it nowhere any more, unbind it there and delete it,
    /// folded into the last undo entry as `kind`. Returns whether it did.
    ///
    /// Conservative by design: a page that places any form XObject keeps
    /// the image, since a form without its own `/Resources` may draw it
    /// through the page's (§7.8.3).
    pub(super) fn drop_replaced_image(
        &mut self,
        kind: CommandKind,
        page_index: usize,
        old: ObjId,
    ) -> Result<bool, EditError> {
        let model = self.page_objects(page_index)?;
        let still_drawn = model.objects.iter().any(|o| match o {
            VectorObject::Image(img) => img.source == ImageSource::Form || img.xobject == Some(old),
            _ => false,
        });
        if still_drawn {
            return Ok(false);
        }
        let counts = self.reference_counts();
        let once = |id: &ObjId| counts.get(id).copied().unwrap_or(0) == 1;
        if !once(&old) {
            return Ok(false);
        }
        let pages = self.pages()?;
        let Some(page) = pages.get(page_index) else {
            return Ok(false);
        };
        let graph = self.graph();
        let Some(page_dict) = graph.value(page.id).and_then(Object::as_dict) else {
            return Ok(false);
        };
        // The page's own /Resources, each indirect link referenced once.
        let Some(res) = page_dict.get(b"Resources") else {
            return Ok(false);
        };
        let res_ref = res.as_reference();
        let res_dict = graph.resolve(res).as_dict();
        let xo = res_dict.and_then(|d| d.get(b"XObject"));
        let xo_ref = xo.and_then(Object::as_reference);
        let binds_old = xo
            .and_then(|o| graph.resolve(o).as_dict())
            .is_some_and(|d| d.iter().any(|(_, v)| v.as_reference() == Some(old)));
        if !binds_old || !res_ref.iter().chain(&xo_ref).all(once) {
            return Ok(false);
        }
        let page_id = page.id;
        let (writes, _) =
            crate::text_edit::addtext::patch_resource(&graph, page_id, false, b"XObject", |e| {
                let names: Vec<Vec<u8>> = e
                    .iter()
                    .filter(|(_, v)| v.as_reference() == Some(old))
                    .map(|(k, _)| k.as_bytes().to_vec())
                    .collect();
                for name in names {
                    e.remove(&name);
                }
            });
        let objects = writes
            .into_iter()
            .map(|(id, value)| ObjectWrite {
                id,
                before: self.state.get(&id).cloned(),
                after: Some(value),
            })
            .collect();
        self.commit(Command {
            kind,
            objects,
            removals: vec![Removal {
                id: old,
                was_deleted: self.deleted.contains(&old),
                is_deleted: true,
            }],
            trailer: None,
        });
        self.coalesce_last(2, kind);
        Ok(true)
    }
}
