//! The reference GLSL ES 3.00 shader for [`crate::DrawData`]: the CPU
//! path's lighting, texturing and shading, so a host need not re-derive
//! them. Text only; the host compiles it.

use crate::TextureFunction;

/// The vertex shader. Attributes, matching [`crate::DrawData::vertices`]:
/// location 0 position (vec3), 1 normal (vec3), 2 uv (vec2), 3 flat
/// (float). Uniforms: `u_view`, `u_projection` (from
/// [`crate::Camera::view_matrix`] / [`crate::Camera::projection_matrix`]),
/// and from [`crate::FrameLights`]: `u_mode` ([`crate::ShadeMode::code`]),
/// `u_ambient`, `u_count`, `u_light_dir[8]`, `u_light_colour[8]`, `u_eye`,
/// `u_forward`, `u_orthographic`.
pub const GLSL_ES_300_VERTEX: &str = r"#version 300 es
precision highp float;
layout(location = 0) in vec3 a_position;
layout(location = 1) in vec3 a_normal;
layout(location = 2) in vec2 a_uv;
layout(location = 3) in float a_flat;
uniform mat4 u_view;
uniform mat4 u_projection;
uniform int u_mode;
uniform vec3 u_ambient;
uniform int u_count;
uniform vec4 u_light_dir[8];
uniform vec3 u_light_colour[8];
uniform vec3 u_eye;
uniform vec3 u_forward;
uniform bool u_orthographic;
out vec3 v_shade;
flat out vec3 v_flat_shade;
flat out float v_flat;
out vec2 v_uv;

vec3 shade(vec3 n, vec3 v) {
    if (u_mode == 0) return vec3(0.3 + 0.7 * abs(dot(n, v)));
    if (u_mode == 1) return vec3(1.0);
    vec3 s = u_ambient;
    for (int i = 0; i < 8; i++) {
        if (i >= u_count) break;
        vec3 d = u_light_dir[i].w > 0.5 ? v : u_light_dir[i].xyz;
        s += u_light_colour[i] * abs(dot(n, d));
    }
    return s;
}

void main() {
    vec3 v = u_orthographic ? -u_forward : normalize(u_eye - a_position);
    vec3 s = shade(normalize(a_normal), v);
    v_shade = s;
    v_flat_shade = s;
    v_flat = a_flat;
    v_uv = a_uv;
    gl_Position = u_projection * u_view * vec4(a_position, 1.0);
}
";

/// The fragment shader. Uniforms: `u_colour` ([`crate::Draw::colour`]),
/// `u_textured`, `u_texture` (the [`crate::Texture`]'s `rgba` uploaded top
/// row first, `GL_LINEAR` both ways, no mipmaps, wrap from its `wrap`),
/// `u_function` and `u_blend_colour` (from
/// [`TextureFunction::shader_code`]). Writes straight RGBA.
pub const GLSL_ES_300_FRAGMENT: &str = r"#version 300 es
precision highp float;
in vec3 v_shade;
flat in vec3 v_flat_shade;
flat in float v_flat;
in vec2 v_uv;
uniform vec4 u_colour;
uniform bool u_textured;
uniform sampler2D u_texture;
uniform int u_function;
uniform vec4 u_blend_colour;
out vec4 o_colour;

void main() {
    vec4 base = u_colour;
    if (u_textured) {
        vec4 t = texture(u_texture, v_uv);
        vec3 rgb = t.rgb;
        float a = base.a * t.a;
        if (u_function == 0) rgb = base.rgb * t.rgb;
        else if (u_function == 2) rgb = base.rgb * (1.0 - t.rgb) + u_blend_colour.rgb * t.rgb;
        else if (u_function == 3) { rgb = base.rgb * (1.0 - t.a) + t.rgb * t.a; a = base.a; }
        base = vec4(clamp(rgb, 0.0, 1.0), a);
    }
    vec3 s = v_flat > 0.5 ? v_flat_shade : v_shade;
    o_colour = vec4(clamp(base.rgb * s, 0.0, 1.0), base.a);
}
";

impl TextureFunction {
    /// The fragment shader's `u_function` (0 Modulate, 1 Replace, 2 Blend,
    /// 3 Decal) and `u_blend_colour` (the Blend colour, else zero).
    #[must_use]
    pub fn shader_code(self) -> (i32, [f32; 4]) {
        match self {
            TextureFunction::Modulate => (0, [0.0; 4]),
            TextureFunction::Replace => (1, [0.0; 4]),
            TextureFunction::Blend { colour } => (2, colour.map(|c| c as f32)),
            TextureFunction::Decal => (3, [0.0; 4]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shaders_declare_every_uniform_the_docs_name() {
        for name in [
            "u_view",
            "u_projection",
            "u_mode",
            "u_ambient",
            "u_count",
            "u_light_dir[8]",
            "u_light_colour[8]",
            "u_eye",
            "u_forward",
            "u_orthographic",
        ] {
            assert!(GLSL_ES_300_VERTEX.contains(&format!(" {name};")), "{name}");
        }
        for name in [
            "u_colour",
            "u_textured",
            "u_texture",
            "u_function",
            "u_blend_colour",
        ] {
            assert!(
                GLSL_ES_300_FRAGMENT.contains(&format!(" {name};")),
                "{name}"
            );
        }
        // The light array matches what FrameLights fills.
        assert!(GLSL_ES_300_VERTEX.contains(&format!("[{}]", crate::MAX_LIGHTS)));
    }

    #[test]
    fn texture_functions_have_distinct_codes() {
        let codes = [
            TextureFunction::Modulate,
            TextureFunction::Replace,
            TextureFunction::Blend { colour: [0.5; 4] },
            TextureFunction::Decal,
        ]
        .map(|f| f.shader_code().0);
        assert_eq!(codes, [0, 1, 2, 3]);
    }
}
