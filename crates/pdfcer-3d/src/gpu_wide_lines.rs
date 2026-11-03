//! Lines of any width for a host's GPU: [`DrawData::lines`] drawn as one
//! instanced quad per segment, covering the pixels
//! [`crate::PreparedModel::render`] covers. `glLineWidth` above 1 is not
//! portable (WebGL clamps it to 1), so width is built from triangles.

#[cfg(doc)]
use crate::{DrawData, GLSL_ES_300_LINE_FRAGMENT, GLSL_ES_300_LINE_VERTEX};

/// The wide-line vertex shader; pair it with [`GLSL_ES_300_LINE_FRAGMENT`].
///
/// Bind [`DrawData::lines`] as **per-instance** attributes (divisor 1),
/// stride `2 *` [`LINE_VERTEX_FLOATS`](crate::LINE_VERTEX_FLOATS) `* 4` =
/// 144 bytes, so one instance is one segment, and draw
/// `drawArraysInstanced(TRIANGLE_STRIP, 0, 4, lines.len() / 36)`. Locations
/// (byte offset): 0 first end (0), 1 its colour (12), 2 midpoint (28),
/// 3 its first normal (40), 4 its second normal (52), 5 kind (64),
/// 6 shaded (68), 7 second end (72), 8 its colour (84), 9 its first normal
/// (112). No per-vertex attribute is read; `gl_VertexID` picks the corner.
///
/// Uniforms: those of [`GLSL_ES_300_LINE_VERTEX`] except `u_point_size`,
/// plus `u_line_width` ([`DrawData::line_width`], pixels) and `u_viewport`
/// (the viewport's width and height, pixels).
///
/// The segment is clipped at the near plane, then widened across its
/// minor screen axis only (the axis it moves along less), into a band
/// `u_line_width` pixels thick whose ends are parallel to that axis: the
/// pixels an aliased OpenGL wide line covers, and the CPU frame's. The
/// facing test and shading are [`GLSL_ES_300_LINE_VERTEX`]'s; each end is
/// shaded by its own first normal.
pub const GLSL_ES_300_WIDE_LINE_VERTEX: &str = r"#version 300 es
precision highp float;
layout(location = 0) in vec3 a_p0;
layout(location = 1) in vec4 a_c0;
layout(location = 2) in vec3 a_mid;
layout(location = 3) in vec3 a_n1;
layout(location = 4) in vec3 a_n2;
layout(location = 5) in float a_kind;
layout(location = 6) in float a_shaded;
layout(location = 7) in vec3 a_p1;
layout(location = 8) in vec4 a_c1;
layout(location = 9) in vec3 a_n1_end;
uniform mat4 u_view;
uniform mat4 u_projection;
uniform vec3 u_eye;
uniform vec3 u_forward;
uniform bool u_orthographic;
uniform float u_bias;
uniform float u_crease_cos;
uniform float u_line_width;
uniform vec2 u_viewport;
out vec4 v_colour;
out float v_w;

vec3 towards(vec3 p) {
    return u_orthographic ? -u_forward : normalize(u_eye - p);
}

vec4 shade(vec4 c, vec3 n, vec3 p) {
    if (a_shaded > 0.5) c.rgb *= 0.25 + 0.75 * abs(dot(n, towards(p)));
    return c;
}

vec4 clip_of(vec3 p) {
    return u_projection * u_view * vec4(p + towards(p) * u_bias, 1.0);
}

void main() {
    vec3 v = towards(a_mid);
    bool keep = true;
    if (a_kind > 0.5) {
        bool f1 = dot(a_n1, v) > 0.0;
        bool f2 = dot(a_n2, v) > 0.0;
        keep = a_kind < 1.5 ? (f1 != f2 || (f1 && dot(a_n1, a_n2) <= u_crease_cos)) : (f1 || f2);
    }
    vec4 pa = clip_of(a_p0);
    vec4 pb = clip_of(a_p1);
    vec4 ca = shade(a_c0, a_n1, a_p0);
    vec4 cb = shade(a_c1, a_n1_end, a_p1);
    float da = pa.z + pa.w;
    float db = pb.z + pb.w;
    v_colour = vec4(0.0);
    v_w = 1.0;
    gl_Position = vec4(2.0, 2.0, 2.0, 1.0);
    if (!keep || (da < 0.0 && db < 0.0)) return;
    if (da < 0.0) {
        float t = da / (da - db);
        pa = mix(pa, pb, t);
        ca = mix(ca, cb, t);
    } else if (db < 0.0) {
        float t = da / (da - db);
        pb = mix(pa, pb, t);
        cb = mix(ca, cb, t);
    }
    vec2 d = (pb.xy / pb.w - pa.xy / pa.w) * u_viewport;
    float w = max(u_line_width, 1.0);
    float k = floor((w - 1.0) / 2.0);
    bool high = (gl_VertexID & 1) == 1;
    vec2 off = abs(d.x) >= abs(d.y)
        ? vec2(0.0, high ? k + 0.5 : -(w - k - 0.5))
        : vec2(high ? w - k - 0.5 : -(k + 0.5), 0.0);
    vec4 p = gl_VertexID < 2 ? pa : pb;
    v_colour = gl_VertexID < 2 ? ca : cb;
    p.xy += off * 2.0 / u_viewport * p.w;
    gl_Position = p;
    v_w = p.w;
}
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shader_declares_what_its_doc_names() {
        for name in [
            "u_bias",
            "u_crease_cos",
            "u_eye",
            "u_forward",
            "u_line_width",
            "u_viewport",
        ] {
            assert!(
                GLSL_ES_300_WIDE_LINE_VERTEX.contains(&format!(" {name};")),
                "{name}"
            );
        }
        for (loc, name) in [(7, "a_p1"), (8, "a_c1"), (9, "a_n1_end")] {
            let decl = format!("layout(location = {loc}) in");
            assert!(GLSL_ES_300_WIDE_LINE_VERTEX.contains(&decl), "{name}");
        }
        assert!(!GLSL_ES_300_WIDE_LINE_VERTEX.contains("u_point_size"));
    }
}
