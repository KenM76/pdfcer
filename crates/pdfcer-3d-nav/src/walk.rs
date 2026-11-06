//! Walking through the model: a first-person camera at a body's eye height.
//!
//! Solid (ghost mode off), the walker stops at what it meets, slides along
//! it, and falls onto the floor under it; with nothing under it, it keeps
//! its height. It cannot rise or sink by itself: stair climbing, when on, is
//! the only way up, over anything lower than [`CLIMB`] of its eye height;
//! off, only a floor seam, and nothing raises it into what is above it. It
//! comes down a walkable slope as it goes up one, standing on it. Ghosting,
//! it flies through everything and rises
//! and sinks as asked. Moving is horizontal when solid or level; a ghost
//! that is not level flies where it looks.
//!
//! The body and the camera turn apart, as a drone and its gimbal do:
//! [`Walk::yaw`] is the body's heading, which moving follows, and
//! [`Walk::look_yaw`] the camera's turn from it. [`Walk::pitch`] is the
//! camera's alone; the body never tilts.
//!
//! Every distance is a share of the model's radius, so a building and a
//! bracket walk alike, except an eye height the operator set
//! ([`Walk::height`]). Model units throughout.

use pdfcer_3d::vec3::{add, cross, dot, length, normalize, scale, span, sub};
use pdfcer_3d::{Camera, Hit, Ray};

use crate::lens::Lens;
use crate::orbit::{Axes, PITCH_LIMIT};

/// The walking field of view, degrees, vertically.
const FOV: f64 = 50.0;
/// The widest aspect the vertical field holds for. A wider window keeps a
/// 16:9 window's sideways field and narrows the vertical one instead.
const WIDEST: f64 = 16.0 / 9.0;
/// Eye height, as a share of the model's radius.
const BODY_SHARE: f64 = 0.08;
/// Walking speed at speed 1, in model radii per second.
const PACE: f64 = 0.25;
/// Running speed, as a multiple of walking.
const RUN: f64 = 3.0;
/// The highest obstacle climbed with stair climbing on, as a share of eye
/// height.
pub const CLIMB: f64 = 1.0 / 3.0;
/// The highest step climbed with it off: a seam or a threshold, never an
/// object.
const BUMP: f64 = 0.05;
/// The camera's narrowest zoom-in, as a multiple of [`FOV`]'s view; the
/// widest zoom-out is the lens's ([`Lens::widest`]).
const ZOOM_IN: f64 = 6.0;
/// A hit whose normal is this close to the up axis is a floor or a ramp,
/// stood on rather than walked into.
const WALKABLE: f64 = 0.7;
/// How close a wall stops the eye, as a share of eye height.
const REACH: f64 = 0.25;
/// Gravity, in eye heights per second squared: about Earth's for a 1.6 m
/// eye.
const GRAVITY: f64 = 6.0;
/// What an eye keeps clear above it when it stands higher, as a share of eye
/// height. Some, or an eye landing exactly on a ceiling's height ignores it and
/// passes through; not [`REACH`], or a low threshold under a door header stops
/// the walker for good.
const HEAD_CLEARANCE: f64 = 0.01;
/// The most parts a solid step is split into, however long it is.
const MOST_PARTS: u32 = 64;

/// `world`, answering a ray that is not finite with a miss. An eye near the
/// largest number can make one.
fn finite_rays<W: FnMut(&Ray) -> Option<Hit>>(
    world: &mut W,
) -> impl FnMut(&Ray) -> Option<Hit> + use<'_, W> {
    move |ray| {
        let finite = ray
            .origin
            .iter()
            .chain(&ray.direction)
            .all(|c| c.is_finite());
        if finite { world(ray) } else { None }
    }
}

/// What the operator asks for this frame. `forward`, `right` and `rise` run
/// from -1 to 1 (a stick's tilt, or a held key's 1).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Intent {
    /// Ahead along the heading; negative backs up.
    pub forward: f64,
    /// Sideways to the heading's right; negative goes left.
    pub right: f64,
    /// Up the up axis; obeyed only by a ghost.
    pub rise: f64,
    /// Move at the running pace.
    pub run: bool,
}

impl Intent {
    /// Whether nothing is asked for: no move and no rise.
    pub fn idle(&self) -> bool {
        self.forward == 0.0 && self.right == 0.0 && self.rise == 0.0
    }
}

/// How walking treats the model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rules {
    /// Walls stop and floors hold the walker (ghost mode off).
    pub solid: bool,
    /// Move horizontally whatever the look's pitch.
    pub level: bool,
    /// A multiple of the walking pace.
    pub speed: f64,
    /// Climb obstacles lower than [`CLIMB`] of the eye height.
    pub stairs: bool,
}

impl Rules {
    /// The highest step climbed, as a share of eye height.
    fn step(self) -> f64 {
        if self.stairs { CLIMB } else { BUMP }
    }
}

/// The walker.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Walk {
    /// Where the eye is.
    pub eye: [f64; 3],
    /// The body's heading: radians round the up axis, as [`Axes::look`]
    /// measures it.
    pub yaw: f64,
    /// The camera's turn from the heading, radians, the same way.
    pub look_yaw: f64,
    /// The camera's radians below the horizon.
    pub pitch: f64,
    /// The camera's zoom, a multiple of the walking field's view.
    pub zoom: f64,
    axes: Axes,
    /// The eye height the operator set, model units; `None` (or one that
    /// is not positive and finite) for the model radius's share.
    pub height: Option<f64>,
    scale: f64,
    fall: f64,
}

impl Walk {
    /// A walker standing where `camera` is, looking where it looks, turning
    /// about `axes`' up; `radius` is the model's ([`crate::radius`]).
    pub fn from_camera(camera: &Camera, axes: Axes, radius: f64) -> Self {
        Self::standing(camera.eye, span(camera.eye, camera.target), axes, radius)
    }

    /// A walker standing at `eye`, looking along `look`, turning about
    /// `axes`' up; `radius` is the model's.
    pub fn standing(eye: [f64; 3], look: [f64; 3], axes: Axes, radius: f64) -> Self {
        let (yaw, pitch) = axes.angles(look).unwrap_or((0.0, 0.0));
        Self {
            eye,
            yaw,
            look_yaw: 0.0,
            pitch,
            zoom: 1.0,
            axes,
            height: None,
            scale: radius,
            fall: 0.0,
        }
    }

    /// The axes the walker turns in.
    pub fn axes(&self) -> Axes {
        self.axes
    }

    /// The eye's height above a floor, model units.
    pub fn body(&self) -> f64 {
        self.height
            .filter(|h| h.is_finite() && *h > 0.0)
            .unwrap_or(self.scale * BODY_SHARE)
    }

    /// Whether it is still falling, so the picture must keep redrawing.
    pub fn falling(&self) -> bool {
        self.fall != 0.0
    }

    /// The unit direction the camera looks.
    pub fn direction(&self) -> [f64; 3] {
        self.axes.look(self.yaw + self.look_yaw, self.pitch)
    }

    /// The camera for a picture of `aspect` (width / height; one that is
    /// not positive and finite counts as 1) through `lens`. The field is the
    /// zoom's, no wider than the lens reaches. The target is a few eye
    /// heights ahead, brought nearer until it is finite.
    pub fn camera(&self, aspect: f64, lens: &Lens) -> Camera {
        let d = self.direction();
        let aspect = if aspect.is_finite() && aspect > 0.0 {
            aspect
        } else {
            1.0
        };
        let natural = (FOV / self.zoom).min(lens.widest());
        let fisheye = lens.fisheye_at(natural);
        Camera {
            eye: self.eye,
            target: ahead_of(self.eye, d, self.body() * 4.0),
            up: self.axes.up(),
            projection: lens.projection(fov_y(natural, aspect, fisheye)),
        }
    }

    /// Turn the body by `yaw` and the camera's pitch by `pitch` radians;
    /// pitch stops short of straight up or down.
    pub fn turn(&mut self, yaw: f64, pitch: f64) {
        self.yaw = wrap(self.yaw + yaw);
        self.tilt(pitch);
    }

    /// Turn the camera alone by `yaw` and `pitch` radians, the heading kept.
    pub fn look(&mut self, yaw: f64, pitch: f64) {
        self.look_yaw = wrap(self.look_yaw + yaw);
        self.tilt(pitch);
    }

    fn tilt(&mut self, pitch: f64) {
        self.pitch = (self.pitch + pitch).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    /// Turn the camera back to the heading.
    pub fn centre_look(&mut self) {
        self.look_yaw = 0.0;
    }

    /// Multiply the camera's zoom by `factor`, out no wider than `lens`
    /// reaches and in no further than six times the walking view. A factor
    /// that is not positive and finite changes nothing.
    pub fn zoom_by(&mut self, factor: f64, lens: &Lens) {
        if factor.is_finite() && factor > 0.0 {
            self.zoom = (self.zoom * factor).clamp(FOV / lens.widest(), ZOOM_IN);
        }
    }

    /// Look and head along `direction`; a zero one changes nothing.
    pub fn face(&mut self, direction: [f64; 3]) {
        if let Some((yaw, pitch)) = self.axes.angles(direction) {
            (self.yaw, self.look_yaw, self.pitch) = (yaw, 0.0, pitch);
        }
    }

    /// Move for `dt` seconds of `intent`. `world` gives the model's nearest
    /// hit along a ray, at any distance ([`crate::Solids::hit`]); it is asked
    /// only when solid. A solid walker ignores [`Intent::rise`], so it can
    /// never lift itself over what it would meet. A move that would not
    /// leave the eye finite is not made.
    ///
    /// A solid step longer than the eye height is made in parts no longer
    /// than it, up to 64, since the floor is only looked at where a part
    /// ends: pace is a share of the model and a set eye height is not. Past
    /// that cap, in a long enough step, a floor can still be stepped over.
    /// `world` is never asked about a ray that is not finite: that counts as
    /// a miss.
    pub fn step(
        &mut self,
        intent: Intent,
        dt: f64,
        rules: Rules,
        world: &mut impl FnMut(&Ray) -> Option<Hit>,
    ) {
        let world = &mut finite_rays(world);
        let pace = self.scale * PACE * rules.speed * if intent.run { RUN } else { 1.0 };
        let [forward, right] = [intent.forward * pace * dt, intent.right * pace * dt];
        if !rules.solid {
            self.travel([forward, right], rules, world);
            self.move_to(along(self.eye, self.axes.up(), intent.rise * pace * dt));
            return;
        }
        let parts = self.parts(forward.hypot(right));
        let share = 1.0 / f64::from(parts);
        for _ in 0..parts {
            let from = self.eye;
            self.travel([forward * share, right * share], rules, world);
            self.settle(dt * share, length(sub(self.eye, from)), world);
        }
    }

    /// How many parts of at most one eye height make up `distance`.
    fn parts(&self, distance: f64) -> u32 {
        (1..MOST_PARTS)
            .find(|parts| distance <= self.body() * f64::from(*parts))
            .unwrap_or(MOST_PARTS)
    }

    /// Move `forward` and `right` model units, sliding along any wall met
    /// when solid. `world` is never asked about a ray that is not finite.
    pub fn travel(
        &mut self,
        [forward, right]: [f64; 2],
        rules: Rules,
        world: &mut impl FnMut(&Ray) -> Option<Hit>,
    ) {
        let world = &mut finite_rays(world);
        let up = self.axes.up();
        let flat = self.axes.look(self.yaw, 0.0);
        let ahead = if rules.solid || rules.level {
            flat
        } else {
            self.axes.look(self.yaw, self.pitch)
        };
        let side = cross(flat, up);
        let mut motion = add(scale(ahead, forward), scale(side, right));
        if !motion.iter().all(|c| c.is_finite()) {
            return;
        }
        if rules.solid {
            motion = self.slide(motion, rules.step(), world);
            if self.lifts(motion, rules.step(), world) {
                motion = [0.0; 3];
            }
        }
        self.move_to(add(self.eye, motion));
    }

    fn move_to(&mut self, eye: [f64; 3]) {
        if eye.iter().all(|c| c.is_finite()) {
            self.eye = eye;
        }
    }

    /// The rise per unit of horizontal travel of a walkable floor; 0 for any
    /// other surface.
    fn slope(&self, floor: &Hit) -> f64 {
        let flat = dot(floor.normal, self.axes.up()).abs();
        if flat < WALKABLE {
            0.0
        } else {
            (1.0 - flat * flat).max(0.0).sqrt() / flat
        }
    }

    /// Whether nothing lies within `rise` and [`HEAD_CLEARANCE`] straight above
    /// `origin`, ignoring a surface the point is exactly on, so a walker
    /// placed on the floor can stand up from it.
    fn head_room(
        &self,
        origin: [f64; 3],
        rise: f64,
        world: &mut impl FnMut(&Ray) -> Option<Hit>,
    ) -> bool {
        world(&Ray {
            origin,
            direction: self.axes.up(),
            min_distance: 1e-9,
        })
        .is_none_or(|hit| hit.distance > rise + self.body() * HEAD_CLEARANCE)
    }

    /// Whether the eye's straight way to `to` is clear. A plate narrower than
    /// a stride lies between the two ends.
    fn way_clear(&self, to: [f64; 3], world: &mut impl FnMut(&Ray) -> Option<Hit>) -> bool {
        let way = sub(to, self.eye);
        let Some(direction) = normalize(way) else {
            return true;
        };
        world(&Ray {
            origin: self.eye,
            direction,
            min_distance: 1e-9,
        })
        .is_none_or(|hit| hit.distance > length(way))
    }

    /// Whether `motion` must not be made because of the ground at its far
    /// end: higher than here, and either rising by more than the walkable
    /// floors under its two ends explain (at least `step` of its height) or
    /// standing the eye up into something above or on its way there. The wall
    /// rays see only two heights, so a slab between them passes them; the
    /// floor under the destination does not. Both ends count: at a ramp's
    /// foot only the far floor is tilted, and over a crest onto a gentler
    /// slope only the near one explains the rise. Ground no higher than here
    /// is never refused, so a walker under a ceiling lower than its height
    /// can walk out.
    fn lifts(
        &self,
        motion: [f64; 3],
        step: f64,
        world: &mut impl FnMut(&Ray) -> Option<Hit>,
    ) -> bool {
        let down = scale(self.axes.up(), -1.0);
        let to = add(self.eye, motion);
        let here = world(&Ray {
            origin: self.eye,
            direction: down,
            min_distance: 0.0,
        });
        let Some(there) = world(&Ray {
            origin: to,
            direction: down,
            min_distance: 0.0,
        }) else {
            return false;
        };
        if here
            .as_ref()
            .is_some_and(|floor| there.distance >= floor.distance - 1e-9)
        {
            return false;
        }
        let slope = here
            .as_ref()
            .map_or(0.0, |floor| self.slope(floor))
            .max(self.slope(&there));
        let allowed = (step * self.body()).max(length(motion) * slope);
        let rise = self.body() - there.distance;
        let up = self.axes.up();
        rise > allowed + 1e-9
            || (rise > 0.0
                && !(self.head_room(to, rise, world) && self.way_clear(along(to, up, rise), world)))
    }

    /// `motion` cut where it would bring the eye or the top of a step within
    /// reach of a wall: the part into the wall dropped, then stopped
    /// outright if the rest meets one too.
    fn slide(
        &self,
        motion: [f64; 3],
        step: f64,
        world: &mut impl FnMut(&Ray) -> Option<Hit>,
    ) -> [f64; 3] {
        let Some(wall) = self.blocked(motion, step, world) else {
            return motion;
        };
        let along = sub(motion, scale(wall, dot(motion, wall)));
        if self.blocked(along, step, world).is_some() {
            [0.0; 3]
        } else {
            along
        }
    }

    /// The horizontal unit normal, facing the walker, of the nearest wall
    /// `motion` comes within reach of, at eye height or `step` of it above
    /// the floor; `None` when the way is clear. A floor or ramp met is no
    /// wall: [`Walk::settle`] stands on it.
    fn blocked(
        &self,
        motion: [f64; 3],
        step: f64,
        world: &mut impl FnMut(&Ray) -> Option<Hit>,
    ) -> Option<[f64; 3]> {
        let length = pdfcer_3d::vec3::length(motion);
        let direction = normalize(motion)?;
        let reach = self.body() * REACH;
        let up = self.axes.up();
        let knee = along(self.eye, up, -self.body() * (1.0 - step));
        [self.eye, knee]
            .into_iter()
            .filter_map(|origin| {
                world(&Ray {
                    origin,
                    direction,
                    min_distance: 0.0,
                })
            })
            .filter(|hit| hit.distance < length + reach && dot(hit.normal, up).abs() < WALKABLE)
            .min_by(|a, b| a.distance.total_cmp(&b.distance))
            .map(|hit| {
                let flat = sub(hit.normal, scale(up, dot(hit.normal, up)));
                let n = normalize(flat).unwrap_or(scale(direction, -1.0));
                if dot(n, direction) > 0.0 {
                    scale(n, -1.0)
                } else {
                    n
                }
            })
    }

    /// Stand on the floor under the eye, falling onto it from above it and
    /// stepping up onto it from below, unless something above is in the way.
    /// A walker not already falling that has `moved` this far stays on a
    /// slope going down, when the floor's slope accounts for the drop. With
    /// no floor under it the walker keeps its height.
    fn settle(&mut self, dt: f64, moved: f64, world: &mut impl FnMut(&Ray) -> Option<Hit>) {
        let up = self.axes.up();
        let floor = world(&Ray {
            origin: self.eye,
            direction: scale(up, -1.0),
            min_distance: 0.0,
        });
        let Some(floor) = floor else {
            self.fall = 0.0;
            return;
        };
        let above = floor.distance - self.body();
        if above <= 0.0 {
            if self.head_room(self.eye, -above, world) {
                self.move_to(along(self.eye, up, -above));
            }
            self.fall = 0.0;
            return;
        }
        if self.fall == 0.0 && above <= moved * self.slope(&floor) + 1e-9 {
            self.move_to(along(self.eye, up, -above));
            return;
        }
        let fall = self.fall + GRAVITY * self.body() * dt;
        let drop = (fall * dt).min(above);
        // A NaN or infinite step leaves the walker where it is, at rest.
        if !(drop.is_finite() && fall.is_finite()) {
            self.fall = 0.0;
            return;
        }
        self.fall = if drop >= above { 0.0 } else { fall };
        self.move_to(along(self.eye, up, -drop));
    }

    /// Go to `hit`, the surface under a tapped point seen from `from`:
    /// standing on it when it faces up, else a step back from it.
    pub fn go_to(&mut self, hit: &Hit, from: [f64; 3]) {
        let up = self.axes.up();
        let back = normalize(span(hit.point, from)).unwrap_or(up);
        let facing = if dot(hit.normal, back) < 0.0 {
            scale(hit.normal, -1.0)
        } else {
            hit.normal
        };
        let eye = if dot(facing, up) > WALKABLE {
            along(hit.point, up, self.body())
        } else {
            along(hit.point, back, self.body())
        };
        self.move_to(eye);
        self.fall = 0.0;
    }

    /// Level the look.
    pub fn level(&mut self) {
        self.pitch = 0.0;
    }
}

/// `angle` from -π to π, for any finite angle: the remainder is exact, where
/// subtracting a multiple of a turn loses it to rounding on a large angle.
fn wrap(angle: f64) -> f64 {
    use std::f64::consts::{PI, TAU};
    (angle + PI).rem_euclid(TAU) - PI
}

fn along(point: [f64; 3], direction: [f64; 3], distance: f64) -> [f64; 3] {
    add(point, scale(direction, distance))
}

/// The point `distance` ahead of `eye` along unit `d`, brought nearer
/// (halving) until it is finite; the eye itself when none is.
fn ahead_of(eye: [f64; 3], d: [f64; 3], distance: f64) -> [f64; 3] {
    let mut distance = distance.min(f64::MAX);
    // Halving from the largest float reaches any representable gap.
    for _ in 0..2100 {
        let target = along(eye, d, distance);
        if target.iter().all(|c| c.is_finite()) {
            return target;
        }
        distance /= 2.0;
    }
    eye
}

/// `natural`'s vertical field, narrowed past [`WIDEST`] so the sideways
/// field stops growing with the window's width. A fisheye's sideways field
/// is its vertical one times the aspect, so it narrows in proportion; a
/// flat one's tangent does.
fn fov_y(natural: f64, aspect: f64, fisheye: bool) -> f64 {
    if aspect <= WIDEST {
        return natural;
    }
    if fisheye {
        return natural * WIDEST / aspect;
    }
    let half = (natural.to_radians() / 2.0).tan() * WIDEST / aspect;
    2.0 * half.atan().to_degrees()
}

#[cfg(test)]
#[path = "walk_tests.rs"]
mod tests;
