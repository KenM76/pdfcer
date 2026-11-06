//! Reference shaders for a fisheye frame ([`crate::FrameLens`]): the
//! resample that composes the lens picture from its faces, and the line
//! and point vertex shaders that project through the lens. GLSL mirrors of
//! [`crate::camera::lens`]'s maps, so the GPU and CPU paths draw the same pixels.

#[cfg(doc)]
use crate::{
    DrawData, FrameLens, GLSL_ES_300_LINE_FRAGMENT, GLSL_ES_300_LINE_VERTEX,
    GLSL_ES_300_SCREEN_VERTEX, GLSL_ES_300_WIDE_LINE_VERTEX, LENS_LINE_PIECES, LensFace,
};

/// The lens maps every lens shader shares; needs `u_lens_mapping`,
/// `u_lens_focal` and `u_viewport` declared before it.
macro_rules! lens_maps {
    () => {
        r"
float lens_radius(float theta) {
    return u_lens_mapping == 0 ? theta : 2.0 * tan(0.5 * theta);
}

bool lens_ndc(vec3 c, out vec2 ndc) {
    float rho = length(c.xy);
    ndc = vec2(0.0);
    if (u_lens_mapping == 2) {
        if (length(c) == 0.0) return false;
        ndc = vec2(atan(c.x, c.z), 2.0 * atan(c.y, length(c.xz))) / 3.14159265358979;
        return true;
    }
    if (rho == 0.0) return c.z > 0.0;
    float r = u_lens_focal * lens_radius(atan(rho, c.z));
    ndc = r * c.xy / rho * 2.0 / u_viewport;
    return true;
}

bool lens_direction(vec2 px, out vec3 d) {
    if (u_lens_mapping == 2) {
        float lambda = (px.x / u_viewport.x - 0.5) * 6.28318530717959;
        float phi = (0.5 - px.y / u_viewport.y) * 3.14159265358979;
        d = vec3(cos(phi) * sin(lambda), sin(phi), cos(phi) * cos(lambda));
        return true;
    }
    vec2 o = vec2(px.x - 0.5 * u_viewport.x, 0.5 * u_viewport.y - px.y);
    float r = length(o);
    float q = r / u_lens_focal;
    d = vec3(0.0, 0.0, 1.0);
    if (u_lens_mapping == 0 && q > 3.14159265358979) return false;
    float theta = u_lens_mapping == 0 ? q : 2.0 * atan(0.5 * q);
    if (r > 0.0) d = vec3(sin(theta) * o / r, cos(theta));
    return true;
}
"
    };
}

/// The line attributes and the uniforms the lens line and point shaders
/// share, then the camera-space and facing helpers.
macro_rules! lens_line_head {
    () => {
        r"#version 300 es
precision highp float;
uniform vec3 u_eye;
uniform vec3 u_right;
uniform vec3 u_up;
uniform vec3 u_forward;
uniform int u_lens_mapping;
uniform float u_lens_focal;
uniform float u_lens_near;
uniform vec2 u_viewport;
uniform float u_bias;
uniform float u_crease_cos;
out vec4 v_colour;
out float v_w;

vec3 towards(vec3 p) {
    return normalize(u_eye - p);
}

vec3 camera_of(vec3 p) {
    vec3 d = p + towards(p) * u_bias - u_eye;
    return vec3(dot(d, u_right), dot(d, u_up), dot(d, u_forward));
}

bool kept(vec3 n1, vec3 n2, float kind, vec3 mid) {
    vec3 v = towards(mid);
    if (kind < 0.5) return true;
    bool f1 = dot(n1, v) > 0.0;
    bool f2 = dot(n2, v) > 0.0;
    return kind < 1.5 ? (f1 != f2 || (f1 && dot(n1, n2) <= u_crease_cos)) : (f1 || f2);
}
"
    };
}

/// The lens resample fragment shader: draw [`GLSL_ES_300_SCREEN_VERTEX`]
/// over the whole picture once per [`LensFace`], with that face's colour
/// and depth targets bound, the depth test **on** with `depthFunc(ALWAYS)`
/// and depth writes on. GL writes no depth while the depth test is
/// disabled, which would leave the picture's depth cleared and draw every
/// lens line and point over the surfaces in front of it.
///
/// Uniforms: `u_face_colour` and `u_face_depth` (the face's colour and
/// depth textures, read by `texelFetch`), `u_face` ([`LensFace::face`]),
/// `u_face_right`, `u_face_up`, `u_face_forward` ([`LensFace::axes`]),
/// `u_face_scale` ([`LensFace::scale`]), `u_face_size` (its width and
/// height), `u_face_depth_range` (the face camera's
/// [`crate::Camera::depth_range`]), `u_depth_range` (the fisheye camera's),
/// `u_lens_mapping`, `u_lens_focal` ([`FrameLens`]), `u_viewport` (the
/// picture's width and height) and `u_background` (straight RGBA 0–1).
///
/// Each pixel takes the nearest texel of the face owning its direction
/// (others discard) and rewrites that texel's logarithmic depth from the
/// face's axis distance to the distance from the eye, which the fisheye's
/// depth range measures. Outside the picture circle the forward face (0)
/// writes the background at depth 1.
pub const GLSL_ES_300_LENS_RESAMPLE_FRAGMENT: &str = concat!(
    r"#version 300 es
precision highp float;
uniform sampler2D u_face_colour;
uniform sampler2D u_face_depth;
uniform int u_face;
uniform vec3 u_face_right;
uniform vec3 u_face_up;
uniform vec3 u_face_forward;
uniform vec2 u_face_scale;
uniform vec2 u_face_size;
uniform vec2 u_face_depth_range;
uniform vec2 u_depth_range;
uniform int u_lens_mapping;
uniform float u_lens_focal;
uniform vec2 u_viewport;
uniform vec4 u_background;
out vec4 o_colour;
",
    lens_maps!(),
    r"
int owner(vec3 d) {
    vec3 a = abs(d);
    if (a.z >= a.x && a.z >= a.y) return d.z > 0.0 ? 0 : 5;
    if (a.x >= a.y) return d.x > 0.0 ? 1 : 2;
    return d.y > 0.0 ? 3 : 4;
}

void main() {
    vec2 px = vec2(gl_FragCoord.x, u_viewport.y - gl_FragCoord.y);
    vec3 d;
    if (!lens_direction(px, d)) {
        if (u_face != 0) discard;
        o_colour = u_background;
        gl_FragDepth = 1.0;
        return;
    }
    if (owner(d) != u_face) discard;
    vec3 fc = vec3(dot(d, u_face_right), dot(d, u_face_up), dot(d, u_face_forward));
    vec2 uv = vec2(fc.x / fc.z * u_face_scale.x + 1.0, 1.0 - fc.y / fc.z * u_face_scale.y)
        * 0.5 * u_face_size;
    ivec2 texel = ivec2(clamp(floor(uv), vec2(0.0), u_face_size - 1.0));
    texel.y = int(u_face_size.y) - 1 - texel.y;
    o_colour = texelFetch(u_face_colour, texel, 0);
    float ds = texelFetch(u_face_depth, texel, 0).r;
    if (ds >= 1.0) {
        gl_FragDepth = 1.0;
        return;
    }
    vec2 f = u_face_depth_range;
    float t = f.x * exp2(ds * log2(f.y / f.x)) / fc.z;
    gl_FragDepth = clamp(log2(t / u_depth_range.x) / log2(u_depth_range.y / u_depth_range.x), 0.0, 1.0);
}
"
);

/// The lens line vertex shader, any width; pair it with
/// [`GLSL_ES_300_LINE_FRAGMENT`] (`u_depth_range` the fisheye camera's
/// [`crate::Camera::depth_range`], `u_orthographic` false).
///
/// Attributes and stride as [`GLSL_ES_300_WIDE_LINE_VERTEX`], but with
/// divisor [`LENS_LINE_PIECES`], drawn
/// `drawArraysInstanced(TRIANGLE_STRIP, 0, 4, segments * LENS_LINE_PIECES)`:
/// instance `k` draws piece `k % LENS_LINE_PIECES` of its segment, whose
/// ends are lens-projected and joined straight, widened as the wide line
/// is. Uniforms: `u_eye`, `u_right`, `u_up`, `u_forward`
/// ([`FrameLens::eye`], [`FrameLens::basis`]), `u_lens_mapping`, `u_lens_focal`,
/// `u_lens_near` ([`FrameLens`]), `u_viewport`, `u_bias`, `u_crease_cos`,
/// `u_line_width` (as the wide line's).
///
/// A piece with an end nearer the eye than `u_lens_near`, or crossing
/// behind the eye between two sides of the back pole, is not drawn. Under
/// the panorama a segment crossing the seam is split so a piece ends
/// exactly on it, on the picture edge of that piece's side.
pub const GLSL_ES_300_LENS_LINE_VERTEX: &str = concat!(
    lens_line_head!(),
    r"layout(location = 0) in vec3 a_p0;
layout(location = 1) in vec4 a_c0;
layout(location = 2) in vec3 a_mid;
layout(location = 3) in vec3 a_n1;
layout(location = 4) in vec3 a_n2;
layout(location = 5) in float a_kind;
layout(location = 6) in float a_shaded;
layout(location = 7) in vec3 a_p1;
layout(location = 8) in vec4 a_c1;
layout(location = 9) in vec3 a_n1_end;
uniform float u_line_width;
const int PIECES = 16;
",
    lens_maps!(),
    r"
vec4 shade(vec4 c, vec3 n, vec3 p) {
    if (a_shaded > 0.5) c.rgb *= 0.25 + 0.75 * abs(dot(n, towards(p)));
    return c;
}

float piece_end(float seam, int m, int k) {
    if (seam < 0.0) return float(k) / float(PIECES);
    if (k <= m) return seam * float(k) / float(m);
    return seam + (1.0 - seam) * float(k - m) / float(PIECES - m);
}

void main() {
    vec3 ea = camera_of(a_p0);
    vec3 eb = camera_of(a_p1);
    float seam = -1.0;
    if (u_lens_mapping == 2 && ea.x * eb.x < 0.0) {
        float s = ea.x / (ea.x - eb.x);
        if (mix(ea, eb, s).z < 0.0) seam = s;
    }
    int m = int(clamp(floor(seam * float(PIECES) + 0.5), 1.0, float(PIECES - 1)));
    int piece = gl_InstanceID % PIECES;
    float s0 = piece_end(seam, m, piece);
    float s1 = piece_end(seam, m, piece + 1);
    vec3 pa = mix(ea, eb, s0);
    vec3 pb = mix(ea, eb, s1);
    vec4 c0 = shade(a_c0, a_n1, a_p0);
    vec4 c1 = shade(a_c1, a_n1_end, a_p1);
    v_colour = vec4(0.0);
    v_w = 1.0;
    gl_Position = vec4(2.0, 2.0, 2.0, 1.0);
    if (!kept(a_n1, a_n2, a_kind, a_mid)) return;
    float ta = length(pa);
    float tb = length(pb);
    if (ta < u_lens_near || tb < u_lens_near) return;
    if (u_lens_mapping != 2 && pa.z < 0.0 && pb.z < 0.0 && dot(pa.xy, pb.xy) < 0.0) return;
    vec2 na;
    vec2 nb;
    if (!lens_ndc(pa, na) || !lens_ndc(pb, nb)) return;
    if (seam >= 0.0 && piece + 1 == m) nb.x = na.x < 0.0 ? -1.0 : 1.0;
    if (seam >= 0.0 && piece == m) na.x = nb.x < 0.0 ? -1.0 : 1.0;
    if (u_lens_mapping == 2 && abs(na.x - nb.x) > 1.0) return;
    vec2 d = (nb - na) * u_viewport;
    float w = max(u_line_width, 1.0);
    float k = floor((w - 1.0) / 2.0);
    bool high = (gl_VertexID & 1) == 1;
    vec2 off = abs(d.x) >= abs(d.y)
        ? vec2(0.0, high ? k + 0.5 : -(w - k - 0.5))
        : vec2(high ? w - k - 0.5 : -(k + 0.5), 0.0);
    vec4 p = gl_VertexID < 2 ? vec4(na * ta, 0.0, ta) : vec4(nb * tb, 0.0, tb);
    v_colour = gl_VertexID < 2 ? mix(c0, c1, s0) : mix(c0, c1, s1);
    p.xy += off * 2.0 / u_viewport * p.w;
    gl_Position = p;
    v_w = p.w;
}
"
);

/// The lens point vertex shader; pair it with
/// [`GLSL_ES_300_LINE_FRAGMENT`] as [`GLSL_ES_300_LENS_LINE_VERTEX`] does.
/// Attributes as [`GLSL_ES_300_LINE_VERTEX`] (locations 0–6, per vertex,
/// drawn as `GL_POINTS`); uniforms as the lens line's but `u_point_size`
/// ([`DrawData::point_size`]) in place of `u_line_width`.
pub const GLSL_ES_300_LENS_POINT_VERTEX: &str = concat!(
    lens_line_head!(),
    r"layout(location = 0) in vec3 a_position;
layout(location = 1) in vec4 a_colour;
layout(location = 2) in vec3 a_mid;
layout(location = 3) in vec3 a_n1;
layout(location = 4) in vec3 a_n2;
layout(location = 5) in float a_kind;
layout(location = 6) in float a_shaded;
uniform float u_point_size;
",
    lens_maps!(),
    r"
void main() {
    vec4 c = a_colour;
    if (a_shaded > 0.5) c.rgb *= 0.25 + 0.75 * abs(dot(a_n1, towards(a_position)));
    v_colour = c;
    vec3 p = camera_of(a_position);
    float t = length(p);
    vec2 n;
    bool drawn = kept(a_n1, a_n2, a_kind, a_mid) && t >= u_lens_near && lens_ndc(p, n);
    gl_Position = drawn ? vec4(n * t, 0.0, t) : vec4(2.0, 2.0, 2.0, 1.0);
    v_w = gl_Position.w;
    gl_PointSize = u_point_size;
}
"
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LENS_LINE_PIECES;

    fn declares(shader: &str, names: &[&str]) {
        for name in names {
            assert!(shader.contains(&format!(" {name};")), "{name}");
        }
    }

    #[test]
    fn each_shader_declares_the_uniforms_its_doc_names() {
        declares(
            GLSL_ES_300_LENS_RESAMPLE_FRAGMENT,
            &[
                "u_face_colour",
                "u_face_depth",
                "u_face",
                "u_face_right",
                "u_face_up",
                "u_face_forward",
                "u_face_scale",
                "u_face_size",
                "u_face_depth_range",
                "u_depth_range",
                "u_lens_mapping",
                "u_lens_focal",
                "u_viewport",
                "u_background",
            ],
        );
        let lines = [
            "u_eye",
            "u_right",
            "u_up",
            "u_forward",
            "u_lens_mapping",
            "u_lens_focal",
            "u_lens_near",
            "u_viewport",
            "u_bias",
            "u_crease_cos",
        ];
        declares(GLSL_ES_300_LENS_LINE_VERTEX, &lines);
        declares(GLSL_ES_300_LENS_LINE_VERTEX, &["u_line_width"]);
        declares(GLSL_ES_300_LENS_POINT_VERTEX, &lines);
        declares(GLSL_ES_300_LENS_POINT_VERTEX, &["u_point_size"]);
    }

    #[test]
    fn the_line_shader_cuts_as_many_pieces_as_the_cpu_path() {
        let decl = format!("const int PIECES = {LENS_LINE_PIECES};");
        assert!(GLSL_ES_300_LENS_LINE_VERTEX.contains(&decl));
    }

    #[test]
    fn every_shader_carries_the_panorama_branch_the_cpu_lens_has() {
        for s in [
            GLSL_ES_300_LENS_RESAMPLE_FRAGMENT,
            GLSL_ES_300_LENS_LINE_VERTEX,
            GLSL_ES_300_LENS_POINT_VERTEX,
        ] {
            assert_eq!(s.matches("if (u_lens_mapping == 2) {").count(), 2);
        }
        assert_eq!(crate::FisheyeMapping::Equirectangular.code(), 2);
        // The seam cut mirrors `Lens::splits`: half the picture is 1 in NDC.
        let line = GLSL_ES_300_LENS_LINE_VERTEX;
        assert!(line.contains("u_lens_mapping == 2 && abs(na.x - nb.x) > 1.0"));
        assert!(line.contains("u_lens_mapping != 2 && pa.z < 0.0"));
    }

    #[test]
    fn every_shader_is_one_glsl_es_300_unit() {
        for s in [
            GLSL_ES_300_LENS_RESAMPLE_FRAGMENT,
            GLSL_ES_300_LENS_LINE_VERTEX,
            GLSL_ES_300_LENS_POINT_VERTEX,
        ] {
            assert!(s.starts_with("#version 300 es\n"));
            assert_eq!(s.matches("#version").count(), 1);
            assert_eq!(s.matches("void main()").count(), 1);
            assert_eq!(s.matches('{').count(), s.matches('}').count());
        }
    }
}
