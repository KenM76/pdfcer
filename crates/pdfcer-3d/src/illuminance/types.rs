//! What a [`LightMeter`](super::LightMeter) is asked and what it answers.

use crate::camera::pick::{Hit, Ray};
use crate::render::RenderError;
use crate::vec3::{dot, scale};

/// Whether a meter may answer in lux.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum MeterBasis {
    /// Lux when the lights and the model allow it, else relative.
    #[default]
    Auto,
    /// The picture's own shading units, whatever the lights carry.
    Relative,
}

/// What a lux reading does with a light's colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum LightTint {
    /// Every light counts at its full stated strength, as a lux meter
    /// reads it. glTF states a light's strength for its colour as given, so
    /// this is the file's figure.
    #[default]
    Neutral,
    /// Each light's strength is weighted by its colour's luminance.
    Coloured,
}

/// How a [`LightMeter`](super::LightMeter) measures.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct MeterSettings {
    /// Whether lux is wanted when it can be had.
    pub basis: MeterBasis,
    /// A light's colour in a lux reading.
    pub tint: LightTint,
    /// Millimetres per model unit, overriding the model's own
    /// ([`AssembledModel::units`](crate::AssembledModel::units)); `None`
    /// uses the model's. Lux from a point or spot light needs one.
    pub mm_per_unit: Option<f64>,
    /// Rays cast from each sample to gather the bounced light (128; at
    /// most 4096). More is steadier and slower.
    pub gather_rays: usize,
}

impl Default for MeterSettings {
    fn default() -> Self {
        MeterSettings {
            basis: MeterBasis::Auto,
            tint: LightTint::Neutral,
            mm_per_unit: None,
            gather_rays: 128,
        }
    }
}

/// Where to measure.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct IlluminanceQuery {
    /// The picked mesh, by index in
    /// [`AssembledModel::meshes`](crate::AssembledModel::meshes).
    pub mesh: usize,
    /// A point on it, model coordinates.
    pub point: [f64; 3],
    /// The side measured: the surface normal at the point, pointing to the
    /// side the light is wanted on. Any length.
    pub normal: [f64; 3],
    /// The disc's radius in model units; 0 measures the point alone.
    pub radius: f64,
    /// Samples over the disc (64); unused for a point.
    pub samples: usize,
}

impl IlluminanceQuery {
    /// The point `point` on mesh `mesh`, on the side `normal` points to.
    #[must_use]
    pub fn new(mesh: usize, point: [f64; 3], normal: [f64; 3]) -> Self {
        IlluminanceQuery {
            mesh,
            point,
            normal,
            radius: 0.0,
            samples: 64,
        }
    }

    /// The point `ray` picked, on the side the ray came from.
    #[must_use]
    pub fn from_pick(hit: &Hit, ray: &Ray) -> Self {
        let towards = if dot(hit.normal, ray.direction) > 0.0 {
            scale(hit.normal, -1.0)
        } else {
            hit.normal
        };
        Self::new(hit.mesh, hit.point, towards)
    }

    /// This query averaged over a disc of `radius` model units with
    /// `samples` samples.
    #[must_use]
    pub fn over_disc(self, radius: f64, samples: usize) -> Self {
        IlluminanceQuery {
            radius,
            samples,
            ..self
        }
    }
}

/// What a reading's numbers are in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum IlluminanceUnit {
    /// Lux: lumens per square metre.
    Lux,
    /// The picture's shading units: 1.0 lights a white surface to full
    /// white. Fit for comparing two places under the same lights, not a
    /// physical figure.
    Relative,
}

/// Why a meter answers in [`IlluminanceUnit::Relative`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RelativeReason {
    /// [`MeterBasis::Relative`] was asked for.
    Asked,
    /// The picture is not lit by the model's own lights: the model has
    /// none, or another lighting scheme is chosen.
    PictureLights,
    /// The light at the camera is on beside the model's lights; it has no
    /// physical strength.
    Headlight,
    /// An aimed sun is on; it has no physical strength.
    Sun,
    /// This many of the model's lights carry no physical strength, as
    /// every light PRC stored without one does.
    UnitlessLights(usize),
    /// A point or spot light needs the model's length unit, and it is not
    /// known.
    NoUnits,
    /// The bounce takes light from the picture's background colour, which
    /// has no physical strength.
    Sky,
}

impl std::fmt::Display for RelativeReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RelativeReason::Asked => f.write_str("a relative figure was asked for"),
            RelativeReason::PictureLights => {
                f.write_str("the picture is not lit by lights of the model")
            }
            RelativeReason::Headlight => {
                f.write_str("the light at the camera has no physical strength")
            }
            RelativeReason::Sun => f.write_str("the aimed sun has no physical strength"),
            RelativeReason::UnitlessLights(n) => {
                write!(f, "{n} of the model's lights carry no physical strength")
            }
            RelativeReason::NoUnits => f.write_str("the model's length unit is not known"),
            RelativeReason::Sky => f.write_str("the bounce takes light from the background colour"),
        }
    }
}

/// Something a meter assumed or left out.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum IlluminanceNote {
    /// Lux comes from each light's stated strength; the scale the importer
    /// gave the lights for the picture, the light gain and the lamp cut-off
    /// are not used.
    PictureScaleUnused,
    /// The ambient fill the importer added for the picture stands for no
    /// physical light and is left out.
    AmbientFillLeftOut,
    /// The model's unit is the exporter's default, not one a CAD system
    /// stated; distances to point and spot lights rest on it.
    UnitAssumed,
    /// The model's unit was overridden by
    /// [`MeterSettings::mm_per_unit`].
    UnitGiven,
    /// Each light's strength was weighted by its colour
    /// ([`LightTint::Coloured`]).
    TintApplied,
    /// Bounce is off or nothing bounces: the figure is direct light only.
    DirectOnly,
    /// The bounced part is an estimate: light past the first bounce is
    /// read from a coarse lattice of probes.
    BounceEstimated,
    /// The bounced part is multiplied by this bounce strength, which is
    /// not 1.
    BounceStrength(f64),
}

impl std::fmt::Display for IlluminanceNote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IlluminanceNote::PictureScaleUnused => f.write_str(
                "each light counts at its stated strength; the picture's light scaling is not used",
            ),
            IlluminanceNote::AmbientFillLeftOut => {
                f.write_str("the ambient fill added for the picture is left out")
            }
            IlluminanceNote::UnitAssumed => {
                f.write_str("the model's length unit is assumed, not stated by a CAD system")
            }
            IlluminanceNote::UnitGiven => f.write_str("the model's length unit was given"),
            IlluminanceNote::TintApplied => {
                f.write_str("each light's strength is weighted by its colour")
            }
            IlluminanceNote::DirectOnly => f.write_str("no bounced light: direct light only"),
            IlluminanceNote::BounceEstimated => f.write_str("the bounced part is an estimate"),
            IlluminanceNote::BounceStrength(s) => {
                write!(f, "the bounced part is multiplied by bounce strength {s}")
            }
        }
    }
}

/// One light's direct part of a reading.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LightShare {
    /// The light's name in the model, or what it is: `light 3`,
    /// `headlight`, `sun`, `scheme light 1`.
    pub name: String,
    /// Its index in [`AssembledModel::lights`](crate::AssembledModel::lights),
    /// for one of the model's.
    pub model_light: Option<usize>,
    /// The light it puts straight on the surface, shadows counted.
    pub direct: f64,
}

/// The light arriving on a surface ([`LightMeter::read`](super::LightMeter::read)).
/// Over a disc every number is the mean of the samples that landed.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Illuminance {
    /// What the numbers are in.
    pub unit: IlluminanceUnit,
    /// [`Self::direct`] + [`Self::bounced`] + [`Self::ambient`].
    pub total: f64,
    /// Straight from the lights, shadowed by the model.
    pub direct: f64,
    /// Reflected off the model's other surfaces.
    pub bounced: f64,
    /// The lights' ambient part, which falls everywhere alike and is not
    /// shadowed. Zero in lux unless a light states an ambient strength.
    pub ambient: f64,
    /// [`Self::direct`] light by light, in the picture's order; ambient
    /// lights are not listed.
    pub lights: Vec<LightShare>,
    /// Samples that landed on the picked surface (1 for a point).
    pub on_surface: usize,
    /// Samples of a disc that fell off the surface's edge and were left
    /// out.
    pub off_edge: usize,
}

/// Why a reading was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum IlluminanceError {
    /// The query's named part is not finite, or the normal has no
    /// direction, or the radius is negative.
    #[error("the query's {0} is not usable")]
    BadQuery(&'static str),
    /// The point is not on the mesh it names.
    #[error("the point is not on the picked surface")]
    OffSurface,
    /// More samples than [`MAX_ILLUMINANCE_SAMPLES`](super::MAX_ILLUMINANCE_SAMPLES).
    #[error("{asked} samples asked for; the most is {limit}")]
    TooManySamples {
        /// Samples asked for.
        asked: usize,
        /// The cap.
        limit: usize,
    },
    /// More rays than [`MAX_ILLUMINANCE_RAYS`](super::MAX_ILLUMINANCE_RAYS):
    /// ask for fewer samples or fewer gather rays.
    #[error("the reading needs {rays} rays; the most is {limit}")]
    TooMuchWork {
        /// Rays the reading would cast.
        rays: usize,
        /// The cap.
        limit: usize,
    },
    /// The camera or the lights are not usable.
    #[error(transparent)]
    Render(#[from] RenderError),
}
