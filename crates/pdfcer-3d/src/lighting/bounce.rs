//! Bounce light (decision 224): the settings, which of a frame's lights
//! bounce, and the probe volume a CPU frame reads. pdfcer's: neither PRC nor
//! ISO 32000 defines indirect light.

use crate::camera::Camera;
use crate::camera::pick::Hit;
use crate::camera::pick_index::PickIndex;
use crate::indirect::{Lamp, Scene, Surfaces, Volume};
use crate::lighting::Shader;
use crate::lighting::shade::Local;
use crate::render::raster::Paint;
use crate::render::{RenderError, RenderOptions, triangle_uvs};
use crate::vec3::{dot, normalize, scale};
use crate::{LightFrame, LightSense, LightingScheme, Projection, ShadowLight, TriangleMesh};

/// The largest [`IndirectOptions::bounce_strength`].
pub const MAX_BOUNCE_STRENGTH: f64 = 4.0;

/// Which lights bounce off surfaces onto others
/// ([`IndirectOptions::bounce`]). Ambient lights never bounce (ambient
/// already stands in for indirect light), and neither does a material's
/// emissive term.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Bounce {
    /// No bounce light.
    Off,
    /// Lights fixed in the model bounce: a model's own directional, point
    /// and spot lights, an aimed sun
    /// ([`ShadowLight::AimedSun`]) and a scheme's lights in
    /// [`LightFrame::World`]. The default. A model lit only from the camera,
    /// as most CAD files are, gets none.
    #[default]
    ModelLights,
    /// Every light bounces, the headlight and camera-following scheme
    /// lights too, so the bounce changes as the view turns.
    EveryLight,
}

/// What a bounce ray that leaves the model sees ([`IndirectOptions::bounce_sky`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum BounceSky {
    /// Nothing: only lit surfaces give bounce light. The default.
    #[default]
    Dark,
    /// The background colour ([`RenderOptions::background`]) times its
    /// alpha, lighting surfaces open to the outside.
    Background,
}

/// How light bounces between surfaces ([`RenderOptions::indirect`]).
///
/// A frame with bounce on builds a lattice of up to 4,096 probes over the
/// model, each tracing 64 rays a bounce for 3 bounces, and adds each pixel's
/// share, times [`Self::bounce_strength`], to its light; a surface then
/// takes the colour of the lit surfaces round it. Bounce off, or no light
/// that bounces, draws the picture as before, pixel for pixel.
///
/// ```
/// use pdfcer_3d::{Bounce, IndirectOptions, RenderOptions};
/// let mut indirect = IndirectOptions::default();
/// indirect.bounce = Bounce::Off;
/// let options = RenderOptions { indirect, ..RenderOptions::default() };
/// assert_eq!(options.indirect.bounce, Bounce::Off);
/// ```
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct IndirectOptions {
    /// Which lights bounce ([`Bounce::ModelLights`]).
    pub bounce: Bounce,
    /// A multiplier on the bounce light, 0 to [`MAX_BOUNCE_STRENGTH`] (1,
    /// the default); clamped to that range, and not finite counts as 0.
    pub bounce_strength: f64,
    /// What a ray leaving the model sees ([`BounceSky::Dark`]).
    pub bounce_sky: BounceSky,
}

impl Default for IndirectOptions {
    fn default() -> Self {
        IndirectOptions {
            bounce: Bounce::ModelLights,
            bounce_strength: 1.0,
            bounce_sky: BounceSky::Dark,
        }
    }
}

impl PartialEq for IndirectOptions {
    /// Equal when every field is, the strength compared by its bits.
    fn eq(&self, other: &Self) -> bool {
        self.bounce == other.bounce
            && self.bounce_strength.to_bits() == other.bounce_strength.to_bits()
            && self.bounce_sky == other.bounce_sky
    }
}

impl Eq for IndirectOptions {}

impl IndirectOptions {
    /// No bounce, for tests that pin the direct light.
    #[cfg(test)]
    pub(crate) fn off() -> IndirectOptions {
        IndirectOptions {
            bounce: Bounce::Off,
            ..IndirectOptions::default()
        }
    }

    /// [`Self::bounce_strength`] as drawn: clamped, 0 when not finite.
    #[must_use]
    pub fn strength(&self) -> f64 {
        if self.bounce_strength.is_finite() {
            self.bounce_strength.clamp(0.0, MAX_BOUNCE_STRENGTH)
        } else {
            0.0
        }
    }
}

/// A frame's bounce light: its built probe volume and strength.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BounceLight {
    volume: Volume,
    strength: f64,
}

impl BounceLight {
    /// Adds the bounce light at model point `p` with normal `n` to `shade`,
    /// on the side of the triangle facing `face`, the seen side.
    pub(crate) fn add(&self, shade: &mut [f64; 3], p: [f64; 3], n: [f64; 3], face: [f64; 3]) {
        let n = normalize(n).unwrap_or(face);
        let n = if dot(n, face) < 0.0 {
            scale(n, -1.0)
        } else {
            n
        };
        for (s, e) in shade.iter_mut().zip(self.volume.irradiance(p, n)) {
            *s += self.strength * e;
        }
    }
}

/// Which of a frame's lights bounce, for the disclosure a shell prints with
/// a picture: [`IndirectOptions`] draws bounce only from lights fixed in the
/// model by default, so a model lit only from the camera gets none, and
/// that has to be said.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct BounceStats {
    /// The lights that bounce in this frame: 0 with bounce off, its
    /// strength 0 or the frame unlit.
    pub bouncing: usize,
    /// The lights that follow the camera and so bounce only under
    /// [`Bounce::EveryLight`]: the headlight and camera-frame scheme
    /// lights. With `bouncing` 0 and this above 0, the model is lit from
    /// the camera alone.
    pub camera_lights: usize,
}

impl BounceStats {
    /// The counts a frame of a model with `lights`, drawn from `camera`
    /// with `options`, bounces.
    ///
    /// ```
    /// use pdfcer_3d::{BounceStats, Camera, Projection, RenderOptions};
    /// let camera = Camera {
    ///     eye: [0.0, 0.0, 10.0],
    ///     target: [0.0; 3],
    ///     up: [0.0, 1.0, 0.0],
    ///     projection: Projection::Perspective { fov_y: 30.0 },
    /// };
    /// // No model lights: the default light at the camera, which does not bounce.
    /// let stats = BounceStats::for_model(&camera, &RenderOptions::default(), &[])?;
    /// assert_eq!((stats.bouncing, stats.camera_lights), (0, 1));
    /// # Ok::<(), pdfcer_3d::RenderError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// As [`crate::render`] for the camera, the light gain and an aimed sun.
    pub fn for_model(
        camera: &Camera,
        options: &RenderOptions,
        lights: &[crate::ModelLight],
    ) -> Result<BounceStats, RenderError> {
        let basis = camera.basis()?;
        let shader = Shader::for_frame(options, lights, &basis)?;
        let artwork = !lights.is_empty();
        let count = |bounce: Bounce| {
            let mut o = *options;
            o.indirect.bounce = bounce;
            bouncing(&shader, &o, artwork, camera, &basis).len()
        };
        let (fixed, every) = (count(Bounce::ModelLights), count(Bounce::EveryLight));
        let on = options.indirect.bounce != Bounce::Off && options.indirect.strength() > 0.0;
        let bouncing = match options.indirect.bounce {
            _ if !on => 0,
            Bounce::EveryLight => every,
            _ => fixed,
        };
        Ok(BounceStats {
            bouncing,
            camera_lights: every - fixed,
        })
    }
}

/// The bounce light of a frame of `model` drawn with `paints` from
/// `camera`; `None` when bounce is off, its strength 0, the frame unlit or
/// no light bounces.
///
/// # Errors
///
/// As [`Shader::for_frame`], and [`RenderError::Camera`] for a camera with
/// no basis.
pub(crate) fn frame_bounce(
    (meshes, model_lights, _): crate::lighting::gloss::Model<'_>,
    paints: &[Paint<'_>],
    camera: &Camera,
    options: &RenderOptions,
) -> Result<Option<BounceLight>, RenderError> {
    let indirect = options.indirect;
    let strength = indirect.strength();
    if indirect.bounce == Bounce::Off || strength == 0.0 {
        return Ok(None);
    }
    let basis = camera.basis()?;
    let shader = Shader::for_frame(options, model_lights, &basis)?;
    let lamps = bouncing(&shader, options, !model_lights.is_empty(), camera, &basis);
    if lamps.is_empty() {
        return Ok(None);
    }
    let Some(mut volume) = Volume::new(meshes) else {
        return Ok(None);
    };
    let index = PickIndex::new(meshes);
    let surfaces = Painted { meshes, paints };
    let sky = match indirect.bounce_sky {
        BounceSky::Dark => [0.0; 3],
        BounceSky::Background => {
            let [r, g, b, a] = options.background.map(|c| f64::from(c) / 255.0);
            [r * a, g * a, b * a]
        }
    };
    let scene = Scene {
        meshes,
        index: &index,
        surfaces: &surfaces,
        lamps: &lamps,
        sky,
    };
    volume.build(&scene, options.threads);
    Ok(Some(BounceLight { volume, strength }))
}

/// The lights of `shader` that bounce under `options`. `artwork` says the
/// shader holds a model's own lights; `basis` is `camera`'s.
fn bouncing(
    shader: &Shader,
    options: &RenderOptions,
    artwork: bool,
    camera: &Camera,
    basis: &[[f64; 3]; 3],
) -> Vec<Lamp> {
    let every = options.indirect.bounce == Bounce::EveryLight;
    let artwork = artwork && options.lighting == LightingScheme::Artwork;
    let sun_last = matches!(options.shadow_light, ShadowLight::AimedSun(_));
    let at_camera =
        |colour: [f64; 3]| -> Option<Lamp> { every.then(|| headlight(camera, basis, colour)) };
    match shader {
        Shader::Unlit => Vec::new(),
        Shader::Headlight { gain } => at_camera([0.7 * gain; 3]).into_iter().collect(),
        Shader::Set { lights, local, .. } => {
            let mut out = Vec::new();
            for (i, (dir, colour)) in lights.iter().enumerate() {
                let Some(d) = dir else {
                    out.extend(at_camera(*colour));
                    continue;
                };
                // `Shader::for_frame` adds an aimed sun as the last light;
                // its direction is towards it whatever the sense.
                let sun = sun_last && i + 1 == lights.len();
                let fixed = artwork || sun || options.light_frame == LightFrame::World;
                if !(fixed || every) {
                    continue;
                }
                let to_light = match options.light_sense {
                    LightSense::Emitted if !sun => scale(*d, -1.0),
                    _ => *d,
                };
                out.push(Lamp::Far {
                    to_light,
                    colour: *colour,
                });
            }
            out.extend(local.iter().map(|l| Lamp::Near(*l)));
            out
        }
    }
}

/// A light at `camera` of `colour`: a lamp at the eye, or a directional
/// light towards the viewer for an orthographic camera.
fn headlight(camera: &Camera, basis: &[[f64; 3]; 3], colour: [f64; 3]) -> Lamp {
    match camera.projection {
        Projection::Orthographic { .. } => Lamp::Far {
            to_light: scale(crate::render::at3(basis, 2), -1.0),
            colour,
        },
        _ => Lamp::Near(Local {
            position: camera.eye,
            colour,
            attenuation: [1.0, 0.0, 0.0],
            cone: None,
            reach: f64::INFINITY,
            model: 0,
            slot: None,
        }),
    }
}

/// The meshes as drawn, for bounce rays.
struct Painted<'a, 'p> {
    meshes: &'a [TriangleMesh],
    paints: &'a [Paint<'p>],
}

impl Painted<'_, '_> {
    /// The colour drawn at `hit`, straight RGBA; clear for a mesh with no
    /// paint, which is not drawn.
    fn colour(&self, hit: &Hit, weights: [f64; 3]) -> [u8; 4] {
        let Some(paint) = self.paints.get(hit.mesh) else {
            return [0; 4];
        };
        let Some(texture) = paint.texture else {
            return paint.colour;
        };
        let corners = self
            .meshes
            .get(hit.mesh)
            .and_then(|m| triangle_uvs(m, texture.uv_set, hit.triangle))
            .unwrap_or([[0.0; 2]; 3]);
        let mut uv = [0.0; 2];
        for (w, c) in weights.iter().zip(corners) {
            for (o, c) in uv.iter_mut().zip(c) {
                *o += w * c;
            }
        }
        texture.apply(paint.colour, texture.sample(uv))
    }
}

impl Surfaces for Painted<'_, '_> {
    fn albedo(&self, hit: &Hit, weights: [f64; 3]) -> [f64; 3] {
        let [r, g, b, _] = self.colour(hit, weights).map(|c| f64::from(c) / 255.0);
        [r, g, b]
    }

    fn passes(&self, hit: &Hit, weights: [f64; 3]) -> f64 {
        let [.., a] = self.colour(hit, weights);
        1.0 - f64::from(a) / 255.0
    }
}
