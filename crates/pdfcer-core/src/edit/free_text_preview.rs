//! Preview of a text box being retyped (pdfcer-gui request G282).

use super::{EditError, EditSession};
use crate::annot_author::{self, FreeTextPreview, TextAnnotSpec};
use crate::object::{ObjId, Object};

impl EditSession {
    /// What [`Self::set_markup_note`] would paint on the `/FreeText`
    /// `annot_id` if its note became `new_text`: the box's current font,
    /// size, colour, alignment, border, fill and wrapping, with new words.
    ///
    /// Reads only; nothing is staged.
    ///
    /// `Ok(None)` when the commit would leave the page as it is: the
    /// annotation is not a `/FreeText`, its style cannot be read, or its
    /// appearance is not one pdfcer drew (`set_markup_note` then reports
    /// `appearance_rebaked: false`).
    ///
    /// A box carrying rich text (`/RC`, `/DS`) previews as plain text in its
    /// `/DA` style, which is what the commit writes: `set_markup_note`
    /// removes both keys and names them in `rich_text_dropped`.
    ///
    /// # Errors
    ///
    /// `annot_id` is not an annotation on any page; a page tree that cannot
    /// be read.
    pub fn free_text_note_preview(
        &self,
        annot_id: ObjId,
        new_text: &str,
    ) -> Result<Option<FreeTextPreview>, EditError> {
        self.locate_annotation(annot_id)?;
        let Some(Object::Dict(dict)) = self.value(annot_id) else {
            return Ok(None);
        };
        let Ok(mut spec) = annot_author::text_spec_from_dict(&self.graph(), dict) else {
            return Ok(None);
        };
        let Some(wraps) = self.measure_free_text_multiline(dict, &spec) else {
            return Ok(None);
        };
        if let TextAnnotSpec::FreeText {
            text, multiline, ..
        } = &mut spec
        {
            new_text.clone_into(text);
            *multiline = wraps;
        }
        Ok(annot_author::free_text_preview(&spec).ok().flatten())
    }
}
