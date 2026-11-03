//! `3d-render`'s shadow settings and the notes saying what cast shadows.

use super::RenderThreeDArgs;
use crate::{ShadowAlphaArg, ShadowCastersArg, ShadowLightArg};
use pdfcer_3d::{
    AssembledModel, Bounds, Camera, FrameExtraShadows, FrameLampShadows, RenderOptions,
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

fn shadows(
    a: &RenderThreeDArgs<'_>,
    model: &AssembledModel,
    camera: &Camera,
    options: &RenderOptions,
) -> String {
    let who = match a.shadow_light {
        _ if !a.shadows => return "off".to_owned(),
        ShadowLightArg::Sun => "a sun fixed above the model on the `--up` axis".to_owned(),
        ShadowLightArg::SunNegative => {
            "a sun fixed on the negative side of the `--up` axis".to_owned()
        }
        ShadowLightArg::Scheme => {
            let extra = Bounds::of(&model.meshes).and_then(|bounds| {
                FrameExtraShadows::for_model(camera, options, &bounds, &model.lights)
                    .ok()
                    .flatten()
            });
            match extra {
                None => "one of the lights in use".to_owned(),
                Some(e) => {
                    let mut who = format!(
                        "{} of the lights in use, each shadow taking only its own light",
                        e.count + 1
                    );
                    if e.dropped > 0 {
                        who.push_str(&format!(
                            "; {} more directional light(s) past the limit of {} casters \
                             light without a shadow",
                            e.dropped,
                            pdfcer_3d::MAX_SHADOW_CASTERS
                        ));
                    }
                    who
                }
            }
        }
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
