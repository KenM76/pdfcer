//! Copy objects inside a form XObject to a clipboard payload (pdfcer-gui
//! request G145), and the content-capture body `copy_selection` shares.

use std::collections::BTreeMap;

use super::{EditError, EditSession};
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::vector::clip::{self, ClipBinding, ClipItem, ClipObject};
use crate::vector::{Bounds, ObjectClip, VectorObject};

/// The content half of a clip under construction.
pub(super) struct ClipContent {
    pub(super) objects: BTreeMap<u32, ClipObject>,
    mapping: BTreeMap<ObjId, u32>,
    next: u32,
    pub(super) items: Vec<ClipItem>,
    pub(super) bbox: Bounds,
}

impl Default for ClipContent {
    fn default() -> Self {
        Self {
            objects: BTreeMap::new(),
            mapping: BTreeMap::new(),
            next: 1,
            items: Vec::new(),
            bbox: Bounds::EMPTY,
        }
    }
}

impl EditSession {
    /// **Copy objects inside a form XObject to a clipboard payload** — the
    /// form-scoped twin of [`Self::copy_objects`], addressed by
    /// [`crate::vector::PageObjects::leaves`] index.
    ///
    /// Returns the same [`ObjectClip`] `copy_objects` does, so
    /// [`Self::paste_objects`] needs nothing new. Each item's CTM is the
    /// leaf's page-space CTM — the form's placement baked in — so the paste
    /// lands where the leaf was drawn, at the size it was drawn. Resource
    /// names resolve through the form's own `/Resources`, or the page's when
    /// the form has none (ISO 32000-1 §7.8.3), and travel with the clip.
    ///
    /// Reads only: nothing is committed and the undo stack is untouched.
    /// `&mut self` only because the leaf list is the session's cached page
    /// model.
    ///
    /// # Errors
    ///
    /// [`EditError::FormLeafOutOfRange`] (also for an empty selection),
    /// [`EditError::FormLeafSelectionSpansForms`] when the leaves are not in
    /// one invocation of one form, and [`EditError::Clip`] for a resource
    /// name the form does not define.
    pub fn copy_objects_in_form(
        &mut self,
        page_index: usize,
        leaf_indices: &[usize],
    ) -> Result<ObjectClip, EditError> {
        let siblings = self.leaf_siblings(page_index, leaf_indices)?;
        let Some(&first) = leaf_indices.first() else {
            return Err(EditError::FormLeafOutOfRange { index: 0, count: 0 });
        };
        let (leaf, form_id) = self.form_leaf_at(page_index, first)?;
        let stream = crate::content::ContentStream::from_form(&self.view(), form_id)
            .map_err(EditError::VectorEditContent)?;
        let Some(Object::Stream(form_stream)) = self.value(form_id) else {
            return Err(EditError::NotADictionary {
                id: form_id,
                key: "Subtype",
            });
        };
        let form_dict = form_stream.dict.clone();
        let model = self.form_model(page_index, &stream, &form_dict, &leaf);
        let objs = Self::resolve_objects(&model, &siblings)?;
        let resources = self.form_resources(page_index, &form_dict);
        let mut content = ClipContent::default();
        self.clip_content(&stream, &resources, &objs, &mut content)?;
        Ok(ObjectClip {
            version: ObjectClip::needed_version(&[]),
            items: content.items,
            objects: content.objects,
            bbox: content.bbox,
            annotations: Vec::new(),
            replies_unthreaded: 0,
        })
    }

    /// Capture `objs` (decomposed from `stream`) as clip items, importing
    /// every resource their bytes or prelude name from `resources`.
    pub(super) fn clip_content(
        &self,
        stream: &crate::content::ContentStream,
        resources: &Dict,
        objs: &[&VectorObject],
        out: &mut ClipContent,
    ) -> Result<(), EditError> {
        // Every resource category resolved ONCE, not per name per object.
        let resolved: BTreeMap<Vec<u8>, Dict> = {
            let graph = self.graph();
            clip::RESOURCE_CATEGORIES
                .iter()
                .filter_map(|category| {
                    resources
                        .get(category)
                        .map(|o| graph.resolve(o))
                        .and_then(Object::as_dict)
                        .map(|d| ((*category).to_vec(), d.clone()))
                })
                .collect()
        };
        for &obj in objs {
            let span = obj.bytes();
            let bytes = stream
                .buf
                .get(span.start..span.end())
                .unwrap_or_default()
                .to_vec();
            // The PRELUDE: state the object depends on but does not establish
            // in its own bytes (`Pass 120.2`). Its names bind like the item's.
            let prelude = clip::item_prelude(obj, &bytes);
            let mut sites = clip::name_sites(&bytes).map_err(EditError::Clip)?;
            sites.extend(clip::name_sites(&prelude).map_err(EditError::Clip)?);
            let mut bindings = Vec::new();
            for site in sites {
                let Some(entry) = resolved
                    .get(&site.category)
                    .and_then(|sub| sub.get(&site.name))
                else {
                    return Err(EditError::Clip(clip::ClipError::UnresolvedResource {
                        category: String::from_utf8_lossy(&site.category).into_owned(),
                        name: String::from_utf8_lossy(&site.name).into_owned(),
                    }));
                };
                let object =
                    self.clip_import(entry, &mut out.objects, &mut out.mapping, &mut out.next)?;
                bindings.push(ClipBinding {
                    category: site.category,
                    name: site.name,
                    object,
                });
            }
            bindings.sort();
            bindings.dedup();
            out.bbox = out.bbox.union(obj.page_bbox());
            out.items.push(ClipItem {
                bytes,
                ctm: clip::item_ctm(obj),
                kind: clip::item_kind(obj),
                bbox: obj.page_bbox(),
                bindings,
                prelude,
            });
        }
        Ok(())
    }
}
