//! Lines and points for a host's GPU: [`crate::DrawData::lines`] and the
//! reference shader that draws them as [`crate::PreparedModel::render`]
//! does — unlit, pulled towards the eye by [`crate::DrawData::line_bias`],
//! depth-tested nearer-or-equal.

use crate::lines::Stroke;
use crate::mode_lines::at;
use crate::{AssembledModel, DrawData, RenderOptions};

/// Floats per vertex in [`DrawData::lines`]:
///
/// | Floats | Meaning |
/// |---|---|
/// | 0–2 | position, model units |
/// | 3–6 | colour, straight RGBA 0–1 |
/// | 7–9 | the segment's midpoint |
/// | 10–12 | unit normal of the first face beside it; when shaded, the vertex normal |
/// | 13–15 | unit normal of the second face (zero when none) |
/// | 16 | kind: 0 always drawn; 1 only on a silhouette; 2 only when a face beside it faces the viewer |
/// | 17 | 1.0 when the colour is shaded by the viewing angle, else 0.0 |
///
/// A model's wires are kind 0, unshaded. A render mode's mesh edges are
/// kind 0 (shaded in the shaded modes), its silhouettes kind 1, its
/// bounding-box edges kind 0; [`DrawData::points`] carry the same layout.
pub const LINE_VERTEX_FLOATS: usize = 18;

/// The line vertex shader. Attributes, matching [`DrawData::lines`]:
/// location 0 position (vec3), 1 colour (vec4), 2 midpoint (vec3), 3 first
/// normal (vec3), 4 second normal (vec3), 5 kind (float), 6 shaded (float).
/// Uniforms: `u_view`, `u_projection`, `u_eye`, `u_forward`,
/// `u_orthographic` (as [`crate::GLSL_ES_300_VERTEX`]'s), `u_bias`
/// ([`DrawData::line_bias`]), `u_crease_cos` ([`DrawData::crease_cos`]:
/// the cosine of the crease angle a kind-1 edge between two faces both
/// facing the viewer must reach), `u_point_size`
/// ([`DrawData::point_size`], read only when drawing `GL_POINTS`). A
/// vertex whose segment is not drawn is moved outside the clip volume, so
/// the segment is clipped away whole.
///
/// The facing test uses the midpoint: a face faces the viewer when
/// `n·v > 0`, `v` towards the eye from the midpoint (−`u_forward`
/// orthographic). A kind-1 segment is drawn when exactly one face does, or
/// both do and their normals' dot product is at most `u_crease_cos`.
pub const GLSL_ES_300_LINE_VERTEX: &str = r"#version 300 es
precision highp float;
layout(location = 0) in vec3 a_position;
layout(location = 1) in vec4 a_colour;
layout(location = 2) in vec3 a_mid;
layout(location = 3) in vec3 a_n1;
layout(location = 4) in vec3 a_n2;
layout(location = 5) in float a_kind;
layout(location = 6) in float a_shaded;
uniform mat4 u_view;
uniform mat4 u_projection;
uniform vec3 u_eye;
uniform vec3 u_forward;
uniform bool u_orthographic;
uniform float u_bias;
uniform float u_crease_cos;
uniform float u_point_size;
out vec4 v_colour;
out float v_w;

void main() {
    vec3 v = u_orthographic ? -u_forward : normalize(u_eye - a_mid);
    bool keep = true;
    if (a_kind > 0.5) {
        bool f1 = dot(a_n1, v) > 0.0;
        bool f2 = dot(a_n2, v) > 0.0;
        keep = a_kind < 1.5 ? (f1 != f2 || (f1 && dot(a_n1, a_n2) <= u_crease_cos)) : (f1 || f2);
    }
    vec3 towards = u_orthographic ? -u_forward : normalize(u_eye - a_position);
    vec4 c = a_colour;
    if (a_shaded > 0.5) c.rgb *= 0.25 + 0.75 * abs(dot(a_n1, towards));
    v_colour = c;
    gl_Position = keep
        ? u_projection * u_view * vec4(a_position + towards * u_bias, 1.0)
        : vec4(2.0, 2.0, 2.0, 1.0);
    v_w = gl_Position.w;
    gl_PointSize = u_point_size;
}
";

/// The line fragment shader: the interpolated colour, straight RGBA, and
/// the same `gl_FragDepth` as [`crate::GLSL_ES_300_FRAGMENT`] (uniforms
/// `u_depth_range`, `u_orthographic`). Draw [`DrawData::lines`] as
/// `GL_LINES` after the faces (and any ambient-occlusion composite), then
/// [`DrawData::points`] as `GL_POINTS`, depth function `LEQUAL`, blending
/// source-alpha over. `glLineWidth` above 1 is not portable (WebGL clamps
/// it to 1), so wider lines are CPU-only.
pub const GLSL_ES_300_LINE_FRAGMENT: &str = r"#version 300 es
precision highp float;
in vec4 v_colour;
in float v_w;
uniform vec2 u_depth_range;
uniform bool u_orthographic;
out vec4 o_colour;

void main() {
    o_colour = v_colour;
    gl_FragDepth = u_orthographic
        ? gl_FragCoord.z
        : clamp(log2(v_w / u_depth_range.x) / log2(u_depth_range.y / u_depth_range.x), 0.0, 1.0);
}
";

/// What else a line vertex carries beyond its position and colour.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct LineExtra {
    pub(crate) normals: [[f64; 3]; 2],
    pub(crate) kind: f32,
    pub(crate) shaded: bool,
}

/// Appends `stroke` as two [`LINE_VERTEX_FLOATS`] vertices, end `i`
/// carrying `extras[i]`.
pub(crate) fn push_stroke(out: &mut Vec<f32>, stroke: &Stroke, extras: &[LineExtra; 2]) {
    let [a, b] = stroke.ends;
    let mid = [0, 1, 2].map(|i| (at(a, i) + at(b, i)) / 2.0);
    for ((p, colour), extra) in [a, b].into_iter().zip(stroke.colours).zip(extras) {
        push_vertex(out, p, colour, mid, extra);
    }
}

fn push_vertex(out: &mut Vec<f32>, p: [f64; 3], colour: [u8; 4], mid: [f64; 3], e: &LineExtra) {
    out.extend(p.map(|c| c as f32));
    out.extend(colour.map(|c| f32::from(c) / 255.0));
    out.extend(mid.map(|c| c as f32));
    for n in e.normals {
        out.extend(n.map(|c| c as f32));
    }
    out.extend([e.kind, if e.shaded { 1.0 } else { 0.0 }]);
}

/// Fills `data`'s line and point fields with what `options`' render mode
/// draws over `model`'s faces, mesh `i` painted `colours[i]`.
pub(crate) fn add_overlays(
    data: &mut DrawData,
    model: &AssembledModel,
    options: &RenderOptions,
    colours: &[[u8; 4]],
) {
    data.line_bias = crate::lines::bias(model) as f32;
    data.crease_cos = options.render_mode.crease_cos() as f32;
    data.point_size = options.render_mode.point_size.max(1) as f32;
    let over = crate::mode_lines::overlays(model, options, colours);
    for item in &over.lines {
        push_stroke(&mut data.lines, &item.stroke, &item.extras);
    }
    for dot in &over.points {
        push_vertex(&mut data.points, dot.at, dot.colour, dot.at, &dot.extra);
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use crate::{ModelLine, PreparedModel};

    #[test]
    fn a_wire_becomes_line_vertices_in_its_colours() {
        let model = AssembledModel {
            lines: vec![ModelLine {
                points: vec![[0.0; 3], [2.0, 0.0, 0.0], [2.0, 2.0, 0.0]],
                colours: vec![[255, 0, 0, 255]; 3],
                ..ModelLine::default()
            }],
            ..AssembledModel::default()
        };
        let d = PreparedModel::new(&model).draw_data(&model, &RenderOptions::default());
        assert_eq!(d.lines.len(), 4 * LINE_VERTEX_FLOATS, "two segments");
        let v = &d.lines[LINE_VERTEX_FLOATS..2 * LINE_VERTEX_FLOATS];
        assert_eq!(&v[..7], &[2.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]);
        assert_eq!(&v[7..10], &[1.0, 0.0, 0.0], "the midpoint");
        assert_eq!(&v[16..], &[0.0, 0.0], "always drawn, unshaded");
    }

    #[test]
    fn a_mode_fills_points_and_per_end_normals() {
        use crate::{RenderMode, RenderModeKind, TriangleMesh};
        let model = AssembledModel {
            meshes: vec![TriangleMesh {
                positions: vec![[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]],
                triangles: vec![[0, 1, 2]],
                ..TriangleMesh::default()
            }],
            ..AssembledModel::default()
        };
        let options = |kind| RenderOptions {
            render_mode: RenderMode::new(kind),
            ..RenderOptions::default()
        };
        let p = PreparedModel::new(&model);
        let dots = p.draw_data(&model, &options(RenderModeKind::ShadedVertices));
        assert!(dots.draws.is_empty(), "no faces");
        assert_eq!(dots.points.len(), 3 * LINE_VERTEX_FLOATS);
        assert_eq!(dots.point_size, 3.0);
        assert_eq!(dots.points[LINE_VERTEX_FLOATS - 1], 1.0, "shaded");
        let wire = p.draw_data(&model, &options(RenderModeKind::ShadedWireframe));
        assert_eq!(wire.lines.len(), 6 * LINE_VERTEX_FLOATS, "three edges");
        assert_eq!(&wire.lines[10..13], &[0.0, 0.0, 1.0], "vertex normal");
        let ill = p.draw_data(&model, &options(RenderModeKind::ShadedIllustration));
        assert_eq!((ill.draws.len(), ill.emissive), (1, 0.5));
        assert_eq!(ill.lines[16], 1.0, "a silhouette");
    }

    #[test]
    fn the_shaders_declare_what_their_docs_name() {
        for name in [
            "u_bias",
            "u_crease_cos",
            "u_eye",
            "u_forward",
            "u_point_size",
        ] {
            assert!(
                GLSL_ES_300_LINE_VERTEX.contains(&format!(" {name};")),
                "{name}"
            );
        }
        assert!(GLSL_ES_300_LINE_FRAGMENT.contains(" u_depth_range;"));
    }
}
