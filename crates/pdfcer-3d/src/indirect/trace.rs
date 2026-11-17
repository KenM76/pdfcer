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
        let seen = arriving(scene, last, origin, direction, far);
        let radiance = add(seen.reflected, seen.emitted);
        let distance = seen.distance.min(cap);
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

/// What one ray sees.
struct Seen {
    /// Radiance the surface met reflects (the sky's, when none is met).
    reflected: [f64; 3],
    /// Radiance the surface met gives off itself.
    emitted: [f64; 3],
    /// How far away the surface is (`far` for the sky).
    distance: f64,
}

/// The radiance `mesh` gives off through a surface keeping `kept` of the
/// light that meets it.
fn glow(scene: &Scene, mesh: usize, kept: f64) -> [f64; 3] {
    scene.glow.get(mesh).map_or([0.0; 3], |g| scale(*g, kept))
}

/// The radiance arriving at `origin` from `direction`.
fn arriving(
    scene: &Scene,
    last: &Published,
    origin: [f64; 3],
    direction: [f64; 3],
    far: f64,
) -> Seen {
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
        return Seen {
            reflected: scene.sky,
            emitted: [0.0; 3],
            distance: far,
        };
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
    Seen {
        reflected: zip3(albedo, add(direct, bounced), |a, l| a * kept * l),
        emitted: glow(scene, hit.mesh, kept),
        distance: hit.distance,
    }
}

/// The light the lamps put straight onto `p` on the side `n` faces, with
/// ray-traced shadows (see-through surfaces pass their share). A lamp on
/// the other side of the surface adds nothing.
pub(crate) fn direct(scene: &Scene, p: [f64; 3], n: [f64; 3], far: f64) -> [f64; 3] {
    scene.lamps.iter().fold([0.0; 3], |sum, lamp| {
        add(sum, lamp_direct(scene, lamp, p, n, far))
    })
}

/// [`direct`] for one lamp.
pub(crate) fn lamp_direct(
    scene: &Scene,
    lamp: &Lamp,
    p: [f64; 3],
    n: [f64; 3],
    far: f64,
) -> [f64; 3] {
    let surfaces = scene.surfaces;
    let eps = 1e-9 * far;
    let (l, distance, colour) = match lamp {
        Lamp::Far { to_light, colour } => (*to_light, far, *colour),
        Lamp::Near(local) => {
            let Some((l, share)) = local.reach(p) else {
                return [0.0; 3];
            };
            let d = length(sub(local.position, p));
            (l, d - eps, local.colour.map(|c| c * share))
        }
    };
    let cos = dot(n, l);
    if cos <= 0.0 || colour.iter().all(|&c| c == 0.0) {
        return [0.0; 3];
    }
    let ray = Ray {
        origin: add(p, scale(n, eps)),
        direction: l,
        min_distance: eps,
    };
    let through = scene
        .index
        .transmittance(&ray, distance, |_| true, |h, w| surfaces.passes(h, w));
    scale(colour, cos * through)
}

/// The bounced light arriving at `p` on the side its unit normal `n`
/// faces, and the light arriving straight from glowing meshes: each the
/// cosine-weighted mean, over about `rays` directions on that side, of
/// what each sees lit directly and by the last bounce, and of what each
/// sees glowing.
pub(crate) fn final_gather(
    scene: &Scene,
    last: &Published,
    p: [f64; 3],
    n: [f64; 3],
    rays: usize,
) -> ([f64; 3], [f64; 3]) {
    let far = 2.0 * last.grid.diagonal();
    let origin = add(p, scale(n, 1e-9 * far));
    let (mut reflected, mut emitted, mut weight) = ([0.0; 3], [0.0; 3], 0.0);
    for direction in sphere_directions(rays.saturating_mul(2), false) {
        let c = dot(n, direction);
        if c <= 0.0 {
            continue;
        }
        let seen = arriving(scene, last, origin, direction, far);
        reflected = add(reflected, scale(seen.reflected, c));
        emitted = add(emitted, scale(seen.emitted, c));
        weight += c;
    }
    if weight > 0.0 {
        (scale(reflected, 1.0 / weight), scale(emitted, 1.0 / weight))
    } else {
        ([0.0; 3], [0.0; 3])
    }
}

/// The light arriving at `p`, on the side its unit normal `n` faces,
/// straight from glowing meshes: [`final_gather`]'s second figure, for a
/// scene with no bounce built. `far` exceeds the model's size.
pub(crate) fn glow_gather(
    scene: &Scene,
    p: [f64; 3],
    n: [f64; 3],
    rays: usize,
    far: f64,
) -> [f64; 3] {
    let origin = add(p, scale(n, 1e-9 * far));
    let surfaces = scene.surfaces;
    let (mut sum, mut weight) = ([0.0; 3], 0.0);
    for direction in sphere_directions(rays.saturating_mul(2), false) {
        let c = dot(n, direction);
        if c <= 0.0 {
            continue;
        }
        weight += c;
        let ray = Ray {
            origin,
            direction,
            min_distance: 1e-9 * far,
        };
        let met = scene
            .index
            .first_hit(&ray, far, |_| true, |h, w| surfaces.passes(h, w) >= 1.0);
        if let Some(hit) = met {
            let w = scene.weights(&hit, &ray);
            let kept = 1.0 - surfaces.passes(&hit, w).clamp(0.0, 1.0);
            sum = add(sum, scale(glow(scene, hit.mesh, kept), c));
        }
    }
    if weight > 0.0 {
        scale(sum, 1.0 / weight)
    } else {
        [0.0; 3]
    }
}
