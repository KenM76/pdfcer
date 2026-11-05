//! `3d-render`'s shadow settings and the notes saying what cast shadows.

use super::RenderThreeDArgs;
use crate::{ShadowAlphaArg, ShadowCastersArg, ShadowLightArg};
use pdfcer_3d::{
    AssembledModel, Bounds, Camera, FrameExtraShadows, FrameLampShadows, LampCut, RenderOptions,
    ShadowAlpha, ShadowCasters,
};

/// `--shadow-casters` as the engine's setting.
pub(super) fn casters(arg: ShadowCastersArg) -> ShadowCasters {
    match arg {
        ShadowCastersArg::Every => ShadowCasters::EveryLight,
        ShadowCastersArg::Key => ShadowCasters::KeyLight,
    }
}

/// `--shadow-alpha` and `--shadow-cutoff` (0–1, rounded to the nearest
/// 255th) as the engine's setting.
pub(super) fn alpha(arg: ShadowAlphaArg, cutoff: f64) -> ShadowAlpha {
    match arg {
        ShadowAlphaArg::Transmit => ShadowAlpha::Transmit,
        ShadowAlphaArg::Cutout => ShadowAlpha::Cutout {
            cutoff: (cutoff.clamp(0.0, 1.0) * 255.0).round() as u8,
        },
        ShadowAlphaArg::Opaque => ShadowAlpha::OpaqueOnly,
    }
}

/// Prints the `shadows` and `lamp shadows` notes: which lights cast, how
/// many lights the cap left without a shadow, and how see-through surfaces
/// cast.
pub(super) fn print(
    a: &RenderThreeDArgs<'_>,
    model: &AssembledModel,
    camera: &Camera,
    options: &RenderOptions,
) {
    println!("note: shadows: {}", shadows(a, model, camera, options));
    println!(
        "note: lamp shadows: {}",
        lamp_shadows(a, model, camera, options)
    );
    if let Some(reach) = lamp_reach(model, camera, options) {
        println!("note: lamp reach: {reach}");
    }
}

/// The `lamp reach` note: how far the model's point and spot lights carry
/// (`--lamp-cut`); `None` without any.
fn lamp_reach(model: &AssembledModel, camera: &Camera, options: &RenderOptions) -> Option<String> {
    let lights = pdfcer_3d::FrameLights::for_model(camera, options, &model.lights).ok()?;
    let n = lights.lamps.in_model;
    if n == 0 {
        return None;
    }
    Some(match options.lamp_cut {
        LampCut::Off => format!("every one of {n} lamps reaches everywhere (--lamp-cut off)"),
        LampCut::Auto => format!(
            "each of {n} lamps stops where the rest of its light would change no pixel \
             (--lamp-cut auto)"
        ),
        LampCut::Threshold(h) => format!(
            "each of {n} lamps stops where it has faded to its share of {} 8-bit levels, \
             so up to that much of their light is left out of any point (--lamp-cut)",
            f64::from(h) / 100.0
        ),
    })
}

fn lamp_shadows(
    a: &RenderThreeDArgs<'_>,
    model: &AssembledModel,
    camera: &Camera,
    options: &RenderOptions,
) -> String {
    if !(a.shadows && a.lamp_shadows) {
        return "off".to_owned();
    }
    let mut out = "cast by the model's own point and spot lights, if any \
                   (`--no-lamp-shadows` turns them off)"
        .to_owned();
    let dropped = Bounds::of(&model.meshes)
        .and_then(|bounds| {
            FrameLampShadows::for_model(camera, options, &bounds, &model.lights)
                .ok()
                .flatten()
        })
        .map_or(0, |l| l.dropped);
    if dropped > 0 {
        out.push_str(&format!(
            "; {dropped} more past the limit of {} light without a shadow",
            pdfcer_3d::MAX_SHADOW_CASTERS
        ));
    }
    out
}

/// How several casters share the shadows, and any past the limit.
fn extras_tail(e: &FrameExtraShadows) -> String {
    let mut tail = ", each shadow taking only its own light".to_owned();
    if e.dropped > 0 {
        tail.push_str(&format!(
            "; {} more directional light(s) past the limit of {} casters light without a \
             shadow",
            e.dropped,
            pdfcer_3d::MAX_SHADOW_CASTERS
        ));
    }
    tail
}

fn shadows(
    a: &RenderThreeDArgs<'_>,
    model: &AssembledModel,
    camera: &Camera,
    options: &RenderOptions,
) -> String {
    if !a.shadows {
        return "off".to_owned();
    }
    let extra = || {
        Bounds::of(&model.meshes).and_then(|bounds| {
            FrameExtraShadows::for_model(camera, options, &bounds, &model.lights)
                .ok()
                .flatten()
        })
    };
    let who = match (options.shadow_light, a.shadow_light) {
        (pdfcer_3d::ShadowLight::AimedSun(sun), _) => {
            let sun = super::light_args::sun_note(&sun);
            match extra() {
                None => sun,
                Some(e) => format!(
                    "{sun}, and {} more of the lights in use{}",
                    e.count,
                    extras_tail(&e)
                ),
            }
        }
        (_, ShadowLightArg::Sun) => "a sun fixed above the model on the `--up` axis".to_owned(),
        (_, ShadowLightArg::SunNegative) => {
            "a sun fixed on the negative side of the `--up` axis".to_owned()
        }
        (_, ShadowLightArg::Scheme) => match extra() {
            None => "one of the lights in use".to_owned(),
            Some(e) => format!("{} of the lights in use{}", e.count + 1, extras_tail(&e)),
        },
    };
    let see_through = match options.shadow_alpha {
        ShadowAlpha::Transmit => {
            "translucent surfaces cast a shadow as dark as they are opaque".to_owned()
        }
        ShadowAlpha::Cutout { cutoff } => format!(
            "translucent surfaces cast a full shadow where their alpha reaches {:.3}, none \
             elsewhere",
            f64::from(cutoff) / 255.0
        ),
        _ => "only wholly opaque surfaces cast".to_owned(),
    };
    format!(
        "cast by {who}; {see_through} (`--no-shadows` turns them off, `--shadow-casters`, \
         `--shadow-alpha` choose what casts)"
    )
}
