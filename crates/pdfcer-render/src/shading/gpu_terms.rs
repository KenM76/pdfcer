//! An axial or radial shading's per-pixel arithmetic as constants, for the
//! graphics card to evaluate. Each constant is computed exactly as
//! [`super::paint_region`] computes it, so the card's per-pixel steps take
//! the same rounded inputs.

use super::{ColorRamp, Geometry, RAMP_SAMPLES, Shading};

/// Which parameter function a shading uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Axial,
    Radial,
}

/// The constants [`super::axial_param`] or [`super::radial_param`] and
/// [`ColorRamp::index_of`] use, and the ramp's samples.
#[derive(Debug, Clone)]
pub(crate) struct Terms {
    pub kind: Kind,
    pub extend: [bool; 2],
    /// `/BBox` as `[x_min, x_max, y_min, y_max]`.
    pub bbox: Option<[f32; 4]>,
    /// `[x0, y0, t0, t1]`.
    pub origin: [f32; 4],
    /// Axial `[dx, dy, 1 / (dx² + dy²), t1 − t0]`; radial `[dx, dy, dr, r0]`.
    pub axis: [f32; 4],
    /// Radial `[a, 4a, 1 / 2a, t1 − t0]`; zero for axial.
    pub cone: [f32; 4],
    /// The ramp's `t0` and `1 / span`, `None` for a zero span.
    pub ramp_scale: (f32, Option<f32>),
    /// Per sample: its straight unit colour and its opaque bytes.
    pub samples: Vec<Option<([f32; 3], [u8; 3])>>,
}

impl Shading {
    /// The terms for the graphics card, or `None` for a shading it does not
    /// evaluate: a mesh, type 1, a 45° cone, or one that paints nothing.
    pub(crate) fn gpu_terms(&self) -> Option<Terms> {
        let ramp = self.ramp.as_ref()?;
        if self.mesh.is_some() {
            return None;
        }
        let (kind, extend, origin, axis, cone) = match self.geometry {
            Geometry::Axial {
                coords: [x0, y0, x1, y1],
                domain: [t0, t1],
                extend,
            } => {
                let (dx, dy) = (x1 - x0, y1 - y0);
                let denom = dx * dx + dy * dy;
                if denom <= f32::EPSILON {
                    return None;
                }
                let axis = [dx, dy, 1.0 / denom, t1 - t0];
                (Kind::Axial, extend, [x0, y0, t0, t1], axis, [0.0; 4])
            }
            Geometry::Radial {
                coords: [x0, y0, r0, x1, y1, r1],
                domain: [t0, t1],
                extend,
            } => {
                if r0 <= 0.0 && r1 <= 0.0 {
                    return None;
                }
                let (dx, dy, dr) = (x1 - x0, y1 - y0, r1 - r0);
                let a = dx * dx + dy * dy - dr * dr;
                if a.abs() <= f32::EPSILON {
                    return None;
                }
                let cone = [a, 4.0 * a, 1.0 / (2.0 * a), t1 - t0];
                (
                    Kind::Radial,
                    extend,
                    [x0, y0, t0, t1],
                    [dx, dy, dr, r0],
                    cone,
                )
            }
            _ => return None,
        };
        let bbox = self
            .bbox
            .map(|[x0, y0, x1, y1]| [x0.min(x1), x0.max(x1), y0.min(y1), y0.max(y1)]);
        Some(Terms {
            kind,
            extend,
            bbox,
            origin,
            axis,
            cone,
            ramp_scale: ramp_scale(ramp),
            samples: samples(ramp),
        })
    }
}

fn ramp_scale(ramp: &ColorRamp) -> (f32, Option<f32>) {
    let [t0, t1] = ramp.domain;
    let span = t1 - t0;
    (t0, (span.abs() >= f32::EPSILON).then(|| 1.0 / span))
}

fn samples(ramp: &ColorRamp) -> Vec<Option<([f32; 3], [u8; 3])>> {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = |src: f32| (src * 255.0).round().clamp(0.0, 255.0) as u8;
    let mut v: Vec<_> = ramp
        .samples
        .iter()
        .map(|s| s.map(|c| ([c.r, c.g, c.b], [byte(c.r), byte(c.g), byte(c.b)])))
        .collect();
    v.resize(RAMP_SAMPLES, None);
    v
}
