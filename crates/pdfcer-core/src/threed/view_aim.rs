//! A saved 3D view as a camera aim (ISO 32000-1 §13.6.4 Table 304 `/C2W`,
//! §13.6.5 Table 305 `/P`), shared by `3d-render` and any viewer.

use super::view::{OrthoBinding, ThreeDSavedView};

/// Where a saved view looks from, as a renderer needs it.
///
/// From [`ThreeDSavedView::aim`]. `direction` is the camera matrix's z
/// column (the camera looks along its +z, §13.6.5) and `up` its y column.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SavedViewAim {
    /// The view's `/XN` name, for [`SavedViewAim::source`].
    pub name: String,
    /// The look direction in world space; not normalised.
    pub direction: [f64; 3],
    /// The direction that appears upward in the image; not normalised.
    pub up: [f64; 3],
    /// Whether the view asks for an orthographic projection.
    pub orthographic: bool,
    /// Whether the view fixes its own centre and scale.
    pub fit: ViewFit,
}

/// How a saved view frames the model.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum ViewFit {
    /// The view fixes no scale (perspective, or no usable `/OS`): zoom to
    /// fit the model.
    FittedToModel,
    /// An orthographic view fixing its framing: the image centre lies on
    /// the camera's z axis through `position`, and the image shows
    /// `height` model units vertically.
    Framed {
        /// The camera position, the matrix's `tx ty tz`.
        position: [f64; 3],
        /// Visible model-space height of the image.
        height: f64,
    },
}

impl ThreeDSavedView {
    /// The aim this view gives an image of `aspect` (width / height), or
    /// `None` when it carries no camera matrix (the artwork's own camera
    /// applies).
    ///
    /// An orthographic view's height is pdfcer's reading of `/OS` and
    /// `/OB` (Table 305 gives the scale no unit): a binding fits the bound
    /// side to `1/OS` camera units; `/Absolute` maps a camera unit to `OS`
    /// default user space units, so the view box's height spans
    /// `height/OS`. The image's sides stand in for the annotation's, and
    /// camera units become model units through the length of the matrix's
    /// y column. [`SavedViewAim::source`] discloses the reading.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::threed::{ThreeDSavedView, ViewFit};
    ///
    /// let mut view = ThreeDSavedView::default();
    /// assert!(view.aim(1.0).is_none(), "no camera matrix");
    /// view.camera_to_world = Some([1., 0., 0., 0., 0., 1., 0., -1., 0., 0., 5., 0.]);
    /// let aim = view.aim(1.0).expect("a matrix");
    /// assert_eq!(aim.direction, [0., -1., 0.]);
    /// assert_eq!(aim.up, [0., 0., 1.]);
    /// assert_eq!(aim.fit, ViewFit::FittedToModel, "perspective");
    /// ```
    #[must_use]
    pub fn aim(&self, aspect: f64) -> Option<SavedViewAim> {
        let m = self.camera_to_world?;
        let [_, _, _, ux, uy, uz, zx, zy, zz, tx, ty, tz] = m;
        let height = if self.orthographic {
            ortho_height(
                self.ortho_scale,
                self.ortho_binding,
                self.view_box,
                &m,
                aspect,
            )
        } else {
            None
        };
        Some(SavedViewAim {
            name: self.name.clone(),
            direction: [zx, zy, zz],
            up: [ux, uy, uz],
            orthographic: self.orthographic,
            fit: height.map_or(ViewFit::FittedToModel, |height| ViewFit::Framed {
                position: [tx, ty, tz],
                height,
            }),
        })
    }
}

impl SavedViewAim {
    /// The sentence `3d-render` prints for where it looks from, naming the
    /// view and disclosing how its scale was read.
    #[must_use]
    pub fn source(&self) -> String {
        let scale = match self.fit {
            ViewFit::Framed { height, .. } => format!(
                "centred on its camera axis, {height:.6} model units high (its orthographic \
                 scale and binding, read as the bound side spanning 1/scale camera units -- \
                 an interpretation: the standard gives the scale no unit)"
            ),
            ViewFit::FittedToModel => "zoomed to fit the model".to_owned(),
        };
        format!(
            "the file's opening view \"{}\" (its direction and projection; {scale})",
            self.name
        )
    }
}

#[cfg(feature = "3d")]
impl SavedViewAim {
    /// A camera on this aim that fits `bounds` in an image of `aspect`,
    /// then, for a [`ViewFit::Framed`] view, moves onto the view's own
    /// centre and scale ([`SavedViewAim::frame`]).
    ///
    /// `target - eye` is parallel to [`SavedViewAim::direction`] and `up`
    /// is [`SavedViewAim::up`]. A perspective camera keeps
    /// [`pdfcer_3d::Camera::fit`]'s field of view.
    ///
    /// # Errors
    ///
    /// [`pdfcer_3d::RenderError::Camera`] when the direction is zero or
    /// parallel to up, or `aspect` is not positive.
    pub fn camera(
        &self,
        bounds: &pdfcer_3d::Bounds,
        aspect: f64,
    ) -> Result<pdfcer_3d::Camera, pdfcer_3d::RenderError> {
        let mut camera =
            pdfcer_3d::Camera::fit(bounds, self.direction, self.up, !self.orthographic, aspect)?;
        self.frame(&mut camera);
        Ok(camera)
    }

    /// For a [`ViewFit::Framed`] view, moves a fitted `camera` so the image
    /// centre lies on the line through the view's position along the
    /// camera's own direction, shows the view's height, and projects
    /// orthographically; depth and distance are kept. Does nothing for
    /// [`ViewFit::FittedToModel`] or a degenerate camera.
    pub fn frame(&self, camera: &mut pdfcer_3d::Camera) {
        let ViewFit::Framed { position, height } = self.fit else {
            return;
        };
        let d = sub(camera.target, camera.eye);
        let len2 = dot(d, d);
        if !(len2.is_finite() && len2 > 0.0) {
            return;
        }
        let along = dot(sub(camera.target, position), d) / len2;
        camera.target = add(position, scale(d, along));
        camera.eye = sub(camera.target, d);
        camera.projection = pdfcer_3d::Projection::Orthographic { height };
    }
}

#[cfg(feature = "3d")]
fn sub([a, b, c]: [f64; 3], [x, y, z]: [f64; 3]) -> [f64; 3] {
    [a - x, b - y, c - z]
}

#[cfg(feature = "3d")]
fn add([a, b, c]: [f64; 3], [x, y, z]: [f64; 3]) -> [f64; 3] {
    [a + x, b + y, c + z]
}

#[cfg(feature = "3d")]
fn scale([a, b, c]: [f64; 3], k: f64) -> [f64; 3] {
    [a * k, b * k, c * k]
}

#[cfg(feature = "3d")]
fn dot([a, b, c]: [f64; 3], [x, y, z]: [f64; 3]) -> f64 {
    a * x + b * y + c * z
}

/// Visible model-space height of an orthographic view at `aspect`, or
/// `None` when the view does not fix one.
fn ortho_height(
    scale: f64,
    binding: OrthoBinding,
    view_box: Option<[f64; 2]>,
    m: &[f64; 12],
    aspect: f64,
) -> Option<f64> {
    let [_, _, _, ux, uy, uz, ..] = *m;
    let unit = (ux * ux + uy * uy + uz * uz).sqrt();
    let bound = unit / scale;
    let landscape = aspect >= 1.0;
    let height = match binding {
        OrthoBinding::Height => bound,
        OrthoBinding::Width => bound / aspect,
        OrthoBinding::Min if landscape => bound,
        OrthoBinding::Min => bound / aspect,
        OrthoBinding::Max if landscape => bound / aspect,
        OrthoBinding::Max => bound,
        OrthoBinding::Absolute => view_box?[1] * bound,
    };
    (height.is_finite() && height > 0.0).then_some(height)
}

#[cfg(test)]
// A panic on a missing value or a short array is the failure these tests report.
#[allow(clippy::expect_used, clippy::indexing_slicing)]
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

    /// A front view (looking along -y, z up) from (1, 10, 2), orthographic,
    /// binding the height at scale 0.25.
    fn front() -> ThreeDSavedView {
        ThreeDSavedView {
            name: "Front".to_owned(),
            camera_to_world: Some([1., 0., 0., 0., 0., 1., 0., -1., 0., 1., 10., 2.]),
            orthographic: true,
            ortho_scale: 0.25,
            ortho_binding: OrthoBinding::Height,
            ..ThreeDSavedView::default()
        }
    }

    #[test]
    fn an_orthographic_view_fixes_its_frame_and_says_so() {
        let aim = front().aim(2.0).expect("a matrix");
        assert_eq!(
            aim.fit,
            ViewFit::Framed {
                position: [1., 10., 2.],
                height: 4.0
            }
        );
        assert!(aim.source().contains("\"Front\""));
        assert!(aim.source().contains("4.000000 model units high"));
    }

    #[cfg(feature = "3d")]
    #[test]
    fn the_camera_looks_along_the_z_column_through_the_view_position() {
        let bounds = pdfcer_3d::Bounds {
            min: [-1.0; 3],
            max: [1.0; 3],
        };
        let aim = front().aim(2.0).expect("a matrix");
        let camera = aim.camera(&bounds, 2.0).expect("a camera");
        let d: [f64; 3] = std::array::from_fn(|i| camera.target[i] - camera.eye[i]);
        let n = d.iter().map(|c| c * c).sum::<f64>().sqrt();
        assert!((d[1] / n + 1.0).abs() < 1e-12 && d[0].abs() < 1e-12 && d[2].abs() < 1e-12);
        assert_eq!(camera.up, [0., 0., 1.]);
        assert!(
            (camera.target[0] - 1.0).abs() < 1e-12,
            "x on the view's axis"
        );
        assert!(
            (camera.target[2] - 2.0).abs() < 1e-12,
            "z on the view's axis"
        );
        assert_eq!(
            camera.projection,
            pdfcer_3d::Projection::Orthographic { height: 4.0 }
        );
    }
}
