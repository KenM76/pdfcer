//! Line width, dash and constant alpha for chosen page paths (pdfcer-gui
//! request G143).

use super::{
    CommandKind, EditError, EditSession, ObjectWrite, PaintOutcome, PaintRefusal,
    PaintRefusalReason, PermissionBit,
};
use crate::object::{ObjId, Object};
use crate::vector::{StrokeStyle, VectorEditError, VectorObject};

/// The `/ExtGState` resource-name prefix pdfcer binds alphas under.
const GS_PREFIX: &str = "pdfcerGS";

impl EditSession {
    /// Set the **line width, dash pattern and/or constant alpha** of page
    /// paths — one command, one undo entry ([`CommandKind::SetObjectStrokeStyle`]).
    ///
    /// Each path's own bytes are wrapped in `q <w> <d> </GS gs> … Q`, exactly
    /// as [`Self::set_object_paint`] wraps colour, so operators shared with
    /// other objects are never rewritten. The alphas go in one new
    /// `/ExtGState` (`/CA`, `/ca`; ISO 32000-2 §8.4.5, §11.6.4.4) bound in the
    /// page's `/Resources` under a free `pdfcerGS<n>` name; when that
    /// dictionary is shared with other pages a disclosure says so (the entry
    /// is inert there).
    ///
    /// `None` fields are left alone; the values set replace (do not combine
    /// with) the ones in force. The current values are on
    /// [`crate::vector::PathObject`]: `line_width`, `dash`, `stroke_alpha`,
    /// `fill_alpha`.
    ///
    /// All-or-nothing over the paths: every index is resolved first, and a
    /// text or image object is reported in [`PaintOutcome::refused`] as
    /// [`PaintRefusalReason::NotAPath`] rather than failing the call. An empty
    /// `style` changes nothing and returns the refusals only.
    ///
    /// # Errors
    ///
    /// - [`VectorEditError::InvalidStrokeStyle`] for a negative or non-finite
    ///   width, an alpha outside `0..=1`, or a dash array that is negative or
    ///   all zero (§8.4.3.6).
    /// - [`VectorEditError::ObjectOutOfRange`] for a bad index.
    /// - [`EditError::PageOutOfRange`], [`EditError::VectorEditNoContents`],
    ///   and the encryption and certification guards.
    pub fn set_object_stroke_style(
        &mut self,
        page_index: usize,
        object_indices: &[usize],
        style: &StrokeStyle,
    ) -> Result<PaintOutcome, EditError> {
        style.validate()?;
        let model = self.page_objects(page_index)?;
        let count = model.objects.len();
        let mut changed = Vec::new();
        let mut refused = Vec::new();
        for &i in object_indices {
            match model.objects.get(i) {
                Some(VectorObject::Path(_)) => changed.push(i),
                Some(_) => refused.push(PaintRefusal {
                    object: i,
                    reason: PaintRefusalReason::NotAPath,
                    space: None,
                }),
                None => return Err(VectorEditError::ObjectOutOfRange { index: i, count }.into()),
            }
        }
        if changed.is_empty() || style.is_empty() {
            changed.clear();
            return Ok(PaintOutcome { changed, refused });
        }
        self.commit_stroke_style(page_index, &changed, style)?;
        Ok(PaintOutcome { changed, refused })
    }

    fn commit_stroke_style(
        &mut self,
        page_index: usize,
        targets: &[usize],
        style: &StrokeStyle,
    ) -> Result<(), EditError> {
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

        let mut disclosures = Vec::new();
        let mut prior = Vec::new();
        let gs_name = if style.sets_alpha() {
            let name = self.free_name_in(&page.resources, b"ExtGState", GS_PREFIX);
            let gs_id = ObjId::new(self.alloc_number()?, 0);
            let (writes, shared) = crate::text_edit::addtext::bind_resource(
                &self.graph(),
                page.id,
                true,
                b"ExtGState",
                &name,
                Object::Reference(gs_id),
            );
            prior.push(ObjectWrite {
                id: gs_id,
                before: None,
                after: Some(Object::Dict(style.ext_gstate())),
            });
            prior.extend(writes.into_iter().map(|(id, value)| ObjectWrite {
                id,
                before: self.state.get(&id).cloned(),
                after: Some(value),
            }));
            if shared {
                disclosures.push(SHARED_GS_NOTE.to_owned());
            }
            Some(name)
        } else {
            None
        };

        let (stream, model) = self.page_content_and_objects(page)?;
        let objs = targets
            .iter()
            .map(|&i| {
                model
                    .objects
                    .get(i)
                    .ok_or(VectorEditError::ObjectOutOfRange {
                        index: i,
                        count: model.objects.len(),
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let prefix = style.prefix(gs_name.as_deref());
        let planned = crate::vector::edit::plan_wrap(&stream, &objs, &prefix)?;
        let (command, _) = self.text_edit_command(
            CommandKind::SetObjectStrokeStyle,
            content_id,
            page,
            planned.content,
            prior,
            &mut disclosures,
        )?;
        self.commit(command);
        Ok(())
    }
}

const SHARED_GS_NOTE: &str = "stroke style: the /ExtGState entry was added to a /Resources \
     dictionary shared with other pages or forms; it is unreferenced there and changes nothing \
     about how they render";
