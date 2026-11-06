//! Drawing a [`MaterialFinish`]: OpenGL's fixed-function material model
//! (PRC `Material` defers to it, `prc__8137__graphics_materials.md` §4)
//! with Blinn-Phong highlights, evaluated per pixel. With `C` the drawn
//! colour, `s` the diffuse shade the plain path uses, `La` the ambient
//! light, `n` the unit normal turned towards the eye `v`, and for each
//! light its unit direction `l` (turned to the lit side for a directional
//! light, whose sense is not known), specular colour `Ls` and reach `k`
//! (fade, cone and shadow), `h = normalize(l + v)` and `e` the exponent
//! [`crate::ShininessScale`] gives:
//!
//! `C·s + (A − C)·La + E + S·Σ Ls·k·max(n·h, 0)^e`
//!
//! clamped per channel. The ambient correction is left out for a textured
//! pixel, whose colour is a texel's; a local light behind the surface
//! adds no highlight. [`Shader::Headlight`] has no ambient light (its 0.3·gain
//! is the default light's fixed floor) and one light along `v`;
//! [`Shader::Unlit`] draws no finish at all.

use crate::camera::Camera;
use crate::lighting::shade::Local;
use crate::lighting::{LightingScheme, Shader};
use crate::model::light::unit;
use crate::render::RenderOptions;
use crate::render::raster::{Corner, PixelLights, View};
use crate::shadow::{CasterKind, SHADOW_DIM};
use crate::vec3::{add, dot, normalize, scale, sub};
use crate::{MaterialFinish, ModelLight, ModelLightKind, TriangleMesh};

/// What a frame draws: its meshes, the model's lights and each mesh's
/// finish (parallel to the meshes; shorter means none).
pub(crate) type Model<'a> = (
    &'a [TriangleMesh],
    &'a [ModelLight],
    &'a [Option<MaterialFinish>],
);

/// Interpolates a corner field perspective-correctly at the pixel.
pub(crate) type Field<'a> = &'a dyn Fn(fn(&Corner) -> [f64; 3]) -> [f64; 3];

/// The headlight's colour, as [`Shader::shade_far`] lights with it.
const HEADLIGHT: f64 = 0.7;

/// The specular side of a frame's lights.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct GlossLights {
    /// Each far light's direction (`None`: along the line of sight) and
    /// specular colour, parallel to [`Shader::Set`]'s `lights`.
    pub(crate) far: Vec<(Option<[f64; 3]>, [f64; 3])>,
    /// Each point and spot light's specular colour, parallel to its
    /// `local`.
    pub(crate) near: Vec<[f64; 3]>,
    /// The ambient light a material's ambient colour multiplies.
    pub(crate) ambient: [f64; 3],
}

impl GlossLights {
    /// The lights of `shader`, which [`Shader::for_frame`] built from
    /// `options` and `model`: a light's specular colour is its colour
    /// unless a model light names another.
    pub(crate) fn new(shader: &Shader, options: &RenderOptions, model: &[ModelLight]) -> Self {
        match shader {
            Shader::Headlight { gain } => GlossLights {
                far: vec![(None, [HEADLIGHT * gain; 3])],
                ..GlossLights::default()
            },
            Shader::Unlit => GlossLights::default(),
            Shader::Set {
                ambient,
                lights,
                local,
            } => {
                let mut out = GlossLights {
                    far: lights.clone(),
                    near: local.iter().map(|l| l.colour).collect(),
                    ambient: *ambient,
                };
                if options.lighting == LightingScheme::Artwork {
                    out.take_model_specular(model, options.light_gain, local);
                }
                out
            }
        }
    }

    /// Replace light colours by the model lights' own specular colours,
    /// walking `model` in the order `Shader::from_model` lists them, times
    /// `gain`; a lamp's goes to the entry of `local` it became, if any.
    fn take_model_specular(&mut self, model: &[ModelLight], gain: f64, local: &[Local]) {
        let mut entry = vec![None; local.iter().map(|l| l.model + 1).max().unwrap_or(0)];
        for (i, l) in local.iter().enumerate() {
            if let Some(slot) = entry.get_mut(l.model) {
                *slot = Some(i);
            }
        }
        let (mut far, mut near) = (0, 0);
        for l in model {
            let k = l.intensity.max(0.0) * gain;
            let specular = l.specular.map(|s| s.map(|c| c.max(0.0) * k));
            let slot = match l.kind {
                ModelLightKind::Ambient => continue,
                ModelLightKind::Directional { direction } => {
                    if unit(direction).is_none() {
                        continue;
                    }
                    far += 1;
                    self.far.get_mut(far - 1).map(|(_, c)| c)
                }
                ModelLightKind::Point { .. } | ModelLightKind::Spot { .. } => {
                    near += 1;
                    let i = entry.get(near - 1).copied().flatten();
                    i.and_then(|i| self.near.get_mut(i))
                }
            };
            if let (Some(slot), Some(s)) = (slot, specular) {
                *slot = s;
            }
        }
    }
}

/// A mesh's finish, resolved for drawing: colours 0–255.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Resolved {
    /// Rounded to 8 bits, so an ambient colour equal to the drawn one
    /// corrects by exactly nothing.
    pub(crate) ambient: [f64; 3],
    pub(crate) specular: [f64; 3],
    pub(crate) emissive: [f64; 3],
    pub(crate) exponent: f64,
}

/// Where the eye is, for `v`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Eye {
    At([f64; 3]),
    Along([f64; 3]),
}

/// One frame's finishes and the lights that draw them.
pub(crate) struct Gloss {
    lights: GlossLights,
    caster: Option<CasterKind>,
    eye: Eye,
    /// The address and length of the frame's mesh slice: a mesh's finish
    /// is found by its place in it, so the draw order need not carry it.
    meshes: (usize, usize),
    finishes: Vec<Option<Resolved>>,
}

impl Gloss {
    /// The finishes of `meshes` lit by `shader`, or `None` when none draws
    /// one.
    pub(crate) fn new(
        (meshes, model, finishes): Model<'_>,
        shader: &Shader,
        (camera, view): (&Camera, &View),
        options: &RenderOptions,
    ) -> Option<Gloss> {
        if finishes.iter().all(Option::is_none) {
            return None;
        }
        let lights = GlossLights::new(shader, options, model);
        let resolved: Vec<_> = finishes
            .iter()
            .map(|f| f.map(|f| resolve(&f, options)))
            .collect();
        if lights == GlossLights::default() {
            return None;
        }
        let eye = match view {
            View::Perspective { .. } => Eye::At(camera.eye),
            View::Orthographic { .. } => Eye::Along(scale(camera.basis().ok()?[2], -1.0)),
        };
        Some(Gloss {
            lights,
            caster: None,
            eye,
            meshes: (meshes.as_ptr() as usize, meshes.len()),
            finishes: resolved,
        })
    }

    /// The shadow caster the frame draws with.
    pub(crate) fn with_caster(mut self, caster: Option<CasterKind>) -> Gloss {
        self.caster = caster;
        self
    }

    /// `mesh`'s finish, when it is one of the frame's meshes and has one.
    pub(crate) fn finish_of(&self, mesh: &TriangleMesh) -> Option<&Resolved> {
        let size = std::mem::size_of::<TriangleMesh>().max(1);
        let offset = (mesh as *const TriangleMesh as usize).checked_sub(self.meshes.0)?;
        let i = offset / size;
        if offset % size != 0 || i >= self.meshes.1 {
            return None;
        }
        self.finishes.get(i)?.as_ref()
    }

    /// The pixel `colour` with diffuse `shade` makes under finish `f`;
    /// `model` interpolates a corner field perspective-correctly.
    pub(crate) fn pixel(
        &self,
        f: &Resolved,
        (colour, shade): ([u8; 4], [f64; 3]),
        textured: bool,
        lights: &PixelLights<'_>,
        model: Field<'_>,
    ) -> [u8; 4] {
        let p = model(|c| c.model[0]);
        let lit = lights
            .shadow
            .map_or(1.0, |m| m.visibility(model(|c| c.light)));
        let dim = match self.caster {
            Some(CasterKind::Dim) => SHADOW_DIM + (1.0 - SHADOW_DIM) * lit,
            _ => 1.0,
        };
        let glow = self.highlights(f.exponent, (p, model(|c| c.model[1])), lit, lights);
        let [r, g, b, a] = colour;
        let c = [r, g, b].map(f64::from);
        let ambient = if textured {
            [0.0; 3]
        } else {
            scale(mul(sub(f.ambient, c), self.lights.ambient), dim)
        };
        let v = add(
            add(add(mul(c, shade), ambient), f.emissive),
            mul(f.specular, glow),
        );
        let [r, g, b] = v.map(|v| v.clamp(0.0, 255.0).round() as u8);
        [r, g, b, a]
    }

    /// `Σ Ls·k·max(n·h, 0)^e` at model point `p` with normal `n`; `lit` is
    /// the shadow caster's visibility there.
    fn highlights(
        &self,
        e: f64,
        (p, n): ([f64; 3], [f64; 3]),
        lit: f64,
        lights: &PixelLights<'_>,
    ) -> [f64; 3] {
        let mut out = [0.0; 3];
        let v = match self.eye {
            Eye::At(eye) => normalize(sub(eye, p)),
            Eye::Along(v) => Some(v),
        };
        let (Some(v), Some(n)) = (v, unit(n)) else {
            return out;
        };
        let n = if dot(n, v) < 0.0 { scale(n, -1.0) } else { n };
        let mut add_light = |colour: [f64; 3], k: f64| {
            for (o, c) in out.iter_mut().zip(colour) {
                *o += c * k;
            }
        };
        for (i, (dir, colour)) in self.lights.far.iter().enumerate() {
            let l = dir.unwrap_or(v);
            let l = if dot(n, l) < 0.0 { scale(l, -1.0) } else { l };
            let shadow = match self.caster {
                Some(CasterKind::Light(j)) if j == i => lit,
                Some(CasterKind::Dim) => SHADOW_DIM + (1.0 - SHADOW_DIM) * lit,
                _ => 1.0,
            };
            let extra = lights
                .extra
                .map_or(1.0, |x| x.visibility(i, p, lights.face));
            add_light(*colour, shadow * extra * blinn_phong(n, v, l, e));
        }
        for (i, (local, colour)) in lights.local.iter().zip(&self.lights.near).enumerate() {
            let Some((l, reach)) = local.reach(p) else {
                continue;
            };
            if dot(n, l) <= 0.0 {
                continue;
            }
            let lamp = lights
                .lamps
                .map_or(1.0, |m| m.visibility(i, p, lights.face));
            add_light(*colour, reach * lamp * blinn_phong(n, v, l, e));
        }
        out
    }
}

/// Per channel `a·b`.
fn mul(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    let [x, y, z] = a;
    let [u, v, w] = b;
    [x * u, y * v, z * w]
}

/// `max(n·h, 0)^e` with `h` the unit half-vector of `l` and `v`; 0 when
/// `n·h` is not positive, so `e` = 0 never meets 0⁰.
fn blinn_phong(n: [f64; 3], v: [f64; 3], l: [f64; 3], e: f64) -> f64 {
    let Some(h) = normalize(add(l, v)) else {
        return 0.0;
    };
    let nh = dot(n, h);
    if nh > 0.0 { nh.powf(e) } else { 0.0 }
}

/// `f` in the 0–255 terms the CPU shader works in: the ambient colour
/// rounded to 8 bits, so one equal to the diffuse colour changes nothing,
/// and the shininess read through [`RenderOptions::shininess`].
pub(crate) fn resolve(f: &MaterialFinish, options: &RenderOptions) -> Resolved {
    let to_255 = |c: [f64; 3]| c.map(|v| v.max(0.0) * 255.0);
    Resolved {
        ambient: f.ambient.map(|v| (v.clamp(0.0, 1.0) * 255.0).round()),
        specular: to_255(f.specular),
        emissive: to_255(f.emissive),
        exponent: options.shininess.exponent(f.shininess),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use crate::render::Projection;
    use crate::{AssembledModel, Camera, LightingScheme, MaterialFinish, RenderOptions};
    use crate::{ModelLight, ModelLightKind, TriangleMesh};

    const GREY: [u8; 4] = [100, 120, 140, 255];

    /// A square facing the camera, tilted so its shade varies.
    fn model(finish: Option<MaterialFinish>) -> AssembledModel {
        let mesh = TriangleMesh {
            positions: vec![
                [-1.0, -1.0, -0.3],
                [1.0, -1.0, 0.3],
                [1.0, 1.0, 0.3],
                [-1.0, 1.0, -0.3],
            ],
            triangles: vec![[0, 1, 2], [0, 2, 3]],
            ..TriangleMesh::default()
        };
        AssembledModel {
            meshes: vec![mesh],
            colours: vec![Some(GREY)],
            finishes: vec![finish],
            ..AssembledModel::default()
        }
    }

    fn draw(model: &AssembledModel, lighting: LightingScheme, perspective: bool) -> Vec<u8> {
        let projection = if perspective {
            Projection::Perspective { fov_y: 40.0 }
        } else {
            Projection::Orthographic { height: 3.0 }
        };
        let camera = Camera {
            eye: [0.0, 0.0, 6.0],
            target: [0.0; 3],
            up: [0.0, 1.0, 0.0],
            projection,
        };
        let options = RenderOptions {
            width: 32,
            height: 32,
            lighting,
            ..RenderOptions::default()
        };
        crate::render_model(model, &camera, &options).unwrap().rgba
    }

    /// The drawn colour as a 0–1 ambient colour that rounds back to it.
    fn diffuse() -> [f64; 3] {
        [GREY[0], GREY[1], GREY[2]].map(|c| f64::from(c) / 255.0)
    }

    fn sum(rgba: &[u8]) -> u64 {
        rgba.iter().map(|&c| u64::from(c)).sum()
    }

    fn channel(rgba: &[u8], j: usize) -> u64 {
        rgba.iter().skip(j).step_by(4).map(|&c| u64::from(c)).sum()
    }

    #[test]
    fn a_black_specular_draws_byte_identical_to_no_finish() {
        let plain = model(None);
        let black = model(Some(MaterialFinish::new(diffuse(), [0.0; 3], 0.5)));
        let schemes = [
            LightingScheme::Artwork,
            LightingScheme::Day,
            LightingScheme::Headlamp,
            LightingScheme::None,
        ];
        for lighting in schemes {
            for perspective in [false, true] {
                assert_eq!(
                    draw(&plain, lighting, perspective),
                    draw(&black, lighting, perspective),
                    "{lighting:?} perspective {perspective}"
                );
            }
        }
    }

    #[test]
    fn a_specular_finish_brightens_and_a_sharper_one_narrows() {
        let plain = draw(&model(None), LightingScheme::Artwork, true);
        let broad = MaterialFinish::new(diffuse(), [1.0; 3], 0.05);
        let broad = draw(&model(Some(broad)), LightingScheme::Artwork, true);
        let sharp = MaterialFinish::new(diffuse(), [1.0; 3], 1.0);
        let sharp = draw(&model(Some(sharp)), LightingScheme::Artwork, true);
        assert!(plain.iter().zip(&broad).all(|(p, b)| b >= p));
        assert!(sum(&broad) > sum(&sharp) && sum(&sharp) > sum(&plain));
    }

    #[test]
    fn emission_adds_and_ambient_multiplies_the_scheme_ambient() {
        let plain = draw(&model(None), LightingScheme::Day, false);
        let mut glow = MaterialFinish::new(diffuse(), [0.0; 3], 0.0);
        glow.emissive = [0.2, 0.0, 0.0];
        let glow = draw(&model(Some(glow)), LightingScheme::Day, false);
        assert!(channel(&glow, 0) > channel(&plain, 0));
        assert_eq!(channel(&glow, 1), channel(&plain, 1));
        let dark = MaterialFinish::new([0.0; 3], [0.0; 3], 0.0);
        let dark_lamp = draw(&model(Some(dark)), LightingScheme::Headlamp, false);
        assert!(sum(&dark_lamp) < sum(&draw(&model(None), LightingScheme::Headlamp, false)));
        // The headlight has no ambient light, so the ambient colour is idle.
        assert_eq!(
            draw(&model(Some(dark)), LightingScheme::Artwork, false),
            draw(&model(None), LightingScheme::Artwork, false)
        );
    }

    #[test]
    fn a_model_light_brings_its_own_specular_colour() {
        let mut sun = ModelLight::new(
            ModelLightKind::Directional {
                direction: [0.0, 0.0, -1.0],
            },
            [1.0; 3],
        );
        let lit = |sun: &ModelLight, finish| {
            let mut m = model(finish);
            m.lights = vec![sun.clone()];
            draw(&m, LightingScheme::Artwork, true)
        };
        let shiny = Some(MaterialFinish::new(diffuse(), [1.0; 3], 0.05));
        let white = lit(&sun, shiny);
        sun.specular = Some([0.0; 3]);
        let none = lit(&sun, shiny);
        assert_eq!(none, lit(&sun, None), "a black light specular adds nothing");
        assert!(sum(&white) > sum(&none));
    }
}
