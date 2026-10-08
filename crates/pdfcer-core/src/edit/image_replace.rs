//! Replace a placed image's pixels in place (pdfcer-gui request G156).

use super::{
    CommandKind, EditError, EditSession, ImageAuthorDisclosures, ImageFit, NewImage, ObjectWrite,
    PermissionBit,
};
use crate::image_import::ImportedImage;
use crate::object::{ObjId, Object};
use crate::page_tree::Rect;
use crate::vector::{ImageSource, Matrix, VectorObject};
use crate::writer::content::emit_number;

/// The `/XObject` resource-name prefix a replacement is bound under.
const XOBJECT_PREFIX: &str = "pdfcerIm";

/// What [`EditSession::replace_image`] did.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ImageReplaceOutcome {
    /// The new image XObject.
    pub image_id: ObjId,
    /// Its `/SMask` image, when the source had an alpha channel.
    pub soft_mask_id: Option<ObjId>,
    /// The `/XObject` resource name the page's content now invokes.
    pub resource_name: Vec<u8>,
    /// The image XObject no longer drawn at this place, or `None` when the
    /// replaced image was inline (§8.9.7). It stays in the file: other
    /// placements may draw it, and an incremental save keeps its bytes.
    pub replaced: Option<ObjId>,
    /// The same facts [`EditSession::add_image`] reports. The rectangle
    /// they are measured against is the replaced image's own extent, so
    /// `letterboxed` means the new image does not fill the old one's area.
    pub disclosures: ImageAuthorDisclosures,
}

impl EditSession {
    /// **Replace the pixels of a placed image**, keeping where it is drawn,
    /// its size and its stacking order — one undo entry
    /// ([`CommandKind::ReplaceImage`]).
    ///
    /// `object_index` indexes [`Self::page_objects`]. The image's `Do` (or
    /// its whole inline `BI … EI`, §8.9.7) is replaced by a `Do` of a new
    /// image XObject at the same point in the content stream, so the CTM in
    /// force — position, size, rotation, clip, opacity and layer — is the
    /// one the old image had. An image occupies the unit square under that
    /// CTM (ISO 32000-1 §8.9.4), so the new image is fitted to the same
    /// unit square:
    ///
    /// - [`ImageFit::Stretch`] fills it exactly, distorting a different
    ///   aspect ratio;
    /// - [`ImageFit::Contain`] keeps the new image's displayed aspect ratio,
    ///   measured against the old image's extent on the page, and centres
    ///   it. The image is then drawn under an extra `q … cm … Q`.
    ///
    /// The source's EXIF orientation is applied as [`Self::add_image`]
    /// applies it. Only this placement changes: other placements of the old
    /// XObject keep drawing it, and its bytes remain in an incremental save
    /// (redaction is the verb that removes content from a file).
    ///
    /// # Errors
    ///
    /// - [`EditError::ReplaceImageOnOther`] when the object is a path, text
    ///   or a form XObject.
    /// - [`EditError::ImageRectDegenerate`] when the old image's extent has
    ///   no area.
    /// - [`crate::vector::VectorEditError::ObjectOutOfRange`],
    ///   [`EditError::PageOutOfRange`], [`EditError::VectorEditNoContents`],
    ///   [`EditError::DocumentEncrypted`],
    ///   [`EditError::CertificationForbidsChange`],
    ///   [`EditError::ObjectCreationWouldExposeHiddenObjects`].
    pub fn replace_image(
        &mut self,
        page_index: usize,
        object_index: usize,
        image: &ImportedImage,
        fit: ImageFit,
    ) -> Result<ImageReplaceOutcome, EditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification()?;
        let suppressed = self.base.suppressed_object_count();
        if suppressed > 0 {
            return Err(EditError::ObjectCreationWouldExposeHiddenObjects { count: suppressed });
        }
        let model = self.page_objects(page_index)?;
        let count = model.objects.len();
        let target = match model.objects.get(object_index) {
            None => {
                return Err(crate::vector::VectorEditError::ObjectOutOfRange {
                    index: object_index,
                    count,
                }
                .into());
            }
            Some(VectorObject::Image(img)) if img.source != ImageSource::Form => *img,
            Some(other) => {
                return Err(EditError::ReplaceImageOnOther {
                    index: object_index,
                    kind: kind_of(other),
                });
            }
        };
        let extent = extent(target.ctm);
        if !(extent.urx > 0.0 && extent.ury > 0.0) {
            return Err(EditError::ImageRectDegenerate {
                w: extent.urx,
                h: extent.ury,
            });
        }
        let spec = NewImage {
            fit,
            ..NewImage::new(page_index, extent, image)
        };
        self.commit_replace_image(&spec, target.bytes, target.xobject)
    }

    fn commit_replace_image(
        &mut self,
        spec: &NewImage<'_>,
        span: crate::span::ByteSpan,
        replaced: Option<ObjId>,
    ) -> Result<ImageReplaceOutcome, EditError> {
        let pages = self.pages()?;
        let page = pages
            .get(spec.page_index)
            .ok_or(EditError::PageOutOfRange {
                index: spec.page_index,
                count: pages.len(),
            })?;
        let content_id = *page
            .contents
            .first()
            .ok_or(EditError::VectorEditNoContents {
                page_index: spec.page_index,
            })?;
        let (stream, _) = self.page_content_and_objects(page)?;

        let mut prior = Vec::new();
        let (image_id, soft_mask_id) = self.stage_image_xobject(spec.image, &mut prior)?;
        let name = self.free_name_in(&page.resources, b"XObject", XOBJECT_PREFIX);
        let (writes, shared) = crate::text_edit::addtext::bind_resource(
            &self.graph(),
            page.id,
            true,
            b"XObject",
            &name,
            Object::Reference(image_id),
        );
        prior.extend(writes.into_iter().map(|(id, value)| ObjectWrite {
            id,
            before: self.state.get(&id).cloned(),
            after: Some(value),
        }));
        let mut disclosures = Vec::new();
        if shared {
            disclosures.push(SHARED_RESOURCES_NOTE.to_owned());
        }

        let placed = spec.placed_rect();
        let replacement = invocation(spec, &placed, &name);
        let mut edits = vec![(span.start, span.end(), replacement)];
        let content = crate::text_edit::edit::splice(&stream.buf, &mut edits);
        let (command, _) = self.text_edit_command(
            CommandKind::ReplaceImage,
            content_id,
            page,
            content,
            prior,
            &mut disclosures,
        )?;
        self.commit(command);
        Ok(ImageReplaceOutcome {
            image_id,
            soft_mask_id,
            resource_name: name,
            replaced,
            disclosures: self.image_disclosures(spec, &placed, soft_mask_id.is_some()),
        })
    }
}

/// The old image's extent on the page as a rectangle at the origin: the
/// lengths of the CTM's two unit-square edges.
fn extent(ctm: Matrix) -> Rect {
    Rect {
        llx: 0.0,
        lly: 0.0,
        urx: ctm.a.hypot(ctm.b),
        ury: ctm.c.hypot(ctm.d),
    }
}

/// `/Name Do`, preceded by `q … cm` and followed by `Q` when the new image
/// does not exactly fill the unit square or is EXIF-oriented. `placed` is
/// inside `spec.rect`, which is [`extent`]; dividing by the extent maps it
/// back into the unit square.
fn invocation(spec: &NewImage<'_>, placed: &Rect, name: &[u8]) -> Vec<u8> {
    let (w, h) = (spec.rect.urx, spec.rect.ury);
    let (sx, sy) = ((placed.urx - placed.llx) / w, (placed.ury - placed.lly) / h);
    let (tx, ty) = (placed.llx / w, placed.lly / h);
    let [oa, ob, oc, od, oe, of] = spec.image.orientation.unit_square_matrix();
    let m = [
        oa * sx,
        ob * sy,
        oc * sx,
        od * sy,
        oe * sx + tx,
        of * sy + ty,
    ];
    let mut out = Vec::new();
    let identity = m == [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    if !identity {
        out.extend_from_slice(b"q ");
        for v in m {
            emit_number(&mut out, v);
            out.push(b' ');
        }
        out.extend_from_slice(b"cm ");
    }
    out.push(b'/');
    // `free_name_in` names are regular-character ASCII (§7.3.5).
    out.extend_from_slice(name);
    out.extend_from_slice(b" Do");
    if !identity {
        out.extend_from_slice(b" Q");
    }
    out
}

const fn kind_of(o: &VectorObject) -> &'static str {
    match o {
        VectorObject::Path(_) => "path",
        VectorObject::Text(_) => "text",
        VectorObject::Image(_) => "form",
    }
}

const SHARED_RESOURCES_NOTE: &str = "replace image: the new /XObject entry was added to a \
     /Resources dictionary shared with other pages or forms; it is unreferenced there and \
     changes nothing about how they render";
