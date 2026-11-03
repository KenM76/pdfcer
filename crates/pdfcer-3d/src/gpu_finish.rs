//! A mesh's [`crate::MaterialFinish`] and the lights' specular colours as
//! GPU uniforms, drawn by [`crate::GLSL_ES_300_FRAGMENT`] with the CPU
//! path's formula (see [`crate::RenderOptions::shininess`]).

use crate::gloss::{GlossLights, resolve};
use crate::gpu_frame::FrameLights;
use crate::{MaterialFinish, RenderOptions};

/// One draw's finish, as the fragment shader's uniforms take it; every
/// colour 0–1.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct DrawFinish {
    /// `u_material_ambient`: the ambient colour, rounded to 8 bits so one
    /// equal to [`crate::Draw::colour`] changes nothing.
    pub ambient: [f32; 3],
    /// `u_specular`.
    pub specular: [f32; 3],
    /// `u_emissive`.
    pub emissive: [f32; 3],
    /// `u_shininess`: the Blinn-Phong exponent, 0–128, already read through
    /// [`crate::RenderOptions::shininess`].
    pub exponent: f32,
}

impl DrawFinish {
    /// `finish` drawn with `options`.
    pub(crate) fn new(finish: &MaterialFinish, options: &RenderOptions) -> DrawFinish {
        let r = resolve(finish, options);
        let f = |c: [f64; 3]| c.map(|v| (v / 255.0) as f32);
        DrawFinish {
            ambient: f(r.ambient),
            specular: f(r.specular),
            emissive: f(r.emissive),
            exponent: r.exponent as f32,
        }
    }
}

impl FrameLights {
    /// Fill `specular` and `local_specular` from `lights`.
    pub(crate) fn set_specular(&mut self, lights: &GlossLights) {
        let f = |c: &[f64; 3]| c.map(|v| v as f32);
        for (slot, (_, c)) in self.specular.iter_mut().zip(&lights.far) {
            *slot = f(c);
        }
        for (slot, c) in self.local_specular.iter_mut().zip(&lights.near) {
            *slot = f(c);
        }
    }
}
