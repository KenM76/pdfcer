//! Where `3d-render` looks from: the operator's options, else the file's
//! opening view (ISO 32000-1 §13.6.4–13.6.5, Tables 304–305).

use super::RenderThreeDArgs;
use crate::LensArg;
use pdfcer_3d::{Camera, FisheyeMapping, Projection, RenderError, TriangleMesh};
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
            "the file's opening view \"{}\" leaves the camera to the model, which is not \
             read yet; iso, z up",
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

/// The camera `3d-render` draws through: fitted to the model along `aim`
/// (in perspective for a fisheye), moved to `--eye`, placed by the opening
/// view's framing (an orthographic placement, so not under a fisheye), then
/// given `--lens` and `--fov`.
pub(super) fn place_camera(
    a: &RenderThreeDArgs<'_>,
    (meshes, framed): (&[TriangleMesh], &[Option<[u8; 4]>]),
    target: [f64; 3],
    aim: &Aim,
) -> Result<Camera, RenderError> {
    let aspect = f64::from(a.width) / f64::from(a.height.max(1));
    let mapping = match a.lens {
        LensArg::Flat => None,
        LensArg::Equidistant => Some(FisheyeMapping::Equidistant),
        LensArg::Stereographic => Some(FisheyeMapping::Stereographic),
        LensArg::Equirectangular => Some(FisheyeMapping::Equirectangular),
    };
    let perspective = !aim.ortho || mapping.is_some();
    let mut camera =
        Camera::fit_coloured(meshes, framed, aim.direction, aim.up, perspective, aspect)?;
    if let Some(eye) = a.eye {
        camera.eye = eye;
        camera.target = target;
    }
    if let (Some(saved), None) = (&aim.framing, mapping) {
        saved.frame(&mut camera);
    }
    if let Some(mapping) = mapping {
        let whole = if mapping == FisheyeMapping::Equirectangular {
            360.0
        } else {
            180.0
        };
        let fov = a.fov.unwrap_or(whole);
        if mapping == FisheyeMapping::Equirectangular {
            camera = levelled(camera, a, aim);
        }
        camera.projection = Projection::Fisheye { fov, mapping };
    } else if let Projection::Perspective { fov_y } = &mut camera.projection {
        *fov_y = a.fov.unwrap_or(30.0);
    }
    Ok(camera)
}

/// `camera` turned level for a 360° picture, whose top edge a photo viewer
/// takes as straight up: onto `--up`; else, from the file's opening view, the
/// model axis nearest that view's up, since the file may be y-up; else the
/// default `--up`. Its own up is the heading when it looks straight along
/// that axis. A camera that cannot be levelled is left for the renderer to
/// refuse.
fn levelled(camera: Camera, a: &RenderThreeDArgs<'_>, aim: &Aim) -> Camera {
    let up = match a.up {
        None if aim.framing.is_some() => nearest_axis(camera.up),
        up => up.unwrap_or_default().vector(),
    };
    camera.levelled_panorama(up, camera.up).unwrap_or(camera)
}

/// The signed model axis nearest `up`, z winning ties, then y.
fn nearest_axis([x, y, z]: [f64; 3]) -> [f64; 3] {
    let (ax, ay, az) = (x.abs(), y.abs(), z.abs());
    if az >= ax && az >= ay {
        [0.0, 0.0, 1f64.copysign(z)]
    } else if ay >= ax {
        [0.0, 1f64.copysign(y), 0.0]
    } else {
        [1f64.copysign(x), 0.0, 0.0]
    }
}

/// The `note:` disclosing that a 360° picture's camera was levelled, and
/// which way up was chosen when the operator did not say.
pub(super) fn print_level_note(a: &RenderThreeDArgs<'_>, camera: &Camera) {
    if a.lens != LensArg::Equirectangular {
        return;
    }
    let [x, y, z] = camera.up;
    let axis = if z != 0.0 {
        if z > 0.0 { "+z" } else { "-z" }
    } else if y != 0.0 {
        if y > 0.0 { "+y" } else { "-y" }
    } else if x > 0.0 {
        "+x"
    } else {
        "-x"
    };
    let how = if a.up.is_some() {
        "as --up asked"
    } else {
        "--up chooses another"
    };
    println!(
        "note: photo sphere: the camera was levelled with {axis} up ({how}), keeping its eye \
         and heading, so the horizon is flat"
    );
}

/// The summary line's `projection=` value.
pub(super) fn projection_name(projection: Projection) -> &'static str {
    match projection {
        Projection::Perspective { .. } => "perspective",
        Projection::Orthographic { .. } => "orthographic",
        Projection::Fisheye {
            mapping: FisheyeMapping::Stereographic,
            ..
        } => "fisheye-stereographic",
        Projection::Fisheye {
            mapping: FisheyeMapping::Equirectangular,
            ..
        } => "equirectangular",
        Projection::Fisheye { .. } => "fisheye-equidistant",
    }
}
