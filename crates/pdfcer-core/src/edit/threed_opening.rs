//! Writing a placed model's lit opening view
//! ([`EditSession::set_3d_opening_view`]).

use super::{EditError, EditSession};
use crate::object::ObjId;
use crate::threed::{
    ThreeDLighting, ThreeDOpeningView, ThreeDSpec, opening_lighting, opening_view,
};

impl EditSession {
    /// Write the view a reader opens the `/3D` annotation `annot_id` on
    /// page `page_index` with, for the model `spec` placed there: one view
    /// named [`crate::threed::OPENING_VIEW_NAME`], made the default, through
    /// [`Self::set_3d_views`] (one undo entry; it replaces any views the
    /// stream held).
    ///
    /// - **Camera:** the one the rendered poster uses, fitted to
    ///   `spec.rect`'s aspect ([`crate::threed::opening_view`]), so the
    ///   model opens where its poster shows it. With no camera (not PRC,
    ///   does not mesh, no `3d` feature) the reader places it.
    /// - **Lighting:** `lighting`, or with `None` (automatic) CAD when the
    ///   model was `converted` from glTF and **no view at all** otherwise
    ///   ([`crate::threed::opening_lighting`]); then nothing is written and
    ///   `Ok(None)` is returned.
    /// - The returned [`ThreeDOpeningView::disclosure`] states the scheme
    ///   and the framing; a shell shows it with
    ///   [`crate::threed::ThreeDViewsOutcome::disclosures`].
    ///
    /// `spec` must be the spec the annotation was placed with
    /// ([`Self::add_3d_annotation`]); the model is not read back from the
    /// file.
    ///
    /// # Errors
    ///
    /// As [`Self::set_3d_views`].
    ///
    /// # Examples
    ///
    /// ```
    /// # use pdfcer_core::{edit::EditSession, object::ObjId, threed::ThreeDSpec};
    /// # use pdfcer_core::threed::ThreeDLighting;
    /// # fn demo(session: &mut EditSession, annot: ObjId, spec: &ThreeDSpec) -> Result<(), Box<dyn std::error::Error>> {
    /// if let Some(opened) = session.set_3d_opening_view(0, annot, spec, None, true)? {
    ///     assert_eq!(opened.lighting, ThreeDLighting::Cad);
    ///     println!("note: {}", opened.disclosure());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_3d_opening_view(
        &mut self,
        page_index: usize,
        annot_id: ObjId,
        spec: &ThreeDSpec,
        lighting: Option<ThreeDLighting>,
        converted: bool,
    ) -> Result<Option<ThreeDOpeningView>, EditError> {
        let Some(scheme) = opening_lighting(lighting, converted) else {
            return Ok(None);
        };
        let view = opening_view(spec).with_lighting(scheme);
        let framed = view.camera_to_world.is_some();
        let outcome =
            self.set_3d_views(page_index, annot_id, std::slice::from_ref(&view), Some(0))?;
        Ok(Some(ThreeDOpeningView {
            lighting: scheme,
            automatic: lighting.is_none(),
            framed,
            view,
            outcome,
        }))
    }
}
