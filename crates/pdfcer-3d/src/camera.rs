//! The camera: where the eye is, what it looks at, and how it projects;
//! framing a model, picking, the lens and the bounds it frames.

pub(crate) mod bounds;
pub(crate) mod lens;
pub(crate) mod lens_frame;
#[cfg(test)]
mod lens_tests;
pub(crate) mod pick;
pub(crate) mod pick_index;
pub(crate) mod projection;
pub(crate) mod sphere;

use crate::TriangleMesh;
use crate::camera::bounds::{Bounds, drawn_corners};
use crate::camera::projection::Projection;
use crate::render::RenderError;
use crate::render::raster::at;
use crate::vec3::{cross, dot, normalize, scale, sub};

/// [`Camera::fit`]'s framed extent over the model's: 5% clear on each side.
pub(crate) const FIT_MARGIN: f64 = 1.1;
/// The least eye-to-target distance a fit uses, over the target's largest
/// coordinate: the view direction then survives the eye's rounding to
/// within about 2^-22 radians.
pub(crate) const EYE_SHARE: f64 = 1.0 / (1u64 << 30) as f64;

/// Where the camera is and where it looks, in model coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// The camera position.
    pub eye: [f64; 3],
    /// The point at the image centre.
    pub target: [f64; 3],
    /// The direction that appears upward; must not be parallel to the view
    /// direction.
    pub up: [f64; 3],
    /// The projection.
    pub projection: Projection,
}

impl Camera {
    /// A camera looking along `direction` that fits `bounds` in an image of
    /// the given aspect ratio (width / height), with a
    /// [`DEFAULT_FOV_Y`](crate::DEFAULT_FOV_Y) perspective or an orthographic
    /// projection.
    ///
    /// The box's eight corners are framed as they project from this
    /// direction, aimed at the box's centre, with about 5% clear on each
    /// side of the tighter axis (perspective: of the corner that limits it).
    /// The eye stays at least 2^-30 of the target's largest coordinate away,
    /// so a model small beside its distance from the origin keeps the view
    /// direction (in perspective it is then framed smaller).
    ///
    /// # Errors
    ///
    /// [`RenderError::Camera`] when `direction` is zero or parallel to
    /// `up`, or `aspect` is not positive.
    pub fn fit(
        bounds: &Bounds,
        direction: [f64; 3],
        up: [f64; 3],
        perspective: bool,
        aspect: f64,
    ) -> Result<Camera, RenderError> {
        let corners = (0..8).map(|k| {
            [0, 1, 2].map(|i| {
                if k >> i & 1 == 0 {
                    at(bounds.min, i)
                } else {
                    at(bounds.max, i)
                }
            })
        });
        Camera::frame(bounds, corners, direction, up, perspective, aspect)
    }

    /// A camera looking along `direction` that fits every vertex the meshes'
    /// triangles use: like [`Camera::fit`], but framing the model itself
    /// rather than its box, so an oblique view is not padded by empty box
    /// corners. Aimed at the middle of the projected model.
    ///
    /// # Errors
    ///
    /// [`RenderError::Camera`] as for [`Camera::fit`], and when the meshes
    /// have no finite vertex.
    ///
    /// ```
    /// use pdfcer_3d::{Camera, TriangleMesh};
    /// let mut mesh = TriangleMesh::default();
    /// mesh.positions = vec![[0.0, 0.0, 0.0], [2.0, 0.0, 0.0], [0.0, 2.0, 0.0]];
    /// mesh.triangles = vec![[0, 1, 2]];
    /// // Seen with x + y upward, the triangle sits low in its box.
    /// let camera = Camera::fit_meshes(&[mesh], [0.0, 0.0, -1.0], [1.0, 1.0, 0.0], false, 1.0)?;
    /// assert!(camera.target.iter().zip([0.5, 0.5, 0.0]).all(|(a, b)| (a - b).abs() < 1e-9));
    /// # Ok::<(), pdfcer_3d::RenderError>(())
    /// ```
    pub fn fit_meshes(
        meshes: &[TriangleMesh],
        direction: [f64; 3],
        up: [f64; 3],
        perspective: bool,
        aspect: f64,
    ) -> Result<Camera, RenderError> {
        Camera::fit_coloured(meshes, &[], direction, up, perspective, aspect)
    }

    /// [`Camera::fit_meshes`] for meshes drawn in `colours`, as
    /// [`crate::render_coloured`] takes them: a fully transparent mesh (alpha 0)
    /// is not drawn, so it is not framed ([`Bounds::of_coloured`]).
    ///
    /// # Errors
    ///
    /// As for [`Camera::fit_meshes`]; "no vertex" also when every mesh
    /// with one is fully transparent.
    pub fn fit_coloured(
        meshes: &[TriangleMesh],
        colours: &[Option<[u8; 4]>],
        direction: [f64; 3],
        up: [f64; 3],
        perspective: bool,
        aspect: f64,
    ) -> Result<Camera, RenderError> {
        let bounds = Bounds::of_coloured(meshes, colours)
            .ok_or(RenderError::Camera("the model has no vertex"))?;
        Camera::frame(
            &bounds,
            drawn_corners(meshes, colours),
            direction,
            up,
            perspective,
            aspect,
        )
    }

    /// Frames `points` (which `bounds` holds) from `direction`, aimed at the
    /// middle of their projection.
    fn frame(
        bounds: &Bounds,
        points: impl Iterator<Item = [f64; 3]> + Clone,
        direction: [f64; 3],
        up: [f64; 3],
        perspective: bool,
        aspect: f64,
    ) -> Result<Camera, RenderError> {
        if !(aspect.is_finite() && aspect > 0.0) {
            return Err(RenderError::Camera("the aspect ratio is not positive"));
        }
        let dir = normalize(direction).ok_or(RenderError::Camera("the view direction is zero"))?;
        let centre = bounds.centre();
        let radius = match bounds.radius() {
            r if r.is_finite() && r > 0.0 => r,
            _ => 1.0,
        };
        // From the direction alone: a point such as `centre - dir` can
        // round back to `centre` for a small model far from the origin.
        let [right, upward, forward] = basis_along(dir, up)?;
        let view = |p: [f64; 3]| {
            let r = sub(p, centre);
            [dot(r, right), dot(r, upward), dot(r, forward)]
        };
        let ((x0, x1), (y0, y1)) = points.clone().map(view).fold(
            (
                (f64::INFINITY, f64::NEG_INFINITY),
                (f64::INFINITY, f64::NEG_INFINITY),
            ),
            |((a0, a1), (b0, b1)), [x, y, _]| ((a0.min(x), a1.max(x)), (b0.min(y), b1.max(y))),
        );
        if !(x0 <= x1 && y0 <= y1) {
            return Err(RenderError::Camera("the model has no vertex"));
        }
        let (mx, my) = (x0 / 2.0 + x1 / 2.0, y0 / 2.0 + y1 / 2.0);
        let target = [0, 1, 2].map(|i| at(centre, i) + mx * at(right, i) + my * at(upward, i));
        let floor = radius * 1e-3;
        let (projection, distance) = if perspective {
            let fov_y = crate::camera::sphere::DEFAULT_FOV_Y;
            let ty = (fov_y.to_radians() / 2.0).tan() / FIT_MARGIN;
            let tx = ty * aspect;
            // A point at lateral (x, y) from the target and depth f past it
            // is in view when |x| <= (d + f)·tx and |y| <= (d + f)·ty.
            let d = points.map(view).fold(floor, |d, [x, y, f]| {
                let need = ((x - mx).abs() / tx).max((y - my).abs() / ty);
                d.max(need - f).max(floor - f)
            });
            (Projection::Perspective { fov_y }, d)
        } else {
            // A model wider than the largest float is framed at the
            // largest height.
            let height = ((y1 - y0).max((x1 - x0) / aspect) * FIT_MARGIN)
                .max(floor)
                .min(f64::MAX);
            (Projection::Orthographic { height }, 2.0 * radius)
        };
        // Far enough off the target that `target - eye` still points along
        // `dir` once the eye's coordinates round (by up to 2^-53 of each).
        let placeable = target.iter().fold(0.0_f64, |m, c| m.max(c.abs())) * EYE_SHARE;
        let camera = Camera {
            eye: eye_at(target, dir, distance.max(placeable)),
            target,
            up,
            projection,
        };
        camera.basis()?;
        Ok(camera)
    }

    /// Right, up and forward unit vectors.
    pub(crate) fn basis(&self) -> Result<[[f64; 3]; 3], RenderError> {
        let forward = normalize(sub(self.target, self.eye))
            .ok_or(RenderError::Camera("the eye and the target coincide"))?;
        basis_along(forward, self.up)
    }
}

/// The point `distance` back from `target` along unit `dir`, brought
/// nearer (halving, at most 64 times) until it is finite, so a model
/// near the largest float still gets an eye.
pub(crate) fn eye_at(target: [f64; 3], dir: [f64; 3], distance: f64) -> [f64; 3] {
    let mut d = distance.min(f64::MAX);
    let mut eye = sub(target, scale(dir, d));
    for _ in 0..64 {
        if eye.iter().all(|c| c.is_finite()) {
            break;
        }
        d /= 2.0;
        eye = sub(target, scale(dir, d));
    }
    eye
}

/// Right, up and forward unit vectors looking along unit `forward`.
pub(crate) fn basis_along(forward: [f64; 3], up: [f64; 3]) -> Result<[[f64; 3]; 3], RenderError> {
    let right = normalize(cross(forward, up)).ok_or(RenderError::Camera(
        "the up direction is zero or parallel to the view direction",
    ))?;
    Ok([right, cross(right, forward), forward])
}
