//! The lighting scheme `3d-render` draws with, and the note that says
//! which lights that is.

use crate::RenderLightingArg;

/// The scheme `3d-render` lights with, and where it came from.
pub(super) struct RenderLighting {
    pub(super) scheme: pdfcer_3d::LightingScheme,
    pub(super) source: &'static str,
}

/// The scheme to draw with: `--lighting` when given, else the opening
/// view's, else artwork.
pub(super) fn render_lighting(
    arg: RenderLightingArg,
    saved: Option<&pdfcer_core::threed::ThreeDSavedView>,
) -> RenderLighting {
    let (scheme, source) = match (arg.scheme(), saved.and_then(|v| v.lighting)) {
        (Some(scheme), _) => (scheme, "--lighting"),
        (None, Some(lighting)) => (lighting.into(), "the opening view's"),
        (None, None) => (
            pdfcer_3d::LightingScheme::Artwork,
            "the default; the opening view names none",
        ),
    };
    RenderLighting { scheme, source }
}

impl RenderLighting {
    /// The lighting note, then, under artwork lighting, one `light` line
    /// per model light.
    pub(super) fn print_note(
        &self,
        model: &pdfcer_3d::AssembledModel,
        headlight: bool,
        dark: bool,
    ) {
        use pdfcer_3d::LightingScheme as S;
        let lights = &model.lights;
        let whose = match model.lights_view {
            Some(i) => format!(
                "the default view {i}'s {} light(s), in place of the model's \
                 (--no-view-lights uses the model's)",
                lights.len()
            ),
            None => format!("the model's {} light(s)", lights.len()),
        };
        let how = match self.scheme {
            S::Artwork if lights.is_empty() && dark => format!(
                "the model has no lights and is drawn with no lamp light \
                 (--dark-without-lights){}",
                if headlight {
                    ", plus the light at the camera (--artwork-headlight)"
                } else {
                    ""
                }
            ),
            S::Artwork if lights.is_empty() => {
                "the model has no lights, so it is lit from the camera \
                 (--dark-without-lights draws it unlit)"
                    .to_owned()
            }
            S::Artwork => format!(
                "{whose}{}",
                if headlight {
                    ", plus the light at the camera (--artwork-headlight)"
                } else {
                    "; --artwork-headlight adds the light at the camera"
                }
            ),
            S::None => "no lights, every surface in its own colour".to_owned(),
            _ => "the scheme's fixed lights (ISO 32000-1 Table 310)".to_owned(),
        };
        println!(
            "note: lighting: {} ({}): {how}",
            self.scheme.name(),
            self.source
        );
        if self.scheme == S::Artwork {
            super::lights::print_model_lights(lights);
        }
        if !model.view_lights.is_empty() {
            println!(
                "note: lighting: {} view(s) carry their own lights, listed as view-light \
                 lines; only a default view's are drawn, and only under artwork",
                model.view_lights.len()
            );
            let used = (self.scheme == S::Artwork)
                .then_some(model.lights_view)
                .flatten();
            super::lights::print_view_lights(&model.view_lights, used);
        }
    }
}
