//! Every directional light's shadow, not only the key light's
//! ([`crate::ShadowCasters::EveryLight`]). The key light keeps its path in
//! [`crate::shadow`] (its share split off at the corners); each further
//! caster has its own map and removes its own light per pixel, so a frame
//! where only the key light casts draws exactly as before.

use crate::TriangleMesh;
use crate::gpu_frame::{MAX_LIGHTS, MAX_SHADOW_CASTERS};
use crate::lamp_shadow::LampShadows;
use crate::lighting::Shader;
use crate::raster::Paint;
use crate::render::{Bounds, RenderError, RenderOptions};
use crate::shadow::{Caster, CasterKind, LightSense, LightView, ShadowMap};
use crate::shadow_cast::{Occluder, ShadowCasters, occluders, veiled};
use crate::vec3::{dot, normalize};
use crate::work::Work;

/// A directional light casting besides the key light.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Extra {
    /// Its index among the shader's lights.
    pub(crate) light: usize,
    /// Unit, model space, towards it.
    pub(crate) to_light: [f64; 3],
    /// Its direction as the shader lights with it.
    dir: [f64; 3],
    /// Its colour as the shader lights with it.
    pub(crate) colour: [f64; 3],
}

/// The lights casting besides `key`, brightest first (ties by index), and
/// how many more would have cast past [`MAX_SHADOW_CASTERS`] in all. Only
/// the first [`MAX_LIGHTS`] lights are candidates, the ones a GPU frame
/// draws ([`crate::FrameLights`]), so both paths shadow the same lights. None
/// unless `casters` is [`ShadowCasters::EveryLight`] and `key` is a scheme
/// light.
pub(crate) fn extras(
    key: Option<&Caster>,
    shader: &Shader,
    sense: LightSense,
    casters: ShadowCasters,
) -> (Vec<Extra>, usize) {
    let (
        Some(Caster {
            kind: CasterKind::Light(key),
            ..
        }),
        ShadowCasters::EveryLight,
        Shader::Set { lights, .. },
    ) = (key, casters, shader)
    else {
        return (Vec::new(), 0);
    };
    let mut out: Vec<(f64, Extra)> = lights
        .iter()
        .enumerate()
        .filter(|(i, _)| i != key)
        .filter_map(|(i, (dir, colour))| {
            let d = (*dir)?;
            let brightness = colour.iter().sum::<f64>();
            let to_light = match sense {
                LightSense::Emitted => d.map(|c| -c),
                _ => d,
            };
            (brightness > 0.0).then_some((
                brightness,
                Extra {
                    light: i,
                    to_light,
                    dir: d,
                    colour: *colour,
                },
            ))
        })
        .collect();
    let eligible = out.len();
    out.retain(|(_, e)| e.light < MAX_LIGHTS);
    out.sort_by(|a, b| b.0.total_cmp(&a.0));
    out.truncate(MAX_SHADOW_CASTERS - 1);
    let dropped = eligible - out.len();
    (out.into_iter().map(|(_, e)| e).collect(), dropped)
}

/// Each of the frame's lamps, `None` for one casting no shadow (its slot,
/// [`crate::lamp_view::shadow_slots`], as [`crate::FrameLampShadows`] on
/// the GPU); those light unshadowed.
pub(crate) fn lamp_casters(shader: &Shader) -> Vec<Option<crate::model_shade::Local>> {
    let lamps = shader.local();
    let slots = crate::lamp_view::shadow_slots(lamps);
    lamps
        .iter()
        .zip(slots)
        .map(|(l, s)| s.map(|_| *l))
        .collect()
}

/// The further casters' maps for one frame.
pub(crate) struct ExtraShadows {
    maps: Vec<(Extra, ShadowMap)>,
}

impl ExtraShadows {
    /// Each of `extras`' maps over `bounds`, `size` texels square, drawn
    /// from `occluders` as [`ShadowMap::build`].
    pub(crate) fn build(
        extras: &[Extra],
        bounds: &Bounds,
        size: u32,
        (occluders, veiled): (&[Occluder<'_>], bool),
        workers: usize,
    ) -> ExtraShadows {
        let maps = extras
            .iter()
            .filter_map(|e| LightView::new(e.to_light, bounds, size).map(|v| (*e, v)))
            .map(|(e, view)| (e, ShadowMap::build(view, occluders, veiled, workers)))
            .collect();
        ExtraShadows { maps }
    }

    /// The texel tests [`Self::build`] makes for the same arguments.
    fn tests(extras: &[Extra], bounds: &Bounds, size: u32, occluders: &[Occluder<'_>]) -> u64 {
        extras
            .iter()
            .filter_map(|e| LightView::new(e.to_light, bounds, size))
            .fold(0u64, |sum, view| {
                sum.saturating_add(ShadowMap::tests(&view, occluders))
            })
    }

    /// Light `light`'s visibility at model point `p` on a face of unit
    /// normal `face`: 1 when it casts no map here.
    pub(crate) fn visibility(&self, light: usize, p: [f64; 3], face: [f64; 3]) -> f64 {
        self.maps
            .iter()
            .find(|(e, _)| e.light == light)
            .map_or(1.0, |(_, map)| {
                map.visibility(map.view.lookup_point(p, face))
            })
    }

    /// Removes from `shade`, at model point `p` with (unnormalised) normal
    /// `n` on a face of unit normal `face`, each caster's light where its
    /// map shadows the point: `(1 − lit) · colour · |n · d|`.
    pub(crate) fn darken(&self, shade: &mut [f64; 3], p: [f64; 3], n: [f64; 3], face: [f64; 3]) {
        let Some(n) = normalize(n) else {
            return;
        };
        for (e, map) in &self.maps {
            let lit = map.visibility(map.view.lookup_point(p, face));
            if lit >= 1.0 {
                continue;
            }
            let k = (1.0 - lit) * dot(n, e.dir).abs();
            for (s, c) in shade.iter_mut().zip(e.colour) {
                *s -= k * c;
            }
        }
    }
}

/// A frame's cast shadows on the CPU: the key light and its map, the
/// further casters' maps and the lamps' cubes, each `None` when not drawn.
pub(crate) struct Shadows {
    pub(crate) caster: Option<Caster>,
    pub(crate) key: Option<ShadowMap>,
    pub(crate) extra: Option<ExtraShadows>,
    pub(crate) lamps: Option<LampShadows>,
}

impl Shadows {
    /// The shadows `options` asks for, lit by `shader` from a camera of
    /// `basis`, over a model of `bounds` whose meshes are drawn with
    /// `paints` (`opaque` where wholly opaque). Every map's and cube's
    /// texel tests are charged to `work` before any is drawn.
    pub(crate) fn build(
        (options, shader, basis): (&RenderOptions, &Shader, &[[f64; 3]; 3]),
        bounds: Option<&Bounds>,
        (meshes, paints, opaque): (&[TriangleMesh], &[Paint<'_>], &[bool]),
        (workers, work): (usize, &mut Work),
    ) -> Result<Shadows, RenderError> {
        let caster = options
            .shadows
            .then(|| Caster::for_light(options.shadow_light, shader, options.light_sense, basis))
            .flatten();
        let lamps_cast = options.shadows && options.lamp_shadows && !shader.local().is_empty();
        let casts = if caster.is_some() || lamps_cast {
            occluders(meshes, paints, opaque, options.shadow_alpha)
        } else {
            Vec::new()
        };
        let veil = veiled(&casts);
        let key_view = caster
            .zip(bounds)
            .and_then(|(c, bounds)| LightView::new(c.to_light, bounds, options.shadow_map_size));
        let caster = caster.filter(|_| key_view.is_some());
        let (list, _) = extras(
            caster.as_ref(),
            shader,
            options.light_sense,
            options.shadow_casters,
        );
        let (size, lamp_size) = (options.shadow_map_size, options.lamp_shadow_map_size);
        let lamps = lamp_casters(shader);
        if let Some(bounds) = bounds {
            let maps = key_view.as_ref().map_or(0, |v| ShadowMap::tests(v, &casts));
            let extra = ExtraShadows::tests(&list, bounds, size, &casts);
            let cubes = lamps_cast.then(|| LampShadows::tests(&lamps, bounds, lamp_size, &casts));
            work.charge(
                maps.saturating_add(extra)
                    .saturating_add(cubes.unwrap_or(0)),
            )?;
        }
        Ok(Shadows {
            caster,
            key: key_view.map(|view| ShadowMap::build(view, &casts, veil, workers)),
            extra: bounds
                .filter(|_| !list.is_empty())
                .map(|bounds| ExtraShadows::build(&list, bounds, size, (&casts, veil), workers)),
            lamps: bounds.filter(|_| lamps_cast).map(|bounds| {
                LampShadows::build(&lamps, bounds, lamp_size, (&casts, veil), workers)
            }),
        })
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use crate::{LightFrame, LightingScheme};

    const BASIS: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

    #[test]
    fn every_other_directional_light_casts_unless_asked_for_one() {
        let shader = Shader::new(LightingScheme::Day.lights(), LightFrame::Camera, &BASIS);
        let key = Caster::choose(&shader, LightSense::TowardsLight, &BASIS).unwrap();
        let (every, dropped) = extras(
            Some(&key),
            &shader,
            LightSense::TowardsLight,
            ShadowCasters::EveryLight,
        );
        assert_eq!(dropped, 0);
        assert!(!every.is_empty());
        assert!(every.iter().all(|e| CasterKind::Light(e.light) != key.kind));
        let (one, _) = extras(
            Some(&key),
            &shader,
            LightSense::TowardsLight,
            ShadowCasters::KeyLight,
        );
        assert!(one.is_empty());
        // The shadow key (camera lights only) stays a single caster.
        let headlight = Shader::new(
            LightingScheme::Headlamp.lights(),
            LightFrame::Camera,
            &BASIS,
        );
        let dim = Caster::choose(&headlight, LightSense::TowardsLight, &BASIS).unwrap();
        let none = extras(
            Some(&dim),
            &headlight,
            LightSense::TowardsLight,
            ShadowCasters::EveryLight,
        );
        assert_eq!(none, (Vec::new(), 0));
    }
}
