//! A partial panorama built from a view ([`Camera::panorama`]).

use crate::camera::band::Band;
use crate::vec3::{dot, length, normalize, sub};
use crate::{Camera, PanoramaMapping, Projection, RenderError};

/// Whether a [`Camera::panorama`] band follows the view's pitch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum PanoramaTilt {
    /// The sweep stays parallel to the ground: the band is centred on the
    /// horizon, whatever the view's pitch, so the horizon is the picture's
    /// middle row.
    #[default]
    Level,
    /// The band's middle is the view's own latitude above the horizon: a
    /// view looking down 20° gives a band centred 20° below it. The sweep
    /// still turns about `up`, so the horizon stays a straight row.
    View,
}

/// How much of the sphere a [`Camera::panorama`] shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanoramaExtent {
    /// Longitude across the picture, degrees: above 0, at most 360.
    pub horizontal: f64,
    /// Latitude down the picture, degrees: above 0, at most 180. `None`
    /// takes the view's own vertical angle: a perspective's `fov_y`, a
    /// fisheye's `fov` (at most 180), a panorama's `vertical`.
    pub vertical: Option<f64>,
    /// Levelled or tilted with the view.
    pub tilt: PanoramaTilt,
    /// How latitude runs down the picture.
    pub mapping: PanoramaMapping,
}

impl Camera {
    /// This camera as a [`Projection::Panorama`] of `extent`: the same eye,
    /// the target turned level with `up` at the same distance (as
    /// [`Camera::levelled_panorama`]), `up` as the camera's up, and the
    /// band centred on the look direction's heading. Under
    /// [`PanoramaTilt::View`] its middle latitude is the view's pitch,
    /// asin(look · up), moved inside the poles as the projection states;
    /// under [`PanoramaTilt::Level`] it is 0.
    ///
    /// `front` is the heading when this camera looks straight along `up`.
    ///
    /// # Errors
    ///
    /// [`RenderError::Camera`] when the camera cannot be levelled (as
    /// [`Camera::levelled_panorama`] returns `None`), or `extent.vertical`
    /// is `None` under an orthographic view, which has no vertical angle;
    /// [`RenderError::PanoramaField`] for angles the band does not draw.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_3d::{Camera, PanoramaExtent, PanoramaMapping, PanoramaTilt, Projection};
    /// let down = Camera {
    ///     eye: [0.0, 0.0, 0.0],
    ///     target: [0.0, 1.0, -1.0],
    ///     up: [0.0, 0.0, 1.0],
    ///     projection: Projection::Perspective { fov_y: 40.0 },
    /// };
    /// let extent = PanoramaExtent {
    ///     horizontal: 180.0,
    ///     vertical: None,
    ///     tilt: PanoramaTilt::View,
    ///     mapping: PanoramaMapping::Equirectangular,
    /// };
    /// let pano = down.panorama([0.0, 0.0, 1.0], [0.0, 1.0, 0.0], extent)?;
    /// let Projection::Panorama { vertical, pitch, .. } = pano.projection else { panic!() };
    /// assert_eq!(vertical, 40.0);
    /// assert!((pitch + 45.0).abs() < 1e-9);
    /// # Ok::<(), pdfcer_3d::RenderError>(())
    /// ```
    pub fn panorama(
        self,
        up: [f64; 3],
        front: [f64; 3],
        extent: PanoramaExtent,
    ) -> Result<Camera, RenderError> {
        let vertical = match (extent.vertical, self.projection) {
            (Some(v), _) => v,
            (None, Projection::Perspective { fov_y }) => fov_y,
            (None, Projection::Fisheye { fov, .. }) => fov.min(180.0),
            (None, Projection::Panorama { vertical, .. }) => vertical,
            (None, Projection::Orthographic { .. }) => {
                return Err(RenderError::Camera(
                    "an orthographic view has no vertical angle",
                ));
            }
        };
        let pitch = match extent.tilt {
            PanoramaTilt::Level => 0.0,
            PanoramaTilt::View => view_pitch(&self, up),
        };
        Band::new(extent.horizontal, vertical, pitch, extent.mapping)?;
        let centre = pitch.clamp(vertical / 2.0 - 90.0, 90.0 - vertical / 2.0);
        let mut done = self
            .levelled_panorama(up, front)
            .ok_or(RenderError::Camera("the camera has no level heading"))?;
        done.projection = Projection::Panorama {
            horizontal: extent.horizontal,
            vertical,
            pitch: centre,
            mapping: extent.mapping,
        };
        Ok(done)
    }
}

/// The view's latitude above the plane square to `up`, degrees; 0 when
/// either vector is degenerate (levelling then refuses).
fn view_pitch(camera: &Camera, up: [f64; 3]) -> f64 {
    let look = sub(camera.target, camera.eye);
    match (normalize(look), normalize(up)) {
        (Some(l), Some(u)) if length(look).is_finite() => {
            dot(l, u).clamp(-1.0, 1.0).asin().to_degrees()
        }
        _ => 0.0,
    }
}
