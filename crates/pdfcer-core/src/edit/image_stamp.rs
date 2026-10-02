//! Image stamps ([`EditSession::add_image_stamp`]).

use super::{AnnotKind, Command, CommandKind, EditError, EditSession, MarkupOptions, ObjectWrite};
use crate::annot_author;
use crate::image_import::ImportedImage;
use crate::object::{ObjId, Object};
use crate::page_tree::Rect;

impl EditSession {
    /// Place `image` on page `page_index` as a **rubber stamp annotation**
    /// (ISO 32000-1 §12.5.6.12, Table 181) whose `/AP /N` draws it. One undo
    /// entry.
    ///
    /// - The image is written by the same image-XObject writer as
    ///   [`Self::add_image`]: an image with alpha gets an `/SMask` (§11.6.5.3),
    ///   so the stamp is transparent where the source was.
    /// - It is fitted inside `rect` preserving its aspect ratio and EXIF
    ///   orientation, centred in the leftover dimension; `/Rect` is `rect`.
    /// - No `/Name` is written: the face is the image, not one of Table 181's
    ///   standard names. `/F` is Print.
    /// - `options` supplies `/CA`, the note (`/Contents`, `/T`, `/M`) and the
    ///   layer, as on every markup verb.
    /// - Resizing the stamp later re-fits the image in the new box.
    ///
    /// Returns the annotation's id.
    ///
    /// # Errors
    ///
    /// [`EditError::MarkupOpacityOutOfRange`] and the other
    /// [`MarkupOptions`] refusals; then [`EditError::DocumentEncrypted`], the
    /// annotation certification gate, [`EditError::PageOutOfRange`] and
    /// [`EditError::ObjectCreationWouldExposeHiddenObjects`].
    ///
    /// # Examples
    ///
    /// ```
    /// # use pdfcer_core::{edit::{EditSession, MarkupOptions}, image_import, page_tree::Rect};
    /// # fn demo(session: &mut EditSession, png: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    /// let image = image_import::import(png)?;
    /// let rect = Rect { llx: 72.0, lly: 700.0, urx: 120.0, ury: 724.0 };
    /// let stamp = session.add_image_stamp(0, rect, &image, &MarkupOptions::default())?;
    /// # let _ = stamp;
    /// # Ok(())
    /// # }
    /// ```
    pub fn add_image_stamp(
        &mut self,
        page_index: usize,
        rect: Rect,
        image: &ImportedImage,
        options: &MarkupOptions,
    ) -> Result<ObjId, EditError> {
        options.validate()?;
        self.on_layer_if(page_index, options.layer, |s| {
            let (slots, page_id) = s.annotation_author_target(page_index)?;
            let mut objects = Vec::new();
            let mut authored = annot_author::image_stamp_frame(rect);
            s.attach_fitted_image(&mut authored, image, &mut objects)?;
            let (annot_id, annot, ap_write) = s.stage_authored_icon(authored, page_id, options)?;
            objects.push(ap_write);
            objects.push(ObjectWrite {
                id: annot_id,
                before: None,
                after: Some(Object::Dict(annot)),
            });
            objects.append(&mut s.annots_append(page_id, &[annot_id], &slots)?);
            s.commit(Command {
                kind: CommandKind::AddAnnotation {
                    kind: AnnotKind::Stamp,
                },
                objects,
                removals: Vec::new(),
                trailer: None,
            });
            Ok(annot_id)
        })
    }
}
