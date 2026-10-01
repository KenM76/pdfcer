//! Where `3d-render` looks from: the operator's options, else the file's
//! opening view (ISO 32000-1 §13.6.4–13.6.5, Tables 304–305).

use super::RenderThreeDArgs;
use pdfcer_core::threed::{OrthoBinding, ThreeDSavedView};

/// The camera `3d-render` uses, and a sentence saying why.
pub(super) struct Aim {
    pub(super) direction: [f64; 3],
    pub(super) up: [f64; 3],
    pub(super) ortho: bool,
    /// The saved view's centre and scale, when it fixes them; `None` fits
    /// the model to the image.
    pub(super) framing: Option<Framing>,
    pub(super) source: String,
}

/// A saved orthographic view's placement: the image centre lies on the
/// camera's z axis through `position`, and the image shows `height` model
/// units vertically.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Framing {
    pub(super) position: [f64; 3],
    pub(super) height: f64,
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
    let Some(m) = view.camera_to_world else {
        return named(format!(
            "the file's opening view \"{}\" leaves the camera to the model, which is not \
             read yet; iso, z up",
            view.name
        ));
    };
    let aspect = f64::from(a.width) / f64::from(a.height.max(1));
    let framing = (view.orthographic && a.target.is_none())
        .then(|| {
            ortho_height(
                view.ortho_scale,
                view.ortho_binding,
                view.view_box,
                &m,
                aspect,
            )
        })
        .flatten()
        .map(|height| Framing {
            position: [m[9], m[10], m[11]],
            height,
        });
    let scale = match framing {
        Some(f) => format!(
            "centred on its camera axis, {:.6} model units high (its orthographic scale and \
             binding, read as the bound side spanning 1/scale camera units -- an \
             interpretation: the standard gives the scale no unit)",
            f.height
        ),
        None => "zoomed to fit the model".to_owned(),
    };
    Aim {
        direction: [m[6], m[7], m[8]],
        up: [m[3], m[4], m[5]],
        ortho: a.ortho || view.orthographic,
        framing,
        source: format!(
            "the file's opening view \"{}\" (its direction and projection; {scale})",
            view.name
        ),
    }
}

/// Visible model-space height of a saved orthographic view rendered at
/// `aspect` (width / height), or `None` when the view does not fix one.
///
/// Table 305 scales the near plane onto the annotation's target coordinate
/// system (default user space units) by `OS` and, in addition, by the `OB`
/// binding. pdfcer reads a binding as fitting one camera unit to the bound
/// side, so that side spans `1/OS` camera units whatever the annotation's
/// size; `/Absolute` maps a camera unit to `OS` units, so the view box's
/// height spans `height/OS`. The image's sides stand in for the
/// annotation's. Camera units become model units through the length of
/// the camera matrix's y column.
fn ortho_height(
    scale: f64,
    binding: OrthoBinding,
    view_box: Option<[f64; 2]>,
    m: &[f64; 12],
    aspect: f64,
) -> Option<f64> {
    let unit = (m[3] * m[3] + m[4] * m[4] + m[5] * m[5]).sqrt();
    let bound = unit / scale;
    let landscape = aspect >= 1.0;
    let height = match binding {
        OrthoBinding::Height => bound,
        OrthoBinding::Width => bound / aspect,
        OrthoBinding::Min if landscape => bound,
        OrthoBinding::Min => bound / aspect,
        OrthoBinding::Max if landscape => bound / aspect,
        OrthoBinding::Max => bound,
        _ => view_box?[1] * bound,
    };
    (height.is_finite() && height > 0.0).then_some(height)
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNIT: [f64; 12] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];

    #[test]
    fn each_binding_fits_its_side_to_one_over_the_scale() {
        let h = |b, aspect| ortho_height(0.5, b, Some([300.0, 100.0]), &UNIT, aspect);
        assert_eq!(h(OrthoBinding::Height, 2.0), Some(2.0));
        assert_eq!(h(OrthoBinding::Width, 2.0), Some(1.0));
        assert_eq!(
            h(OrthoBinding::Min, 2.0),
            Some(2.0),
            "landscape: height is lesser"
        );
        assert_eq!(
            h(OrthoBinding::Min, 0.5),
            Some(4.0),
            "portrait: width is lesser"
        );
        assert_eq!(h(OrthoBinding::Max, 2.0), Some(1.0));
        assert_eq!(h(OrthoBinding::Max, 0.5), Some(2.0));
        assert_eq!(
            h(OrthoBinding::Absolute, 2.0),
            Some(200.0),
            "a 100-unit-high box at scale 0.5"
        );
        assert_eq!(
            ortho_height(0.5, OrthoBinding::Absolute, None, &UNIT, 1.0),
            None,
            "absolute needs a view box"
        );
    }

    #[test]
    fn a_scaled_camera_matrix_scales_the_span() {
        let mut m = UNIT;
        m[3..6].copy_from_slice(&[0.0, 3.0, 4.0]);
        assert_eq!(
            ortho_height(1.0, OrthoBinding::Height, None, &m, 1.0),
            Some(5.0)
        );
    }
}
