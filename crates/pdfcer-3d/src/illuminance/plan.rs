//! Which lights a meter reads, in which unit, and what it has to say
//! about that.

use super::{
    IlluminanceNote, IlluminanceUnit, LightTint, MeterBasis, MeterSettings, RelativeReason,
};
use crate::lighting::bounce::{Bounce, BounceSky};
use crate::lighting::{LightingScheme, Shader};
use crate::model::light::unit;
use crate::{
    AssembledModel, LampCut, ModelLight, ModelLightKind, Photometric, RenderOptions, ShadowLight,
};

/// The picture's ambient share of the default light at the camera.
const HEADLIGHT_AMBIENT: f64 = 0.3;

/// A meter's lights and unit.
pub(super) struct Plan {
    pub(super) unit: IlluminanceUnit,
    pub(super) reasons: Vec<RelativeReason>,
    pub(super) notes: Vec<IlluminanceNote>,
    /// The model's lights as the meter shades them: the model's own for a
    /// relative reading, their physical strengths for lux.
    pub(super) lights: Vec<ModelLight>,
    pub(super) options: RenderOptions,
}

impl Plan {
    /// Decides once, for every reading, whether lux can be honest and what
    /// stands in the way, so a point and a disc never disagree on the unit.
    pub(super) fn of(model: &AssembledModel, options: &RenderOptions, s: &MeterSettings) -> Plan {
        let given = s.mm_per_unit.filter(|m| m.is_finite() && *m > 0.0);
        let mm = given.or(model.units.map(|u| u.mm_per_unit));
        let reasons = reasons(&model.lights, options, s, mm.is_some());
        let mut notes = Vec::new();
        let (unit, lights, options) = if reasons.is_empty() {
            let lamps = model.lights.iter().any(|l| candela(l) > 0.0);
            notes.push(IlluminanceNote::PictureScaleUnused);
            if model.lights.iter().any(is_fill) {
                notes.push(IlluminanceNote::AmbientFillLeftOut);
            }
            match (lamps, given, model.units) {
                (true, Some(_), _) => notes.push(IlluminanceNote::UnitGiven),
                (true, None, Some(u)) if !u.from_cad_file => {
                    notes.push(IlluminanceNote::UnitAssumed);
                }
                _ => {}
            }
            if s.tint == LightTint::Coloured {
                notes.push(IlluminanceNote::TintApplied);
            }
            let metres = mm.unwrap_or(1000.0) / 1000.0;
            let lights = model
                .lights
                .iter()
                .map(|l| physical(l, s.tint, metres))
                .collect();
            let options = RenderOptions {
                light_gain: 1.0,
                lamp_cut: LampCut::Off,
                ..*options
            };
            (IlluminanceUnit::Lux, lights, options)
        } else {
            (IlluminanceUnit::Relative, model.lights.clone(), *options)
        };
        notes.extend(bounce_notes(&options));
        Plan {
            unit,
            reasons,
            notes,
            lights,
            options,
        }
    }

    /// Whether the picture is lit by the model's own lights.
    fn artwork(&self) -> bool {
        artwork(&self.lights, &self.options)
    }
}

fn artwork(lights: &[ModelLight], options: &RenderOptions) -> bool {
    crate::lighting::model_lit(options, lights)
}

/// The candela of a point or spot light; 0 for any other light.
fn candela(l: &ModelLight) -> f64 {
    match l.photometric {
        Some(Photometric::Candela(v)) => v,
        _ => 0.0,
    }
}

/// Whether `l` is an ambient light standing for no physical light.
fn is_fill(l: &ModelLight) -> bool {
    matches!(l.kind, ModelLightKind::Ambient) && l.photometric == Some(Photometric::Lux(0.0))
}

/// Why a reading cannot be lux; empty when it can.
fn reasons(
    lights: &[ModelLight],
    options: &RenderOptions,
    s: &MeterSettings,
    units: bool,
) -> Vec<RelativeReason> {
    let mut out = Vec::new();
    if s.basis == MeterBasis::Relative {
        out.push(RelativeReason::Asked);
    }
    if !artwork(lights, options) {
        out.push(RelativeReason::PictureLights);
        return out;
    }
    if options.artwork_headlight {
        out.push(RelativeReason::Headlight);
    }
    if matches!(options.shadow_light, ShadowLight::AimedSun(_)) {
        out.push(RelativeReason::Sun);
    }
    let unitless = lights.iter().filter(|l| l.photometric.is_none()).count();
    if unitless > 0 {
        out.push(RelativeReason::UnitlessLights(unitless));
    }
    if !units && lights.iter().any(|l| candela(l) > 0.0) {
        out.push(RelativeReason::NoUnits);
    }
    let indirect = options.indirect;
    let bounce = indirect.bounce != Bounce::Off && indirect.strength() > 0.0;
    if bounce && indirect.bounce_sky == BounceSky::Background {
        out.push(RelativeReason::Sky);
    }
    out
}

/// `l` shaded by its physical strength: a directional light's colour times
/// intensity is its lux; a point or spot light's is its candela, fading
/// with the square of the distance in metres (`metres` per model unit).
fn physical(l: &ModelLight, tint: LightTint, metres: f64) -> ModelLight {
    let fade = [0.0, 0.0, metres * metres];
    let kind = match l.kind {
        ModelLightKind::Point { position, .. } => ModelLightKind::Point {
            position,
            attenuation: fade,
        },
        ModelLightKind::Spot {
            position,
            direction,
            angle,
            exponent,
            ..
        } => ModelLightKind::Spot {
            position,
            direction,
            attenuation: fade,
            angle,
            exponent,
        },
        kind => kind,
    };
    ModelLight {
        kind,
        colour: match tint {
            LightTint::Neutral => [1.0; 3],
            LightTint::Coloured => l.colour,
        },
        ambient: [0.0; 3],
        intensity: l.photometric.map_or(0.0, Photometric::value),
        ..l.clone()
    }
}

/// What to say about the bounce `options` asks for.
fn bounce_notes(options: &RenderOptions) -> Vec<IlluminanceNote> {
    let indirect = options.indirect;
    let strength = indirect.strength();
    if indirect.bounce == Bounce::Off || strength == 0.0 {
        return vec![IlluminanceNote::DirectOnly];
    }
    let mut out = vec![IlluminanceNote::BounceEstimated];
    if (strength - 1.0).abs() > 1e-12 {
        out.push(IlluminanceNote::BounceStrength(strength));
    }
    out
}

/// The shader's ambient light.
pub(super) fn ambient(shader: &Shader) -> [f64; 3] {
    match shader {
        Shader::Set { ambient, .. } => *ambient,
        Shader::Headlight { gain } => [HEADLIGHT_AMBIENT * gain; 3],
        // Unlit: every surface is drawn at its full colour.
        Shader::Unlit => [1.0; 3],
    }
}

/// A name and model-light index for each of the `count` lamps
/// [`frame_lamps`](crate::lighting::bounce::frame_lamps) gives for `plan`:
/// directional lights, the light at the camera, the sun, then point and
/// spot lights.
pub(super) fn names(plan: &Plan, count: usize) -> Vec<(String, Option<usize>)> {
    let sun = matches!(plan.options.shadow_light, ShadowLight::AimedSun(_));
    let (mut far, mut near) = (Vec::new(), Vec::new());
    if plan.artwork() {
        for (i, l) in plan.lights.iter().enumerate() {
            let name = l.name.clone().unwrap_or_else(|| format!("light {}", i + 1));
            match l.kind {
                ModelLightKind::Ambient => {}
                ModelLightKind::Directional { direction } => {
                    if unit(direction).is_some() {
                        far.push((name, Some(i)));
                    }
                }
                _ => near.push((name, Some(i))),
            }
        }
        if plan.options.artwork_headlight {
            far.push(("headlight".to_owned(), None));
        }
    }
    let scheme = count.saturating_sub(far.len() + near.len() + usize::from(sun));
    if scheme == 1 && !plan.artwork() && plan.options.lighting == LightingScheme::Artwork {
        far.push(("headlight".to_owned(), None));
    } else {
        far.extend((1..=scheme).map(|k| (format!("scheme light {k}"), None)));
    }
    if sun {
        far.push(("sun".to_owned(), None));
    }
    far.extend(near);
    far.resize(count, ("light".to_owned(), None));
    far
}
