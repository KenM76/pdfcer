//! Cameras built from a centre and a reach rather than a box, and the
//! height a camera sees at its target: the fitted views of an interactive
//! viewer (`pdfcer-3d-nav`), safe for any finite model.

use crate::camera::{EYE_SHARE, basis_along, eye_at};
use crate::vec3::{length, normalize, span, sub};
use crate::{Camera, Projection, RenderError};

/// The vertical field of view, in degrees, of every perspective camera the
/// engine fits ([`Camera::fit`], [`Camera::fit_meshes`],
/// [`Camera::fit_sphere`]).
pub const DEFAULT_FOV_Y: f64 = 30.0;

/// The widest perspective or fisheye angle, in degrees,
/// [`Camera::visible_height`] measures with; wider angles are measured as
/// this one (`tan` of a half-angle near 90° grows without bound).
const WIDEST_MEASURED: f64 = 179.0;

impl Camera {
    /// A camera looking along `direction` at `centre` that frames the
    /// sphere of radius `reach` about it exactly (no margin; scale `reach`
    /// up for one), in an image of the given aspect ratio (width / height).
    ///
    /// Perspective uses [`DEFAULT_FOV_Y`] and stands back
    /// `reach / sin(half)`, where `half` is the tighter of the half-angles
    /// across the height and the width; orthographic stands back
    /// `2 · reach` and shows a height of `2 · reach / min(aspect, 1)`. A
    /// distance or height beyond the largest float is clamped to it, the
    /// eye stays at least 2^-30 of the centre's largest coordinate away (as
    /// [`Camera::fit`]), and it is brought nearer until it is finite.
    ///
    /// # Errors
    ///
    /// [`RenderError::Camera`] when `aspect` or `reach` is not positive and
    /// finite, `centre` is not finite, `direction` is zero or parallel to
    /// `up`, or no finite eye exists (a centre at the edge of the float
    /// range looked at from beyond it).
    ///
    /// ```
    /// use pdfcer_3d::{Camera, Projection};
    /// let camera = Camera::fit_sphere([0.0; 3], 1.0, [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], false, 2.0)?;
    /// assert_eq!(camera.projection, Projection::Orthographic { height: 2.0 });
    /// assert_eq!(camera.eye, [0.0, -2.0, 0.0]);
    /// # Ok::<(), pdfcer_3d::RenderError>(())
    /// ```
    pub fn fit_sphere(
        centre: [f64; 3],
        reach: f64,
        direction: [f64; 3],
        up: [f64; 3],
        perspective: bool,
        aspect: f64,
    ) -> Result<Camera, RenderError> {
        if !(aspect.is_finite() && aspect > 0.0) {
            return Err(RenderError::Camera("the aspect ratio is not positive"));
        }
        if !(reach.is_finite() && reach > 0.0) {
            return Err(RenderError::Camera("the reach is not positive"));
        }
        let half_fov = DEFAULT_FOV_Y.to_radians() / 2.0;
        let half = half_fov.min((half_fov.tan() * aspect).atan());
        let (projection, distance) = if perspective {
            let fov_y = DEFAULT_FOV_Y;
            (Projection::Perspective { fov_y }, reach / half.sin())
        } else {
            let height = (2.0 * reach / aspect.min(1.0)).min(f64::MAX);
            (Projection::Orthographic { height }, 2.0 * reach)
        };
        Camera::looking_at(centre, direction, distance, up, projection)
    }

    /// A camera at `target` looking along `direction` from `distance` back,
    /// with `up` upward and the given projection.
    ///
    /// The distance is clamped to the largest float and raised to at least
    /// 2^-30 of the target's largest coordinate (so the view direction
    /// survives the eye's rounding), then shortened (halving) until the eye
    /// is finite.
    ///
    /// # Errors
    ///
    /// [`RenderError::Camera`] when `target` is not finite, `distance` is
    /// not positive (or is NaN), `direction` is zero or parallel to `up`,
    /// or no finite eye exists.
    pub fn looking_at(
        target: [f64; 3],
        direction: [f64; 3],
        distance: f64,
        up: [f64; 3],
        projection: Projection,
    ) -> Result<Camera, RenderError> {
        if !target.iter().all(|c| c.is_finite()) {
            return Err(RenderError::Camera("the target is not finite"));
        }
        if distance.is_nan() || distance <= 0.0 {
            return Err(RenderError::Camera("the distance is not positive"));
        }
        let dir = normalize(direction).ok_or(RenderError::Camera("the view direction is zero"))?;
        basis_along(dir, up)?;
        let placeable = target.iter().fold(0.0_f64, |m, c| m.max(c.abs())) * EYE_SHARE;
        let eye = eye_at(target, dir, distance.max(placeable));
        if !eye.iter().all(|c| c.is_finite()) {
            return Err(RenderError::Camera("no finite eye sees the target"));
        }
        let camera = Camera {
            eye,
            target,
            up,
            projection,
        };
        camera.basis()?;
        Ok(camera)
    }

    /// The height of the scene the camera shows at its target, in model
    /// units: an orthographic projection's height, or `2 · d · tan(fov / 2)`
    /// for perspective and fisheye, where `d` is the eye-to-target distance
    /// and `fov` is clamped to 179°.
    ///
    /// Finite for any camera whose eye, target and projection are finite: the
    /// distance is measured with scaled norms and the result is clamped to
    /// the largest float. A NaN angle or height gives NaN.
    pub fn visible_height(&self) -> f64 {
        let fov = match self.projection {
            Projection::Orthographic { height } => return height,
            Projection::Perspective { fov_y } => fov_y,
            Projection::Fisheye { fov, .. } => fov,
        };
        let d = sub(self.target, self.eye);
        // `span` halves a difference that overflows; measure half and double
        // it after the angle, which is below 1 for the clamped field.
        let (half_distance, halved) = if d.iter().all(|c| c.is_finite()) {
            (length(d) / 2.0, false)
        } else {
            (length(span(self.eye, self.target)) / 2.0, true)
        };
        let tan = (fov.min(WIDEST_MEASURED).to_radians() / 2.0).tan();
        let height = half_distance * tan * if halved { 8.0 } else { 4.0 };
        height.min(f64::MAX)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    const UP: [f64; 3] = [0.0, 0.0, 1.0];
    const ALONG_Y: [f64; 3] = [0.0, 1.0, 0.0];

    fn distance(c: &Camera) -> f64 {
        length(span(c.eye, c.target))
    }

    fn finite(c: &Camera) -> bool {
        c.eye.iter().chain(&c.target).all(|v| v.is_finite())
    }

    #[test]
    fn a_perspective_fit_touches_the_sphere_on_the_tighter_side() {
        let half = (DEFAULT_FOV_Y / 2.0).to_radians();
        let wide = Camera::fit_sphere([0.0; 3], 1.0, ALONG_Y, UP, true, 2.0).unwrap();
        assert!((distance(&wide) - 1.0 / half.sin()).abs() < 1e-12);
        let tall = Camera::fit_sphere([0.0; 3], 1.0, ALONG_Y, UP, true, 0.5).unwrap();
        let side = (half.tan() * 0.5).atan();
        assert!((distance(&tall) - 1.0 / side.sin()).abs() < 1e-12);
        assert_eq!(
            wide.projection,
            Projection::Perspective {
                fov_y: DEFAULT_FOV_Y
            }
        );
    }

    #[test]
    fn an_orthographic_fit_widens_its_height_for_a_tall_picture() {
        let tall = Camera::fit_sphere([1.0, 2.0, 3.0], 2.0, ALONG_Y, UP, false, 0.5).unwrap();
        assert_eq!(tall.projection, Projection::Orthographic { height: 8.0 });
        assert_eq!(tall.eye, [1.0, -2.0, 3.0]);
        assert_eq!(tall.visible_height(), 8.0);
    }

    #[test]
    fn spheres_from_1e_minus_200_to_1e200_fit_at_a_finite_positive_distance() {
        for reach in [1e-200, 1e-100, 1.0, 1e100, 1e200, f64::MAX] {
            for perspective in [true, false] {
                let c = Camera::fit_sphere([0.0; 3], reach, ALONG_Y, UP, perspective, 1.5).unwrap();
                let d = distance(&c);
                assert!(
                    finite(&c) && d.is_finite() && d > 0.0,
                    "{reach} {perspective}"
                );
                let h = c.visible_height();
                assert!(h.is_finite() && h > 0.0, "{reach} {perspective}");
            }
        }
    }

    #[test]
    fn a_sphere_at_the_edge_of_the_floats_gives_a_finite_eye_or_no_fit() {
        for centre in [[f64::MAX, 0.0, 0.0], [0.0, -f64::MAX, 0.0], [1e300; 3]] {
            for aspect in [1e-6, 1.0, 1e6] {
                for perspective in [true, false] {
                    let fit =
                        Camera::fit_sphere(centre, f64::MAX, ALONG_Y, UP, perspective, aspect);
                    if let Ok(c) = fit {
                        assert!(finite(&c), "{centre:?} {aspect} {perspective}");
                        assert!(c.visible_height().is_finite());
                    }
                }
            }
        }
    }

    #[test]
    fn a_degenerate_request_is_refused() {
        let fit = |reach, dir, aspect| Camera::fit_sphere([0.0; 3], reach, dir, UP, true, aspect);
        assert!(fit(1.0, ALONG_Y, 0.0).is_err());
        assert!(fit(1.0, ALONG_Y, f64::NAN).is_err());
        assert!(fit(0.0, ALONG_Y, 1.0).is_err());
        assert!(fit(f64::INFINITY, ALONG_Y, 1.0).is_err());
        assert!(fit(1.0, [0.0; 3], 1.0).is_err());
        assert!(fit(1.0, UP, 1.0).is_err());
        let p = Projection::Perspective { fov_y: 30.0 };
        assert!(Camera::looking_at([f64::NAN; 3], ALONG_Y, 1.0, UP, p).is_err());
        assert!(Camera::looking_at([0.0; 3], ALONG_Y, f64::NAN, UP, p).is_err());
    }

    #[test]
    fn the_visible_height_of_a_camera_spanning_the_floats_is_finite() {
        let c = Camera {
            eye: [-f64::MAX, 0.0, 0.0],
            target: [f64::MAX, 0.0, 0.0],
            up: UP,
            projection: Projection::Perspective { fov_y: 179.9 },
        };
        assert_eq!(c.visible_height(), f64::MAX);
        // 1.5 * MAX apart: the distance overflows, the 30 degree height does not.
        let apart = Camera {
            eye: [-0.75 * f64::MAX, 0.0, 0.0],
            target: [0.75 * f64::MAX, 0.0, 0.0],
            projection: Projection::Perspective { fov_y: 30.0 },
            ..c
        };
        let wanted = 0.75 * f64::MAX * (4.0 * 15f64.to_radians().tan());
        assert!((apart.visible_height() / wanted - 1.0).abs() < 1e-12);
        let near = Camera {
            eye: [0.0, -1.0, 0.0],
            target: [0.0; 3],
            projection: Projection::Perspective { fov_y: 90.0 },
            ..c
        };
        assert!((near.visible_height() - 2.0).abs() < 1e-12);
    }
}
