//! One probe's rays for one bounce: what each ray sees, and the probe maps
//! built from them.

use super::grid::{Grid, gather};
use super::octa::{self, TEXELS};
use super::{Lamp, Probe, RAYS, Scene, sphere_directions, zip3};
use crate::camera::pick::Ray;
use crate::vec3::{add, dot, length, scale, sub};

/// Depth moments weigh a ray by its cosine to the texel to this power, so
/// each texel's depth comes from the rays nearest it.
const DEPTH_SHARPNESS: i32 = 64;

/// The previous bounce, read while the next is built.
pub(crate) struct Published<'a> {
    pub(crate) grid: &'a Grid,
    pub(crate) probes: &'a [Probe],
    pub(crate) active: &'a [bool],
}

/// Builds the probe at `origin` from `RAYS` rays: each texel holds the
/// cosine-weighted mean of the radiance arriving round its direction, and
/// the sharpened mean distance and squared distance to what it sees, each
/// distance capped at one and a half cell diagonals.
pub(crate) fn trace_probe(scene: &Scene, last: &Published, origin: [f64; 3]) -> Probe {
    let far = 2.0 * last.grid.diagonal();
    // Only the probes round a point are read, so depth past this tells
    // nothing, and storing it would swamp the variance near every edge.
    let cap = 1.5 * length(last.grid.spacing);
    let texel_dirs: [[f64; 3]; TEXELS] = std::array::from_fn(octa::texel_direction);
    let mut irr = [([0.0f64; 3], 0.0f64); TEXELS];
    let mut depth = [([0.0f64; 2], 0.0f64); TEXELS];
    let mut nearest_ray = [(f64::NEG_INFINITY, cap); TEXELS];
    for direction in sphere_directions(RAYS, false) {
        let (radiance, distance) = arriving(scene, last, origin, direction, far);
        let distance = distance.min(cap);
        for (((td, (e, ew)), (m, mw)), best) in texel_dirs
            .iter()
            .zip(irr.iter_mut())
            .zip(depth.iter_mut())
            .zip(nearest_ray.iter_mut())
        {
            let c = dot(*td, direction);
            if c > best.0 {
                *best = (c, distance);
            }
            if c <= 0.0 {
                continue;
            }
            *e = add(*e, scale(radiance, c));
            *ew += c;
            let dw = c.powi(DEPTH_SHARPNESS);
            *m = [m[0] + dw * distance, m[1] + dw * distance * distance];
            *mw += dw;
        }
    }
    let mut probe = Probe::dark(far);
    for (((out, d_out), (e, ew)), ((m, mw), best)) in probe
        .irradiance
        .iter_mut()
        .zip(probe.depth.iter_mut())
        .zip(irr)
        .zip(depth.into_iter().zip(nearest_ray))
    {
        if ew > 0.0 {
            *out = e.map(|v| (v / ew) as f32);
        }
        *d_out = if mw > 1e-300 {
            [(m[0] / mw) as f32, (m[1] / mw) as f32]
        } else {
            [best.1 as f32, (best.1 * best.1) as f32]
        };
    }
    probe
}

/// The radiance arriving at `origin` from `direction`, and how far away its
/// source is (`far` for the sky).
fn arriving(
    scene: &Scene,
    last: &Published,
    origin: [f64; 3],
    direction: [f64; 3],
    far: f64,
) -> ([f64; 3], f64) {
    let ray = Ray {
        origin,
        direction,
        min_distance: 1e-9 * far,
    };
    let surfaces = scene.surfaces;
    let Some(hit) = scene
        .index
        .first_hit(&ray, far, |_| true, |h, w| surfaces.passes(h, w) >= 1.0)
    else {
        return (scene.sky, far);
    };
    let n = if dot(hit.normal, direction) > 0.0 {
        scale(hit.normal, -1.0)
    } else {
        hit.normal
    };
    let w = scene.weights(&hit, &ray);
    let albedo = surfaces.albedo(&hit, w);
    let kept = 1.0 - surfaces.passes(&hit, w).clamp(0.0, 1.0);
    let direct = direct(scene, hit.point, n, far);
    let bounced = gather(last.grid, last.probes, last.active, hit.point, n);
    let radiance = zip3(albedo, add(direct, bounced), |a, l| a * kept * l);
    (radiance, hit.distance)
}

/// The light the lamps put straight onto `p` on the side `n` faces, with
/// ray-traced shadows (see-through surfaces pass their share). A lamp on
/// the other side of the surface adds nothing.
pub(crate) fn direct(scene: &Scene, p: [f64; 3], n: [f64; 3], far: f64) -> [f64; 3] {
    let surfaces = scene.surfaces;
    let eps = 1e-9 * far;
    let mut sum = [0.0; 3];
    for lamp in scene.lamps {
        let (l, distance, colour) = match lamp {
            Lamp::Far { to_light, colour } => (*to_light, far, *colour),
            Lamp::Near(local) => {
                let Some((l, share)) = local.reach(p) else {
                    continue;
                };
                let d = length(sub(local.position, p));
                (l, d - eps, local.colour.map(|c| c * share))
            }
        };
        let cos = dot(n, l);
        if cos <= 0.0 || colour.iter().all(|&c| c == 0.0) {
            continue;
        }
        let ray = Ray {
            origin: add(p, scale(n, eps)),
            direction: l,
            min_distance: eps,
        };
        let through =
            scene
                .index
                .transmittance(&ray, distance, |_| true, |h, w| surfaces.passes(h, w));
        sum = add(sum, scale(colour, cos * through));
    }
    sum
}
