//! The session OCR-layer writer's steps: guards, per-page planning and the
//! per-page writes, including the editable output's image rewrites.

use std::collections::BTreeMap;

use super::{EditSession, ObjectWrite, OcrPageLayer, PermissionBit};
use crate::object::{ObjId, Object};
use crate::ocr::editable::{ImageTarget, OcrOutput};
use crate::ocr::layer::{
    OcrLayerError, OcrLayerOptions, OcrLayerPrep, OcrLayerReport, existing_layer_strip,
    plan_ocr_layer,
};
use crate::ocr::marker::LayerStrip;
use crate::page_tree::WrapStreams;
use crate::text_edit::edit::make_raw_stream;

impl EditSession {
    /// The refusals every OCR write makes before any work, in the order
    /// `add_text` and the one-shot make them: encryption, certification,
    /// `/Size`-hidden objects, then a page named twice.
    pub(super) fn ocr_layer_guards(&self, pages: &[OcrPageLayer<'_>]) -> Result<(), OcrLayerError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(OcrLayerError::Encrypted);
        }
        let census = crate::signature::census(&*self.base);
        if census.forbids_structural_change() {
            return Err(OcrLayerError::CertificationForbidsChange {
                permission: census.certification_permission.unwrap_or(2),
            });
        }
        let suppressed = self.base.suppressed_object_count();
        if suppressed > 0 {
            return Err(OcrLayerError::HiddenObjects { count: suppressed });
        }
        let mut seen = std::collections::BTreeSet::new();
        match pages.iter().find(|l| !seen.insert(l.page_index)) {
            Some(l) => Err(OcrLayerError::DuplicatePage {
                page_index: l.page_index,
            }),
            None => Ok(()),
        }
    }

    /// Plan every page, allocating nothing, so a refusal on any page leaves
    /// no burnt numbers or orphaned staged bytes.
    #[allow(clippy::type_complexity)] // the plans and strips, consumed together by one caller
    pub(super) fn plan_ocr_pages(
        &self,
        pages: &[OcrPageLayer<'_>],
        opts: &OcrLayerOptions,
    ) -> Result<(Vec<OcrLayerPrep>, Vec<(usize, LayerStrip)>), OcrLayerError> {
        let page_list = self.pages().map_err(OcrLayerError::PageTree)?;
        let uses = match opts.output {
            OcrOutput::EditableText(_) => {
                let base_pages = crate::page_tree::pages(&self.base)?;
                crate::redact_image::image_use_census(&self.base, &base_pages)
            }
            _ => BTreeMap::new(),
        };
        let view = self.view();
        let mut preps = Vec::with_capacity(pages.len());
        let mut strips = Vec::with_capacity(pages.len());
        for layer in pages {
            let page = page_list
                .get(layer.page_index)
                .ok_or(OcrLayerError::PageIndex(layer.page_index))?;
            let strip = existing_layer_strip(&view, page, layer.page_index, opts)?;
            preps.push(plan_ocr_layer(
                page,
                layer.recognised,
                opts,
                &strip,
                &view,
                &uses,
            )?);
            strips.push((layer.page_index, strip));
        }
        Ok((preps, strips))
    }

    /// Allocate, stage and record one page's writes; the report gains the
    /// object numbers.
    pub(super) fn write_ocr_prep(
        &mut self,
        prep: &OcrLayerPrep,
        wrap: &mut Option<WrapStreams>,
        objects: &mut Vec<ObjectWrite>,
    ) -> Result<OcrLayerReport, OcrLayerError> {
        let exhausted = |_| OcrLayerError::ObjectNumbersExhausted;
        let content_num = self.alloc_number().map_err(exhausted)?;
        let font_num = self.alloc_number().map_err(exhausted)?;
        let content_id = ObjId::new(content_num, 0);
        let font_id = ObjId::new(font_num, 0);
        let mut report = prep.report.clone();
        let clones = self.write_ocr_images(prep, &mut report, objects)?;

        let contents = self
            .finish_overlay(&prep.append, content_id, wrap, objects)
            .map_err(exhausted)?;
        let new_page = prep.build_page_dict(contents, font_id, &clones);
        let span = self.stage_bytes(&prep.content_data);
        let page_before = self.value(prep.page_id).cloned();
        objects.push(ObjectWrite {
            id: content_id,
            before: None,
            after: Some(make_raw_stream(span, prep.content_data.len())),
        });
        objects.push(ObjectWrite {
            id: font_id,
            before: None,
            after: Some(prep.font_dict.clone()),
        });
        objects.push(ObjectWrite {
            id: prep.page_id,
            before: page_before,
            after: Some(Object::Dict(new_page)),
        });
        report.content_object = content_num;
        report.font_object = font_num;
        Ok(report)
    }

    /// Stage each rewritten image: in place, or as a new object when the
    /// source is shared. Returns the clones' ids in [`OcrLayerPrep::images`]
    /// order.
    fn write_ocr_images(
        &mut self,
        prep: &OcrLayerPrep,
        report: &mut OcrLayerReport,
        objects: &mut Vec<ObjectWrite>,
    ) -> Result<Vec<ObjId>, OcrLayerError> {
        let mut clones = Vec::new();
        for (k, image) in prep.images.iter().enumerate() {
            let (id, before) = match image.target {
                ImageTarget::InPlace(id) => (id, self.value(id).cloned()),
                ImageTarget::Clone => {
                    let num = self
                        .alloc_number()
                        .map_err(|_| OcrLayerError::ObjectNumbersExhausted)?;
                    let id = ObjId::new(num, 0);
                    clones.push(id);
                    (id, None)
                }
            };
            if let Some(r) = report.images_rewritten.get_mut(k) {
                r.image = id.num;
            }
            let span = self.stage_bytes(&image.data);
            objects.push(ObjectWrite {
                id,
                before,
                after: Some(image.staged(span.start)),
            });
        }
        Ok(clones)
    }
}
