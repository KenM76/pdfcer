//! The orbit camera: a turn about an up axis, a zoom and a pan, each
//! relative to the view that fits the model, or, once the operator has
//! turned it by hand, relative to the model point it turns about
//! ([`Anchor`]).
//!
//! The fitted view frames the sphere round the model's centre that holds
//! every vertex ([`crate::reach`]), so turning the model or reshaping the
//! picture never changes its size. A named view turns about the operator's
//! [`Upright`]; a file's own view ([`SavedAim`]) turns about its own up from
//! its own direction, and an orthographic one that fixes its framing keeps
//! it ([`ViewFrame`]) until a fit or a named view drops it.
//!
//! Every length is a scaled norm and every eye comes from
//! [`Camera::looking_at`], so a model of any finite size gives a finite
//! camera or an error, never a non-finite one.

use std::f64::consts::FRAC_PI_2;

use pdfcer_3d::vec3::{add, cross, dot, normalize, scale, span, sub, unit_cross};
use pdfcer_3d::{Bounds, Camera, DEFAULT_FOV_Y, NamedView, Projection, RenderError, UpAxis};

use crate::axes::Upright;
use crate::reach::{distance, radius};
use crate::saved::{SavedAim, ViewFrame};

/// The steepest pitch, radians: kept off ±90° so the view never looks
/// straight along the up axis.
pub const PITCH_LIMIT: f64 = FRAC_PI_2 - 0.01;
/// The zoom range, as a multiple of the fitted view: (closest out, farthest in).
pub const ZOOM_RANGE: (f64, f64) = (0.05, 50.0);
/// The clear space round the fitted sphere, as a multiple of its radius.
pub const FIT_MARGIN: f64 = 1.05;

/// The model point an orbit turns about, its depth along the look, and
/// where it sits off the picture's centre. An orthographic view shows the
/// height a perspective one would at that depth, so switching projection
/// keeps the scale. The point keeps its place on the picture however the
/// view turns.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    /// The model point turned about.
    pub point: [f64; 3],
    /// Its depth along the look from the eye, model units; positive.
    pub distance: f64,
    /// Its offset from the look's axis, right and up in the picture, model
    /// units.
    pub offset: [f64; 2],
}

impl Anchor {
    /// The point on the look's axis level with [`Anchor::point`], for
    /// picture right and up `right` and `up`; the point itself when that is
    /// not finite.
    fn base(&self, right: [f64; 3], up: [f64; 3]) -> [f64; 3] {
        let shift = add(scale(right, self.offset[0]), scale(up, self.offset[1]));
        let base = sub(self.point, shift);
        if finite(&base) { base } else { self.point }
    }
}

/// The axes a turn is measured in: yaw 0, pitch 0 looks along
/// [`Axes::front`], and [`Axes::up`] appears upward. Both unit length and
/// perpendicular.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Axes {
    front: [f64; 3],
    up: [f64; 3],
}

impl Axes {
    /// The operator's choice; an `upright` with both axes along one line is
    /// replaced by its up with that up's conventional Front.
    pub fn upright(upright: Upright) -> Self {
        let upright = if upright.up.perpendicular(upright.front) {
            upright
        } else {
            upright.with_up(upright.up)
        };
        Self {
            front: upright.front.vector(),
            up: upright.up.vector(),
        }
    }

    /// The axes of a look `direction` with `up` upward: up made unit, front
    /// the direction's part across it. `None` when the two are not
    /// independent directions (or either is zero or not finite).
    pub fn from_look(direction: [f64; 3], up: [f64; 3]) -> Option<Self> {
        let up = normalize(up)?;
        let direction = normalize(direction)?;
        let front = sub(direction, scale(up, dot(direction, up)));
        // Rounding leaves a residue of a direction along `up`, not a front.
        if pdfcer_3d::vec3::length(front) < 1e-12 {
            return None;
        }
        Some(Self {
            front: normalize(front)?,
            up,
        })
    }

    /// The unit direction yaw 0, pitch 0 looks along.
    pub fn front(&self) -> [f64; 3] {
        self.front
    }

    /// The unit direction that appears upward.
    pub fn up(&self) -> [f64; 3] {
        self.up
    }

    /// The picture's right at yaw 0, pitch 0: `front × up`.
    pub fn right(&self) -> [f64; 3] {
        cross(self.front, self.up)
    }

    /// The unit direction `yaw` radians round (towards the left) and
    /// `pitch` radians down from [`Axes::front`].
    pub fn look(&self, yaw: f64, pitch: f64) -> [f64; 3] {
        let (sy, cy) = yaw.sin_cos();
        let (sp, cp) = pitch.sin_cos();
        let turned = add(scale(self.right(), -sy * cp), scale(self.front, cy * cp));
        add(turned, scale(self.up, -sp))
    }

    /// The yaw and pitch [`Axes::look`] turns into `direction`, the pitch
    /// held inside [`PITCH_LIMIT`]; `None` for a zero or non-finite
    /// direction.
    pub fn angles(&self, direction: [f64; 3]) -> Option<(f64, f64)> {
        let d = normalize(direction)?;
        let pitch = (-dot(d, self.up)).clamp(-1.0, 1.0).asin();
        let yaw = (-dot(d, self.right())).atan2(dot(d, self.front));
        Some((yaw, pitch.clamp(-PITCH_LIMIT, PITCH_LIMIT)))
    }
}

/// Where the orbit camera is, relative to a fitted view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Orbit {
    /// Radians around the up axis.
    pub yaw: f64,
    /// Radians below the horizon: looking down when positive.
    pub pitch: f64,
    /// 1 frames the whole model; meant to lie in [`ZOOM_RANGE`]. A zoom that
    /// is not positive and finite counts as 1.
    pub zoom: f64,
    /// The target's shift along the picture's right and up, in model radii.
    pub pan: [f64; 2],
    /// Perspective, or orthographic.
    pub perspective: bool,
    axes: Axes,
    /// A file's view fixes the centre and scale ([`ViewFrame`]), applied
    /// while the projection is orthographic and no point is turned about.
    pub framed: bool,
    /// The point turned about, in place of the fitted view, from the first
    /// turn by hand until a fit or a named view.
    pub anchor: Option<Anchor>,
}

impl Orbit {
    /// `view`, turned about `upright`: its direction for a z-up model, with
    /// x, y and z read as the picture's right, the Front look and up.
    pub fn named(view: NamedView, perspective: bool, upright: Upright) -> Self {
        let axes = Axes::upright(upright);
        let ([x, y, z], _) = view.direction(UpAxis::Z);
        let look = add(
            add(scale(axes.right(), x), scale(axes.front, y)),
            scale(axes.up, z),
        );
        let (yaw, pitch) = axes.angles(look).unwrap_or((0.0, 0.0));
        Self {
            yaw,
            pitch,
            zoom: 1.0,
            pan: [0.0, 0.0],
            perspective,
            axes,
            framed: false,
            anchor: None,
        }
    }

    /// A file's view, turned about its own up; an up not square to the
    /// direction is kept as the turning axis, the view pitched off it.
    /// `None` when its direction and up are not independent.
    pub fn saved(aim: &SavedAim) -> Option<Self> {
        let axes = Axes::from_look(aim.direction, aim.up)?;
        let (yaw, pitch) = axes.angles(aim.direction)?;
        Some(Self {
            yaw,
            pitch,
            zoom: 1.0,
            pan: [0.0, 0.0],
            perspective: !aim.orthographic,
            axes,
            framed: aim.orthographic && aim.framed,
            anchor: None,
        })
    }

    /// The axes the turn is measured in.
    pub fn axes(&self) -> Axes {
        self.axes
    }

    /// The unit direction the camera looks along.
    pub fn direction(&self) -> [f64; 3] {
        self.axes.look(self.yaw, self.pitch)
    }

    /// Turn to look along `direction`, fitted again; a direction along the
    /// up axis stops just short of it, and a zero one changes nothing.
    pub fn look_along(&mut self, direction: [f64; 3]) {
        if let Some((yaw, pitch)) = self.axes.angles(direction) {
            (self.yaw, self.pitch) = (yaw, pitch);
            self.refit();
        }
    }

    /// Back to the view that fits the whole model, keeping the direction:
    /// no zoom, pan, file framing or point turned about.
    pub fn refit(&mut self) {
        (self.zoom, self.pan, self.framed) = (1.0, [0.0, 0.0], false);
        self.anchor = None;
    }

    /// Turn about `point` from now on without moving the picture: `camera`
    /// is this orbit's camera, and `point` stays where it shows on it.
    /// Changes nothing when the point is not finite or not in front of the
    /// eye.
    pub fn anchor_at(&mut self, camera: &Camera, point: [f64; 3]) {
        let dir = self.direction();
        let (right, image_up) = picture_axes(dir, self.axes.up);
        let v = sub(point, camera.eye);
        let depth = dot(v, dir);
        let offset = [dot(v, right), dot(v, image_up)];
        let distance = match camera.projection {
            Projection::Orthographic { height } => height / 2.0 / half_fov().tan(),
            Projection::Perspective { .. } | Projection::Fisheye { .. } => depth,
        };
        if finite(&point) && offset.iter().all(|c| c.is_finite()) && positive(distance) {
            self.anchor = Some(Anchor {
                point,
                distance,
                offset,
            });
            (self.zoom, self.pan, self.framed) = (1.0, [0.0, 0.0], false);
        }
    }

    /// Pan so `point` sits at the picture's centre, keeping the turn and
    /// the zoom; changes nothing when the pan would not be finite.
    pub fn centre_on(&mut self, bounds: &Bounds, point: [f64; 3]) {
        let dir = self.direction();
        let (right, image_up) = picture_axes(dir, self.axes.up);
        let centre = self
            .anchor
            .map_or_else(|| bounds.centre(), |a| a.base(right, image_up));
        let off = sub(point, centre);
        let r = radius(bounds);
        let pan = [dot(off, right) / r, dot(off, image_up) / r];
        if pan.iter().all(|c| c.is_finite()) {
            self.pan = pan;
            self.framed = false;
        }
    }

    /// Multiply the zoom by `factor` (the result held in [`ZOOM_RANGE`]),
    /// keeping the model point under the pointer where it is on the picture.
    ///
    /// `offset` is the pointer's position from the picture's centre, right
    /// and up, in picture heights; `bounds`, `reach`, `aspect` and `frame`
    /// are as [`Orbit::camera`] takes them. Exact on the plane through the
    /// target facing the camera, the plane the pan moves in. A factor that
    /// is not positive and finite changes nothing, and a pan that would not
    /// be finite is kept.
    pub fn zoom_at(
        &mut self,
        bounds: &Bounds,
        reach: f64,
        aspect: f64,
        factor: f64,
        offset: [f64; 2],
        frame: Option<ViewFrame>,
    ) {
        if !positive(factor) {
            return;
        }
        let before = self.zoom_or_one();
        let after = (before * factor).clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
        let fitted = Self {
            zoom: 1.0,
            pan: [0.0, 0.0],
            ..*self
        };
        // The fitted view's visible height in model radii: what one picture
        // height spans at zoom 1.
        let height = fitted
            .camera(bounds, reach, aspect, frame)
            .map_or(2.0, |c| c.visible_height())
            / radius(bounds);
        let step = height * (1.0 / before - 1.0 / after);
        let pan = [
            self.pan[0] + offset[0] * step,
            self.pan[1] + offset[1] * step,
        ];
        if pan.iter().all(|c| c.is_finite()) {
            self.pan = pan;
        }
        self.zoom = after;
    }

    /// The camera for `bounds`, whose every drawn vertex is within `reach`
    /// of its centre ([`crate::reach`]; a reach that is not positive and
    /// finite counts as the box's [`radius`]), in a picture of `aspect`
    /// (width / height; one that is not positive and finite counts as 1).
    /// `frame` is the file's view's framing at that aspect, applied while
    /// [`Orbit::framed`], orthographic and no point is turned about.
    ///
    /// The pan is dropped when it would move the target beyond the largest
    /// float.
    ///
    /// # Errors
    ///
    /// [`RenderError::Camera`] when no finite camera exists: the look is
    /// along the up axis, or no finite eye sees the target.
    pub fn camera(
        &self,
        bounds: &Bounds,
        reach: f64,
        aspect: f64,
        frame: Option<ViewFrame>,
    ) -> Result<Camera, RenderError> {
        let dir = self.direction();
        let up = self.axes.up;
        let mut camera = match self.anchor {
            Some(a) => about(a, dir, up, self.perspective)?,
            None => fitted(bounds, reach, dir, up, self.perspective, aspect)?,
        };
        if self.anchor.is_none()
            && self.framed
            && !self.perspective
            && let Some(frame) = frame
        {
            frame.apply(&mut camera);
        }
        let (right, image_up) = picture_axes(dir, up);
        let r = radius(bounds);
        let shift = add(
            scale(right, self.pan[0] * r),
            scale(image_up, self.pan[1] * r),
        );
        let shifted = add(camera.target, shift);
        let target = if finite(&shifted) {
            shifted
        } else {
            camera.target
        };
        let zoom = self.zoom_or_one();
        let back = distance(camera.eye, camera.target).unwrap_or(0.0) / zoom;
        let projection = match camera.projection {
            Projection::Orthographic { height } => Projection::Orthographic {
                height: (height / zoom).min(f64::MAX),
            },
            other => other,
        };
        let look = span(camera.eye, camera.target);
        Camera::looking_at(target, look, back, up, projection)
    }

    fn zoom_or_one(&self) -> f64 {
        if positive(self.zoom) { self.zoom } else { 1.0 }
    }
}

/// Half the fitted perspective's vertical field, radians.
fn half_fov() -> f64 {
    (DEFAULT_FOV_Y / 2.0).to_radians()
}

fn positive(x: f64) -> bool {
    x.is_finite() && x > 0.0
}

fn finite(v: &[f64; 3]) -> bool {
    v.iter().all(|c| c.is_finite())
}

/// The picture's unit right and up looking along unit `dir`.
fn picture_axes(dir: [f64; 3], up: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let right = unit_cross(dir, up).unwrap_or([1.0, 0.0, 0.0]);
    (right, cross(right, dir))
}

/// The fitted camera looking along `dir`, holding the sphere of `reach`
/// (with [`FIT_MARGIN`]) round `bounds`' centre whichever way it looks.
fn fitted(
    bounds: &Bounds,
    reach: f64,
    dir: [f64; 3],
    up: [f64; 3],
    perspective: bool,
    aspect: f64,
) -> Result<Camera, RenderError> {
    let reach = if positive(reach) {
        reach
    } else {
        radius(bounds)
    };
    let aspect = if positive(aspect) { aspect } else { 1.0 };
    let sphere = (reach * FIT_MARGIN).min(f64::MAX);
    Camera::fit_sphere(bounds.centre(), sphere, dir, up, perspective, aspect)
}

/// The camera looking along `dir` that shows `anchor`'s point at its depth
/// and offset.
fn about(
    anchor: Anchor,
    dir: [f64; 3],
    up: [f64; 3],
    perspective: bool,
) -> Result<Camera, RenderError> {
    let (right, image_up) = picture_axes(dir, up);
    let base = anchor.base(right, image_up);
    let projection = if perspective {
        Projection::Perspective {
            fov_y: DEFAULT_FOV_Y,
        }
    } else {
        Projection::Orthographic {
            height: (2.0 * anchor.distance * half_fov().tan()).min(f64::MAX),
        }
    };
    Camera::looking_at(base, dir, anchor.distance, up, projection)
}

#[cfg(test)]
#[path = "orbit_tests.rs"]
mod tests;
