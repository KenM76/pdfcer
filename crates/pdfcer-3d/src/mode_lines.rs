//! The lines and points a frame draws over its faces under its render mode
//! — wires, mesh edges, silhouettes, bounding-box edges, vertices — built
//! once, camera-independent, for both the CPU path and
//! [`crate::DrawData`]. What depends on the camera (whether a silhouette
//! shows, a shaded colour) is carried in [`LineExtra`] and decided per frame
//! by [`Item::resolve`] or the line shader, with the same expressions.

use crate::edges::{self, MeshEdges, is_silhouette, shade_factor, shaded};
use crate::gpu_lines::LineExtra;
use crate::lines::{Overlay, Stroke, wire_strokes};
use crate::render_mode::{Edges, Plan, WireLook};
use crate::{AssembledModel, EdgeSet, IDENTITY, NodeBox, RenderOptions, TriangleMesh};

/// A segment and what its ends carry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Item {
    pub(crate) stroke: Stroke,
    pub(crate) extras: [LineExtra; 2],
}

/// A point, its unshaded colour and what it carries.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Dot {
    pub(crate) at: [f64; 3],
    pub(crate) colour: [u8; 4],
    pub(crate) extra: LineExtra,
}

/// Everything drawn over the faces, lines before points, in draw order.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Overlays {
    pub(crate) lines: Vec<Item>,
    pub(crate) points: Vec<Dot>,
}

/// The line shader's kinds.
const ALWAYS: f32 = 0.0;
const SILHOUETTE: f32 = 1.0;

impl Item {
    /// The stroke this frame draws, or `None` when it is a silhouette edge
    /// that is not one from here.
    pub(crate) fn resolve(&self, overlay: &Overlay, crease_cos: f64) -> Option<Stroke> {
        let [a, b] = self.stroke.ends;
        let [first, second] = self.extras;
        if first.kind == SILHOUETTE {
            let mid = [0, 1, 2].map(|i| (at(a, i) + at(b, i)) / 2.0);
            let edge = edges::Edge {
                ends: [a, b],
                vertex_normals: [[0.0; 3]; 2],
                normals: first.normals,
                crease: true,
            };
            if !is_silhouette(&edge, overlay.towards(mid), crease_cos) {
                return None;
            }
        }
        let mut stroke = self.stroke;
        for ((c, p), e) in stroke.colours.iter_mut().zip([a, b]).zip([first, second]) {
            if e.shaded {
                *c = shaded(*c, shade_factor(e.normals[0], overlay.towards(p)));
            }
        }
        Some(stroke)
    }
}

impl Dot {
    /// The colour this frame draws it in.
    pub(crate) fn resolve(&self, overlay: &Overlay) -> [u8; 4] {
        if self.extra.shaded {
            shaded(
                self.colour,
                shade_factor(self.extra.normals[0], overlay.towards(self.at)),
            )
        } else {
            self.colour
        }
    }
}

/// Component `i` of `v`, 0 when out of range.
pub(crate) fn at(v: [f64; 3], i: usize) -> f64 {
    v.get(i).copied().unwrap_or(0.0)
}

/// The boxes the bounding-box modes draw: each placement's that draws a
/// visible mesh (`visible[i]` for mesh `i`), or, when the model lists none,
/// each visible mesh's own.
pub(crate) fn visible_boxes(model: &AssembledModel, visible: &[bool]) -> Vec<NodeBox> {
    let shown = |i: usize| visible.get(i).copied().unwrap_or(true);
    if model.boxes.is_empty() {
        return model
            .meshes
            .iter()
            .enumerate()
            .filter(|&(i, _)| shown(i))
            .filter_map(|(_, m)| NodeBox::of(m, &IDENTITY, None))
            .collect();
    }
    model
        .boxes
        .iter()
        .filter(|b| {
            let Some(k) = b.placement else {
                return true;
            };
            let mut drawn = model
                .mesh_placements
                .iter()
                .enumerate()
                .filter(|&(_, &p)| p == k)
                .peekable();
            drawn.peek().is_none() || drawn.any(|(i, _)| shown(i))
        })
        .copied()
        .collect()
}

/// A box as a mesh of twelve triangles.
pub(crate) fn box_mesh(b: &NodeBox) -> TriangleMesh {
    TriangleMesh {
        positions: b.corners.to_vec(),
        triangles: NodeBox::TRIANGLES.to_vec(),
        ..TriangleMesh::default()
    }
}

/// What `options.render_mode` draws over `model`'s faces; mesh `i` drawn
/// in `colours[i]`, left out when its alpha is 0.
pub(crate) fn overlays(
    model: &AssembledModel,
    options: &RenderOptions,
    colours: &[[u8; 4]],
) -> Overlays {
    let plan = Plan::of(options);
    let mode = &options.render_mode;
    let [r, g, b] = mode.aux_colour;
    let aux = [r, g, b, 255];
    let mut out = Overlays::default();
    add_wires(&mut out, model, options, plan.wires, aux);
    let visible: Vec<bool> = colours.iter().map(|c| c[3] > 0).collect();
    if plan.edges == Edges::Boxes {
        for nb in visible_boxes(model, &visible) {
            for [i, j] in NodeBox::EDGES {
                let end = |k: usize| nb.corners.get(k).copied().unwrap_or([0.0; 3]);
                out.lines.push(Item {
                    stroke: Stroke {
                        ends: [end(i), end(j)],
                        colours: [aux; 2],
                    },
                    extras: [LineExtra::default(); 2],
                });
            }
        }
    }
    let meshes = matches!(plan.edges, Edges::Mesh | Edges::Silhouette) || plan.points;
    if !meshes {
        return out;
    }
    let crease_cos = mode.crease_cos();
    for (i, mesh) in model.meshes.iter().enumerate() {
        let Some(&colour) = colours.get(i).filter(|c| c[3] > 0) else {
            continue;
        };
        let base = if plan.shaded { colour } else { aux };
        let welded = edges::of(mesh, crease_cos);
        add_mesh(&mut out, &welded, &plan, mode.edges, base);
    }
    out
}

fn add_mesh(out: &mut Overlays, welded: &MeshEdges, plan: &Plan, set: EdgeSet, base: [u8; 4]) {
    let vertex = |n: [f64; 3]| LineExtra {
        normals: [n, [0.0; 3]],
        kind: ALWAYS,
        shaded: plan.shaded,
    };
    if plan.points {
        out.points.extend(welded.vertices.iter().map(|v| Dot {
            at: v.at,
            colour: base,
            extra: vertex(v.normal),
        }));
    }
    for e in &welded.edges {
        let extras = match plan.edges {
            Edges::Mesh if set == EdgeSet::Triangles || e.crease => e.vertex_normals.map(vertex),
            Edges::Silhouette => {
                [LineExtra {
                    normals: e.normals,
                    kind: SILHOUETTE,
                    shaded: false,
                }; 2]
            }
            _ => continue,
        };
        out.lines.push(Item {
            stroke: Stroke {
                ends: e.ends,
                colours: [base; 2],
            },
            extras,
        });
    }
}

fn add_wires(
    out: &mut Overlays,
    model: &AssembledModel,
    options: &RenderOptions,
    look: WireLook,
    aux: [u8; 4],
) {
    let points = |out: &mut Overlays, colour: Option<[u8; 4]>| {
        for line in &model.lines {
            for (i, &p) in line.points.iter().enumerate() {
                out.points.push(Dot {
                    at: p,
                    colour: colour.unwrap_or_else(|| line.colour_at(i, options.colour)),
                    extra: LineExtra::default(),
                });
            }
        }
    };
    match look {
        WireLook::Hidden => {}
        WireLook::AuxPoints => points(out, Some(aux)),
        WireLook::OwnPoints => points(out, None),
        WireLook::Own | WireLook::Aux => {
            for mut stroke in wire_strokes(&model.lines, options.colour) {
                if look == WireLook::Aux {
                    stroke.colours = [aux; 2];
                }
                out.lines.push(Item {
                    stroke,
                    extras: [LineExtra::default(); 2],
                });
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use crate::{ModelLine, RenderMode, RenderModeKind};

    fn model() -> AssembledModel {
        AssembledModel {
            meshes: vec![TriangleMesh {
                positions: vec![[0., 0., 0.], [1., 0., 0.], [1., 1., 0.], [0., 1., 0.]],
                triangles: vec![[0, 1, 2], [0, 2, 3]],
                ..TriangleMesh::default()
            }],
            lines: vec![ModelLine {
                points: vec![[0.0; 3], [1.0, 1.0, 1.0]],
                colour: Some([0, 255, 0, 255]),
                ..ModelLine::default()
            }],
            ..AssembledModel::default()
        }
    }

    fn under(kind: RenderModeKind) -> Overlays {
        let options = RenderOptions {
            render_mode: RenderMode {
                aux_colour: [9, 9, 9],
                ..RenderMode::new(kind)
            },
            ..RenderOptions::default()
        };
        overlays(&model(), &options, &[[200, 0, 0, 255]])
    }

    #[test]
    fn each_mode_family_gathers_what_it_draws() {
        let solid = under(RenderModeKind::Solid);
        assert_eq!((solid.lines.len(), solid.points.len()), (1, 0), "the wire");
        let wire = under(RenderModeKind::Wireframe);
        assert_eq!(wire.lines.len(), 6, "the wire and five edges");
        assert!(
            wire.lines
                .iter()
                .all(|l| l.stroke.colours[0] == [9, 9, 9, 255])
        );
        let boxes = under(RenderModeKind::BoundingBox);
        assert_eq!(boxes.lines.len(), 12, "no wire, one box");
        let dots = under(RenderModeKind::ShadedVertices);
        assert_eq!(dots.points.len(), 6, "four vertices, two wire points");
        assert!(dots.points[2].extra.shaded && !dots.points[0].extra.shaded);
        let outline = under(RenderModeKind::Illustration);
        assert!(
            outline.lines[1..]
                .iter()
                .all(|l| l.extras[0].kind == SILHOUETTE)
        );
    }

    #[test]
    fn a_hidden_mesh_draws_no_edges_and_no_box() {
        let options = RenderOptions {
            render_mode: RenderMode::new(RenderModeKind::Wireframe),
            ..RenderOptions::default()
        };
        assert_eq!(overlays(&model(), &options, &[[1, 2, 3, 0]]).lines.len(), 1);
        assert!(visible_boxes(&model(), &[false]).is_empty());
    }
}
