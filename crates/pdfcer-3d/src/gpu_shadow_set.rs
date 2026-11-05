//! Per-frame GPU inputs for the shadows past the key light's: every further
//! directional light's map ([`crate::ShadowCasters::EveryLight`]) and the
//! see-through layers of translucent and cut-out casters
//! ([`crate::ShadowAlpha`]), mirroring [`crate::shadow_set`] and
//! [`crate::shadow_cast`] so both paths shadow the same texels.

use crate::gpu_frame::MAX_SHADOW_CASTERS;
use crate::lighting::Shader;
use crate::render::{Bounds, Camera, RenderError, RenderOptions};
use crate::shadow::{Caster, LightView};
use crate::shadow_set::extras;

/// The most further casters: every light but the key light.
pub const MAX_EXTRA_SHADOWS: usize = MAX_SHADOW_CASTERS - 1;

/// The alpha depth pass's vertex shader, for a draw whose
/// [`crate::Draw::translucent`] is true: location 0 position (vec3) and 2
/// uv (vec2) from [`crate::DrawData::vertices`]; uniform `u_shadow_matrix`.
pub const GLSL_ES_300_SHADOW_ALPHA_VERTEX: &str = r"#version 300 es
precision highp float;
layout(location = 0) in vec3 a_position;
layout(location = 2) in vec2 a_uv;
uniform mat4 u_shadow_matrix;
out vec2 v_uv;

void main() {
    gl_Position = u_shadow_matrix * vec4(a_position, 1.0);
    v_uv = a_uv;
}
";

/// The alpha depth pass's fragment shader: the draw's alpha as the main
/// pass paints it (`u_colour`, `u_textured`, `u_texture`, `u_function` as
/// in [`crate::GLSL_ES_300_FRAGMENT`]); fragments outside `u_keep` (lowest
/// kept alpha, first alpha past it) are discarded. Writes `1 − alpha` to
/// colour, the share it lets through.
pub const GLSL_ES_300_SHADOW_ALPHA_FRAGMENT: &str = r"#version 300 es
precision highp float;
in vec2 v_uv;
uniform vec4 u_colour;
uniform bool u_textured;
uniform sampler2D u_texture;
uniform int u_function;
uniform vec2 u_keep;
out vec4 o_through;

void main() {
    float a = u_colour.a;
    if (u_textured && u_function != 3) a *= texture(u_texture, v_uv).a;
    if (a < u_keep.x || a >= u_keep.y) discard;
    o_through = vec4(1.0 - a);
}
";

/// The lamp alpha depth pass's vertex shader: as
/// [`GLSL_ES_300_SHADOW_ALPHA_VERTEX`] with `u_lamp_matrix`.
pub const GLSL_ES_300_LAMP_SHADOW_ALPHA_VERTEX: &str = r"#version 300 es
precision highp float;
layout(location = 0) in vec3 a_position;
layout(location = 2) in vec2 a_uv;
uniform mat4 u_lamp_matrix;
out vec2 v_uv;
out float v_z;

void main() {
    gl_Position = u_lamp_matrix * vec4(a_position, 1.0);
    v_uv = a_uv;
    v_z = gl_Position.w;
}
";

/// The lamp alpha depth pass's fragment shader: as
/// [`GLSL_ES_300_SHADOW_ALPHA_FRAGMENT`], depth as
/// [`crate::GLSL_ES_300_LAMP_SHADOW_FRAGMENT`] (`u_lamp_far`).
pub const GLSL_ES_300_LAMP_SHADOW_ALPHA_FRAGMENT: &str = r"#version 300 es
precision highp float;
in vec2 v_uv;
in float v_z;
uniform vec4 u_colour;
uniform bool u_textured;
uniform sampler2D u_texture;
uniform int u_function;
uniform vec2 u_keep;
uniform float u_lamp_far;
out vec4 o_through;

void main() {
    float a = u_colour.a;
    if (u_textured && u_function != 3) a *= texture(u_texture, v_uv).a;
    if (a < u_keep.x || a >= u_keep.y) discard;
    o_through = vec4(1.0 - a);
    gl_FragDepth = v_z / u_lamp_far;
}
";

/// One frame's further directional casters: the maps a host renders
/// beside [`crate::FrameShadow`]'s and the main fragment shader's uniforms
/// for them.
///
/// The host allocates a `size` × `size` × `count` `DEPTH_COMPONENT32F` 2D
/// array texture (`NEAREST`, no compare mode) and draws layer `e` as it
/// draws [`crate::FrameShadow`]'s map, with
/// `u_shadow_matrix` = [`FrameExtraShadows::matrix`]`(e)`. The main pass
/// binds it as `u_shadow_extra_map` with `u_shadow_extra` = `count` and
/// the uniforms named on each field; the lookup reuses `u_shadow_size`,
/// `u_shadow_offset` and `u_shadow_bias`, which are the same for every map
/// of a frame. Each map removes only its own light, per pixel, so a frame
/// with `count` 0 draws exactly as with the key light alone.
///
/// Translucent and cut-out casters ([`RenderOptions::shadow_alpha`]) add,
/// per directional map (layer 0 the key light's, `1 + e` extra map `e`):
/// solid depth from each translucent draw with
/// [`GLSL_ES_300_SHADOW_ALPHA_VERTEX`] / [`GLSL_ES_300_SHADOW_ALPHA_FRAGMENT`],
/// `u_keep` = [`crate::ShadowAlpha::solid_range`] (skipped when `None`),
/// into the same depth layer after the opaque draws; and when
/// [`crate::ShadowAlpha::veil_range`] is `Some` and a translucent draw
/// exists, two veil layers, `u_keep` = that range: the nearest such depth
/// into layer `l` of `u_shadow_veil` (`DEPTH_COMPONENT32F` array, cleared
/// to 1.0, `LESS`, colour writes off) and their product of `1 − alpha` into
/// layer `l` of `u_shadow_transmit` (`R8` array, cleared to 1.0, depth test
/// off, blend `ZERO`, `SRC_COLOR`), with `u_shadow_veiled` true. The lamp
/// cubes do the same with the lamp twins into `u_lamp_veil` /
/// `u_lamp_transmit`, layer `6 i + f`, `u_lamp_veiled` true. Every sampler
/// the shader declares takes its own texture unit even when unused.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct FrameExtraShadows {
    /// Each map's side, texels: the key map's.
    pub size: u32,
    /// How many further casters (`u_shadow_extra`).
    pub count: usize,
    /// Each one's light, an index into [`crate::FrameLights::directions`]
    /// (`u_shadow_extra_light[15]`, so its map also shades its highlight).
    pub lights: [i32; MAX_EXTRA_SHADOWS],
    /// Unit, model space, towards each (`u_shadow_extra_to_light[15]`).
    pub to_light: [[f32; 3]; MAX_EXTRA_SHADOWS],
    /// Each one's colour, the share its shadow removes
    /// (`u_shadow_extra_colour[15]`).
    pub colour: [[f32; 3]; MAX_EXTRA_SHADOWS],
    /// The maps' centre, model space (`u_shadow_extra_centre`).
    pub centre: [f32; 3],
    /// The maps' half-width, model units (`u_shadow_extra_reach`).
    pub reach: f32,
    /// How many more lights would have cast past [`MAX_EXTRA_SHADOWS`],
    /// to disclose; they light without a shadow.
    pub dropped: usize,
    matrices: [[f32; 16]; MAX_EXTRA_SHADOWS],
}

impl FrameExtraShadows {
    /// The further casters `options` asks for over a model of `bounds` with
    /// `lights`, its own, seen from `camera`; `None` when only one light
    /// casts (see [`RenderOptions::shadow_casters`]).
    ///
    /// # Errors
    ///
    /// [`RenderError::Camera`] for a degenerate camera, as [`crate::render`].
    pub fn for_model(
        camera: &Camera,
        options: &RenderOptions,
        bounds: &Bounds,
        lights: &[crate::ModelLight],
    ) -> Result<Option<Self>, RenderError> {
        let basis = camera.basis()?;
        if !options.shadows {
            return Ok(None);
        }
        let shader = Shader::for_frame(options, lights, &basis)?;
        let size = options.shadow_map_size;
        let caster = Caster::for_light(options.shadow_light, &shader, options.light_sense, &basis)
            .filter(|c| LightView::new(c.to_light, bounds, size).is_some());
        let (list, dropped) = extras(
            caster.as_ref(),
            &shader,
            options.light_sense,
            options.shadow_casters,
        );
        let mut out = FrameExtraShadows {
            size: 0,
            count: 0,
            lights: [-1; MAX_EXTRA_SHADOWS],
            to_light: [[0.0; 3]; MAX_EXTRA_SHADOWS],
            colour: [[0.0; 3]; MAX_EXTRA_SHADOWS],
            centre: [0.0; 3],
            reach: 0.0,
            dropped,
            matrices: [[0.0; 16]; MAX_EXTRA_SHADOWS],
        };
        for e in &list {
            let Some(view) = LightView::new(e.to_light, bounds, size) else {
                continue;
            };
            let (centre, reach) = view.frame();
            let k = out.count;
            if let (Some(l), Some(t), Some(c), Some(m)) = (
                out.lights.get_mut(k),
                out.to_light.get_mut(k),
                out.colour.get_mut(k),
                out.matrices.get_mut(k),
            ) {
                *l = i32::try_from(e.light).unwrap_or(-1);
                *t = e.to_light.map(|v| v as f32);
                *c = e.colour.map(|v| v as f32);
                *m = view.matrix();
                out.count += 1;
            }
            out.size = view.size;
            out.centre = centre.map(|v| v as f32);
            out.reach = reach as f32;
        }
        Ok((out.count > 0).then_some(out))
    }

    /// Extra map `e`'s model to light clip space, column-major
    /// (`u_shadow_matrix` while drawing layer `e`); `None` past `count`.
    #[must_use]
    pub fn matrix(&self, e: usize) -> Option<[f32; 16]> {
        self.matrices.get(e).filter(|_| e < self.count).copied()
    }

    /// The depth texture's layer count.
    #[must_use]
    pub fn layers(&self) -> u32 {
        self.count as u32
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use crate::{
        FrameLights, FrameShadow, GLSL_ES_300_FRAGMENT, LightingScheme, ModelLight, ModelLightKind,
        Projection, ShadowCasters,
    };

    fn camera() -> Camera {
        Camera {
            eye: [3.0, -4.0, 5.0],
            target: [0.0; 3],
            up: [0.0, 0.0, 1.0],
            projection: Projection::Perspective { fov_y: 40.0 },
        }
    }

    const BOUNDS: Bounds = Bounds {
        min: [-1.0; 3],
        max: [1.0; 3],
    };

    #[test]
    fn every_other_light_of_a_scheme_casts_with_the_key_maps_frame() {
        let day = RenderOptions {
            lighting: LightingScheme::Day,
            ..RenderOptions::default()
        };
        let extra = FrameExtraShadows::for_model(&camera(), &day, &BOUNDS, &[])
            .unwrap()
            .unwrap();
        let key = FrameShadow::new(&camera(), &day, &BOUNDS).unwrap().unwrap();
        assert!(extra.count > 0 && extra.dropped == 0);
        assert_eq!(extra.size, key.size);
        assert!(extra.lights[..extra.count].iter().all(|&l| l != key.light));
        assert_eq!(extra.matrix(extra.count), None);
        assert_eq!(extra.layers(), extra.count as u32);
        let one = RenderOptions {
            shadow_casters: ShadowCasters::KeyLight,
            ..day
        };
        assert_eq!(
            FrameExtraShadows::for_model(&camera(), &one, &BOUNDS, &[]).unwrap(),
            None
        );
        // The camera light's fallback casts alone.
        let artwork = RenderOptions::default();
        assert_eq!(
            FrameExtraShadows::for_model(&camera(), &artwork, &BOUNDS, &[]).unwrap(),
            None
        );
    }

    #[test]
    fn past_the_light_limit_the_rest_light_without_a_shadow() {
        let suns: Vec<ModelLight> = (0..20)
            .map(|i| {
                // Light 0 the key; the rest brighter with their index, so the
                // dimmest further lights are past the caster limit.
                let a = f64::from(i) * 0.3;
                let (direction, grey) = match i {
                    0 => ([0.3, -0.4, 0.8], 1.0),
                    _ => ([a.cos() * 0.6, a.sin() * 0.6, 0.8], 0.02 * f64::from(i)),
                };
                ModelLight::new(ModelLightKind::Directional { direction }, [grey; 3])
            })
            .collect();
        let options = RenderOptions::default();
        let extra = FrameExtraShadows::for_model(&camera(), &options, &BOUNDS, &suns)
            .unwrap()
            .unwrap();
        let key = FrameShadow::for_model(&camera(), &options, &BOUNDS, &suns)
            .unwrap()
            .unwrap();
        assert_eq!(extra.count, MAX_EXTRA_SHADOWS);
        assert_eq!(extra.dropped, 20 - MAX_SHADOW_CASTERS);
        // Every caster is a light the GPU frame draws, and none twice.
        let drawn = FrameLights::for_model(&camera(), &options, &suns).unwrap();
        let mut cast: Vec<i32> = extra.lights.to_vec();
        cast.push(key.light);
        cast.sort_unstable();
        cast.dedup();
        assert_eq!(cast.len(), MAX_SHADOW_CASTERS);
        assert!(cast.iter().all(|&l| (l as usize) < drawn.count));
    }

    #[test]
    fn the_shaders_declare_every_extra_and_veil_uniform() {
        for name in [
            "u_shadow_extra",
            "u_shadow_extra_map",
            "u_shadow_extra_to_light[15]",
            "u_shadow_extra_light[15]",
            "u_shadow_extra_colour[15]",
            "u_shadow_extra_centre",
            "u_shadow_extra_reach",
            "u_shadow_veiled",
            "u_shadow_veil",
            "u_shadow_transmit",
            "u_lamp_veiled",
            "u_lamp_veil",
            "u_lamp_transmit",
        ] {
            assert!(
                GLSL_ES_300_FRAGMENT.contains(&format!(" {name};")),
                "{name}"
            );
        }
        assert!(GLSL_ES_300_FRAGMENT.contains(&format!("[{MAX_EXTRA_SHADOWS}]")));
        for shader in [
            GLSL_ES_300_SHADOW_ALPHA_FRAGMENT,
            GLSL_ES_300_LAMP_SHADOW_ALPHA_FRAGMENT,
        ] {
            for name in [
                "u_colour",
                "u_textured",
                "u_texture",
                "u_function",
                "u_keep",
            ] {
                assert!(shader.contains(&format!(" {name};")), "{name}");
            }
        }
        assert!(GLSL_ES_300_SHADOW_ALPHA_VERTEX.contains(" u_shadow_matrix;"));
        assert!(GLSL_ES_300_LAMP_SHADOW_ALPHA_VERTEX.contains(" u_lamp_matrix;"));
        assert!(GLSL_ES_300_LAMP_SHADOW_ALPHA_FRAGMENT.contains(" u_lamp_far;"));
    }
}
