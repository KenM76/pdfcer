//! The reference GLSL ES 3.00 shader for [`crate::DrawData`]: the CPU
//! path's lighting, texturing and shading, so a host need not re-derive
//! them. Text only; the host compiles it.

use crate::TextureFunction;

/// The vertex shader. Attributes, matching [`crate::DrawData::vertices`]:
/// location 0 position (vec3), 1 normal (vec3), 2 uv (vec2), 3 flat
/// (float). Uniforms: `u_view`, `u_projection` (from
/// [`crate::Camera::view_matrix`] / [`crate::Camera::projection_matrix`]),
/// and from [`crate::FrameLights`]: `u_mode` ([`crate::ShadeMode::code`]),
/// `u_ambient`, `u_count`, `u_light_dir[16]`, `u_light_colour[16]`, `u_eye`,
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
uniform vec4 u_light_dir[16];
uniform vec3 u_light_colour[16];
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
out vec3 v_normal;
flat out vec3 v_flat_normal;

vec3 shade(vec3 n, vec3 v) {
    if (u_mode == 0) return vec3(0.3 + 0.7 * abs(dot(n, v)));
    if (u_mode == 1) return vec3(1.0);
    vec3 s = u_ambient;
    for (int i = 0; i < 16; i++) {
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
    for (int i = 0; i < 16; i++) {
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
    v_normal = n;
    v_flat_normal = n;
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
/// `u_shadow_to_light`, `u_shadow_offset` and `u_shadow_bias`; from
/// [`crate::FrameLights`], the point and spot lights, added per pixel:
/// `u_local_count`, `u_local_position[16]`, `u_local_colour[16]`,
/// `u_local_attenuation[16]`, `u_local_cone[16]`; and from
/// [`crate::FrameLampShadows`] their shadows: `u_lamp_shadows` (false
/// without one), `u_lamp_shadow_map`, `u_lamp_shadow_size`,
/// `u_lamp_far[16]`; and from [`crate::FrameExtraShadows`] the further
/// casters and the see-through layers: `u_shadow_extra` (0 without one),
/// `u_shadow_extra_map`, `u_shadow_extra_to_light[15]`,
/// `u_shadow_extra_colour[15]`, `u_shadow_extra_centre`,
/// `u_shadow_extra_reach`, `u_shadow_veiled`, `u_shadow_veil`,
/// `u_shadow_transmit`, `u_lamp_veiled`, `u_lamp_veil`, `u_lamp_transmit`.
/// A [`crate::DrawFinish`] adds `u_finished` (false
/// without one), `u_material_ambient`, `u_specular`, `u_emissive` and
/// `u_shininess`, drawn with [`crate::FrameLights::specular`] and
/// `local_specular` as `u_light_specular[16]` and `u_local_specular[16]`;
/// the fragment shader also reads the vertex shader's `u_mode`,
/// `u_ambient`, `u_count`, `u_light_dir[16]`, `u_eye`, `u_forward`,
/// `u_shadow_light` and `u_shadow_dim` (its ints declared `highp`, as the
/// vertex stage's default). Writes straight RGBA.
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
in vec3 v_normal;
flat in vec3 v_flat_normal;
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
uniform int u_local_count;
uniform vec4 u_local_position[16];
uniform vec3 u_local_colour[16];
uniform vec3 u_local_attenuation[16];
uniform vec4 u_local_cone[16];
uniform bool u_lamp_shadows;
uniform highp sampler2DArray u_lamp_shadow_map;
uniform float u_lamp_shadow_size;
uniform float u_lamp_far[16];
uniform int u_shadow_extra;
uniform highp sampler2DArray u_shadow_extra_map;
uniform vec3 u_shadow_extra_to_light[15];
uniform vec3 u_shadow_extra_colour[15];
uniform vec3 u_shadow_extra_centre;
uniform float u_shadow_extra_reach;
uniform bool u_shadow_veiled;
uniform highp sampler2DArray u_shadow_veil;
uniform highp sampler2DArray u_shadow_transmit;
uniform bool u_lamp_veiled;
uniform highp sampler2DArray u_lamp_veil;
uniform highp sampler2DArray u_lamp_transmit;
uniform highp int u_mode;
uniform vec3 u_ambient;
uniform highp int u_count;
uniform vec4 u_light_dir[16];
uniform vec3 u_eye;
uniform vec3 u_forward;
uniform highp int u_shadow_light;
uniform float u_shadow_dim;
uniform vec3 u_light_specular[16];
uniform vec3 u_local_specular[16];
uniform bool u_finished;
uniform vec3 u_material_ambient;
uniform vec3 u_specular;
uniform vec3 u_emissive;
uniform float u_shininess;
out vec4 o_colour;

// The share texel q of a veil lets through to depth d: its transmission
// when d is past its nearest see-through fragment, else all.
float through(bool on, highp sampler2DArray veil, highp sampler2DArray transmit, ivec3 q, float d) {
    if (!on || d <= texelFetch(veil, q, 0).r) return 1.0;
    return texelFetch(transmit, q, 0).r;
}

// Lamp i's lit share, 0-1, at p on a face with unit normal f: 3 x 3 texels
// of its cube face around the point moved off its face towards the lamp,
// clamped to that face.
float lamp_lit(int i, vec3 p, vec3 f) {
    float far = u_lamp_far[i];
    if (!u_lamp_shadows || far <= 0.0) return 1.0;
    vec3 o = p - u_local_position[i].xyz;
    vec3 m = abs(o);
    float z0 = max(m.x, max(m.y, m.z));
    if (z0 <= 0.0) return 1.0;
    float texel = 2.0 * z0 / u_lamp_shadow_size;
    o += (dot(f, o) > 0.0 ? -f : f) * (1.5 * texel);
    m = abs(o);
    int face;
    float z;
    vec2 uv;
    if (m.x >= m.y && m.x >= m.z) { face = o.x < 0.0 ? 1 : 0; z = m.x; uv = o.yz; }
    else if (m.y >= m.z) { face = o.y < 0.0 ? 3 : 2; z = m.y; uv = o.zx; }
    else { face = o.z < 0.0 ? 5 : 4; z = m.z; uv = o.xy; }
    if (z <= 0.0) return 1.0;
    int size = int(u_lamp_shadow_size);
    ivec2 t = ivec2(floor((uv / z + 1.0) * 0.5 * u_lamp_shadow_size));
    float d = (z - 2.0 * texel) / far;
    int layer = i * 6 + face;
    float n = 0.0;
    for (int y = -1; y <= 1; y++) {
        for (int x = -1; x <= 1; x++) {
            ivec3 q = ivec3(clamp(t + ivec2(x, y), ivec2(0), ivec2(size - 1)), layer);
            if (d <= texelFetch(u_lamp_shadow_map, q, 0).r)
                n += through(u_lamp_veiled, u_lamp_veil, u_lamp_transmit, q, d);
        }
    }
    return n / 9.0;
}

// The point and spot lights at p with unit normal n on a face with unit
// normal f.
vec3 local(vec3 p, vec3 n, vec3 f) {
    vec3 s = vec3(0.0);
    for (int i = 0; i < 16; i++) {
        if (i >= u_local_count) break;
        vec3 to_light = u_local_position[i].xyz - p;
        float d = length(to_light);
        if (d <= 0.0) { s += u_local_colour[i]; continue; }
        vec3 l = to_light / d;
        vec3 a = u_local_attenuation[i];
        float den = a.x + a.y * d + a.z * d * d;
        float fade = den > 0.0 ? 1.0 / den : 1.0;
        float cone = 1.0;
        if (u_local_position[i].w > -1.5) {
            float c = -dot(l, u_local_cone[i].xyz);
            if (c < u_local_position[i].w) cone = 0.0;
            else if (u_local_cone[i].w > 0.0) cone = pow(max(c, 0.0), u_local_cone[i].w);
        }
        float k = fade * cone * abs(dot(n, l));
        if (k > 0.0) k *= lamp_lit(i, p, f);
        s += u_local_colour[i] * k;
    }
    return s;
}

// The lit share, 0-1, at map point p (0-1 each way) of directional map l
// (0 the key light's, 1 + e extra map e): 3 x 3 texels; a texel off the
// map is lit.
float pcf(vec3 p, int l) {
    ivec2 t = ivec2(floor(p.xy * u_shadow_size));
    int size = int(u_shadow_size);
    float d = p.z - u_shadow_bias;
    float n = 0.0;
    for (int y = -1; y <= 1; y++) {
        for (int x = -1; x <= 1; x++) {
            ivec2 q = t + ivec2(x, y);
            if (q.x < 0 || q.y < 0 || q.x >= size || q.y >= size) { n += 1.0; continue; }
            float stored = l == 0
                ? texelFetch(u_shadow_map, q, 0).r
                : texelFetch(u_shadow_extra_map, ivec3(q, l - 1), 0).r;
            if (d <= stored) n += through(u_shadow_veiled, u_shadow_veil, u_shadow_transmit, ivec3(q, l), d);
        }
    }
    return n / 9.0;
}

// The key light's lit share on a face with unit normal f, looked up at the
// point moved off its face towards the light.
float lit(vec3 f) {
    if (dot(f, u_shadow_to_light) < 0.0) f = -f;
    vec4 c = u_shadow_matrix * vec4(v_world + f * u_shadow_offset, 1.0);
    return pcf((c.xyz + 1.0) * 0.5, 0);
}

// What the further casters' shadows remove at unit normal n on a face with
// unit normal f: each one's light where its map shadows the point. Each map
// is the key map's frame turned to its light.
vec3 extra(vec3 n, vec3 f) {
    vec3 s = vec3(0.0);
    for (int e = 0; e < 15; e++) {
        if (e >= u_shadow_extra) break;
        vec3 l = u_shadow_extra_to_light[e];
        vec3 g = dot(f, l) < 0.0 ? -f : f;
        vec3 d = v_world + g * u_shadow_offset - u_shadow_extra_centre;
        vec3 r = normalize(cross(abs(l.z) < 0.9 ? vec3(0.0, 0.0, 1.0) : vec3(1.0, 0.0, 0.0), l));
        vec3 c = vec3(dot(d, r), dot(d, cross(l, r)), -dot(d, l)) / u_shadow_extra_reach;
        s += (1.0 - pcf((c + 1.0) * 0.5, e + 1)) * abs(dot(n, l)) * u_shadow_extra_colour[e];
    }
    return s;
}

// max(n.h, 0)^e with h the unit half-vector of l and v; 0 when n.h is
// not positive, so e = 0 never meets 0^0.
float blinn(vec3 n, vec3 v, vec3 l) {
    vec3 h = l + v;
    float hl = length(h);
    if (hl <= 0.0) return 0.0;
    float nh = dot(n, h / hl);
    return nh > 0.0 ? pow(nh, u_shininess) : 0.0;
}

// The specular light at p with unit normal n on a face with unit normal f,
// sh the shadow map's lit share there: each light's specular colour times
// its reach and blinn(). A directional light is turned to the lit side; a
// point or spot light behind the surface adds nothing.
vec3 highlights(vec3 p, vec3 n, vec3 f, float sh) {
    vec3 tv = u_orthographic ? -u_forward : u_eye - p;
    float vl = length(tv);
    if (vl <= 0.0) return vec3(0.0);
    vec3 v = tv / vl;
    if (dot(n, v) < 0.0) n = -n;
    if (u_mode == 0) return vec3(0.7 * blinn(n, v, v));
    vec3 s = vec3(0.0);
    for (int i = 0; i < 16; i++) {
        if (i >= u_count) break;
        vec3 l = u_light_dir[i].w > 0.5 ? v : u_light_dir[i].xyz;
        if (dot(n, l) < 0.0) l = -l;
        float k = 1.0;
        if (u_shadows && u_shadow_light == i) k = sh;
        if (u_shadows && u_shadow_light < 0) k = u_shadow_dim + (1.0 - u_shadow_dim) * sh;
        s += u_light_specular[i] * (k * blinn(n, v, l));
    }
    for (int i = 0; i < 16; i++) {
        if (i >= u_local_count) break;
        vec3 to_light = u_local_position[i].xyz - p;
        float d = length(to_light);
        if (d <= 0.0) continue;
        vec3 l = to_light / d;
        if (dot(n, l) <= 0.0) continue;
        vec3 a = u_local_attenuation[i];
        float den = a.x + a.y * d + a.z * d * d;
        float k = den > 0.0 ? 1.0 / den : 1.0;
        if (u_local_position[i].w > -1.5) {
            float c = -dot(l, u_local_cone[i].xyz);
            if (c < u_local_position[i].w) k = 0.0;
            else if (u_local_cone[i].w > 0.0) k *= pow(max(c, 0.0), u_local_cone[i].w);
        }
        if (k > 0.0) k *= lamp_lit(i, p, f);
        s += u_local_specular[i] * (k * blinn(n, v, l));
    }
    return s;
}

// The finish's terms added to the lit colour rgb of base, as the CPU path
// adds them: (ambient - base) * ambient light (dimmed in shadow, none when
// textured), the emission, and the highlights.
vec3 finish(vec3 rgb, vec3 base, vec3 f, float sh) {
    if (u_mode == 2 && !u_textured) {
        float dim = u_shadows && u_shadow_light < 0 ? u_shadow_dim + (1.0 - u_shadow_dim) * sh : 1.0;
        rgb += (u_material_ambient - base) * u_ambient * dim;
    }
    rgb += u_emissive;
    vec3 n = v_flat > 0.5 ? v_flat_normal : v_normal;
    float l = length(n);
    if (l > 0.0) rgb += u_specular * highlights(v_world, n / l, f, sh);
    return rgb;
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
    vec3 f = cross(dFdx(v_world), dFdy(v_world));
    float fl = length(f);
    f = fl > 0.0 ? f / fl : vec3(0.0);
    vec3 s = v_flat > 0.5 ? v_flat_shade : v_shade;
    float sh = u_shadows ? lit(f) : 1.0;
    if (u_shadows) s += sh * (v_flat > 0.5 ? v_flat_key : v_key);
    vec3 n = v_flat > 0.5 ? v_flat_normal : v_normal;
    float nl = length(n);
    if (u_shadow_extra > 0 && nl > 0.0) s -= extra(n / nl, f);
    if (u_local_count > 0 && nl > 0.0) s += local(v_world, n / nl, f);
    vec3 rgb = base.rgb * s;
    if (u_finished && u_mode != 1) rgb = finish(rgb, base.rgb, f, sh);
    o_colour = vec4(clamp(rgb, 0.0, 1.0), base.a);
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
            "u_light_dir[16]",
            "u_light_colour[16]",
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
            "u_local_count",
            "u_local_position[16]",
            "u_local_colour[16]",
            "u_local_attenuation[16]",
            "u_local_cone[16]",
            "u_mode",
            "u_ambient",
            "u_count",
            "u_light_dir[16]",
            "u_eye",
            "u_forward",
            "u_shadow_light",
            "u_shadow_dim",
            "u_light_specular[16]",
            "u_local_specular[16]",
            "u_finished",
            "u_material_ambient",
            "u_specular",
            "u_emissive",
            "u_shininess",
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
