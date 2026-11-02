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
/// `u_forward`, `u_orthographic`; from [`crate::FrameShadow`]: `u_shadows`
/// (false without one), `u_shadow_light`, `u_shadow_dim`.
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
uniform bool u_shadows;
uniform int u_shadow_light;
uniform float u_shadow_dim;
out vec3 v_shade;
flat out vec3 v_flat_shade;
out vec3 v_key;
flat out vec3 v_flat_key;
flat out float v_flat;
out vec2 v_uv;
out float v_w;
out vec3 v_world;

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

// The share of shade s the casting light's shadow removes.
vec3 key(vec3 n, vec3 s) {
    if (!u_shadows) return vec3(0.0);
    if (u_shadow_light < 0) return s * (1.0 - u_shadow_dim);
    for (int i = 0; i < 8; i++) {
        if (i == u_shadow_light) return u_light_colour[i] * abs(dot(n, u_light_dir[i].xyz));
    }
    return vec3(0.0);
}

void main() {
    vec3 v = u_orthographic ? -u_forward : normalize(u_eye - a_position);
    vec3 n = normalize(a_normal);
    vec3 s = shade(n, v);
    vec3 k = key(n, s);
    v_shade = s - k;
    v_flat_shade = s - k;
    v_key = k;
    v_flat_key = k;
    v_world = a_position;
    v_flat = a_flat;
    v_uv = a_uv;
    gl_Position = u_projection * u_view * vec4(a_position, 1.0);
    v_w = gl_Position.w;
}
";

/// The fragment shader. Uniforms: `u_colour` ([`crate::Draw::colour`]),
/// `u_textured`, `u_texture` (the [`crate::Texture`]'s `rgba` uploaded top
/// row first, `GL_LINEAR` both ways, no mipmaps, wrap from its `wrap`),
/// `u_function` and `u_blend_colour` (from
/// [`TextureFunction::shader_code`]), `u_depth_range` ([`crate::Camera::depth_range`])
/// and `u_orthographic`; from [`crate::FrameShadow`]: `u_shadows`,
/// `u_shadow_matrix`, `u_shadow_map` (its depth texture), `u_shadow_size`,
/// `u_shadow_to_light`, `u_shadow_offset` and `u_shadow_bias`. Writes
/// straight RGBA.
///
/// It writes `gl_FragDepth`: logarithmic in perspective (see
/// [`crate::Camera::depth_range`] for the precision), the matrix's linear
/// depth in orthographic. It assumes a depth buffer of at least 24 bits
/// cleared to 1.0 with `LESS`; a 16-bit buffer resolves 256 times coarser.
pub const GLSL_ES_300_FRAGMENT: &str = r"#version 300 es
precision highp float;
in vec3 v_shade;
flat in vec3 v_flat_shade;
flat in float v_flat;
in vec2 v_uv;
in float v_w;
in vec3 v_key;
flat in vec3 v_flat_key;
in vec3 v_world;
uniform vec2 u_depth_range;
uniform bool u_orthographic;
uniform vec4 u_colour;
uniform bool u_textured;
uniform sampler2D u_texture;
uniform int u_function;
uniform vec4 u_blend_colour;
uniform bool u_shadows;
uniform mat4 u_shadow_matrix;
uniform highp sampler2D u_shadow_map;
uniform float u_shadow_size;
uniform vec3 u_shadow_to_light;
uniform float u_shadow_offset;
uniform float u_shadow_bias;
out vec4 o_colour;

// The lit share, 0-1: 3 x 3 map texels around the point moved off its face
// towards the light; a texel off the map is lit.
float lit() {
    vec3 f = cross(dFdx(v_world), dFdy(v_world));
    float l = length(f);
    f = l > 0.0 ? f / l : vec3(0.0);
    if (dot(f, u_shadow_to_light) < 0.0) f = -f;
    vec4 c = u_shadow_matrix * vec4(v_world + f * u_shadow_offset, 1.0);
    vec3 p = (c.xyz + 1.0) * 0.5;
    ivec2 t = ivec2(floor(p.xy * u_shadow_size));
    int size = int(u_shadow_size);
    float d = p.z - u_shadow_bias;
    int n = 0;
    for (int y = -1; y <= 1; y++) {
        for (int x = -1; x <= 1; x++) {
            ivec2 q = t + ivec2(x, y);
            bool off = q.x < 0 || q.y < 0 || q.x >= size || q.y >= size;
            if (off || d <= texelFetch(u_shadow_map, q, 0).r) n++;
        }
    }
    return float(n) / 9.0;
}

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
    if (u_shadows) s += lit() * (v_flat > 0.5 ? v_flat_key : v_key);
    o_colour = vec4(clamp(base.rgb * s, 0.0, 1.0), base.a);
    gl_FragDepth = u_orthographic
        ? gl_FragCoord.z
        : clamp(log2(v_w / u_depth_range.x) / log2(u_depth_range.y / u_depth_range.x), 0.0, 1.0);
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
            "u_depth_range",
            "u_orthographic",
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
