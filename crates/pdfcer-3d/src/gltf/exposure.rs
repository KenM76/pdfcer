//! [`GltfLights::Exposure`](super::GltfLights::Exposure): one scale for
//! every light, chosen from what the lights do to the model's surfaces.
//!
//! A camera's automatic exposure meters the scene it sees; this meters the
//! whole model. Each sampled triangle's light is summed over every lamp
//! (inverse square past a floor distance, inside a spot's cone, no
//! occlusion or facing), and the scale puts the area-weighted median of the
//! lit triangles at [`TARGET`]. Lamps spread room by room through a
//! building then light their own rooms, where scaling their sum at one
//! point divides each by the number of lamps.

use super::lights::Placed;
use crate::{Matrix, SceneMesh, transform_point, vec3};

/// Where the median lit surface lands, before facing: the share the lights
/// carry beside the ambient light.
const TARGET: f64 = 1.0 - super::lights::AMBIENT;
/// The most triangles metered; a larger model is sampled evenly.
const MAX_SAMPLES: usize = 20_000;

/// A placed lamp reduced to what metering needs.
pub(super) struct Lamp {
    /// Brightest channel times intensity.
    pub bright: f64,
    /// `None` for a directional light.
    pub position: Option<[f64; 3]>,
    /// A spot's axis, the cosine of its cone half-angle and its falloff
    /// exponent.
    pub cone: Option<([f64; 3], f64, f64)>,
}

/// The scale for every light, or `None` when no sampled surface is lit
/// (no meshes, or every lamp aimed away), for the caller to fall back.
pub(super) fn scale(
    lamps: &[Lamp],
    instances: &[(usize, Matrix)],
    meshes: &[SceneMesh],
    floor: f64,
) -> Option<f64> {
    let mut lit: Vec<(f64, f64)> = samples(instances, meshes)
        .into_iter()
        .map(|(centre, area)| (illuminance(lamps, centre, floor), area))
        .filter(|&(e, area)| e > 0.0 && e.is_finite() && area > 0.0)
        .collect();
    let median = weighted_median(&mut lit)?;
    Some(TARGET / median).filter(|s| s.is_finite() && *s > 0.0)
}

/// Each sampled triangle's world centroid and area.
fn samples(instances: &[(usize, Matrix)], meshes: &[SceneMesh]) -> Vec<([f64; 3], f64)> {
    let count = |&(m, _): &(usize, Matrix)| {
        meshes.get(m).map_or(0, |mesh| {
            mesh.primitives.iter().map(|p| p.triangles.len()).sum()
        })
    };
    let total: usize = instances.iter().map(count).sum();
    let stride = total.div_ceil(MAX_SAMPLES).max(1);
    let mut out = Vec::with_capacity(total / stride + 1);
    let mut n = 0usize;
    for (m, matrix) in instances {
        let Some(mesh) = meshes.get(*m) else { continue };
        for p in &mesh.primitives {
            for t in &p.triangles {
                n += 1;
                if !n.is_multiple_of(stride) {
                    continue;
                }
                let corner = |i: u32| {
                    p.positions
                        .get(i as usize)
                        .map(|&v| transform_point(matrix, v))
                };
                if let (Some(a), Some(b), Some(c)) = (corner(t[0]), corner(t[1]), corner(t[2])) {
                    let area = vec3::length(vec3::cross(vec3::sub(b, a), vec3::sub(c, a))) / 2.0;
                    let centre = vec3::scale(vec3::add(vec3::add(a, b), c), 1.0 / 3.0);
                    out.push((centre, area));
                }
            }
        }
    }
    out
}

/// What every lamp adds at `at`.
fn illuminance(lamps: &[Lamp], at: [f64; 3], floor: f64) -> f64 {
    lamps
        .iter()
        .map(|l| {
            let Some(position) = l.position else {
                return l.bright;
            };
            let d = vec3::sub(at, position);
            let r2 = vec3::dot(d, d);
            let cone = l.cone.map_or(1.0, |(axis, cos_half, exponent)| {
                let cos = vec3::dot(d, axis) / r2.sqrt().max(f64::MIN_POSITIVE);
                if cos < cos_half {
                    0.0
                } else {
                    cos.powf(exponent)
                }
            });
            l.bright * cone / (r2 + floor * floor)
        })
        .sum()
}

/// The value at half the total weight of `(value, weight)` pairs.
fn weighted_median(pairs: &mut [(f64, f64)]) -> Option<f64> {
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total: f64 = pairs.iter().map(|p| p.1).sum();
    let mut seen = 0.0;
    for &(value, weight) in pairs.iter() {
        seen += weight;
        if seen >= total / 2.0 {
            return Some(value);
        }
    }
    None
}

/// `p` as a [`Lamp`], or `None` for a type that is not drawn.
pub(super) fn lamp(p: &Placed, kind: Option<&str>, bright: f64) -> Option<Lamp> {
    Some(match kind? {
        "directional" => Lamp {
            bright,
            position: None,
            cone: None,
        },
        "point" => Lamp {
            bright,
            position: Some(p.position),
            cone: None,
        },
        "spot" => {
            let crate::ModelLightKind::Spot {
                angle, exponent, ..
            } = super::lights::spot(p, [1.0, 0.0, 0.0])
            else {
                return None;
            };
            Lamp {
                bright,
                position: Some(p.position),
                cone: Some((p.direction, angle.cos(), exponent)),
            }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    // Tests fail loudly by design.
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn the_median_is_taken_by_area() {
        let mut pairs = [(1.0, 1.0), (5.0, 10.0), (9.0, 1.0)];
        assert_eq!(weighted_median(&mut pairs), Some(5.0));
        assert_eq!(weighted_median(&mut []), None);
    }

    #[test]
    fn a_spot_lights_nothing_outside_its_cone() {
        let spot = Lamp {
            bright: 4.0,
            position: Some([0.0, 0.0, 2.0]),
            cone: Some(([0.0, 0.0, -1.0], 0.5f64.cos(), 0.0)),
        };
        let below = illuminance(std::slice::from_ref(&spot), [0.0; 3], 0.0);
        assert!((below - 1.0).abs() < 1e-12, "{below}");
        assert_eq!(
            illuminance(std::slice::from_ref(&spot), [5.0, 0.0, 2.0], 0.0),
            0.0
        );
    }
}
