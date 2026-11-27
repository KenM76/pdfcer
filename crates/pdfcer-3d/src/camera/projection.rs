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
    /// 0 and up to 360 for `Equidistant`, below 360 for `Stereographic`,
    /// exactly 360 for [`FisheyeMapping::Equirectangular`]; anything else
    /// (or not finite) is refused with
    /// [`RenderError::FisheyeField`](crate::RenderError::FisheyeField).
    ///
    /// The lens sees every direction but the one straight behind the eye.
    /// `Equidistant` maps directions up to 180° off the axis onto a disc;
    /// a pixel outside it (beyond 180°, reached when the corners or edges
    /// lie past the disc) is drawn in the background colour and has no
    /// [`Camera::ray_through`](crate::Camera::ray_through) ray.
    /// `Stereographic` maps the whole sphere onto the plane, so it always
    /// fills the frame. `Equirectangular` is not round about the centre:
    /// it is the 360° panorama its own documentation describes.
    ///
    /// How it is drawn, and its cost, is [`crate::FrameLens`]'s contract.
    Fisheye {
        /// Angle across the picture's height, degrees.
        fov: f64,
        /// The lens curve.
        mapping: FisheyeMapping,
    },
    /// A part of the sphere round the eye: `horizontal` degrees of
    /// longitude, centred on the look direction and turning about the
    /// camera's up, by `vertical` degrees of latitude centred `pitch`
    /// degrees above the camera's horizon (the plane square to its up).
    /// Pixel position (x, y) of a w × h picture shows longitude
    /// λ = (x/w − ½)·`horizontal` to the right of the look direction; its
    /// latitude is as [`PanoramaMapping`] states. The horizon is a straight
    /// row wherever it falls in the picture.
    ///
    /// `horizontal` is above 0 and at most 360, `vertical` above 0 and at
    /// most 180, `pitch` finite. Under
    /// [`PanoramaMapping::Equirectangular`] a band reaching past a pole is
    /// moved, keeping its height, until it ends there: the centre latitude
    /// drawn is `pitch` clamped to ±(90 − `vertical`/2). Under
    /// [`PanoramaMapping::Cylindrical`] the band must lie strictly between
    /// the poles (`|pitch| + vertical/2 < 90`). Anything else is refused
    /// with [`RenderError::PanoramaField`](crate::RenderError::PanoramaField).
    /// [`Camera::panorama`](crate::Camera::panorama) builds a levelled or
    /// tilted one from a view; 360 × 180 at pitch 0 is the whole sphere
    /// [`FisheyeMapping::Equirectangular`] draws.
    ///
    /// Only the band is drawn: the picture's pixels, and the cube faces
    /// that own one of them ([`crate::FrameLens`]), not the sphere. The
    /// GPU lens ([`crate::FrameLens::new`]) draws only the whole sphere and
    /// refuses a smaller band; the CPU renderer ([`crate::render`]) draws
    /// any.
    Panorama {
        /// Longitude across the picture's width, degrees.
        horizontal: f64,
        /// Latitude across the picture's height, degrees.
        vertical: f64,
        /// Latitude of the band's middle above the camera's horizon,
        /// degrees.
        pitch: f64,
        /// How latitude runs down the picture.
        mapping: PanoramaMapping,
    },
}

/// How a [`Projection::Panorama`] spaces latitude down the picture, the
/// band's top row at latitude t = centre + `vertical`/2 and its bottom at
/// b = centre − `vertical`/2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum PanoramaMapping {
    /// Rows evenly spaced in latitude: y/h runs from t at the top to b at
    /// the bottom. Pixels are square when w/h = `horizontal`/`vertical`.
    /// The photo-sphere tag describes this projection.
    #[default]
    Equirectangular,
    /// Rows evenly spaced in tan(latitude), from tan t at the top to tan b
    /// at the bottom: the picture a camera swung round a vertical axis
    /// takes, so vertical lines stay straight and nothing near the horizon
    /// is squeezed, at the cost of stretching towards the top and bottom.
    /// Pixels are square at the horizon when
    /// w/h = `horizontal` (radians) / (tan t − tan b). No photo-sphere tag
    /// describes it.
    Cylindrical,
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
    /// The full sphere as a 360° × 180° panorama, the picture photo
    /// viewers open as a photo sphere you can look around in: longitude
    /// runs across the width, latitude up the height. Pixel position (x,
    /// y) of a w × h picture shows longitude λ = (x/w − ½)·2π to the
    /// camera's right of its look direction and latitude φ = (½ − y/h)·π
    /// above it, so the picture's centre is the look direction, its top
    /// and bottom rows are straight up and down along the camera's up, and
    /// its left and right edges meet straight behind the eye.
    ///
    /// Not radial, so "r from the centre" does not apply. The field must
    /// be exactly 360; the picture should be twice as wide as it is tall,
    /// which is what viewers expect, but any size draws (the panorama is
    /// stretched to fit). Every pixel has a direction and a
    /// [`Camera::ray_through`](crate::Camera::ray_through) ray. Lines and
    /// wires are cut where they cross the left/right seam rather than drawn
    /// across the picture.
    Equirectangular,
}

impl FisheyeMapping {
    /// r / f for a direction `theta` radians off the axis. The panorama
    /// is not radial; callers branch on it first, and it reads as
    /// equidistant here.
    pub(crate) fn radius(self, theta: f64) -> f64 {
        match self {
            FisheyeMapping::Equidistant | FisheyeMapping::Equirectangular => theta,
            FisheyeMapping::Stereographic => 2.0 * (theta / 2.0).tan(),
        }
    }

    /// The angle off the axis, radians, drawn at `r` / f = `ratio`; `None`
    /// past the equidistant disc (θ > π). As [`Self::radius`] for the
    /// panorama.
    pub(crate) fn angle(self, ratio: f64) -> Option<f64> {
        match self {
            FisheyeMapping::Equidistant | FisheyeMapping::Equirectangular => {
                (ratio <= std::f64::consts::PI).then_some(ratio)
            }
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
            // A row at latitude φ spends f / cos φ around, unbounded at the
            // poles, so the faces take the size cap as the equidistant rim does.
            FisheyeMapping::Equirectangular => f64::INFINITY,
        }
    }

    /// The code the lens shaders take as `u_lens_mapping`.
    pub(crate) fn code(self) -> i32 {
        match self {
            FisheyeMapping::Equidistant => 0,
            FisheyeMapping::Stereographic => 1,
            FisheyeMapping::Equirectangular => 2,
        }
    }

    /// Whether `fov` degrees is a field this lens draws.
    pub(crate) fn accepts(self, fov: f64) -> bool {
        fov.is_finite()
            && fov > 0.0
            && match self {
                FisheyeMapping::Equidistant => fov <= 360.0,
                FisheyeMapping::Stereographic => fov < 360.0,
                FisheyeMapping::Equirectangular => fov == 360.0,
            }
    }
}
