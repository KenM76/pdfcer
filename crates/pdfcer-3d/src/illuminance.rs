//! Illuminance at a point on a model's surface, or averaged over a disc
//! round it: [`LightMeter`].
//!
//! The figure is the light arriving on the surface, not the light it sends
//! to the eye, so the surface's own colour plays no part in it; the colours
//! of the surfaces light bounces off do. Light comes from the lights and,
//! unless left out ([`GlowLight`]), from the model's glowing surfaces.

mod disc;
mod exposure;
mod gpu_meter;
mod map;
mod plan;
mod types;

#[cfg(test)]
mod exposure_tests;
#[cfg(test)]
mod gpu_tests;
#[cfg(test)]
mod tests;

pub use exposure::{ExposedLighting, Exposure, ExposureError};
pub use gpu_meter::GpuLightMeter;
pub use map::GpuLightRead;
pub use map::{
    GpuLightMap, IlluminanceMap, MAP_VERTEX_FLOATS, MAX_MAP_POINTS, MAX_MAP_RAYS, MapPatch,
    MapProgress, MapSide,
};
pub use types::{
    GlowLight, Illuminance, IlluminanceError, IlluminanceNote, IlluminanceQuery, IlluminanceUnit,
    LightShare, LightTint, MeterBasis, MeterSettings, RelativeReason,
};

use crate::camera::pick_index::PickIndex;
use crate::indirect::{Lamp, Scene, Volume, for_chunks, glow_gather};
use crate::lighting::Shader;
use crate::lighting::bounce::{Bounce, BounceSky, Painted, frame_lamps};
use crate::model::light::unit;
use crate::render::raster::Paint;
use crate::vec3::{add, length, scale, sub};
use crate::{AssembledModel, Bounds, Camera, PreparedModel, RenderOptions, TriangleMesh};

/// The most samples one [`LightMeter::read`] takes over a disc.
pub const MAX_ILLUMINANCE_SAMPLES: usize = 4096;
/// The most rays one [`LightMeter::read`] casts.
pub const MAX_ILLUMINANCE_RAYS: usize = 4_000_000;
/// The most [`MeterSettings::gather_rays`] used.
const MAX_GATHER_RAYS: usize = 4096;
/// Rec. 709 luminance weights: one number from linear red, green, blue.
const LUMINANCE: [f64; 3] = [0.2126, 0.7152, 0.0722];

/// One linear RGB triple as one number.
fn luminance(c: [f64; 3]) -> f64 {
    c.iter().zip(LUMINANCE).map(|(c, w)| c * w).sum()
}

/// Measures the light arriving on a model's surfaces under one set of
/// lights. Built once ([`Self::new`], which builds the bounce as a picture
/// does), then read at as many points as wanted ([`Self::read`]).
///
/// Every mesh of the model blocks and bounces light, painted as a solid
/// picture paints it, whatever the render mode.
pub struct LightMeter<'m> {
    meshes: &'m [TriangleMesh],
    /// Meshes drawn fully transparent, which a map leaves out.
    hidden: Vec<bool>,
    /// The camera a map takes a triangle's seen side from.
    camera: Camera,
    paints: Vec<Paint<'m>>,
    index: PickIndex,
    /// Every light, and those among them that bounce.
    lamps: Vec<Lamp>,
    bouncing: Vec<Lamp>,
    names: Vec<(String, Option<usize>)>,
    /// The radiance each mesh gives off; empty when no glow is counted.
    glow: Vec<[f64; 3]>,
    volume: Option<Volume>,
    strength: f64,
    sky: [f64; 3],
    ambient: [f64; 3],
    /// Longer than any path inside the model.
    far: f64,
    gather_rays: usize,
    threads: usize,
    unit: IlluminanceUnit,
    reasons: Vec<RelativeReason>,
    notes: Vec<IlluminanceNote>,
}

impl std::fmt::Debug for LightMeter<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LightMeter")
            .field("unit", &self.unit)
            .field("lamps", &self.lamps.len())
            .field("bounce", &self.volume.is_some())
            .finish_non_exhaustive()
    }
}

impl<'m> LightMeter<'m> {
    /// A meter for `model` under the lights a picture from `camera` with
    /// `options` uses: the model's own under
    /// [`LightingScheme::Artwork`](crate::LightingScheme::Artwork), else the
    /// scheme's; an aimed sun; and the bounce settings
    /// ([`RenderOptions::indirect`]). The camera matters only for a light
    /// that follows it.
    ///
    /// The figures are lux when every light in force has a
    /// [`ModelLight::photometric`](crate::ModelLight::photometric), the
    /// model's unit is known and no glowing surface is counted; otherwise
    /// they are in the picture's own shading units, and [`Self::reasons`]
    /// says why.
    ///
    /// A glowing surface ([`MeterSettings::glow`]) lights what it faces
    /// with its emissive colour: in a relative reading a surface that sees
    /// nothing but emissive white reads 1; in lux, one that sees nothing
    /// but a glow of luminance L cd/m² reads π·L. That light is
    /// [`Illuminance::emitted`]; where bounce
    /// is on, what other surfaces reflect of it is in
    /// [`Illuminance::bounced`].
    ///
    /// # Errors
    ///
    /// [`IlluminanceError::Render`] as [`crate::render`] for the camera,
    /// the light gain and an aimed sun.
    pub fn new(
        model: &'m AssembledModel,
        prepared: &PreparedModel,
        camera: &Camera,
        options: &RenderOptions,
        settings: &MeterSettings,
    ) -> Result<Self, IlluminanceError> {
        let mut meter = Self::unbuilt(model, prepared, camera, options, settings)?;
        let threads = meter.threads;
        meter.on_volume(|volume, scene, _| volume.build(scene, threads));
        Ok(meter)
    }

    /// [`Self::new`] with the bounce volume made but not built.
    fn unbuilt(
        model: &'m AssembledModel,
        prepared: &PreparedModel,
        camera: &Camera,
        options: &RenderOptions,
        settings: &MeterSettings,
    ) -> Result<Self, IlluminanceError> {
        let basis = camera.basis()?;
        let plan = plan::Plan::of(model, options, settings);
        let shader = Shader::for_frame(&plan.options, &plan.lights, &basis)?;
        let artwork = crate::lighting::model_lit(&plan.options, &plan.lights);
        let all = frame_lamps(&shader, &plan.options, artwork, camera, &basis);
        let names = plan::names(&plan, all.len());
        let indirect = plan.options.indirect;
        let on = indirect.bounce != Bounce::Off && indirect.strength() > 0.0;
        let bouncing: Vec<Lamp> = all
            .iter()
            .filter_map(|(lamp, bounces)| (on && *bounces).then_some(*lamp))
            .collect();
        let lamps: Vec<Lamp> = all.into_iter().map(|(lamp, _)| lamp).collect();
        let meshes = &model.meshes[..];
        let (paints, _) = prepared.paints(model, options);
        let index = PickIndex::new(meshes);
        let sky = match indirect.bounce_sky {
            BounceSky::Dark => [0.0; 3],
            BounceSky::Background => {
                let [r, g, b, a] = options.background.map(|c| f64::from(c) / 255.0);
                [r * a, g * a, b * a]
            }
        };
        let glow = plan.glow;
        let volume = match on && !(bouncing.is_empty() && glow.is_empty()) {
            false => None,
            true => Volume::new(meshes),
        };
        let far = Bounds::of(meshes).map_or(1.0, |b| 2.0 * length(sub(b.max, b.min)));
        Ok(LightMeter {
            meshes,
            camera: *camera,
            hidden: (0..meshes.len())
                .map(|m| {
                    model
                        .colours
                        .get(m)
                        .copied()
                        .flatten()
                        .is_some_and(|c| c[3] == 0)
                })
                .collect(),
            paints,
            index,
            lamps,
            bouncing,
            names,
            glow,
            volume,
            strength: indirect.strength(),
            sky,
            ambient: plan::ambient(&shader),
            far: if far > 0.0 { far } else { 1.0 },
            gather_rays: settings.gather_rays.clamp(1, MAX_GATHER_RAYS),
            threads: options.threads,
            unit: plan.unit,
            reasons: plan.reasons,
            notes: plan.notes,
        })
    }

    /// Runs `f` on the bounce volume and the scene it is built from (the
    /// bouncing lamps and the glow); `None` when there is no volume.
    fn on_volume<T>(&mut self, f: impl FnOnce(&mut Volume, &Scene, &Painted) -> T) -> Option<T> {
        let mut volume = self.volume.take()?;
        let surfaces = Painted {
            meshes: self.meshes,
            paints: &self.paints,
        };
        let scene = Scene {
            meshes: self.meshes,
            index: &self.index,
            surfaces: &surfaces,
            lamps: &self.bouncing,
            sky: self.sky,
            glow: &self.glow,
        };
        let out = f(&mut volume, &scene, &surfaces);
        self.volume = Some(volume);
        Some(out)
    }

    /// What every reading is in.
    #[must_use]
    pub fn unit(&self) -> IlluminanceUnit {
        self.unit
    }

    /// Why the readings are [`IlluminanceUnit::Relative`]; empty for lux.
    #[must_use]
    pub fn reasons(&self) -> &[RelativeReason] {
        &self.reasons
    }

    /// What the meter assumed or left out: to be shown beside a reading.
    #[must_use]
    pub fn notes(&self) -> &[IlluminanceNote] {
        &self.notes
    }

    /// The rays one sample costs.
    fn rays_per_sample(&self) -> usize {
        let gather = match self.volume {
            Some(_) => self.gather_rays.saturating_mul(1 + self.bouncing.len()),
            None if self.glow.is_empty() => 0,
            None => self.gather_rays,
        };
        gather.saturating_add(self.lamps.len() + 2)
    }

    /// The light arriving at `query`'s point, or the mean over its disc.
    ///
    /// A disc lies in the plane through the point square to its normal;
    /// each sample is dropped onto the picked mesh along the normal and
    /// measured where it lands. A sample that lands on nothing, or on a
    /// face turned more than 60 degrees from the normal, is counted in
    /// [`Illuminance::off_edge`] and left out of the mean.
    ///
    /// # Errors
    ///
    /// [`IlluminanceError::BadQuery`] for a point, normal or radius that is
    /// not usable; [`IlluminanceError::OffSurface`] when the point is not on
    /// the mesh; [`IlluminanceError::TooManySamples`] and
    /// [`IlluminanceError::TooMuchWork`] past the caps.
    pub fn read(&self, query: &IlluminanceQuery) -> Result<Illuminance, IlluminanceError> {
        let (spots, asked) = self.spots(query)?;
        let mut samples: Vec<Sample> = vec![Sample::default(); spots.len()];
        for_chunks(&mut samples, 0, self.threads, &|i, slot: &mut Sample| {
            if let Some(spot) = spots.get(i) {
                *slot = self.measure(spot.point, spot.normal);
            }
        });
        Ok(self.mean(&samples, asked - spots.len()))
    }

    /// The spots `query` measures and how many it asked for, after
    /// [`Self::read`]'s checks.
    fn spots(
        &self,
        query: &IlluminanceQuery,
    ) -> Result<(Vec<disc::Spot>, usize), IlluminanceError> {
        let normal = unit(query.normal).ok_or(IlluminanceError::BadQuery("normal"))?;
        if !query.point.iter().all(|c| c.is_finite()) {
            return Err(IlluminanceError::BadQuery("point"));
        }
        if !(query.radius.is_finite() && query.radius >= 0.0) {
            return Err(IlluminanceError::BadQuery("radius"));
        }
        let asked = if query.radius > 0.0 {
            query.samples.max(1)
        } else {
            1
        };
        if asked > MAX_ILLUMINANCE_SAMPLES {
            return Err(IlluminanceError::TooManySamples {
                asked,
                limit: MAX_ILLUMINANCE_SAMPLES,
            });
        }
        let rays = asked.saturating_mul(self.rays_per_sample());
        if rays > MAX_ILLUMINANCE_RAYS {
            return Err(IlluminanceError::TooMuchWork {
                rays,
                limit: MAX_ILLUMINANCE_RAYS,
            });
        }
        let spots = disc::spots(&self.index, self.far, query, normal, asked)
            .ok_or(IlluminanceError::OffSurface)?;
        Ok((spots, asked))
    }

    /// [`Self::measure`]'s direct light alone.
    fn measure_direct(&self, p: [f64; 3], n: [f64; 3]) -> Sample {
        let surfaces = Painted {
            meshes: self.meshes,
            paints: &self.paints,
        };
        let all = Scene {
            meshes: self.meshes,
            index: &self.index,
            surfaces: &surfaces,
            lamps: &self.lamps,
            sky: self.sky,
            glow: &self.glow,
        };
        let direct = self
            .lamps
            .iter()
            .map(|lamp| crate::indirect::lamp_direct(&all, lamp, p, n, self.far))
            .collect();
        Sample {
            direct,
            ..Sample::default()
        }
    }

    /// The light at `p` on the side its unit normal `n` faces.
    fn measure(&self, p: [f64; 3], n: [f64; 3]) -> Sample {
        let surfaces = Painted {
            meshes: self.meshes,
            paints: &self.paints,
        };
        let scene = |lamps| Scene {
            meshes: self.meshes,
            index: &self.index,
            surfaces: &surfaces,
            lamps,
            sky: self.sky,
            glow: &self.glow,
        };
        let direct = self.measure_direct(p, n).direct;
        let rays = self.gather_rays;
        let (bounced, emitted) = match &self.volume {
            Some(v) => {
                let (bounced, emitted) = v.gathered(&scene(&self.bouncing), p, n, rays);
                (scale(bounced, self.strength), emitted)
            }
            None if self.glow.is_empty() => ([0.0; 3], [0.0; 3]),
            None => ([0.0; 3], glow_gather(&scene(&[]), p, n, rays, self.far)),
        };
        Sample {
            direct,
            bounced,
            emitted,
        }
    }

    /// The mean of `samples` as a reading; `off_edge` samples were dropped.
    fn mean(&self, samples: &[Sample], off_edge: usize) -> Illuminance {
        let k = 1.0 / samples.len().max(1) as f64;
        let mut per_lamp = vec![[0.0; 3]; self.lamps.len()];
        let (mut bounced, mut emitted) = ([0.0; 3], [0.0; 3]);
        for s in samples {
            for (sum, d) in per_lamp.iter_mut().zip(&s.direct) {
                *sum = add(*sum, scale(*d, k));
            }
            bounced = add(bounced, scale(s.bounced, k));
            emitted = add(emitted, scale(s.emitted, k));
        }
        let lights: Vec<LightShare> = per_lamp
            .iter()
            .zip(&self.names)
            .map(|(d, (name, model_light))| LightShare {
                name: name.clone(),
                model_light: *model_light,
                direct: luminance(*d),
            })
            .collect();
        let direct = lights.iter().map(|l| l.direct).sum();
        let (bounced, ambient) = (luminance(bounced), luminance(self.ambient));
        let emitted = luminance(emitted);
        Illuminance {
            unit: self.unit,
            total: direct + bounced + ambient + emitted,
            direct,
            bounced,
            emitted,
            ambient,
            lights,
            on_surface: samples.len(),
            off_edge,
        }
    }
}

/// One sample: each lamp's direct light, the bounced light, and the light
/// straight from glowing surfaces.
#[derive(Debug, Clone, Default)]
struct Sample {
    direct: Vec<[f64; 3]>,
    bounced: [f64; 3],
    emitted: [f64; 3],
}
