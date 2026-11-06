//! A view saved in the file, as navigation needs it: where its camera stands
//! and looks, and the framing an orthographic one fixes. The shell fills
//! these from the document's own view type, so this crate needs no PDF
//! reader.

use std::fmt;

use pdfcer_3d::vec3::{add, dot, normalize, scale, span, sub};
use pdfcer_3d::{Bounds, Camera, Projection};

use crate::reach::distance;

/// A saved view's camera: the direction it looks along, its up, where it
/// stands, and how it projects.
///
/// From a PDF 3D view (ISO 32000-1 §13.6.4, `/C2W`): `direction` is the
/// camera-to-world matrix's z column, `up` its y column, and `position`
/// its translation, all in model units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SavedAim {
    /// The direction the camera looks along; need not be unit length.
    pub direction: [f64; 3],
    /// The direction that appears upward; need not be unit length.
    pub up: [f64; 3],
    /// Where the camera stands.
    pub position: [f64; 3],
    /// Whether the view is orthographic.
    pub orthographic: bool,
    /// Whether an orthographic view fixes its own centre and scale
    /// ([`ViewFrame`]) rather than fitting the model.
    pub framed: bool,
}

/// What an orthographic saved view fixes, for one picture aspect: the
/// point its view line passes through and the height it shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewFrame {
    /// A point on the view's line of sight, model units.
    pub position: [f64; 3],
    /// The visible height, model units.
    pub height: f64,
}

/// Why a saved view's camera is not a place to stand ([`SavedAim::eye_inside`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NotInside {
    /// The view is orthographic: it has a line of sight but no standing
    /// point.
    NoStandingPoint,
    /// The camera stands outside the model's box (or not at a finite point).
    Outside,
}

impl NotInside {
    /// A short token naming the reason: `none` or `outside`.
    pub fn as_str(self) -> &'static str {
        match self {
            NotInside::NoStandingPoint => "none",
            NotInside::Outside => "outside",
        }
    }
}

impl fmt::Display for NotInside {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            NotInside::NoStandingPoint => "an orthographic view has no standing point",
            NotInside::Outside => "the camera stands outside the model",
        })
    }
}

impl std::error::Error for NotInside {}

impl SavedAim {
    /// Where the camera stands, when that is inside `bounds` (each
    /// coordinate within the box, ends included) and the view is
    /// perspective: a view made from inside a room walks from there rather
    /// than orbiting from outside.
    ///
    /// # Errors
    ///
    /// [`NotInside::NoStandingPoint`] for an orthographic view;
    /// [`NotInside::Outside`] when the position is outside the box or not
    /// finite.
    pub fn eye_inside(&self, bounds: &Bounds) -> Result<[f64; 3], NotInside> {
        if self.orthographic {
            return Err(NotInside::NoStandingPoint);
        }
        let inside = self
            .position
            .iter()
            .zip(bounds.min.iter().zip(&bounds.max))
            .all(|(p, (lo, hi))| lo <= p && p <= hi);
        if inside {
            Ok(self.position)
        } else {
            Err(NotInside::Outside)
        }
    }
}

impl ViewFrame {
    /// `camera` moved along its line of sight's normal plane so the line
    /// passes through [`ViewFrame::position`], keeping its direction and
    /// its distance from the target, and made orthographic at
    /// [`ViewFrame::height`].
    ///
    /// Leaves the camera unchanged when the frame or the camera is
    /// degenerate: a position that is not finite, a height that is not
    /// positive and finite, an eye on the target, or a move that would put
    /// the target or eye beyond the largest float.
    pub fn apply(&self, camera: &mut Camera) {
        if !(self.height.is_finite() && self.height > 0.0) {
            return;
        }
        let Some(dir) = normalize(span(camera.eye, camera.target)) else {
            return;
        };
        let Some(back) = distance(camera.eye, camera.target) else {
            return;
        };
        // The target's depth along the look from the position, halved first
        // so the difference cannot overflow.
        let half = sub(scale(camera.target, 0.5), scale(self.position, 0.5));
        let along = 2.0 * dot(half, dir);
        let target = add(self.position, scale(dir, along));
        let eye = sub(target, scale(dir, back));
        if !target.iter().chain(&eye).all(|c| c.is_finite()) {
            return;
        }
        camera.target = target;
        camera.eye = eye;
        camera.projection = Projection::Orthographic {
            height: self.height,
        };
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn aim(eye: [f64; 3], orthographic: bool) -> SavedAim {
        SavedAim {
            direction: [0.0, 1.0, 0.0],
            up: [0.0, 0.0, 1.0],
            position: eye,
            orthographic,
            framed: false,
        }
    }

    #[test]
    fn a_camera_in_the_box_is_inside_and_one_beyond_it_is_not() {
        let bounds = Bounds {
            min: [0.0; 3],
            max: [10.0, 10.0, 3.0],
        };
        let inside = aim([5.0, 5.0, 1.6], false).eye_inside(&bounds);
        assert_eq!(inside, Ok([5.0, 5.0, 1.6]));
        let outside = aim([5.0, -20.0, 1.6], false).eye_inside(&bounds);
        assert_eq!(outside, Err(NotInside::Outside));
        let flat = aim([5.0, 5.0, 1.6], true).eye_inside(&bounds);
        assert_eq!(flat, Err(NotInside::NoStandingPoint));
        let nan = aim([f64::NAN, 5.0, 1.6], false).eye_inside(&bounds);
        assert_eq!(nan, Err(NotInside::Outside));
        assert_eq!(NotInside::Outside.as_str(), "outside");
    }

    #[test]
    fn a_frame_moves_the_line_of_sight_through_its_point() {
        let mut camera = Camera {
            eye: [0.0, -5.0, 0.0],
            target: [0.0; 3],
            up: [0.0, 0.0, 1.0],
            projection: Projection::Perspective { fov_y: 30.0 },
        };
        let frame = ViewFrame {
            position: [2.0, 7.0, -1.0],
            height: 4.0,
        };
        frame.apply(&mut camera);
        assert_eq!(camera.target, [2.0, 0.0, -1.0]);
        assert_eq!(camera.eye, [2.0, -5.0, -1.0]);
        assert_eq!(camera.projection, Projection::Orthographic { height: 4.0 });
    }

    #[test]
    fn a_frame_at_the_edge_of_the_floats_leaves_the_camera_finite() {
        let start = Camera {
            eye: [0.0, -f64::MAX, 0.0],
            target: [0.0, f64::MAX, 0.0],
            up: [0.0, 0.0, 1.0],
            projection: Projection::Orthographic { height: 1.0 },
        };
        for position in [[f64::MAX, -f64::MAX, 0.0], [0.0, f64::MAX, f64::MAX]] {
            let mut camera = start;
            ViewFrame {
                position,
                height: 2.0,
            }
            .apply(&mut camera);
            let all = camera.eye.iter().chain(&camera.target);
            assert!(all.into_iter().all(|c| c.is_finite()), "{camera:?}");
        }
        let mut camera = start;
        ViewFrame {
            position: [0.0; 3],
            height: f64::NAN,
        }
        .apply(&mut camera);
        assert_eq!(camera, start);
    }
}
