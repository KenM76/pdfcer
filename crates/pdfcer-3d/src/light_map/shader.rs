//! The light map's shaders: GLSL ES 3.00, the same `u_view` and
//! `u_projection` as [`crate::GLSL_ES_300_VERTEX`].

/// The light-map vertex shader, for [`crate::IlluminanceMap::vertex_buffer`]
/// drawn as a triangle list: `a_position` (location 0) its first three
/// floats, `a_figure` (location 1) its fourth, a stride of
/// [`crate::MAP_VERTEX_FLOATS`] floats. Draw opaque, depth-tested and
/// depth-writing, with [`GLSL_ES_300_LIGHT_MAP_FRAGMENT`].
pub const GLSL_ES_300_LIGHT_MAP_VERTEX: &str = r"#version 300 es
precision highp float;
layout(location = 0) in vec3 a_position;
layout(location = 1) in float a_figure;
uniform mat4 u_view;
uniform mat4 u_projection;
out float v_figure;
invariant gl_Position;

void main() {
    gl_Position = u_projection * u_view * vec4(a_position, 1.0);
    v_figure = a_figure;
}
";

/// The light-map fragment shader: the figure, interpolated
/// perspective-correct, coloured by the band table
/// ([`crate::LuxBands::uniforms`]: `u_band_from`, `u_band_colour`,
/// `u_band_count`, `u_graded`) by the rule [`crate::LuxBands::colour`]
/// applies on the CPU.
pub const GLSL_ES_300_LIGHT_MAP_FRAGMENT: &str = r"#version 300 es
precision highp float;
in float v_figure;
uniform float u_band_from[16];
uniform vec3 u_band_colour[16];
uniform int u_band_count;
uniform bool u_graded;
out vec4 o_colour;

void main() {
    int i = -1;
    for (int k = 0; k < 16; k++) {
        if (k < u_band_count && u_band_from[k] <= v_figure) i = k;
    }
    vec3 c = u_band_colour[0];
    if (i >= 0) {
        c = u_band_colour[i];
        if (u_graded && i + 1 < u_band_count) {
            float t = (v_figure - u_band_from[i]) / (u_band_from[i + 1] - u_band_from[i]);
            c = mix(u_band_colour[i], u_band_colour[i + 1], t);
        }
    }
    o_colour = vec4(c, 1.0);
}
";
