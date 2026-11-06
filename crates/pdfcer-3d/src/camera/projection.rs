//! How a [`crate::Camera`] maps the scene onto the image.

/// How the camera projects the scene onto the image.
///
/// `Perspective` and `Orthographic` are linear: straight lines stay
/// straight, and a wide perspective stretches the picture's edges by about
/// 1/cos² of the angle off the axis. `Fisheye` is a curved lens that sees
/// up to the whole sphere without that stretch, bending straight lines
/// that do not pass through the centre.
///
/// Only `Perspective` and `Orthographic` can be written into a PDF 3D view
/// (ISO 32000-1 Table 305, `/P` `/Subtype` is `O` or `P`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Projection {
    /// A pinhole camera with this vertical field of view, in degrees
    /// (exclusive range 0–180).
    Perspective {
        /// Vertical field of view, degrees.
        fov_y: f64,
    },
    /// A parallel projection showing this much of the scene vertically, in
    /// model units.
    Orthographic {
        /// Visible height, model units.
        height: f64,
    },
    /// A fisheye lens: a pixel's angle off the view direction is a function
    /// of its distance from the image centre alone, so the picture is round
    /// about its centre.
    ///
    /// `fov` is the angle, in degrees, between the directions drawn at the
    /// middle of the image's top and bottom edges: the angle across the
    /// picture's **height**, as `Perspective::fov_y` measures it, so
    /// switching lens keeps the content at those edges. The width shows
    /// `fov` × width / height under [`FisheyeMapping::Equidistant`] and
    /// a little less under [`FisheyeMapping::Stereographic`]. Range: above
    /// 0 and up to 360 for `Equidistant`, below 360 for `Stereographic`;
    /// anything else (or not finite) is refused with
    /// [`RenderError::FisheyeField`](crate::RenderError::FisheyeField).
    ///
    /// The lens sees every direction but the one straight behind the eye.
    /// `Equidistant` maps directions up to 180° off the axis onto a disc;
    /// a pixel outside it (beyond 180°, reached when the corners or edges
    /// lie past the disc) is drawn in the background colour and has no
    /// [`Camera::ray_through`](crate::Camera::ray_through) ray.
    /// `Stereographic` maps the whole sphere onto the plane, so it always
    /// fills the frame.
    ///
    /// How it is drawn, and its cost, is [`crate::FrameLens`]'s contract.
    Fisheye {
        /// Angle across the picture's height, degrees.
        fov: f64,
        /// The lens curve.
        mapping: FisheyeMapping,
    },
}

/// The curve of a [`Projection::Fisheye`] lens: how a direction θ off the
/// view axis maps to a distance r from the image centre, `f` the lens's
/// scale in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum FisheyeMapping {
    /// r = f·θ: equal angles take equal distances, the classic fisheye.
    /// Squeezes shapes radially towards the rim.
    #[default]
    Equidistant,
    /// r = 2f·tan(θ/2): conformal, so small shapes stay round anywhere in
    /// the picture; the least stretched-looking for buildings and rooms.
    /// Enlarges towards the rim instead.
    Stereographic,
}

impl FisheyeMapping {
    /// r / f for a direction `theta` radians off the axis.
    pub(crate) fn radius(self, theta: f64) -> f64 {
        match self {
            FisheyeMapping::Equidistant => theta,
            FisheyeMapping::Stereographic => 2.0 * (theta / 2.0).tan(),
        }
    }

    /// The angle off the axis, radians, drawn at `r` / f = `ratio`; `None`
    /// past the equidistant disc (θ > π).
    pub(crate) fn angle(self, ratio: f64) -> Option<f64> {
        match self {
            FisheyeMapping::Equidistant => (ratio <= std::f64::consts::PI).then_some(ratio),
            FisheyeMapping::Stereographic => Some(2.0 * (ratio / 2.0).atan()),
        }
    }

    /// The most pixels per radian, radially or around, the lens spends at
    /// `theta` off the axis, per unit f.
    pub(crate) fn density(self, theta: f64) -> f64 {
        match self {
            // Around: r / sin θ = θ / sin θ, at least the radial 1.
            FisheyeMapping::Equidistant if theta > 0.0 => (theta / theta.sin()).max(1.0),
            FisheyeMapping::Equidistant => 1.0,
            // Conformal: sec²(θ/2) both ways.
            FisheyeMapping::Stereographic => 1.0 / (theta / 2.0).cos().powi(2),
        }
    }

    /// The code the lens shaders take as `u_lens_mapping`.
    pub(crate) fn code(self) -> i32 {
        match self {
            FisheyeMapping::Equidistant => 0,
            FisheyeMapping::Stereographic => 1,
        }
    }

    /// Whether `fov` degrees is a field this lens draws.
    pub(crate) fn accepts(self, fov: f64) -> bool {
        fov.is_finite()
            && fov > 0.0
            && match self {
                FisheyeMapping::Equidistant => fov <= 360.0,
                FisheyeMapping::Stereographic => fov < 360.0,
            }
    }
}
