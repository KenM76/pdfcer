//! A placed model's opening view: the view a reader opens a new `/3D`
//! annotation on, framed as its rendered poster and lit by a chosen scheme
//! ([`crate::edit::EditSession::set_3d_opening_view`]).

use super::{ThreeDLighting, ThreeDSavedView, ThreeDSpec, ThreeDViewsOutcome};

/// The name of the view [`crate::edit::EditSession::set_3d_opening_view`]
/// writes.
pub const OPENING_VIEW_NAME: &str = "Default";

/// What [`crate::edit::EditSession::set_3d_opening_view`] wrote.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ThreeDOpeningView {
    /// The scheme the view lights the model with.
    pub lighting: ThreeDLighting,
    /// The scheme was chosen by the automatic rule (CAD for a model
    /// converted from glTF), not named by the caller.
    pub automatic: bool,
    /// The view carries the poster's camera. `false` leaves the camera to
    /// the reader: a model pdfcer cannot mesh, or a build without the `3d`
    /// feature.
    pub framed: bool,
    /// The view as written.
    pub view: ThreeDSavedView,
    /// What writing it changed, with its disclosures.
    pub outcome: ThreeDViewsOutcome,
}

impl ThreeDOpeningView {
    /// The off-canvas statement a shell shows for this view (rule 4: the
    /// scheme and the framing are pdfcer's choices).
    #[must_use]
    pub fn disclosure(&self) -> String {
        let camera = if self.framed {
            "seen as the poster shows it"
        } else {
            "the camera left to the reader"
        };
        let why = if self.automatic {
            " (the converted model carries no lights of its own)"
        } else {
            ""
        };
        format!(
            "the model opens on view \"{OPENING_VIEW_NAME}\", {camera}, lit by the {} scheme{why}",
            String::from_utf8_lossy(self.lighting.as_name())
        )
    }
}

/// The scheme an opening view uses: `lighting`, or with `None` (automatic)
/// CAD for a model `converted` from glTF, which carries no lights of its
/// own, and no view otherwise.
#[must_use]
pub fn opening_lighting(
    lighting: Option<ThreeDLighting>,
    converted: bool,
) -> Option<ThreeDLighting> {
    match lighting {
        Some(scheme) => Some(scheme),
        None if converted => Some(ThreeDLighting::Cad),
        None => None,
    }
}

/// The view named [`OPENING_VIEW_NAME`] whose camera is the one the
/// rendered poster uses ([`pdfcer_3d::default_view_camera`]) fitted to the
/// annotation rectangle's aspect, unlit. With no camera (not a PRC model,
/// a model that does not mesh, or no `3d` feature) the view has no
/// `/C2W`, so the reader places the camera.
#[must_use]
pub fn opening_view(spec: &ThreeDSpec) -> ThreeDSavedView {
    let mut view = poster_camera_view(spec).unwrap_or_default();
    view.name = OPENING_VIEW_NAME.to_owned();
    view
}

#[cfg(feature = "3d")]
fn poster_camera_view(spec: &ThreeDSpec) -> Option<ThreeDSavedView> {
    if spec.format != super::ThreeDFormat::Prc {
        return None;
    }
    let model = pdfcer_3d::assemble(&spec.data).ok()?;
    let r = spec.rect;
    let aspect = (r.urx - r.llx).abs() / (r.ury - r.lly).abs();
    let camera = pdfcer_3d::default_view_camera(&model, aspect).ok()?;
    ThreeDSavedView::from_camera(OPENING_VIEW_NAME, &camera, aspect)
}

#[cfg(not(feature = "3d"))]
fn poster_camera_view(_spec: &ThreeDSpec) -> Option<ThreeDSavedView> {
    None
}
