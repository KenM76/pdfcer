//! [`Exposure`]: a picture of the model's own lights drawn at a level fixed
//! in lux, so a dim room looks dim beside a bright one.

use super::plan::Plan;
use super::{GlowLight, IlluminanceNote, MeterSettings, RelativeReason};
use crate::{
    AssembledModel, Camera, FrameBounce, FrameLights, Image, ModelLight, PreparedModel,
    RenderError, RenderOptions,
};

/// How bright a picture is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
#[non_exhaustive]
pub enum Exposure {
    /// The picture as drawn without this setting: the model's lights at the
    /// strengths they are stored with (scaled on import to suit the model),
    /// times [`RenderOptions::light_gain`].
    #[default]
    Metered,
    /// A white surface that a light meter ([`LightMeter`](super::LightMeter))
    /// would read at `white_lux` lux is drawn full white, and every lit
    /// surface in proportion to its reading, so the same lamps in a darker
    /// building draw darker. Each light shines with its stated physical
    /// strength ([`ModelLight::photometric`]), not the strength it is drawn
    /// with: the scale an importer gave the lights, the ambient fill it
    /// added and [`RenderOptions::light_gain`] play no part. A colour
    /// brighter than full white rolls toward white
    /// ([`crate::OverRange::TowardWhite`]) on both paths, unless the
    /// options ask for [`crate::OverRange::Clip`].
    Fixed {
        /// Lux drawn as full white; positive and finite.
        white_lux: f64,
    },
}

/// The lights and options a frame is drawn with under one [`Exposure`]
/// ([`Exposure::lighting`]). The CPU picture ([`Self::render`]) and every
/// piece of the GPU frame data are built from these, so the two pictures
/// match: pass [`Self::lights`] and [`Self::options`] wherever a GPU call
/// takes a model's lights and options ([`FrameLights::for_model`],
/// [`crate::FrameLampShadows::for_model`],
/// [`crate::FrameExtraShadows::for_model`],
/// [`crate::FrameShadow`], [`crate::FrameAmbientOcclusion`]), or use the
/// methods here.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ExposedLighting {
    /// The lights to draw with, in the model's order.
    pub lights: Vec<ModelLight>,
    /// The options to draw with.
    pub options: RenderOptions,
    /// Lux drawn as full white; `None` under [`Exposure::Metered`].
    pub white_lux: Option<f64>,
    /// What the figures rest on (the meter's own notes, such as an assumed
    /// length unit), to be shown beside the white point; empty under
    /// [`Exposure::Metered`].
    pub notes: Vec<IlluminanceNote>,
}

/// Why [`Exposure::Fixed`] was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ExposureError {
    /// `white_lux` is not a positive finite number.
    #[error("the lux drawn as full white must be a positive number")]
    WhiteLux,
    /// The lights lighting the picture do not all carry a physical
    /// strength, for these reasons (the meter's: it would read relative).
    #[error("the picture cannot be drawn in lux: {}", join(.0))]
    Relative(Vec<RelativeReason>),
}

fn join(reasons: &[RelativeReason]) -> String {
    let parts: Vec<String> = reasons.iter().map(ToString::to_string).collect();
    parts.join("; ")
}

impl Exposure {
    /// The lights and options a frame of `model` with `options` is drawn
    /// with. [`Exposure::Metered`] gives the model's lights and `options`
    /// unchanged.
    ///
    /// [`Exposure::Fixed`] lights the picture as `settings`'
    /// [`LightMeter`](super::LightMeter) reads (its tint and length unit),
    /// with the meter's light strengths divided by `white_lux`, a light
    /// gain of 1, and [`crate::OverRange::Auto`] read as
    /// [`crate::OverRange::TowardWhite`]. A glowing surface draws its own colour as in any picture
    /// and lights nothing ([`GlowLight::LeftOut`] whatever `settings` say).
    ///
    /// # Errors
    ///
    /// [`ExposureError::WhiteLux`] for a `white_lux` that is not positive
    /// and finite; [`ExposureError::Relative`] when the meter would read
    /// relative ([`super::LightMeter::reasons`]): the picture is not lit by
    /// the model's lights, a light carries no physical strength, the length
    /// unit a lamp needs is unknown, or a headlight, aimed sun or
    /// background-lit bounce is on.
    ///
    /// ```
    /// use pdfcer_3d::{AssembledModel, Exposure, ExposureError, MeterSettings, RelativeReason};
    /// use pdfcer_3d::RenderOptions;
    /// let model = AssembledModel::default();
    /// let o = RenderOptions::default();
    /// let s = MeterSettings::default();
    /// assert_eq!(Exposure::Metered.lighting(&model, &o, &s)?.options, o);
    /// // No lights of the model's own: nothing to draw in lux.
    /// assert_eq!(
    ///     Exposure::Fixed { white_lux: 500.0 }.lighting(&model, &o, &s),
    ///     Err(ExposureError::Relative(vec![RelativeReason::PictureLights]))
    /// );
    /// # Ok::<(), ExposureError>(())
    /// ```
    pub fn lighting(
        self,
        model: &AssembledModel,
        options: &RenderOptions,
        settings: &MeterSettings,
    ) -> Result<ExposedLighting, ExposureError> {
        let white_lux = match self {
            Exposure::Metered => {
                return Ok(ExposedLighting {
                    lights: model.lights.clone(),
                    options: *options,
                    white_lux: None,
                    notes: Vec::new(),
                });
            }
            Exposure::Fixed { white_lux } => white_lux,
        };
        if !(white_lux.is_finite() && white_lux > 0.0) {
            return Err(ExposureError::WhiteLux);
        }
        let settings = MeterSettings {
            glow: GlowLight::LeftOut,
            ..*settings
        };
        let plan = Plan::of(model, options, &settings);
        if !plan.reasons.is_empty() {
            return Err(ExposureError::Relative(plan.reasons));
        }
        let lights = plan
            .lights
            .into_iter()
            .map(|l| ModelLight {
                intensity: l.intensity / white_lux,
                ..l
            })
            .collect();
        let mut notes: Vec<IlluminanceNote> = plan
            .notes
            .into_iter()
            .filter(|n| !matches!(n, IlluminanceNote::GlowLeftOut(_)))
            .collect();
        let relative = model
            .finishes
            .iter()
            .flatten()
            .filter(|f| f.is_emissive() && f.emissive_nits.is_none())
            .count();
        if relative > 0 {
            notes.push(IlluminanceNote::GlowRelative(relative));
        }
        Ok(ExposedLighting {
            lights,
            options: RenderOptions {
                light_gain: 1.0,
                over_range: options.over_range.in_lux(),
                emissive_white_lux: Some(white_lux),
                ..*options
            },
            white_lux: Some(white_lux),
            notes,
        })
    }
}

impl ExposedLighting {
    /// `model` drawn as [`PreparedModel::render`] draws it, with these
    /// lights and options.
    ///
    /// # Errors
    ///
    /// As [`PreparedModel::render`].
    pub fn render(
        &self,
        prepared: &PreparedModel,
        model: &AssembledModel,
        camera: &Camera,
    ) -> Result<Image, RenderError> {
        crate::render::mode_frame::render(prepared, (model, &self.lights), camera, &self.options)
    }

    /// [`FrameLights::for_model`] with these lights and options.
    ///
    /// # Errors
    ///
    /// As [`FrameLights::for_model`].
    pub fn frame_lights(&self, camera: &Camera) -> Result<FrameLights, RenderError> {
        FrameLights::for_model(camera, &self.options, &self.lights)
    }

    /// [`PreparedModel::frame_bounce`] with these lights and options.
    ///
    /// # Errors
    ///
    /// As [`PreparedModel::frame_bounce`].
    pub fn frame_bounce(
        &self,
        prepared: &PreparedModel,
        model: &AssembledModel,
        camera: &Camera,
    ) -> Result<Option<FrameBounce>, RenderError> {
        prepared.frame_bounce_lit((model, &self.lights), camera, &self.options)
    }

    /// [`PreparedModel::gpu_bounce_build`] with these lights and options:
    /// the GPU twin of [`Self::frame_bounce`], building the same bounce.
    ///
    /// # Errors
    ///
    /// As [`PreparedModel::gpu_bounce_build`].
    pub fn gpu_bounce_build(
        &self,
        prepared: &PreparedModel,
        model: &AssembledModel,
        camera: &Camera,
    ) -> Result<Option<crate::GpuBounceBuild>, RenderError> {
        prepared.gpu_bounce_build_lit((model, &self.lights), camera, &self.options)
    }
}
