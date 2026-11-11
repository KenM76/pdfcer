//! Draw data for a host's GPU: plain buffers a GLES 3.0 / OpenGL context
//! draws to the same picture as [`PreparedModel::render`]. No GPU code or
//! dependency lives here; the host makes every graphics call. Design:
//! `docs/research/gpu-3d-draw-data-design.md`.

pub(crate) mod ao;
#[macro_use]
pub(crate) mod bounce;
pub(crate) mod finish;
pub(crate) mod frame;
pub(crate) mod lamp_shadow;
pub(crate) mod lens;
pub(crate) mod lines;
pub(crate) mod prepass;
pub(crate) mod shader;
pub(crate) mod shadow;
pub(crate) mod shadow_set;
pub(crate) mod wide_lines;

use crate::render::raster::Paint;
use crate::render::{RenderOptions, at3, corners, stored_normals, triangle_uvs};
use crate::vec3::{span, unit_cross};
use crate::{AssembledModel, PreparedModel, Texture, TextureOrigin, TriangleMesh};

/// Floats per vertex in [`DrawData::vertices`].
pub const VERTEX_FLOATS: usize = 9;

/// A model's geometry and paint as GPU-ready buffers. It does not depend on
/// the camera: build it when the model or its paint changes, not per frame.
///
/// [`DrawData::vertices`] holds [`VERTEX_FLOATS`] floats per vertex, three
/// vertices per triangle, unshared:
///
/// | Floats | Meaning |
/// |---|---|
/// | 0–2 | position, model units |
/// | 3–5 | unit normal, model space |
/// | 6–7 | texture coordinate, ready to sample: the texture's `uv_matrix` and `origin` already applied, so with its rows uploaded top first `t` is the row direction |
/// | 8 | 1.0 when the triangle is flat-shaded, else 0.0 |
///
/// A flat triangle (no stored normals) takes ONE shade, the one its
/// **last** vertex computes: draw the `flat` term as a `flat` varying under
/// OpenGL's default last-vertex convention. No triangle needs culling;
/// lighting is two-sided.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct DrawData {
    /// Interleaved vertices, [`VERTEX_FLOATS`] per vertex.
    pub vertices: Vec<f32>,
    /// What to draw, in order: every opaque draw, then every translucent
    /// one, each group in mesh order.
    pub draws: Vec<Draw>,
    /// Line segments to draw after the faces, two vertices each,
    /// [`LINE_VERTEX_FLOATS`](crate::LINE_VERTEX_FLOATS) floats per vertex:
    /// as instanced quads with
    /// [`GLSL_ES_300_WIDE_LINE_VERTEX`](crate::GLSL_ES_300_WIDE_LINE_VERTEX)
    /// (any width), or as `GL_LINES` with
    /// [`GLSL_ES_300_LINE_VERTEX`](crate::GLSL_ES_300_LINE_VERTEX) when
    /// [`line_width`](Self::line_width) is 1.
    pub lines: Vec<f32>,
    /// Model units every line vertex is moved towards the eye (the line
    /// shader's `u_bias`), so an edge on a face is not hidden by it.
    pub line_bias: f32,
    /// Points to draw after the lines, as `GL_POINTS`, in the same vertex
    /// format and shader as [`DrawData::lines`] (midpoint = position).
    pub points: Vec<f32>,
    /// The points' size in pixels (the line shader's `u_point_size`).
    pub point_size: f32,
    /// Lines' width in pixels, at least 1 ([`RenderOptions::line_width`](crate::RenderOptions::line_width)):
    /// the wide-line shader's `u_line_width`.
    pub line_width: f32,
    /// The line shader's `u_crease_cos` under the render mode.
    pub crease_cos: f32,
    /// The share of the unlit face colour added over the lit frame
    /// ([`RenderModeKind::ShadedIllustration`](crate::RenderModeKind::ShadedIllustration));
    /// 0 for every other mode. A host draws the opaque draws a second time
    /// under an unlit [`crate::FrameLights`] (lighting
    /// [`crate::LightingScheme::None`]), depth function `EQUAL`, blending
    /// `glBlendFunc(GL_CONSTANT_COLOR, GL_ONE)` with every channel of the
    /// blend colour set to this, before the lines.
    pub emissive: f32,
}

/// One mesh's triangles and paint.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct Draw {
    /// The mesh, an index into the model's `meshes`.
    pub mesh: usize,
    /// The first vertex (not float) in [`DrawData::vertices`].
    pub first: u32,
    /// The vertex count, a multiple of three.
    pub count: u32,
    /// The base colour, straight RGBA, each channel 0–1.
    pub colour: [f32; 4],
    /// The texture, an index into the model's `textures`; its wrap and
    /// function are read from there.
    pub texture: Option<usize>,
    /// Drawn after every opaque draw, blended, depth-tested but not
    /// writing depth.
    pub translucent: bool,
    /// The mesh's specular, emissive and ambient terms; `None` (the
    /// shader's `u_finished` false) draws the plain colour.
    pub finish: Option<crate::DrawFinish>,
}

impl PreparedModel {
    /// The model's draw data, painted as [`PreparedModel::render`] paints
    /// it with `options` ([`RenderOptions::colour`] and
    /// [`RenderOptions::render_mode`] matter here). Fully transparent
    /// meshes, and triangles with a missing or non-finite corner or no
    /// area, are left out, as the CPU path leaves them out.
    ///
    /// The render mode is applied: draws are recoloured or made translucent
    /// as the mode's faces are, a mode that draws no faces has no draws, and
    /// in the two transparent bounding-box modes each [`Draw::mesh`]
    /// indexes the model's visible [`AssembledModel::boxes`] (box `i` as
    /// twelve triangles), not its meshes. Edges, silhouettes and vertices
    /// are in [`DrawData::lines`] and [`DrawData::points`].
    #[must_use]
    pub fn draw_data(&self, model: &AssembledModel, options: &RenderOptions) -> DrawData {
        let plan = crate::render::mode::Plan::of(options);
        let painted = self.paints(model, options);
        let colours: Vec<[u8; 4]> = painted.0.iter().map(|p| p.colour).collect();
        // Finishes index the model's own meshes, drawn in their own colour.
        let finished = matches!(
            plan.faces,
            crate::render::mode::Faces::Painted | crate::render::mode::Faces::Translucent(_)
        );
        let faces = crate::render::mode_frame::face_pass(&plan, model, painted);
        let (paints, opaque) = (&faces.paints, &faces.opaque);
        let mut data = DrawData::default();
        for pass in [true, false] {
            for (i, (mesh, paint)) in faces.meshes.iter().zip(paints).enumerate() {
                if paint.colour[3] == 0 || opaque.get(i).copied().unwrap_or(false) != pass {
                    continue;
                }
                let first = data.vertices.len() / VERTEX_FLOATS;
                push_mesh(&mut data.vertices, mesh, paint);
                let count = data.vertices.len() / VERTEX_FLOATS - first;
                if count == 0 {
                    continue;
                }
                data.draws.push(Draw {
                    mesh: i,
                    first: first as u32,
                    count: count as u32,
                    colour: paint.colour.map(|c| f32::from(c) / 255.0),
                    texture: model
                        .mesh_textures
                        .get(i)
                        .copied()
                        .flatten()
                        .filter(|_| paint.texture.is_some()),
                    translucent: !pass,
                    finish: model
                        .finishes
                        .get(i)
                        .copied()
                        .flatten()
                        .filter(|_| finished)
                        .map(|f| crate::DrawFinish::new(&f, options)),
                });
            }
        }
        crate::gpu::lines::add_overlays(&mut data, model, options, &colours);
        data.emissive = plan.emissive as f32;
        data
    }
}

/// Appends every drawable triangle of `mesh`.
fn push_mesh(out: &mut Vec<f32>, mesh: &TriangleMesh, paint: &Paint<'_>) {
    for (t, tri) in mesh.triangles.iter().enumerate() {
        let Some(world) = corners(mesh, tri) else {
            continue;
        };
        let Some(face) = unit_cross(
            span(at3(&world, 0), at3(&world, 1)),
            span(at3(&world, 0), at3(&world, 2)),
        ) else {
            continue;
        };
        let stored = stored_normals(mesh, t);
        let uvs = paint
            .texture
            .and_then(|tex| triangle_uvs(mesh, tex.uv_set, t));
        // Corner 0 last: a flat triangle's shade is the last vertex's.
        for k in [1, 2, 0] {
            let normal = stored
                .and_then(|n| n.get(k).copied().flatten())
                .unwrap_or(face);
            let uv = uvs.and_then(|c| c.get(k).copied()).unwrap_or([0.0; 2]);
            let [u, v] = paint.texture.map_or([0.0; 2], |tex| sample_space(tex, uv));
            out.extend(at3(&world, k).map(|c| c as f32));
            out.extend(normal.map(|c| c as f32));
            out.extend([u as f32, v as f32, if stored.is_none() { 1.0 } else { 0.0 }]);
        }
    }
}

/// `uv` as [`Texture::sample`] addresses the picture: `uv_matrix`, then
/// `origin`, so v counts rows from the top.
fn sample_space(texture: &Texture, uv: [f64; 2]) -> [f64; 2] {
    let [u, v] = uv;
    let [mu, mv] = texture.uv_matrix;
    let row = |m: [f64; 3]| m[0] * u + m[1] * v + m[2];
    let v = row(mv);
    let v = match texture.origin {
        TextureOrigin::BottomLeft => 1.0 - v,
        TextureOrigin::TopLeft => v,
    };
    [row(mu), v]
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use crate::{TextureFunction, TextureWrap};

    fn triangle(z: f64) -> TriangleMesh {
        TriangleMesh {
            positions: vec![[0.0, 0.0, z], [1.0, 0.0, z], [0.0, 1.0, z]],
            triangles: vec![[0, 1, 2]],
            ..TriangleMesh::default()
        }
    }

    fn data(model: &AssembledModel) -> DrawData {
        PreparedModel::new(model).draw_data(model, &RenderOptions::default())
    }

    #[test]
    fn opaque_draws_come_first_and_transparent_meshes_are_left_out() {
        let model = AssembledModel {
            meshes: vec![triangle(0.0), triangle(1.0), triangle(2.0)],
            colours: vec![Some([0, 0, 255, 128]), Some([255, 0, 0, 255]), Some([0; 4])],
            ..AssembledModel::default()
        };
        let d = data(&model);
        let order: Vec<(usize, bool, u32)> = d
            .draws
            .iter()
            .map(|x| (x.mesh, x.translucent, x.first))
            .collect();
        assert_eq!(order, [(1, false, 0), (0, true, 3)]);
        assert_eq!(d.vertices.len(), 6 * VERTEX_FLOATS);
        // The opaque mesh's first vertex is corner 1, at z = 1.
        assert_eq!(&d.vertices[..3], &[1.0, 0.0, 1.0]);
    }

    #[test]
    fn a_triangle_with_edges_near_1e160_is_emitted() {
        // Its cross product overflows; the face normal must not.
        let mut mesh = triangle(0.0);
        mesh.positions = vec![[0.0; 3], [1e160, 0.0, 0.0], [0.0, 1e160, 0.0]];
        let model = AssembledModel {
            meshes: vec![mesh],
            ..AssembledModel::default()
        };
        let d = data(&model);
        assert_eq!(d.vertices.len(), 3 * VERTEX_FLOATS);
        assert_eq!(&d.vertices[3..6], &[0.0, 0.0, 1.0], "the face normal");
    }

    #[test]
    fn a_flat_triangle_puts_corner_zero_last_and_says_so() {
        let mut model = AssembledModel {
            meshes: vec![triangle(0.0)],
            ..AssembledModel::default()
        };
        let d = data(&model);
        let last = &d.vertices[2 * VERTEX_FLOATS..];
        assert_eq!(&last[..3], &[0.0, 0.0, 0.0]);
        assert_eq!(&last[3..6], &[0.0, 0.0, 1.0], "the face normal");
        assert_eq!(last[8], 1.0);
        model.meshes[0].normals = vec![[0.0, 0.0, 2.0], [1.0, 0.0, 0.0]];
        model.meshes[0].triangle_normals = vec![[0, 1, 0]];
        let d = data(&model);
        assert_eq!(
            &d.vertices[3..6],
            &[1.0, 0.0, 0.0],
            "corner 1's stored normal"
        );
        assert!(d.vertices.chunks(VERTEX_FLOATS).all(|v| v[8] == 0.0));
    }

    #[test]
    fn texture_coordinates_arrive_in_sampling_space() {
        let mut mesh = triangle(0.0);
        mesh.uvs = vec![[0.25, 0.25]];
        mesh.triangle_uvs = vec![vec![Some([0, 0, 0])]];
        let model = AssembledModel {
            meshes: vec![mesh],
            textures: vec![Texture {
                width: 1,
                height: 1,
                rgba: vec![255; 4],
                wrap: [TextureWrap::Repeat; 2],
                function: TextureFunction::Modulate,
                uv_matrix: [[2.0, 0.0, 0.0], [0.0, 1.0, 0.5]],
                uv_set: 0,
                origin: TextureOrigin::BottomLeft,
            }],
            mesh_textures: vec![Some(0)],
            ..AssembledModel::default()
        };
        let d = data(&model);
        assert_eq!(d.draws[0].texture, Some(0));
        assert_eq!(
            &d.vertices[6..8],
            &[0.5, 0.25],
            "u doubled; v moved then flipped"
        );
    }
}
