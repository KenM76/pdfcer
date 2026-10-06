//! A PRC file assembled into placed, coloured triangle meshes: what mesh
//! export, rendering and the default 3D poster all draw.

use crate::{
    EntityOverrides, PictureFiles, PrcError, PrcFile, StyleAlpha, Tessellation, TextureOrigin,
    TriangleMesh, WrapBase,
};

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
    /// Meshes drawn opaque because [`StyleAlpha::ZeroUnset`] read their
    /// material's diffuse alpha of 0.0 as unset; ISO 14739-1 read
    /// literally draws them invisible.
    pub alpha_unset: usize,
    /// Wire tessellations, not drawn.
    pub wires: usize,
    /// Markup tessellations, not drawn.
    pub markups: usize,
    /// Compressed tessellations rebuilt into triangles.
    pub rebuilt: usize,
    /// Of [`Self::rebuilt`], those only a best-fit search rebuilt (see
    /// [`Tessellation::Compressed`]): the shape drawn consumes every stored
    /// array, but another shape might as well.
    pub best_fit: usize,
    /// Compressed tessellations left out.
    pub compressed: usize,
    /// Each distinct reason a compressed mesh was left out, with its count.
    pub skipped_why: Vec<(String, usize)>,
    /// Why placements were not applied (each mesh is then drawn once in its
    /// own coordinates), or `None` when they were.
    pub unplaced: Option<String>,
    /// The textures meshes draw with, each decoded once.
    pub textures: Vec<crate::Texture>,
    /// Each mesh's texture, an index into [`Self::textures`], parallel to
    /// [`Self::meshes`] (empty when placements were not applied).
    pub mesh_textures: Vec<Option<usize>>,
    /// Meshes drawn textured.
    pub textured: usize,
    /// Placements an assembly's entity references recoloured, in whole or
    /// face by face, under [`AssembleOptions::entity_overrides`].
    pub overridden: usize,
    /// Each distinct reason a textured surface drew its base colour (or a
    /// texture was drawn only in part), with the placements it affected.
    pub texture_notes: Vec<(String, usize)>,
}

/// How [`assemble_with_options`] reads choices ISO 14739-1 leaves open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct AssembleOptions {
    /// How style transparency combines with material alpha.
    pub style_alpha: StyleAlpha,
    /// How stored texture wrapping modes are numbered.
    pub wrap_base: WrapBase,
    /// Where a texture picture's file is looked up first.
    pub picture_files: PictureFiles,
    /// Which picture row texture coordinate v = 0 names.
    pub texture_origin: TextureOrigin,
    /// Which placements an occurrence's colour, visibility and
    /// coordinate-system overrides reach.
    pub entity_overrides: EntityOverrides,
    /// Whether a compressed mesh only a best-fit search rebuilds is drawn.
    pub mesh_fit: MeshFit,
}

/// Whether a compressed mesh only a best-fit search rebuilds is drawn.
/// ISO 14739-1 does not specify the decoder, and an encoder's quantisation
/// can leave the arrays admitting more than one shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum MeshFit {
    /// Draw it, counted in [`AssembledModel::best_fit`].
    #[default]
    BestFit,
    /// Leave it out: draw only meshes the arrays determine.
    Unique,
}

/// Why a best-fit mesh is left out under [`MeshFit::Unique`].
const BEST_FIT_REFUSED: &str =
    "only a best-fit search rebuilds it, and the strict mesh-fit setting draws unique fits only";

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
    let options = AssembleOptions {
        style_alpha: rule,
        ..AssembleOptions::default()
    };
    assemble_with_options(data, &options)
}

/// [`assemble`], reading each choice the standard leaves open as `options`
/// says.
///
/// # Errors
///
/// As [`assemble`].
///
/// ```
/// use pdfcer_3d::{AssembleError, AssembleOptions, assemble_with_options};
/// let options = AssembleOptions::default();
/// assert_eq!(assemble_with_options(b"U3D\0", &options), Err(AssembleError::NotPrc));
/// ```
pub fn assemble_with_options(
    data: &[u8],
    options: &AssembleOptions,
) -> Result<AssembledModel, AssembleError> {
    if !data.starts_with(b"PRC") {
        return Err(AssembleError::NotPrc);
    }
    let prc = PrcFile::parse(data)?;
    let mut model = AssembledModel::default();
    let by_index = decode_meshes(&prc, options.mesh_fit, &mut model)?;
    place(&prc, by_index, options, &mut model);
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
    fit: MeshFit,
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
                Tessellation::Compressed {
                    mesh: Some(m),
                    best_fit,
                    ..
                } if !best_fit || fit == MeshFit::BestFit => {
                    model.rebuilt += 1;
                    model.best_fit += usize::from(best_fit);
                    Some(m)
                }
                Tessellation::Compressed {
                    mesh, not_rebuilt, ..
                } => {
                    model.compressed += 1;
                    let why = match mesh {
                        Some(_) => BEST_FIT_REFUSED.to_owned(),
                        None => not_rebuilt.unwrap_or_default(),
                    };
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

/// A 0-1 colour as 8-bit straight RGBA.
fn to_rgba8(c: [f64; 4]) -> [u8; 4] {
    c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// A placed triangle's look: its 8-bit colour and its texture's index into
/// [`AssembledModel::textures`].
type Look = (Option<[u8; 4]>, Option<usize>);

/// Place each mesh where the assembly tree draws it, split by face look.
fn place(
    prc: &PrcFile,
    by_index: Vec<Vec<Option<TriangleMesh>>>,
    options: &AssembleOptions,
    model: &mut AssembledModel,
) {
    let rules = crate::tree::TextureRules {
        files: options.picture_files,
        wrap: options.wrap_base,
        origin: options.texture_origin,
    };
    model.unplaced =
        match prc.textured_placements(options.style_alpha, rules, options.entity_overrides) {
            Ok(placements) if !placements.is_empty() => {
                let mut textures = Textures::default();
                for p in &placements {
                    let mesh = by_index
                        .get(p.file_structure)
                        .and_then(|row| row.get(p.tessellation))
                        .and_then(Option::as_ref);
                    if let Some(mesh) = mesh {
                        model.overridden +=
                            usize::from(p.item_override.is_some() || !p.face_overrides.is_empty());
                        place_one(p, mesh, &mut textures, model);
                    }
                }
                model.textures = textures.drawn;
                model.texture_notes = textures.notes;
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
        model.mesh_textures.clear();
    }
    model.textured = model.mesh_textures.iter().flatten().count();
}

/// Draw `mesh` once where `p` places it, one mesh per distinct look.
fn place_one(
    p: &crate::Placement,
    mesh: &TriangleMesh,
    textures: &mut Textures,
    model: &mut AssembledModel,
) {
    let placed = mesh.transformed(&p.matrix);
    let looks = p.triangle_looks(mesh);
    let mut notes = Vec::new();
    let mut unset: Vec<Look> = Vec::new();
    let per: Vec<Look> = looks
        .iter()
        .enumerate()
        .map(|(k, (paint, skin))| {
            let texture = skin
                .as_ref()
                .and_then(|s| textures.index(s, mesh, k, &mut notes));
            let look = (paint.map(|p| to_rgba8(p.rgba)), texture);
            if paint.is_some_and(|p| p.alpha_unset) && !unset.contains(&look) {
                unset.push(look);
            }
            look
        })
        .collect();
    notes.sort_unstable();
    notes.dedup();
    for why in notes {
        textures.note(why);
    }
    for (part, (colour, texture)) in split_by_look(&placed, &per) {
        model.alpha_unset += usize::from(unset.contains(&(colour, texture)));
        model.meshes.push(part);
        model.colours.push(colour);
        model.mesh_textures.push(texture);
    }
}

/// The textures drawn so far, shared by every placement that uses them,
/// and the count of each reason a texture was not drawn.
#[derive(Default)]
struct Textures {
    drawn: Vec<crate::Texture>,
    keys: Vec<*const crate::Texture>,
    notes: Vec<(String, usize)>,
}

impl Textures {
    /// Triangle `k` of `mesh`'s texture index, or `None` (with the reason
    /// pushed to `notes`) when it draws its base colour.
    fn index(
        &mut self,
        skin: &crate::tree::Skin,
        mesh: &TriangleMesh,
        k: usize,
        notes: &mut Vec<&'static str>,
    ) -> Option<usize> {
        use crate::tree::{Skin, why};
        let (texture, more) = match skin {
            Skin::Drawn { texture, more } => (texture, *more),
            Skin::Undrawn(why) => {
                notes.push(why);
                return None;
            }
        };
        let has_uvs = mesh
            .triangle_uvs
            .get(texture.uv_set)
            .and_then(|set| set.get(k))
            .is_some_and(Option::is_some);
        if !has_uvs {
            notes.push(why::NO_UVS);
            return None;
        }
        if more {
            notes.push(why::MORE_LEVELS);
        }
        let key = std::sync::Arc::as_ptr(texture);
        Some(match self.keys.iter().position(|&p| p == key) {
            Some(i) => i,
            None => {
                self.keys.push(key);
                self.drawn.push((**texture).clone());
                self.drawn.len() - 1
            }
        })
    }

    fn note(&mut self, why: &str) {
        match self.notes.iter_mut().find(|(w, _)| w == why) {
            Some((_, n)) => *n += 1,
            None => self.notes.push((why.to_owned(), 1)),
        }
    }
}

/// `mesh` split into one mesh per distinct entry of `per` (one per
/// triangle), in order of first appearance, so each draws its own look.
fn split_by_look(mesh: &TriangleMesh, per: &[Look]) -> Vec<(TriangleMesh, Look)> {
    let mut groups: Vec<(Look, Vec<usize>)> = Vec::new();
    for (k, look) in per.iter().enumerate().take(mesh.triangles.len()) {
        match groups.iter_mut().find(|g| g.0 == *look) {
            Some(g) => g.1.push(k),
            None => groups.push((*look, vec![k])),
        }
    }
    if let [(look, _)] = groups.as_slice() {
        return vec![(mesh.clone(), *look)];
    }
    fn pick<T: Copy>(from: &[T], keep: &[usize]) -> Vec<T> {
        keep.iter().filter_map(|&k| from.get(k).copied()).collect()
    }
    groups
        .into_iter()
        .map(|(look, keep)| {
            let mut part = mesh.clone();
            part.triangles = pick(&mesh.triangles, &keep);
            if !mesh.triangle_normals.is_empty() {
                part.triangle_normals = pick(&mesh.triangle_normals, &keep);
            }
            part.triangle_uvs = mesh.triangle_uvs.iter().map(|s| pick(s, &keep)).collect();
            part.triangle_graphics = Vec::new();
            part.triangle_faces = Vec::new();
            part.faces = std::iter::once(0..part.triangles.len()).collect();
            (part, look)
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
    render_model(model, &camera, &options)
}

/// Draw `model` as [`crate::render_coloured`] draws its meshes in their
/// tree colours, each textured mesh sampling its texture.
///
/// # Errors
///
/// As [`crate::render`].
#[cfg(feature = "render")]
pub fn render_model(
    model: &AssembledModel,
    camera: &crate::Camera,
    options: &crate::RenderOptions,
) -> Result<crate::Image, crate::RenderError> {
    let [r, g, b] = options.colour;
    crate::render::render_painted(
        &model.meshes,
        &|i| crate::raster::Paint {
            colour: model
                .colours
                .get(i)
                .copied()
                .flatten()
                .unwrap_or([r, g, b, 255]),
            texture: model
                .mesh_textures
                .get(i)
                .copied()
                .flatten()
                .and_then(|t| model.textures.get(t)),
        },
        camera,
        options,
    )
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn a_mesh_splits_by_triangle_look_in_first_seen_order() {
        let mesh = TriangleMesh {
            positions: vec![[0.0; 3]; 4],
            triangles: vec![[0, 1, 2], [0, 2, 3], [1, 2, 3]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            triangle_normals: vec![[0, 0, 0], [1, 1, 1], [2, 2, 2]],
            triangle_uvs: vec![vec![Some([0, 1, 2]), None, Some([3, 4, 5])]],
            ..TriangleMesh::default()
        };
        let red = Some([255, 0, 0, 255]);
        let parts = split_by_look(&mesh, &[(red, Some(0)), (None, None), (red, Some(0))]);
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].1, (red, Some(0)));
        assert_eq!(
            parts[0].0.triangle_uvs,
            [vec![Some([0, 1, 2]), Some([3, 4, 5])]]
        );
        assert_eq!(parts[1].0.triangle_uvs, [vec![None]]);
        let textured = split_by_look(&mesh, &[(red, Some(0)), (red, None), (red, Some(0))]);
        assert_eq!(textured.len(), 2, "a texture alone splits a mesh");
        assert_eq!(parts[0].0.triangles, [[0, 1, 2], [1, 2, 3]]);
        assert_eq!(parts[0].0.faces.len(), 1);
        assert_eq!(parts[0].0.faces[0], 0..2);
        assert_eq!(parts[1].1, (None, None));
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
