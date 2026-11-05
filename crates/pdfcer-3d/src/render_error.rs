//! The error every drawing and camera call returns.

#[cfg(doc)]
use crate::Camera;
use crate::FisheyeMapping;

/// Why [`crate::render`] or [`Camera::fit`] refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RenderError {
    /// The image is empty or larger than [`crate::MAX_RENDER_PIXELS`].
    #[error("a {width}x{height} image is empty or larger than the render ceiling")]
    Size {
        /// Requested width.
        width: u32,
        /// Requested height.
        height: u32,
    },
    /// The camera cannot form a view.
    #[error("invalid camera: {0}")]
    Camera(&'static str),
    /// Drawing would test more pixel and texel centres than `limit`
    /// ([`crate::MAX_RENDER_PIXEL_TESTS`]).
    #[error("drawing the model would take more than {limit} pixel tests")]
    TooMuchWork {
        /// The ceiling passed.
        limit: u64,
    },
    /// A [`crate::Projection::Fisheye`] field of view outside what
    /// `mapping` draws: not finite, not above 0, or above 360° (at or
    /// above 360° for [`FisheyeMapping::Stereographic`], whose scale is
    /// infinite there).
    #[error("a {mapping:?} fisheye's field of view must be above 0 and within its range")]
    FisheyeField {
        /// The lens asked for.
        mapping: FisheyeMapping,
    },
    /// The pixel lies outside a fisheye lens's picture circle: no direction
    /// is drawn there ([`Camera::ray_through`]).
    #[error("the pixel lies outside the fisheye lens's picture circle")]
    OutsideLens,
    /// [`crate::RenderOptions::light_gain`] is not finite or lies outside
    /// [`crate::MIN_LIGHT_GAIN`]–[`crate::MAX_LIGHT_GAIN`].
    #[error("the light gain must be a number from 0.25 to 4")]
    LightGain,
}
