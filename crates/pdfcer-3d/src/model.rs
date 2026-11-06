//! A PRC file assembled into placed, coloured triangle meshes: what mesh
//! export, rendering and the default 3D poster all draw.

pub(crate) mod export;
pub(crate) mod light;
pub(crate) mod named_view;
pub(crate) mod node_box;
pub(crate) mod wire;

use crate::prc::mesh::{self as mesh_decode, Flagged, decode_meshes, part_labels};
use crate::{
    EntityOverrides, PictureFiles, PrcError, PrcFile, StyleAlpha, TextureOrigin, TriangleMesh,
    WrapBase,
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
    /// Wire tessellations; each placed copy draws as [`Self::lines`].
    pub wires: usize,
    /// The wires' polylines, placed as the meshes are.
    pub lines: Vec<crate::ModelLine>,
    /// Each placement's bounding box in its own axes, for the bounding-box
    /// render modes; one per mesh, in model axes, when placements were not
    /// applied.
    pub boxes: Vec<crate::NodeBox>,
    /// Markup tessellations, not drawn.
    pub markups: usize,
    /// Compressed tessellations rebuilt into triangles.
    pub rebuilt: usize,
    /// Of [`Self::rebuilt`], those only a best-fit search rebuilt (see
    /// [`Tessellation::Compressed`](crate::prc::tess::Tessellation::Compressed)): the shape drawn consumes every stored
    /// array, but another shape might as well.
    pub best_fit: usize,
    /// Compressed tessellations drawn in part under [`MeshFit::Partial`]
    /// or [`MeshFit::Continue`]: no fit rebuilt them, so what is drawn is
    /// what decoded, cut where the arrays stopped fitting (`Partial`) or
    /// carried past it (`Continue`). Disclose it.
    pub partial: usize,
    /// Of the triangles [`Self::partial`] meshes hold, how many are drawn:
    /// `(drawn, total)`.
    pub partial_triangles: (usize, usize),
    /// Each distinct reason a partial mesh's sound decode stopped, with its
    /// count.
    pub partial_why: Vec<(String, usize)>,
    /// The parts that draw a partial mesh, labelled as
    /// [`Self::left_out_parts`].
    pub partial_parts: Vec<String>,
    /// Compressed tessellations left out.
    pub compressed: usize,
    /// Each distinct reason a compressed mesh was left out, with its count.
    pub skipped_why: Vec<(String, usize)>,
    /// The parts that draw a left-out compressed mesh, by the label
    /// [`ModelNode`](crate::ModelNode) lists them under (its name, or
    /// `occurrence F:I`), in tree order; a label `N` parts share is listed
    /// once as `label xN`. Empty when none was left
    /// out or the assembly tree cannot be read.
    pub left_out_parts: Vec<String>,
    /// The parts that draw a best-fit mesh (see [`Self::best_fit`]), labelled
    /// as [`Self::left_out_parts`].
    pub best_fit_parts: Vec<String>,
    /// Units of damaged data left out so the rest draws, under any
    /// [`MeshFit`] but [`MeshFit::Unique`], which refuses the model
    /// instead: a point, face, tessellation or placement, or a section's
    /// tail read past.
    pub damaged: usize,
    /// Each distinct damage reason, `defect: what is left out`, with its
    /// count of units; the counts sum to [`Self::damaged`]. Kept apart from
    /// [`Self::skipped_why`] and [`Self::partial_why`], which describe
    /// sound data pdfcer does not rebuild.
    pub damaged_why: Vec<(String, usize)>,
    /// The parts that draw a damaged or left-out unit, labelled as
    /// [`Self::left_out_parts`].
    pub damaged_parts: Vec<String>,
    /// Why placements were not applied (each mesh is then drawn once in its
    /// own coordinates), or `None` when they were.
    pub unplaced: Option<String>,
    /// The textures meshes draw with, each decoded once.
    pub textures: Vec<crate::Texture>,
    /// Each mesh's texture, an index into [`Self::textures`], parallel to
    /// [`Self::meshes`] (empty when placements were not applied).
    pub mesh_textures: Vec<Option<usize>>,
    /// Each mesh's material terms beyond its colour, parallel to
    /// [`Self::meshes`] (empty when placements were not applied); `None`
    /// for a mesh whose material has none that change the picture.
    pub finishes: Vec<Option<crate::MaterialFinish>>,
    /// Meshes drawn textured.
    pub textured: usize,
    /// Placements an assembly's entity references recoloured, in whole or
    /// face by face, under [`AssembleOptions::entity_overrides`].
    pub overridden: usize,
    /// Each distinct reason a textured surface drew its base colour (or a
    /// texture was drawn only in part), with the placements it affected.
    pub texture_notes: Vec<(String, usize)>,
    /// Each mesh's placement, an index into the placements
    /// [`Self::tree`]'s ranges index, parallel to [`Self::meshes`] (empty
    /// when placements were not applied). A placement draws no mesh when
    /// its tessellation was left out or is a wire, and several when its
    /// colours split it.
    pub mesh_placements: Vec<usize>,
    /// The model tree, as [`PrcFile::model_tree`] lists it, read with
    /// [`AssembleOptions::entity_overrides`] (an occurrence's override can
    /// hide a placement, which shifts the ranges); empty when placements
    /// were not applied.
    pub tree: Vec<crate::ModelNode>,
    /// The unit the model is drawn in ([`PrcFile::units`]); `None` when the
    /// file gives none or it does not read.
    pub units: Option<crate::ModelUnits>,
    /// The lights it is drawn with, in the space [`Self::meshes`] are
    /// placed in: the occurrences' ([`PrcFile::lights`]), or a default
    /// view's per [`AssembleOptions::view_lights`]. Empty when it has none
    /// or placements were not applied.
    pub lights: Vec<crate::ModelLight>,
    /// Every lit view's lights ([`PrcFile::view_lights`]), same space.
    pub view_lights: Vec<crate::ViewLights>,
    /// Which of [`Self::view_lights`] supplied [`Self::lights`]; `None`
    /// when they are the occurrences'.
    pub lights_view: Option<usize>,
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
    /// Whether occurrences the file stores hidden or suppressed are drawn.
    pub stored_visibility: StoredVisibility,
    /// Whether a default view's lights replace the occurrences'.
    pub view_lights: crate::ViewLightUse,
}

/// Whether [`assemble_with_options`] draws product occurrences the file
/// stores hidden or suppressed [WD 7.3.10]. Either way the model tree lists
/// them with their stored state ([`ModelNode::drawn`](crate::ModelNode::drawn)).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum StoredVisibility {
    /// Leave them out, as the file asks.
    #[default]
    Honour,
    /// Draw them too, each mesh still named by
    /// [`AssembledModel::mesh_placements`], so a shell can start with the
    /// stored state and let the operator show a hidden part. Entities hidden
    /// inside a part (by the part or by an entity reference) stay hidden.
    DrawAll,
}

/// How far [`assemble_with_options`] goes to draw a compressed mesh the
/// arrays do not determine. ISO 14739-1 does not specify the decoder, and
/// an encoder's quantisation can leave the arrays admitting more than one
/// shape, or none pdfcer's traversal reads. Each step up draws everything
/// the one below does; everything past `Unique` is disclosed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum MeshFit {
    /// Draw a mesh only a best-fit search rebuilds, counted in
    /// [`AssembledModel::best_fit`].
    #[default]
    BestFit,
    /// Leave it out: draw only meshes the arrays determine. Damaged data
    /// is refused, not left out ([`AssembledModel::damaged`]).
    Unique,
    /// Also draw, of a mesh no fit rebuilds, the triangles that decoded
    /// before its arrays stopped fitting ([`crate::PartialMesh::sound`]), counted
    /// in [`AssembledModel::partial`].
    Partial,
    /// As `Partial`, and keep decoding past the first inconsistency until
    /// the point array runs out or the geometry leaves the sound part's
    /// neighbourhood ([`crate::PartialMesh::mesh`] entire).
    Continue,
}

impl MeshFit {
    /// Whether a mesh no fit rebuilds is drawn in part.
    pub(crate) fn draws_partial(self) -> bool {
        matches!(self, Self::Partial | Self::Continue)
    }
}

/// Why a best-fit mesh is left out under [`MeshFit::Unique`].
pub(crate) const BEST_FIT_REFUSED: &str =
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
    /// Nothing drawable survives once the damaged units are left out.
    #[error("nothing in the model draws once its damaged data is left out ({reasons})")]
    Damaged {
        /// Units left out, as [`AssembledModel::damaged`].
        count: usize,
        /// Their distinct reasons, `; `-joined.
        reasons: String,
    },
    /// Placing the model's meshes would copy more than a fixed ceiling of
    /// `what`: [`MAX_PLACED_TRIANGLES`] triangles or
    /// [`MAX_PLACED_ATTRIBUTES`] vertex attributes. Checked before each
    /// copy, so nothing past the ceiling is allocated. The decoded meshes,
    /// all held before any is placed, are charged too, one file structure
    /// at a time.
    #[error("placing the model needs more than {limit} {what}")]
    PlacedTooLarge {
        /// `"triangles"` or `"vertex attributes"`.
        what: &'static str,
        /// The ceiling that was hit.
        limit: usize,
    },
}

/// Ceiling on the triangles every placement together draws. A shared part
/// is copied once per placement, and the tree may place one up to 2^20
/// times, so a small file can describe more geometry than memory holds.
/// 2^25 triangles (with their corner indices about 0.8 GB) is several
/// times a large CAD assembly.
pub const MAX_PLACED_TRIANGLES: usize = 1 << 25;

/// Ceiling on the vertex attributes (positions, normals, texture
/// coordinates and wire points) every placement together copies: 2^25,
/// about 0.8 GB of positions.
pub const MAX_PLACED_ATTRIBUTES: usize = 1 << 25;

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
    let mut model = AssembledModel::default();
    let prc = mesh_decode::parse(data, options.mesh_fit, &mut model)?;
    let mut flagged = Flagged::default();
    let by_index = decode_meshes(&prc, options.mesh_fit, &mut model, &mut flagged)?;
    place(&prc, by_index, &mut flagged, options, &mut model)?;
    model.left_out_parts = part_labels(&prc, &flagged.left_out);
    model.best_fit_parts = part_labels(&prc, &flagged.best_fit);
    model.partial_parts = part_labels(&prc, &flagged.partial);
    if model.damaged > 0 {
        model.damaged_parts = mesh_decode::part_labels_where(&prc, |k| flagged.is_damaged(k));
    }
    model.triangles = model.meshes.iter().map(|m| m.triangles.len()).sum();
    model.units = prc.units().ok().flatten();
    if model.triangles == 0 {
        return Err(nothing_drawn(&model));
    }
    Ok(model)
}

/// Why a model that draws no triangle is refused: damage first, as the
/// likeliest cause, then compressed meshes left out.
fn nothing_drawn(model: &AssembledModel) -> AssembleError {
    let join = |tally: &[(String, usize)]| {
        let why: Vec<&str> = tally.iter().map(|(w, _)| w.as_str()).collect();
        why.join("; ")
    };
    if model.damaged > 0 {
        AssembleError::Damaged {
            count: model.damaged,
            reasons: join(&model.damaged_why),
        }
    } else if model.compressed > 0 {
        AssembleError::CompressedOnly {
            count: model.compressed,
            reasons: join(&model.skipped_why),
        }
    } else {
        AssembleError::NoTriangles
    }
}

/// A 0-1 colour as 8-bit straight RGBA.
pub(crate) fn to_rgba8(c: [f64; 4]) -> [u8; 4] {
    c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// A placed triangle's look: its 8-bit colour, its texture's index into
/// [`AssembledModel::textures`] and its material finish.
type Look = (
    Option<[u8; 4]>,
    Option<usize>,
    Option<crate::MaterialFinish>,
);

/// Place each mesh where the assembly tree draws it, split by face look.
pub(crate) fn place(
    prc: &PrcFile,
    by_index: Vec<Vec<Option<TriangleMesh>>>,
    flagged: &mut Flagged,
    options: &AssembleOptions,
    model: &mut AssembledModel,
) -> Result<(), AssembleError> {
    let rules = crate::prc::tree::TextureRules {
        files: options.picture_files,
        wrap: options.wrap_base,
        origin: options.texture_origin,
    };
    model.unplaced = match prc.textured_placements(
        options.style_alpha,
        rules,
        options.entity_overrides,
        options.stored_visibility,
    ) {
        Ok((placements, tree, (lights, views))) if !placements.is_empty() => {
            model.lights_view = options.view_lights.pick(&views);
            model.lights = match model.lights_view.and_then(|i| views.get(i)) {
                Some(v) => v.lights.clone(),
                None => lights,
            };
            model.view_lights = views;
            let textures = place_all(&placements, (&by_index, flagged), model)?;
            model.tree = tree;
            model.textures = textures.drawn;
            model.texture_notes = textures.notes;
            None
        }
        Ok(_) if !prc.stores_tree() => Some("the file stores no assembly tree".to_owned()),
        Ok(_) => Some("the model's tree places no tessellation".to_owned()),
        Err(err) => Some(err.to_string()),
    };
    if model.unplaced.is_some() {
        model
            .meshes
            .extend(by_index.into_iter().flatten().flatten());
        model.colours.clear();
        model.mesh_textures.clear();
        model.finishes.clear();
        model.mesh_placements.clear();
        model.lines.clear();
        model.boxes = model
            .meshes
            .iter()
            .filter_map(|m| crate::NodeBox::of(m, &crate::IDENTITY, None))
            .collect();
        crate::model::wire::unplaced(&flagged.wires, model);
    }
    model.textured = model.mesh_textures.iter().flatten().count();
    Ok(())
}

/// Places each mesh and wire `placements` draw. A placement naming a
/// tessellation its structure does not hold is malformed, or, while
/// `flagged` is lenient, left out and counted; one naming a wire, a markup
/// or a tessellation left out draws no mesh.
fn place_all<'p>(
    placements: impl IntoIterator<Item = &'p crate::Placement>,
    (by_index, flagged): (&[Vec<Option<TriangleMesh>>], &mut Flagged),
    model: &mut AssembledModel,
) -> Result<Textures, AssembleError> {
    let mut textures = Textures::default();
    let mut budget = Budget::default();
    for (k, p) in placements.into_iter().enumerate() {
        budget.take(0, crate::model::wire::points(p, &flagged.wires))?;
        crate::model::wire::place(p, k, &flagged.wires, model);
        let slot = by_index
            .get(p.file_structure)
            .and_then(|row| row.get(p.tessellation));
        let Some(slot) = slot else {
            let at = (p.file_structure, p.tessellation);
            if !flagged.lenient {
                return Err(AssembleError::Prc(PrcError::Malformed(format!(
                    "a placement draws tessellation {} of file structure {}, which does not exist",
                    at.1, at.0
                ))));
            }
            // A tessellation lost after unreadable data is counted already.
            if !flagged.is_lost(at) {
                let why = mesh_decode::MISSING_TESSELLATION.to_owned();
                mesh_decode::damage(model, why, 1);
                flagged.damaged.push(at);
            }
            continue;
        };
        let Some(mesh) = slot else { continue };
        model.overridden += usize::from(p.item_override.is_some() || !p.face_overrides.is_empty());
        let shown = p.shown(mesh);
        place_one(p, &shown, &mut textures, &mut budget, model)?;
        model
            .boxes
            .extend(crate::NodeBox::of(&shown, &p.matrix, Some(k)));
        model.mesh_placements.resize(model.meshes.len(), k);
    }
    Ok(textures)
}

/// What the placements copied (or the decoder built) so far, against
/// [`MAX_PLACED_TRIANGLES`] and [`MAX_PLACED_ATTRIBUTES`].
#[derive(Default)]
pub(crate) struct Budget {
    triangles: usize,
    attributes: usize,
}

impl Budget {
    /// Counts `triangles` and `attributes` about to be copied, refusing
    /// before a ceiling is passed.
    pub(crate) fn take(
        &mut self,
        triangles: usize,
        attributes: usize,
    ) -> Result<(), AssembleError> {
        let add = |have: usize, more: usize, limit: usize, what| {
            have.checked_add(more)
                .filter(|&n| n <= limit)
                .ok_or(AssembleError::PlacedTooLarge { what, limit })
        };
        self.triangles = add(self.triangles, triangles, MAX_PLACED_TRIANGLES, "triangles")?;
        self.attributes = add(
            self.attributes,
            attributes,
            MAX_PLACED_ATTRIBUTES,
            "vertex attributes",
        )?;
        Ok(())
    }
}

/// The vertex attributes `mesh` holds: positions, normals and texture
/// coordinates.
pub(crate) fn attributes(mesh: &TriangleMesh) -> usize {
    mesh.positions.len() + mesh.normals.len() + mesh.uvs.len()
}

/// Draw `mesh` once where `p` places it, one mesh per distinct look.
fn place_one(
    p: &crate::Placement,
    mesh: &TriangleMesh,
    textures: &mut Textures,
    budget: &mut Budget,
    model: &mut AssembledModel,
) -> Result<(), AssembleError> {
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
            let look = (
                paint.map(|p| to_rgba8(p.rgba)),
                texture,
                paint.and_then(|p| p.finish),
            );
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
    let mut distinct: Vec<&Look> = Vec::new();
    for look in &per {
        if !distinct.contains(&look) {
            distinct.push(look);
        }
    }
    // Each look's part copies every vertex attribute; the transform one more.
    let attributes = attributes(mesh);
    budget.take(
        mesh.triangles.len(),
        attributes.saturating_mul(distinct.len().max(1) + 1),
    )?;
    let placed = mesh.transformed(&p.matrix);
    for (part, look) in split_by_look(&placed, &per) {
        model.alpha_unset += usize::from(unset.contains(&look));
        model.meshes.push(part);
        model.colours.push(look.0);
        model.mesh_textures.push(look.1);
        model.finishes.push(look.2);
    }
    Ok(())
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
        skin: &crate::prc::tree::Skin,
        mesh: &TriangleMesh,
        k: usize,
        notes: &mut Vec<&'static str>,
    ) -> Option<usize> {
        use crate::prc::tree::{Skin, why};
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

/// The camera [`render_default_view`] draws `model` with in an image of
/// `aspect` (width / height): looking along [`DEFAULT_VIEW_DIRECTION`] with
/// [`DEFAULT_VIEW_UP`] upward, in perspective, fitted to the meshes its
/// colours draw (a fully transparent mesh is not framed).
///
/// # Errors
///
/// [`crate::RenderError::Camera`] for a model with no finite vertex or a
/// non-positive `aspect`.
#[cfg(feature = "render")]
pub fn default_view_camera(
    model: &AssembledModel,
    aspect: f64,
) -> Result<crate::Camera, crate::RenderError> {
    crate::Camera::fit_coloured(
        &model.meshes,
        &model.colours,
        DEFAULT_VIEW_DIRECTION,
        DEFAULT_VIEW_UP,
        true,
        aspect,
    )
}

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
    let camera = default_view_camera(model, aspect)?;
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
    crate::PreparedModel::new(model).render(model, camera, options)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)] // Tests fail loudly.
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
        let parts = split_by_look(
            &mesh,
            &[
                (red, Some(0), None),
                (None, None, None),
                (red, Some(0), None),
            ],
        );
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].1, (red, Some(0), None));
        assert_eq!(
            parts[0].0.triangle_uvs,
            [vec![Some([0, 1, 2]), Some([3, 4, 5])]]
        );
        assert_eq!(parts[1].0.triangle_uvs, [vec![None]]);
        let textured = split_by_look(
            &mesh,
            &[
                (red, Some(0), None),
                (red, None, None),
                (red, Some(0), None),
            ],
        );
        assert_eq!(textured.len(), 2, "a texture alone splits a mesh");
        assert_eq!(parts[0].0.triangles, [[0, 1, 2], [1, 2, 3]]);
        assert_eq!(parts[0].0.faces.len(), 1);
        assert_eq!(parts[0].0.faces[0], 0..2);
        assert_eq!(parts[1].1, (None, None, None));
        assert_eq!(parts[1].0.triangles, [[0, 2, 3]]);
        assert_eq!(parts[1].0.positions.len(), 4);
        assert_eq!(parts[0].0.triangle_normals, [[0, 0, 0], [2, 2, 2]]);
        assert_eq!(parts[1].0.triangle_normals, [[1, 1, 1]]);
    }

    /// A placement with no style, override or transform.
    fn bare_placement() -> crate::Placement {
        crate::Placement {
            file_structure: 0,
            tessellation: 0,
            matrix: crate::IDENTITY,
            colour: None,
            chain: Vec::new(),
            palettes: std::sync::Arc::from(Vec::new()),
            skins: std::sync::Arc::from(Vec::new()),
            item_override: None,
            face_overrides: Vec::new(),
            hidden_faces: Vec::new(),
        }
    }

    /// One triangle over 100k positions placed 2^20 times stops at the
    /// attribute ceiling, after a few hundred copies, rather than asking
    /// for terabytes.
    #[test]
    fn placing_past_the_attribute_ceiling_is_refused_before_the_copy() {
        let mesh = TriangleMesh {
            positions: vec![[0.0; 3]; 100_000],
            triangles: vec![[0, 1, 2]],
            ..TriangleMesh::default()
        };
        let p = bare_placement();
        let by_index = vec![vec![Some(mesh)]];
        let mut model = AssembledModel::default();
        let mut placed = 0usize;
        let placements = std::iter::repeat_n(&p, 1 << 20).inspect(|_| {
            placed += 1;
            // Guards the machine when the ceiling is missing.
            assert!(placed <= 400, "no ceiling after {placed} placements");
        });
        let err = place_all(placements, (&by_index, &mut Flagged::default()), &mut model).err();
        assert_eq!(
            err,
            Some(AssembleError::PlacedTooLarge {
                what: "vertex attributes",
                limit: MAX_PLACED_ATTRIBUTES,
            })
        );
        let copied: usize = model.meshes.iter().map(|m| m.positions.len()).sum();
        assert!(copied <= MAX_PLACED_ATTRIBUTES);
    }

    #[test]
    fn the_triangle_ceiling_refuses_the_triangle_past_it() {
        let mut budget = Budget::default();
        assert!(budget.take(MAX_PLACED_TRIANGLES, 0).is_ok());
        assert_eq!(
            budget.take(1, 0),
            Err(AssembleError::PlacedTooLarge {
                what: "triangles",
                limit: MAX_PLACED_TRIANGLES,
            })
        );
        assert!(budget.take(0, usize::MAX).is_err(), "no overflow");
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

    /// A tessellation-only file lists an empty tree and draws unplaced,
    /// saying why; a stored tree is read.
    #[test]
    fn a_file_with_no_assembly_tree_lists_an_empty_one() {
        let square = PrcFile::parse(&fixture("square.prc")).unwrap();
        assert!(!square.stores_tree());
        assert_eq!(square.model_tree().unwrap(), []);
        let model = assemble(&fixture("square.prc")).unwrap();
        assert_eq!(
            model.unplaced.as_deref(),
            Some("the file stores no assembly tree")
        );
        assert!(model.tree.is_empty() && model.mesh_placements.is_empty());
        let assembly = PrcFile::parse(&fixture("assembly.prc")).unwrap();
        assert!(assembly.stores_tree());
        assert_eq!(assembly.model_tree().unwrap().len(), 3);
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

    /// A placement whose faces differ in look draws one mesh per look, each
    /// naming that placement: the overridden fixture's square given a
    /// second face, which the second occurrence's face-0 override leaves
    /// alone.
    #[test]
    fn every_mesh_a_split_placement_draws_names_it() {
        let prc = PrcFile::parse(&fixture("overridden.prc")).unwrap();
        let mut model = AssembledModel::default();
        let mut by_index = decode_meshes(
            &prc,
            MeshFit::default(),
            &mut model,
            &mut Flagged::default(),
        )
        .unwrap();
        for mesh in by_index.iter_mut().flatten().flatten() {
            let n = mesh.triangles.len();
            mesh.triangles.extend_from_within(..);
            mesh.faces = vec![0..n, n..2 * n];
        }
        place(
            &prc,
            by_index,
            &mut Flagged::default(),
            &AssembleOptions::default(),
            &mut model,
        )
        .unwrap();
        assert_eq!(model.mesh_placements, [0, 1, 1]);
        assert_eq!(model.meshes.len(), 3);
        assert_ne!(model.colours[1], model.colours[2]);
    }
}
