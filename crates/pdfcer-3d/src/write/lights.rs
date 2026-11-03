//! The scene's lights as one scene display parameters entity (741) on the
//! root occurrence, with lights 731-734 [WD 7.5.15-7.5.19].

use super::BitWriter;
use super::sections::ref_base;
use crate::{ModelLight, ModelLightKind};

const SCENE_DISPLAY_PARAMETERS: u32 = 741;
const AMBIENT_LIGHT: u32 = 731;
const POINT_LIGHT: u32 = 732;
const DIRECTIONAL_LIGHT: u32 = 733;
const SPOT_LIGHT: u32 = 734;

/// What a light writes: its colour scaled into 0-1 and the scale left over
/// (PRC colours are 0-1; the rest goes into attenuation or intensity).
/// A directional light keeps its intensity, a 0-1 coefficient
/// [WD 7.5.17], clamped.
pub(super) fn stored(l: &ModelLight) -> ([f64; 3], f64) {
    let intensity = l.intensity.max(0.0);
    if let ModelLightKind::Directional { .. } = l.kind {
        let c = l.colour.map(|c| c.max(0.0));
        let top = c.iter().copied().fold(0.0, f64::max);
        return match top > 1.0 {
            true => (c.map(|c| c / top), (intensity * top).min(1.0)),
            false => (c, intensity.min(1.0)),
        };
    }
    let c = l.colour.map(|c| c.max(0.0) * intensity);
    let top = c.iter().copied().fold(0.0, f64::max);
    if top <= 1.0 || matches!(l.kind, ModelLightKind::Ambient) {
        (c.map(|c| c.min(1.0)), 1.0)
    } else {
        (c.map(|c| c / top), top)
    }
}

/// The colours the lights add to the globals' colour table, in order:
/// each light's diffuse, then its ambient when it has one.
pub(super) fn colours(lights: &[ModelLight]) -> Vec<[f64; 3]> {
    let mut out = Vec::new();
    for l in lights {
        out.push(stored(l).0);
        if has_ambient(l) {
            out.push(l.ambient.map(|c| c.clamp(0.0, 1.0)));
        }
    }
    out
}

fn has_ambient(l: &ModelLight) -> bool {
    !matches!(l.kind, ModelLightKind::Ambient) && l.ambient.iter().any(|&c| c > 0.0)
}

/// The scene display parameters entity, or its absence (a zero count)
/// when there are no lights. `first` is the 0-based colour index of the
/// first light colour; `ids` hands out unique ids. The lights are in model
/// space, so the entity is absolute.
pub(super) fn scene(
    w: &mut BitWriter,
    lights: &[ModelLight],
    first: u32,
    mut ids: impl FnMut() -> u32,
) {
    if lights.is_empty() {
        w.uint(0);
        return;
    }
    w.uint(1).uint(SCENE_DISPLAY_PARAMETERS);
    ref_base(w, Some(None), ids());
    w.bit(true).uint(lights.len() as u32);
    let mut colour = first;
    for l in lights {
        let diffuse = 3 * colour + 1;
        colour += 1;
        let ambient = if has_ambient(l) {
            colour += 1;
            3 * (colour - 1) + 1
        } else {
            0
        };
        light(w, l, diffuse, ambient, ids());
    }
    // No camera, no rotation centre, no clipping planes, no background or
    // default style, no default styles; absolute.
    w.bit(false).bit(false).uint(0).uint(0).uint(0).uint(0);
    w.bit(true);
}

fn light(w: &mut BitWriter, l: &ModelLight, diffuse: u32, ambient: u32, id: u32) {
    let (_, scale) = stored(l);
    let fade = |a: [f64; 3]| a.map(|c| c / scale);
    let code = match l.kind {
        ModelLightKind::Ambient => AMBIENT_LIGHT,
        ModelLightKind::Point { .. } => POINT_LIGHT,
        ModelLightKind::Directional { .. } => DIRECTIONAL_LIGHT,
        _ => SPOT_LIGHT,
    };
    w.uint(code);
    ref_base(w, Some(l.name.as_deref()), id);
    // Ambient, diffuse, specular (biased, double-scaled); an ambient light's
    // own colour is its ambient one.
    match l.kind {
        ModelLightKind::Ambient => w.uint(diffuse).uint(0).uint(0),
        _ => w.uint(ambient).uint(diffuse).uint(diffuse),
    };
    match l.kind {
        ModelLightKind::Ambient => {}
        ModelLightKind::Point {
            position,
            attenuation,
        } => {
            w.doubles(position).doubles(fade(attenuation));
        }
        ModelLightKind::Directional { direction } => {
            w.doubles(direction).double(scale);
        }
        ModelLightKind::Spot {
            position,
            direction,
            attenuation,
            angle,
            exponent,
        } => {
            w.doubles(position).doubles(fade(attenuation));
            w.doubles(direction)
                .double(angle.to_degrees())
                .double(exponent);
        }
    }
}
