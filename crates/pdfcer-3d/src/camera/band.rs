//! The part of the sphere a panorama picture shows: the whole sphere of
//! [`FisheyeMapping::Equirectangular`](crate::FisheyeMapping::Equirectangular)
//! or the band of a [`Projection::Panorama`](crate::Projection::Panorama).
//!
//! A picture fraction (u, v), u across and v down, shows longitude
//! λ = (u − ½)·`lon_span` and, in the rows' coordinate (latitude, or
//! tan(latitude) when cylindrical), `mid` + (½ − v)·`span`.

use crate::render::{PanoramaMapping, RenderError};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

/// A panorama's band, in radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Band {
    /// Longitude across the width.
    lon_span: f64,
    /// The rows' coordinate at the picture's middle row, and across its
    /// height.
    mid: f64,
    span: f64,
    cylindrical: bool,
    /// Latitude of the top and bottom edges.
    top: f64,
    bottom: f64,
}

impl Band {
    /// The whole sphere, 360° × 180° about the horizon.
    pub(crate) const SPHERE: Band = Band {
        lon_span: TAU,
        mid: 0.0,
        span: PI,
        cylindrical: false,
        top: FRAC_PI_2,
        bottom: -FRAC_PI_2,
    };

    /// The band of a [`Projection::Panorama`](crate::Projection::Panorama)
    /// (its contract states the ranges and the pole clamp).
    pub(crate) fn new(
        horizontal: f64,
        vertical: f64,
        pitch: f64,
        mapping: PanoramaMapping,
    ) -> Result<Band, RenderError> {
        let refused = Err(RenderError::PanoramaField { mapping });
        let angles = horizontal.is_finite()
            && horizontal > 0.0
            && horizontal <= 360.0
            && vertical.is_finite()
            && vertical > 0.0
            && vertical <= 180.0
            && pitch.is_finite();
        if !angles {
            return refused;
        }
        let half = vertical / 2.0;
        let cylindrical = mapping == PanoramaMapping::Cylindrical;
        if cylindrical && pitch.abs() + half >= 90.0 {
            return refused;
        }
        if !cylindrical && horizontal == 360.0 && vertical == 180.0 {
            return Ok(Band::SPHERE);
        }
        let centre = pitch.clamp(half - 90.0, 90.0 - half);
        let (top, bottom) = ((centre + half).to_radians(), (centre - half).to_radians());
        let (hi, lo) = if cylindrical {
            (top.tan(), bottom.tan())
        } else {
            (top, bottom)
        };
        Ok(Band {
            lon_span: horizontal.to_radians(),
            mid: (hi + lo) / 2.0,
            span: hi - lo,
            cylindrical,
            top,
            bottom,
        })
    }

    /// Longitude and latitude shown at picture fraction (`u`, `v`).
    pub(crate) fn angles(&self, u: f64, v: f64) -> (f64, f64) {
        let row = self.mid + (0.5 - v) * self.span;
        let phi = if self.cylindrical { row.atan() } else { row };
        ((u - 0.5) * self.lon_span, phi)
    }

    /// The picture fraction showing camera-space direction (`x`, `y`, `z`);
    /// `None` where it is not finite (straight up or down, cylindrically).
    pub(crate) fn fractions(&self, [x, y, z]: [f64; 3]) -> Option<[f64; 2]> {
        let rho = x.hypot(z);
        let row = if self.cylindrical {
            y / rho
        } else {
            y.atan2(rho)
        };
        let out = [
            x.atan2(z) / self.lon_span + 0.5,
            0.5 - (row - self.mid) / self.span,
        ];
        out.iter().all(|c| c.is_finite()).then_some(out)
    }

    /// The width, in pixels, a full turn of longitude takes in a picture
    /// `width` wide.
    pub(crate) fn circle(&self, width: f64) -> f64 {
        width / (self.lon_span / TAU)
    }

    /// Pixels per radian across and down at the horizon, the denser.
    pub(crate) fn focal(&self, width: f64, height: f64) -> f64 {
        (width / self.lon_span).max(height / self.span)
    }

    /// The most pixels per radian the picture spends anywhere: around a row
    /// grows as 1/cos(latitude), and a cylindrical column as 1/cos².
    pub(crate) fn density(&self, width: f64, height: f64) -> f64 {
        let c = self.top.abs().max(self.bottom.abs()).cos();
        let down = if self.cylindrical {
            height / self.span / (c * c)
        } else {
            height / self.span
        };
        (width / self.lon_span / c).max(down)
    }

    /// The largest angle off the look direction the picture shows: at the
    /// side edges, at the latitude nearest the poles when they lie within
    /// 90° of the look direction, else nearest the horizon.
    pub(crate) fn max_angle(&self) -> f64 {
        let half = (self.lon_span / 2.0).min(PI);
        let lat = if half <= FRAC_PI_2 {
            self.top.abs().max(self.bottom.abs())
        } else if self.top >= 0.0 && self.bottom <= 0.0 {
            0.0
        } else {
            self.top.abs().min(self.bottom.abs())
        };
        (lat.cos() * half.cos()).clamp(-1.0, 1.0).acos()
    }

    /// The tangents across and up the picture reaches, when every direction
    /// it shows is within 90° of the look direction: x/z = tan λ, and
    /// y/z = tan φ / cos λ, both largest at the side edges.
    pub(crate) fn reach(&self) -> Option<(f64, f64)> {
        if self.max_angle() >= FRAC_PI_2 {
            return None;
        }
        let half = self.lon_span / 2.0;
        let up = self.top.tan().abs().max(self.bottom.tan().abs());
        Some((half.tan(), up / half.cos()))
    }
}
