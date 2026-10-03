//! A PRC file assembled into placed, coloured triangle meshes: what mesh
//! export, rendering and the default 3D poster all draw.

use crate::{PrcError, PrcFile, StyleAlpha, Tessellation, TriangleMesh};

/// A PRC model's triangle meshes, placed where its assembly tree draws them,
/// with counts of what was not drawn.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct AssembledModel {
    /// The placed meshes; a colour-split part is its own mesh.
    pub meshes: Vec<TriangleMesh>,
    /// Each mesh's colour from the model tree, straight RGBA, parallel to
    /// [`Self::meshes`] (empty when placements were not applied).
    pub colours: Vec<Option<[u8; 4]>>,
    /// Triangles across every mesh.
    pub triangles: usize,
    /// Wire tessellations, not drawn.
    pub wires: usize,
    /// Markup tessellations, not drawn.
    pub markups: usize,
    /// Compressed tessellations rebuilt into triangles.
    pub rebuilt: usize,
    /// Compressed tessellations left out.
    pub compressed: usize,
    /// Each distinct reason a compressed mesh was left out, with its count.
    pub skipped_why: Vec<(String, usize)>,
    /// Why placements were not applied (each mesh is then drawn once in its
    /// own coordinates), or `None` when they were.
    pub unplaced: Option<String>,
}

/// Why [`assemble`] found nothing to draw.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AssembleError {
    /// The bytes do not open with the PRC signature; only PRC is decoded.
    #[error("not a PRC model; only PRC is decoded")]
    NotPrc,
    /// The PRC container or a tessellation section could not be read.
    #[error(transparent)]
    Prc(#[from] PrcError),
    /// Every mesh uses a compressed tessellation pdfcer does not rebuild.
    #[error(
        "the model's {count} mesh(es) use compressed tessellation in a form pdfcer does not yet rebuild into triangles ({reasons})"
    )]
    CompressedOnly {
        /// Compressed meshes left out.
        count: usize,
        /// Their distinct reasons, `; `-joined.
        reasons: String,
    },
    /// The model holds no triangle tessellation at all.
    #[error("the model holds no triangle tessellation")]
    NoTriangles,
}

/// Decode a PRC model and place its meshes.
///
/// A tree whose placements cannot be read falls back to every mesh once, in
/// its own coordinates, uncoloured, and says why in
/// [`AssembledModel::unplaced`].
///
/// # Errors
///
/// [`AssembleError`] when the bytes are not PRC, do not parse, or hold no
/// drawable triangle.
///
/// ```
/// assert_eq!(pdfcer_3d::assemble(b"U3D\0"), Err(pdfcer_3d::AssembleError::NotPrc));
/// ```
pub fn assemble(data: &[u8]) -> Result<AssembledModel, AssembleError> {
    assemble_with(data, StyleAlpha::default())
}

/// [`assemble`], combining each style's transparency with its material's
/// alpha by `rule`.
///
/// # Errors
///
/// As [`assemble`].
///
/// ```
/// use pdfcer_3d::{AssembleError, StyleAlpha, assemble_with};
/// assert_eq!(assemble_with(b"U3D\0", StyleAlpha::Multiply), Err(AssembleError::NotPrc));
/// ```
pub fn assemble_with(data: &[u8], rule: StyleAlpha) -> Result<AssembledModel, AssembleError> {
    if !data.starts_with(b"PRC") {
        return Err(AssembleError::NotPrc);
    }
    let prc = PrcFile::parse(data)?;
    let mut model = AssembledModel::default();
    let by_index = decode_meshes(&prc, &mut model)?;
    place(&prc, by_index, rule, &mut model);
    model.triangles = model.meshes.iter().map(|m| m.triangles.len()).sum();
    if model.triangles == 0 {
        return Err(if model.compressed > 0 {
            let why: Vec<&str> = model.skipped_why.iter().map(|(w, _)| w.as_str()).collect();
            AssembleError::CompressedOnly {
                count: model.compressed,
                reasons: why.join("; "),
            }
        } else {
            AssembleError::NoTriangles
        });
    }
    Ok(model)
}

/// Per file structure, each tessellation's triangle mesh (if it has one),
/// counting what is not drawn into `model`.
fn decode_meshes(
    prc: &PrcFile,
    model: &mut AssembledModel,
) -> Result<Vec<Vec<Option<TriangleMesh>>>, PrcError> {
    let mut by_index = Vec::new();
    for fs in &prc.file_structures {
        let tess = fs.tessellations()?;
        let mut row = Vec::with_capacity(tess.len());
        for t in tess {
            row.push(match t {
                Tessellation::Mesh(m) => Some(m),
                Tessellation::Wire(_) => {
                    model.wires += 1;
                    None
                }
                Tessellation::Compressed { mesh: Some(m), .. } => {
                    model.rebuilt += 1;
                    Some(m)
                }
                Tessellation::Compressed { not_rebuilt, .. } => {
                    model.compressed += 1;
                    let why = not_rebuilt.unwrap_or_default();
                    match model.skipped_why.iter_mut().find(|(w, _)| *w == why) {
                        Some((_, n)) => *n += 1,
                        None => model.skipped_why.push((why, 1)),
                    }
                    None
                }
                _ => {
                    model.markups += 1;
                    None
                }
            });
        }
        by_index.push(row);
    }
    Ok(by_index)
}

/// Place each mesh where the assembly tree draws it, split by face colour.
fn place(
    prc: &PrcFile,
    by_index: Vec<Vec<Option<TriangleMesh>>>,
    rule: StyleAlpha,
    model: &mut AssembledModel,
) {
    model.unplaced = match prc.placements_with(rule) {
        Ok(placements) if !placements.is_empty() => {
            for p in &placements {
                let mesh = by_index
                    .get(p.file_structure)
                    .and_then(|row| row.get(p.tessellation))
                    .and_then(Option::as_ref);
                let Some(mesh) = mesh else { continue };
                let placed = mesh.transformed(&p.matrix);
                match p.triangle_colours(mesh) {
                    Some(per) => {
                        let per: Vec<_> = per.into_iter().map(|c| c.map(to_rgba8)).collect();
                        for (part, colour) in split_by_colour(&placed, &per) {
                            model.meshes.push(part);
                            model.colours.push(colour);
                        }
                    }
                    None => {
                        model.meshes.push(placed);
                        model.colours.push(p.colour.map(to_rgba8));
                    }
                }
            }
            None
        }
        Ok(_) => Some("the model's tree places no tessellation".to_owned()),
        Err(err) => Some(err.to_string()),
    };
    if model.unplaced.is_some() {
        model
            .meshes
            .extend(by_index.into_iter().flatten().flatten());
        model.colours.clear();
    }
}

/// A 0-1 colour as 8-bit straight RGBA.
fn to_rgba8(c: [f64; 4]) -> [u8; 4] {
    c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// `mesh` split into one mesh per distinct entry of `per` (one per
/// triangle), in order of first appearance, so each draws in its own colour.
fn split_by_colour(
    mesh: &TriangleMesh,
    per: &[Option<[u8; 4]>],
) -> Vec<(TriangleMesh, Option<[u8; 4]>)> {
    type Group = (Option<[u8; 4]>, Vec<usize>);
    let mut groups: Vec<Group> = Vec::new();
    for (k, colour) in per.iter().enumerate().take(mesh.triangles.len()) {
        match groups.iter_mut().find(|g| g.0 == *colour) {
            Some(g) => g.1.push(k),
            None => groups.push((*colour, vec![k])),
        }
    }
    let pick = |from: &[[u32; 3]], keep: &[usize]| -> Vec<[u32; 3]> {
        keep.iter().filter_map(|&k| from.get(k).copied()).collect()
    };
    groups
        .into_iter()
        .map(|(colour, keep)| {
            let mut part = mesh.clone();
            part.triangles = pick(&mesh.triangles, &keep);
            if !mesh.triangle_normals.is_empty() {
                part.triangle_normals = pick(&mesh.triangle_normals, &keep);
            }
            part.faces = std::iter::once(0..part.triangles.len()).collect();
            (part, colour)
        })
        .collect()
}

/// The direction [`render_default_view`] looks along: from above the
/// front-right corner of a z-up model (`3d-render`'s `iso` view).
pub const DEFAULT_VIEW_DIRECTION: [f64; 3] = [-1.0, 1.0, -1.0];

/// The model axis [`render_default_view`] shows upward.
pub const DEFAULT_VIEW_UP: [f64; 3] = [0.0, 0.0, 1.0];

/// Draw `model` from [`DEFAULT_VIEW_DIRECTION`] with a 30° perspective
/// camera fitted to the meshes, each mesh in its tree colour (grey when it
/// has none) on an opaque white background.
///
/// # Errors
///
/// [`crate::RenderError`] for an empty or over-ceiling image, or a model
/// with no finite vertex.
#[cfg(feature = "render")]
pub fn render_default_view(
    model: &AssembledModel,
    width: u32,
    height: u32,
) -> Result<crate::Image, crate::RenderError> {
    let aspect = f64::from(width) / f64::from(height.max(1));
    let camera = crate::Camera::fit_meshes(
        &model.meshes,
        DEFAULT_VIEW_DIRECTION,
        DEFAULT_VIEW_UP,
        true,
        aspect,
    )?;
    let options = crate::RenderOptions {
        width,
        height,
        ..crate::RenderOptions::default()
    };
    crate::render_coloured(&model.meshes, &model.colours, &camera, &options)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn a_mesh_splits_by_triangle_colour_in_first_seen_order() {
        let mesh = TriangleMesh {
            positions: vec![[0.0; 3]; 4],
            triangles: vec![[0, 1, 2], [0, 2, 3], [1, 2, 3]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            triangle_normals: vec![[0, 0, 0], [1, 1, 1], [2, 2, 2]],
            ..TriangleMesh::default()
        };
        let red = Some([255, 0, 0, 255]);
        let parts = split_by_colour(&mesh, &[red, None, red]);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].1, red);
        assert_eq!(parts[0].0.triangles, [[0, 1, 2], [1, 2, 3]]);
        assert_eq!(parts[0].0.faces.len(), 1);
        assert_eq!(parts[0].0.faces[0], 0..2);
        assert_eq!(parts[1].1, None);
        assert_eq!(parts[1].0.triangles, [[0, 2, 3]]);
        assert_eq!(parts[1].0.positions.len(), 4);
        assert_eq!(parts[0].0.triangle_normals, [[0, 0, 0], [2, 2, 2]]);
        assert_eq!(parts[1].0.triangle_normals, [[1, 1, 1]]);
    }

    fn fixture(name: &str) -> Vec<u8> {
        let path = format!(
            "{}/../../fixtures/synthetic/prc/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read(path).expect("synthetic PRC fixture")
    }

    #[test]
    fn the_square_assembles_and_non_prc_is_refused() {
        let model = assemble(&fixture("square.prc")).expect("square meshes");
        assert!(model.triangles > 0);
        assert_eq!(
            model.triangles,
            model.meshes.iter().map(|m| m.triangles.len()).sum()
        );
        assert_eq!(assemble(b"U3D\0rest"), Err(AssembleError::NotPrc));
        assert!(matches!(
            assemble(b"PRC\x08\x00"),
            Err(AssembleError::Prc(_))
        ));
    }

    #[cfg(feature = "render")]
    #[test]
    fn the_default_view_draws_the_model_on_white() {
        let model = assemble(&fixture("square.prc")).expect("square meshes");
        let image = render_default_view(&model, 64, 48).expect("renders");
        assert_eq!((image.width, image.height), (64, 48));
        let px = |x: usize, y: usize| &image.rgba[(y * 64 + x) * 4..][..4];
        assert_eq!(px(0, 0), [255, 255, 255, 255], "corner is background");
        assert_ne!(px(32, 24), [255, 255, 255, 255], "centre is the model");
    }
}
