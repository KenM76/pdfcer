//! One CPU frame under [`crate::RenderOptions::render_mode`]: the faces the
//! mode draws, the emissive term, then its lines and points.

use crate::camera::Camera;
use crate::render::mode::{Faces, Plan, frame_options};
use crate::render::mode_lines::{box_mesh, overlays, visible_boxes};
use crate::render::raster::Paint;
use crate::render::{Image, RenderError, RenderOptions, render_finished};
use crate::{AssembledModel, LightingScheme, PreparedModel, TriangleMesh};

/// The faces a mode draws: meshes, their paints and which are opaque.
pub(crate) struct FacePass<'m> {
    pub(crate) meshes: std::borrow::Cow<'m, [TriangleMesh]>,
    pub(crate) paints: Vec<Paint<'m>>,
    pub(crate) opaque: Vec<bool>,
}

/// The faces `plan` draws of `model`, painted `paints` / `opaque` as Solid
/// would paint them.
pub(crate) fn face_pass<'m>(
    plan: &Plan,
    model: &'m AssembledModel,
    (paints, opaque): (Vec<Paint<'m>>, Vec<bool>),
) -> FacePass<'m> {
    use std::borrow::Cow;
    let recolour = |f: &dyn Fn(&Paint<'m>) -> [u8; 4]| -> Vec<Paint<'m>> {
        paints
            .iter()
            .map(|p| Paint {
                colour: f(p),
                texture: None,
            })
            .collect()
    };
    let (paints, opaque) = match plan.faces {
        Faces::None => (Vec::new(), Vec::new()),
        Faces::Painted => (paints, opaque),
        Faces::Translucent(o) if o >= 1.0 => (paints, opaque),
        Faces::Translucent(o) => {
            let scaled = paints
                .iter()
                .map(|p| {
                    let [r, g, b, a] = p.colour;
                    let a = (f64::from(a) * o).round() as u8;
                    Paint {
                        colour: [r, g, b, a],
                        texture: p.texture,
                    }
                })
                .collect();
            (scaled, vec![false; paints.len()])
        }
        Faces::Coloured([r, g, b]) => {
            let p = recolour(&|p| [r, g, b, p.colour[3]]);
            let o = p.iter().map(|p| p.colour[3] == 255).collect();
            (p, o)
        }
        Faces::Hiding([r, g, b]) => {
            let p = recolour(&|p| [r, g, b, if p.colour[3] > 0 { 255 } else { 0 }]);
            let o = p.iter().map(|p| p.colour[3] == 255).collect();
            (p, o)
        }
        Faces::Boxes([r, g, b], o) => {
            let visible: Vec<bool> = paints.iter().map(|p| p.colour[3] > 0).collect();
            let boxes: Vec<TriangleMesh> = visible_boxes(model, &visible)
                .iter()
                .map(box_mesh)
                .collect();
            let alpha = (o * 255.0).round() as u8;
            let paint = Paint {
                colour: [r, g, b, alpha],
                texture: None,
            };
            return FacePass {
                paints: vec![paint; boxes.len()],
                opaque: vec![alpha == 255; boxes.len()],
                meshes: Cow::Owned(boxes),
            };
        }
    };
    let meshes = if plan.faces == Faces::None {
        Cow::Borrowed(&[][..])
    } else {
        Cow::Borrowed(&model.meshes[..])
    };
    FacePass {
        meshes,
        paints,
        opaque,
    }
}

/// Draws `model` as [`PreparedModel::render`] documents.
pub(crate) fn render(
    prepared: &PreparedModel,
    model: &AssembledModel,
    camera: &Camera,
    options: &RenderOptions,
) -> Result<Image, RenderError> {
    let plan = Plan::of(options);
    let painted = prepared.paints(model, options);
    let colours: Vec<[u8; 4]> = painted.0.iter().map(|p| p.colour).collect();
    let pass = face_pass(&plan, model, painted);
    let lit = frame_options(options);
    // Finishes index the model's own meshes; a mode drawing other faces or
    // its own face colour draws none.
    let finishes = match plan.faces {
        Faces::Painted | Faces::Translucent(_) => &model.finishes[..],
        _ => &[][..],
    };
    let scene = (&pass.meshes[..], &model.lights[..], finishes);
    let (mut image, mut depth) = render_finished(scene, &pass.paints, &pass.opaque, camera, &lit)?;
    if plan.emissive > 0.0 {
        let flat = RenderOptions {
            lighting: LightingScheme::None,
            shadows: false,
            ambient_occlusion: false,
            ..lit
        };
        let (unlit, _) = render_finished(scene, &pass.paints, &pass.opaque, camera, &flat)?;
        add_emissive(&mut image.rgba, &unlit.rgba, &depth, plan.emissive);
    }
    let over = overlays(model, options, &colours);
    let bias = crate::render::lines::bias(model);
    crate::render::lines::draw_overlays(
        &over,
        camera,
        (&mut image.rgba, &mut depth),
        options,
        bias,
    )?;
    Ok(image)
}

/// Adds `share` of `unlit` to `lit` wherever an opaque surface was drawn
/// (a finite depth key), each channel clamped to 255.
fn add_emissive(lit: &mut [u8], unlit: &[u8], depth: &[f64], share: f64) {
    for ((px, flat), key) in lit
        .chunks_exact_mut(4)
        .zip(unlit.chunks_exact(4))
        .zip(depth)
    {
        if key.is_finite() {
            for (c, &u) in px.iter_mut().zip(flat).take(3) {
                *c = (f64::from(*c) + share * f64::from(u)).round().min(255.0) as u8;
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use crate::{Bounds, FaceColour, RenderMode, RenderModeKind, render_model};

    const SIZE: u32 = 64;
    const GREY: [u8; 4] = [128, 128, 128, 255];

    /// A unit cube, orange, seen from a corner, orthographic, unlit unless
    /// a mode lights it.
    fn cube() -> AssembledModel {
        let p = |i: u32| [f64::from(i & 1), f64::from((i >> 1) & 1), f64::from(i >> 2)];
        let positions = (0..8).map(p).collect();
        let tri = crate::NodeBox::TRIANGLES.to_vec();
        AssembledModel {
            meshes: vec![TriangleMesh {
                positions,
                triangles: tri,
                ..TriangleMesh::default()
            }],
            colours: vec![Some([230, 120, 20, 255])],
            ..AssembledModel::default()
        }
    }

    fn frame(model: &AssembledModel, mode: RenderMode) -> Vec<u8> {
        let bounds = Bounds::of(&model.meshes).unwrap();
        let camera = Camera::fit(&bounds, [-1.0, -0.8, -0.6], [0., 0., 1.], false, 1.0).unwrap();
        let options = RenderOptions {
            width: SIZE,
            height: SIZE,
            background: GREY,
            render_mode: mode,
            ambient_occlusion: false,
            shadows: false,
            ..RenderOptions::default()
        };
        render_model(model, &camera, &options).unwrap().rgba
    }

    fn count(rgba: &[u8], f: impl Fn(&[u8]) -> bool) -> usize {
        rgba.chunks_exact(4).filter(|p| f(p)).count()
    }

    fn mode(kind: RenderModeKind) -> RenderMode {
        RenderMode {
            aux_colour: [0, 0, 255],
            ..RenderMode::new(kind)
        }
    }

    const BLUE: [u8; 4] = [0, 0, 255, 255];

    #[test]
    fn solid_is_unchanged_and_wireframe_adds_edges_over_it() {
        let model = cube();
        let solid = frame(&model, RenderMode::default());
        assert_eq!(count(&solid, |p| p == BLUE), 0);
        let wired = frame(&model, mode(RenderModeKind::SolidWireframe));
        let edges = count(&wired, |p| p == BLUE);
        assert!(edges > 50, "{edges} edge pixels");
        let differ = solid
            .chunks(4)
            .zip(wired.chunks(4))
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(differ, edges, "only edge pixels change");
    }

    #[test]
    fn wireframe_shows_hidden_edges_and_hidden_wireframe_removes_them() {
        let model = cube();
        let all = count(&frame(&model, mode(RenderModeKind::Wireframe)), |p| {
            p == BLUE
        });
        let hidden = frame(&model, mode(RenderModeKind::HiddenWireframe));
        let kept = count(&hidden, |p| p == BLUE);
        assert!(kept < all && kept > all / 2, "{kept} of {all}");
        // No face colour: the faces are background.
        assert_eq!(count(&hidden, |p| p != BLUE && p != GREY), 0);
    }

    #[test]
    fn transparent_faces_let_the_background_through() {
        let model = cube();
        let solid = frame(&model, RenderMode::default());
        let glass = frame(&model, mode(RenderModeKind::Transparent));
        let centre = (32 * 64 + 32) * 4;
        assert_eq!(solid[centre + 3], 255);
        assert!(glass[centre] < solid[centre] || glass[centre + 2] > solid[centre + 2]);
        assert_ne!(&glass[centre..centre + 4], &solid[centre..centre + 4]);
        let wire = frame(&model, mode(RenderModeKind::TransparentWireframe));
        assert!(count(&wire, |p| p == BLUE) > 50);
    }

    #[test]
    fn bounding_boxes_draw_edges_and_tinted_faces() {
        let model = cube();
        let edges = frame(&model, mode(RenderModeKind::BoundingBox));
        assert!(count(&edges, |p| p == BLUE) > 50);
        assert_eq!(count(&edges, |p| p != BLUE && p != GREY), 0, "no faces");
        let tinted = RenderMode {
            face_colour: FaceColour::Rgb([255, 0, 0]),
            opacity: 1.0,
            ..mode(RenderModeKind::TransparentBoundingBox)
        };
        let faces = frame(&model, tinted);
        assert!(count(&faces, |p| p == [255, 0, 0, 255]) > 500);
        let outline = RenderMode {
            kind: RenderModeKind::TransparentBoundingBoxOutline,
            ..tinted
        };
        assert!(count(&frame(&model, outline), |p| p == BLUE) > 30);
    }

    #[test]
    fn vertices_draw_points_in_the_aux_or_their_shaded_colour() {
        let model = cube();
        let dots = frame(&model, mode(RenderModeKind::Vertices));
        let blue = count(&dots, |p| p == BLUE);
        assert!((8 * 4..=8 * 9).contains(&blue), "{blue}");
        let shaded = frame(&model, mode(RenderModeKind::ShadedVertices));
        assert_eq!(count(&shaded, |p| p == BLUE), 0);
        assert!(count(&shaded, |p| p[0] > p[2] && p != GREY) >= 8 * 4);
        let wire = frame(&model, mode(RenderModeKind::ShadedWireframe));
        assert!(count(&wire, |p| p[0] > p[2] && p != GREY) > 50);
    }

    #[test]
    fn illustration_outlines_the_silhouette_over_flat_faces() {
        let model = cube();
        let flat = RenderMode {
            face_colour: FaceColour::Rgb([255, 255, 255]),
            ..mode(RenderModeKind::Illustration)
        };
        let ill = frame(&model, flat);
        assert_eq!(
            count(&ill, |p| p != BLUE
                && p != GREY
                && p != [255, 255, 255, 255]),
            0,
            "unlit faces, edges, background only"
        );
        // A cube from a corner: the six-sided outline and three inner
        // creases (90° ≥ 45°), never the three hidden edges.
        let lines = count(&ill, |p| p == BLUE);
        let all = count(&frame(&model, mode(RenderModeKind::Wireframe)), |p| {
            p == BLUE
        });
        assert!(lines > 50 && lines < all, "{lines} of {all}");
        let blunt = RenderMode {
            crease_angle: 120.0,
            ..flat
        };
        let outline = count(&frame(&model, blunt), |p| p == BLUE);
        assert!(outline < lines, "a 120° crease drops the inner edges");
    }

    #[test]
    fn shaded_illustration_is_brighter_than_solid_outline() {
        let model = cube();
        let plain = frame(&model, mode(RenderModeKind::SolidOutline));
        let lifted = frame(&model, mode(RenderModeKind::ShadedIllustration));
        let sum = |v: &[u8]| -> u64 { v.iter().map(|&c| u64::from(c)).sum() };
        assert!(sum(&lifted) > sum(&plain));
        assert_eq!(
            count(&plain, |p| p == BLUE),
            count(&lifted, |p| p == BLUE),
            "same silhouettes"
        );
    }
}
