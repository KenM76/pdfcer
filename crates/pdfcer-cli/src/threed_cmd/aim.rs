//! Where `3d-render` looks from: the operator's options, else the file's
//! opening view (ISO 32000-1 §13.6.4–13.6.5, Tables 304–305).

use super::RenderThreeDArgs;
use pdfcer_core::threed::{SavedViewAim, ThreeDSavedView, ViewFit};

/// The camera `3d-render` uses, and a sentence saying why.
pub(super) struct Aim {
    pub(super) direction: [f64; 3],
    pub(super) up: [f64; 3],
    pub(super) ortho: bool,
    /// The file's opening view, when it aims the camera; its
    /// [`SavedViewAim::frame`] places a fitted camera.
    pub(super) framing: Option<SavedViewAim>,
    pub(super) source: String,
}

/// `--eye`, else `--view`/`--up`, else the file's opening view when it
/// carries a camera matrix (its z column is the look direction and its y
/// column the image's up), else the default named view.
pub(super) fn aim_camera(
    a: &RenderThreeDArgs<'_>,
    target: [f64; 3],
    saved: Option<&ThreeDSavedView>,
) -> Aim {
    let named = |source: String| {
        let (direction, up) = a
            .view
            .unwrap_or_default()
            .direction(a.up.unwrap_or_default());
        Aim {
            direction,
            up,
            ortho: a.ortho,
            framing: None,
            source,
        }
    };
    if let Some([ex, ey, ez]) = a.eye {
        let [tx, ty, tz] = target;
        return Aim {
            direction: [tx - ex, ty - ey, tz - ez],
            up: a.up.unwrap_or_default().vector(),
            ortho: a.ortho,
            framing: None,
            source: "placed by --eye".to_owned(),
        };
    }
    if a.view.is_some() || a.up.is_some() {
        return named("the named view asked for".to_owned());
    }
    let Some(view) = saved else {
        return named("the file names no opening view; iso, z up".to_owned());
    };
    let aspect = f64::from(a.width) / f64::from(a.height.max(1));
    let Some(mut saved) = view.aim(aspect) else {
        return named(format!(
            "the file's opening view \"{}\" leaves the camera to the model, which is not              read yet; iso, z up",
            view.name
        ));
    };
    if a.target.is_some() {
        saved.fit = ViewFit::FittedToModel;
    }
    Aim {
        direction: saved.direction,
        up: saved.up,
        ortho: a.ortho || saved.orthographic,
        source: saved.source(),
        framing: Some(saved),
    }
}
