//! The tree section (`FileStructureTree`, 304) and the model file (301):
//! the product-occurrence tree that places each tessellation in the model
//! [WD 7.3.3-7.3.11, 7.6; PRCRS prc.json]. Field order and index biasing
//! follow the spec RAG's `threed/prc__8137__model_tree_asm.md`,
//! `prc__8137__transformations.md` and `prc__8137__representation_items.md`.
//!
//! Markups, views, scene lights and clipping planes cannot be read past
//! yet; a tree carrying any of them is [`PrcError::Unsupported`].

use crate::PrcError;
use crate::container::UniqueId;
use crate::tess::{Ctx, malformed};

mod geometry;
mod node;
mod overrides;
mod style;
mod texture;
mod walk;

use crate::vec3::cross;
pub use geometry::{IDENTITY, Matrix, multiply, transform_point};
pub use node::{ModelNode, NameSource};
pub use overrides::EntityOverrides;
pub(crate) use overrides::{Override, Reached, Target};
pub use style::StyleAlpha;
pub(crate) use style::{Globals, Graphics, Paint, resolve_style};
pub use texture::{PictureFiles, WrapBase};
pub(crate) use texture::{Skin, TextureRules, why};
pub(crate) use walk::{At, Walk};

const BASE_WITH_GRAPHICS: u32 = 2;
const MODEL_FILE: u32 = 301;
const SCENE_DISPLAY_PARAMETERS: u32 = 741;
const CAMERA: u32 = 742;
/// `is_absolute` exists from this authoring version [PRCRS].
const ABSOLUTE_SCENE_FROM: u32 = 8137;
const FILE_STRUCTURE: u32 = 302;
const FILE_STRUCTURE_GLOBALS: u32 = 303;
const STYLE: u32 = 701;
const MATERIAL: u32 = 702;
const TEXTURE_APPLICATION: u32 = 711;
const PICTURE: u32 = 703;
const TEXTURE_DEFINITION: u32 = 712;
const TEXTURE_TRANSFORMATION: u32 = 713;
/// 0-based `EPRCTextureMappingType::Operator` and `EPRCTextureFunction::Blend`
/// (ISS #485; `prc__8137__graphics_materials.md` §7).
const TEXTURE_MAPPING_OPERATOR: i32 = 3;
const TEXTURE_FUNCTION_BLEND: i32 = 3;
/// Texture application mode: alpha test [WD 7.5.7].
const TEXTURE_ALPHA_TEST: u8 = 0x02;
const LINE_PATTERN: u32 = 721;
const FILE_STRUCTURE_TREE: u32 = 304;
const PRODUCT_OCCURRENCE: u32 = 310;
const PART_DEFINITION: u32 = 311;
const FILTER: u32 = 320;
const CARTESIAN_TRANSFORMATION: u32 = 202;
const ENTITY_REFERENCE: u32 = 203;
const REFERENCE_ON_PRC_BASE: u32 = 205;
const REFERENCE_ON_TOPOLOGY: u32 = 206;
const GENERAL_TRANSFORMATION: u32 = 207;
const RI_BREP_MODEL: u32 = 232;
const RI_CURVE: u32 = 233;
const RI_DIRECTION: u32 = 234;
const RI_PLANE: u32 = 235;
const RI_POINT_SET: u32 = 236;
const RI_POLY_BREP_MODEL: u32 = 237;
const RI_POLY_WIRE: u32 = 238;
const RI_SET: u32 = 239;
const RI_COORDINATE_SYSTEM: u32 = 240;

/// `RI_Set` nesting and occurrence-walk depth ceilings (`ARCHITECTURE.md`
/// §10).
const MAX_DEPTH: usize = 64;
/// Ceiling on occurrences visited in one walk: prototypes share subtrees,
/// so a small file can describe an exponential walk.
const MAX_VISITS: usize = 1 << 20;

/// `GraphicsContent` behaviour bits [WD 7.2.4.2].
const SHOW: u16 = 0x0001;
const REMOVED: u16 = 0x2000;
const SON_HERIT_COLOR: u16 = 0x0008;
const FATHER_HERIT_COLOR: u16 = 0x0010;
/// `product_behavior` SUPPRESSED [WD 7.3.10].
const SUPPRESSED: u8 = 0x01;

/// A tessellation drawn at a place in the model: world = `matrix` ×
/// the tessellation's own coordinates.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Placement {
    /// Index into [`crate::PrcFile::file_structures`].
    pub file_structure: usize,
    /// Index into that file structure's
    /// [`tessellations`](crate::FileStructure::tessellations).
    pub tessellation: usize,
    /// Occurrence path × representation-item placement.
    pub matrix: Matrix,
    /// Straight RGBA, each 0–1, alpha the opacity, from the style the
    /// tree resolves for this item; `None` when no style reaches it or the
    /// style names no colour this reader resolves (a textured material).
    /// Faces that carry their own style are coloured by
    /// [`Self::triangle_colours`].
    pub colour: Option<[f64; 4]>,
    /// The graphics from the root to this item, outermost first.
    pub(crate) chain: Vec<Graphics>,
    /// The style colours of every file structure's globals.
    pub(crate) palettes: Palettes,
    /// The style textures of every file structure's globals.
    pub(crate) skins: Skins,
    /// An occurrence's entity reference replacing the item's style.
    pub(crate) item_override: Option<Graphics>,
    /// Per face index, an entity reference replacing that face's style.
    pub(crate) face_overrides: Vec<(u32, Graphics)>,
}

/// Per file structure, entry `b` is the colour of biased style index `b`.
pub(crate) type Palettes = std::sync::Arc<[std::sync::Arc<[Option<Paint>]>]>;

/// The colour `chain` resolves to, each style read in its own file
/// structure's globals.
pub(crate) fn chain_colour(palettes: &Palettes, chain: &[Graphics]) -> Option<[f64; 4]> {
    paint_of(palettes, resolve_style(chain)).map(|p| p.rgba)
}

/// The paint of the style `won` names, in its file structure's globals.
fn paint_of(palettes: &Palettes, won: Graphics) -> Option<Paint> {
    palettes
        .get(won.fs)?
        .get(won.style as usize)
        .copied()
        .flatten()
}

/// Per file structure, entry `b` is the texture of biased style index `b`
/// (`None` when untextured); empty when textures were not resolved.
pub(crate) type Skins = std::sync::Arc<[std::sync::Arc<[Option<Skin>]>]>;

impl Placement {
    /// Per triangle of `mesh` (this placement's tessellation), its colour
    /// as [`Self::colour`] describes, with each face's own style taking
    /// part in the inheritance [WD 7.2.4, 7.8.6]; `None` when no face of
    /// `mesh` carries a style, so every triangle is [`Self::colour`].
    #[must_use]
    pub fn triangle_colours(&self, mesh: &crate::TriangleMesh) -> Option<Vec<Option<[f64; 4]>>> {
        if mesh.triangle_graphics.is_empty() && self.face_overrides.is_empty() {
            return None;
        }
        let looks = self.triangle_looks(mesh).into_iter();
        Some(looks.map(|l| l.0.map(|p| p.rgba)).collect())
    }

    /// Per triangle of `mesh`, its paint and its style's texture.
    pub(crate) fn triangle_looks(
        &self,
        mesh: &crate::TriangleMesh,
    ) -> Vec<(Option<Paint>, Option<Skin>)> {
        let look = |won: Graphics| {
            let skin = self
                .skins
                .get(won.fs)
                .and_then(|row| row.get(won.style as usize))
                .cloned()
                .flatten();
            (paint_of(&self.palettes, won), skin)
        };
        let faces = if self.face_overrides.is_empty() {
            Vec::new()
        } else {
            overrides::triangle_faces(mesh)
        };
        let by_face = |k: usize| {
            let f = faces.get(k)?;
            let i = self.face_overrides.binary_search_by_key(f, |(i, _)| *i);
            self.face_overrides.get(i.ok()?).map(|(_, g)| *g)
        };
        if mesh.triangle_graphics.is_empty() {
            let whole = self
                .item_override
                .unwrap_or_else(|| resolve_style(&self.chain));
            return (0..mesh.triangles.len())
                .map(|k| look(by_face(k).unwrap_or(whole)))
                .collect();
        }
        let mut chain = self.chain.clone();
        chain.push(Graphics::default());
        mesh.triangle_graphics
            .iter()
            .enumerate()
            .map(|(k, g)| {
                if let Some(won) = by_face(k).or(self.item_override) {
                    return look(won);
                }
                if let Some(last) = chain.last_mut() {
                    *last = Graphics {
                        fs: self.file_structure,
                        ..*g
                    };
                }
                look(resolve_style(&chain))
            })
            .collect()
    }
}

/// A representation item that can be drawn, flattened out of any `RI_Set`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Item {
    /// `index_local_coordinate_system + 1` of the item and each enclosing
    /// set, outermost first; 0 entries dropped.
    pub(crate) local: Vec<u32>,
    /// `index_tessellation + 1`; never 0 here.
    pub(crate) tessellation: u32,
    /// The part's graphics, each enclosing set's, then the item's own.
    pub(crate) graphics: Vec<Graphics>,
    /// The PRC unique id of each enclosing set, then the item's own.
    pub(crate) uids: Vec<u32>,
    /// A B-rep model's stored `(context, body)` ids [WD 7.6.2].
    pub(crate) brep: Option<(u32, u32)>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Product {
    pub(crate) part: u32,
    pub(crate) prototype: u32,
    pub(crate) prototype_fs: Option<UniqueId>,
    pub(crate) external: u32,
    pub(crate) external_fs: Option<UniqueId>,
    pub(crate) sons: Vec<u32>,
    /// Its own graphics hide it (Show clear or Removed set).
    pub(crate) hidden: bool,
    /// `product_behavior` SUPPRESSED.
    pub(crate) suppressed: bool,
    pub(crate) graphics: Graphics,
    pub(crate) location: Option<Matrix>,
    pub(crate) name: Option<String>,
    /// Its entity references that name a target.
    pub(crate) overrides: Vec<Override>,
}

/// One file structure's tree section.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Tree {
    pub(crate) parts: Vec<Vec<Item>>,
    /// Each part definition's name, parallel to `parts`.
    pub(crate) part_names: Vec<Option<String>>,
    pub(crate) products: Vec<Product>,
    /// `index_product_occurrence + 1` of this structure's root.
    pub(crate) root: u32,
}

/// The model file's roots: file structure id and biased occurrence index.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ModelFile {
    pub(crate) roots: Vec<(UniqueId, u32)>,
    pub(crate) units_in_mm: f64,
}

impl Ctx<'_, '_> {
    fn vector3(&mut self) -> Result<[f64; 3], PrcError> {
        Ok([self.r.double()?, self.r.double()?, self.r.double()?])
    }

    fn unique_id(&mut self) -> Result<UniqueId, PrcError> {
        Ok(UniqueId([
            self.r.unsigned_integer()?,
            self.r.unsigned_integer()?,
            self.r.unsigned_integer()?,
            self.r.unsigned_integer()?,
        ]))
    }

    /// `ContentPRCRefBase` [WD 7.2.3.2; ISS #697]: base, then the CAD and
    /// PRC ids; the PRC unique id becomes [`Ctx::uid`].
    fn content_prc_ref_base(&mut self) -> Result<(), PrcError> {
        self.content_prc_base()?;
        self.r.unsigned_integer()?; // CAD identifier
        self.r.unsigned_integer()?; // CAD persistent identifier
        self.uid = self.r.unsigned_integer()?;
        Ok(())
    }

    /// `PRCBaseWithGraphics` [WD 7.2.4; ISS #405]; returns whether the
    /// entity's own graphics hide it (Show clear or Removed set), and its
    /// graphics. Reused graphics (`same_graphics`) are the current graphics
    /// [WD 5.4] and never hide.
    fn base_with_graphics(&mut self) -> Result<(bool, Graphics), PrcError> {
        self.content_prc_ref_base()?;
        let hidden = if self.r.bit()? {
            false
        } else {
            self.r.unsigned_integer()?; // layer + 1
            let style = self.r.unsigned_integer()?;
            let lo = u16::from(self.r.character()?);
            let hi = u16::from(self.r.character()?);
            let bits = lo | hi << 8;
            self.graphics = Graphics { style, bits, fs: 0 };
            bits & SHOW == 0 || bits & REMOVED != 0
        };
        // Schema additions to the base itself, after the graphics [PRCRS].
        self.schema
            .skip_added_fields(BASE_WITH_GRAPHICS, &mut self.r)?;
        Ok((hidden, self.graphics))
    }

    /// `CartesianTransformation` (202) or `GeneralTransformation` (207),
    /// type code included [WD 7.4.8, 7.4.9, 7.4.11; ISS #430].
    pub(crate) fn transformation(&mut self) -> Result<Matrix, PrcError> {
        let t = self.r.unsigned_integer()?;
        let m = match t {
            GENERAL_TRANSFORMATION => {
                // Column-major [WD 7.4.9].
                let mut m = [[0.0; 4]; 4];
                for col in 0..4 {
                    for row in m.iter_mut() {
                        if let Some(c) = row.get_mut(col) {
                            *c = self.r.double()?;
                        }
                    }
                }
                m
            }
            CARTESIAN_TRANSFORMATION => self.transformation_3d()?,
            t => return Err(malformed(format!("entity type {t} as a transformation"))),
        };
        self.schema.skip_added_fields(t, &mut self.r)?;
        Ok(m)
    }

    /// The embedded `Transformation3D`: a behaviour byte, then each field
    /// its bits name [WD 7.4.11.1]. Scale and non-uniform scale are both
    /// read when both bits are set [PRCRS].
    fn transformation_3d(&mut self) -> Result<Matrix, PrcError> {
        let b = self.r.character()?;
        let mut m = IDENTITY;
        let set_col = |m: &mut Matrix, col: usize, v: [f64; 3]| {
            for (row, x) in m.iter_mut().zip(v) {
                if let Some(c) = row.get_mut(col) {
                    *c = x;
                }
            }
        };
        if b & 0x01 != 0 {
            let t = self.vector3()?;
            set_col(&mut m, 3, t);
        }
        if b & 0x20 != 0 {
            for col in 0..3 {
                let v = self.vector3()?;
                set_col(&mut m, col, v);
            }
        } else if b & 0x02 != 0 {
            let x = self.vector3()?;
            let y = self.vector3()?;
            let z = if b & 0x04 != 0 {
                cross(y, x)
            } else {
                cross(x, y)
            };
            set_col(&mut m, 0, x);
            set_col(&mut m, 1, y);
            set_col(&mut m, 2, z);
        }
        let mut scale = [1.0; 3];
        if b & 0x10 != 0 {
            scale = self.vector3()?;
        }
        if b & 0x08 != 0 {
            let s = self.r.double()?;
            scale = scale.map(|v| v * s);
        }
        for row in m.iter_mut().take(3) {
            for (c, s) in row.iter_mut().zip(scale) {
                *c *= s;
            }
        }
        if b & 0x40 != 0 {
            let h = [
                self.r.double()?,
                self.r.double()?,
                self.r.double()?,
                self.r.double()?,
            ];
            if let Some(r) = m.get_mut(3) {
                *r = h;
            }
        }
        Ok(m)
    }

    /// A polymorphic representation item [WD 7.6; PRCRS]; drawable ones are
    /// pushed onto `out` with `local` (the enclosing sets' coordinate
    /// systems) prefixed.
    fn representation_item(
        &mut self,
        local: &[u32],
        graphics: &[Graphics],
        uids: &[u32],
        out: &mut Vec<Item>,
        depth: usize,
    ) -> Result<(), PrcError> {
        if depth > MAX_DEPTH {
            return Err(malformed("representation sets nest too deep".into()));
        }
        let t = self.r.unsigned_integer()?;
        let (hidden, own) = self.base_with_graphics()?;
        let mut ids = uids.to_vec();
        ids.push(self.uid);
        let cs = self.r.unsigned_integer()?;
        let tess = self.r.unsigned_integer()?;
        let mut path = local.to_vec();
        if cs != 0 {
            path.push(cs);
        }
        let mut chain = graphics.to_vec();
        chain.push(own);
        let (drawable, brep) = if t == RI_SET {
            let n = self.count(1, "representation set")?;
            let mut members = Vec::new();
            for _ in 0..n {
                self.representation_item(&path, &chain, &ids, &mut members, depth + 1)?;
            }
            if !hidden {
                out.extend(members);
            }
            (false, None)
        } else {
            self.leaf_item_fields(t)?
        };
        self.schema.skip_added_fields(t, &mut self.r)?;
        self.user_data()?;
        if drawable && !hidden && tess != 0 {
            out.push(Item {
                local: path,
                tessellation: tess,
                graphics: chain,
                uids: ids,
                brep,
            });
        }
        Ok(())
    }

    /// The type-specific fields of a representation item other than
    /// `RI_Set` [WD 7.6]; returns whether the item can be drawn and a
    /// B-rep model's `(context, body)`.
    fn leaf_item_fields(&mut self, t: u32) -> Result<(bool, Option<(u32, u32)>), PrcError> {
        match t {
            RI_BREP_MODEL => {
                let brep = self.exact_tolerance()?;
                self.r.bit()?; // is_closed
                return Ok((true, brep));
            }
            RI_CURVE | RI_PLANE => {
                self.exact_tolerance()?;
            }
            RI_DIRECTION => {
                if self.r.bit()? {
                    self.vector3()?;
                }
                self.vector3()?;
                return Ok((false, None));
            }
            RI_POINT_SET => {
                let n = self.count(6, "point set")?;
                for _ in 0..n {
                    self.vector3()?;
                }
            }
            RI_POLY_BREP_MODEL => {
                self.r.bit()?;
            }
            RI_POLY_WIRE => {}
            RI_COORDINATE_SYSTEM => {
                self.transformation()?;
                return Ok((false, None));
            }
            t => {
                return Err(malformed(format!(
                    "entity type {t} as a representation item"
                )));
            }
        }
        Ok((true, None))
    }

    /// The optional exact-geometry pair of a B-rep, curve or plane item:
    /// its topological context and body ids [WD 7.6.2].
    fn exact_tolerance(&mut self) -> Result<Option<(u32, u32)>, PrcError> {
        if self.r.bit()? {
            let context = self.r.unsigned_integer()?;
            let body = self.r.unsigned_integer()?;
            return Ok(Some((context, body)));
        }
        Ok(None)
    }

    /// `SceneDisplayParameters` (741) [WD 7.5.19; PRCRS]: read past. Lights
    /// and clipping planes are [`PrcError::Unsupported`].
    fn scene_display_parameters(&mut self) -> Result<(), PrcError> {
        self.expect_type(SCENE_DISPLAY_PARAMETERS)?;
        self.content_prc_ref_base()?;
        self.r.bit()?; // is_active
        if self.r.unsigned_integer()? != 0 {
            return Err(PrcError::Unsupported("scene lights"));
        }
        if self.r.bit()? {
            self.camera()?;
        }
        if self.r.bit()? {
            self.vector3()?; // rotation centre
        }
        if self.r.unsigned_integer()? != 0 {
            return Err(PrcError::Unsupported("scene clipping planes"));
        }
        self.r.unsigned_integer()?; // background style + 1
        self.r.unsigned_integer()?; // default style + 1
        let n = self.count(2, "scene default styles")?;
        for _ in 0..2 * n {
            self.r.unsigned_integer()?;
        }
        if self.version >= ABSOLUTE_SCENE_FROM {
            self.r.bit()?; // is_absolute
        }
        self.schema
            .skip_added_fields(SCENE_DISPLAY_PARAMETERS, &mut self.r)
    }

    /// `Camera` (742) [WD 7.5.20; PRCRS]: read past.
    fn camera(&mut self) -> Result<(), PrcError> {
        self.expect_type(CAMERA)?;
        self.content_prc_ref_base()?;
        self.r.bit()?; // is_orthographic
        for _ in 0..3 {
            self.vector3()?; // position, look, up
        }
        for _ in 0..6 {
            self.r.double()?; // x, y, ratio, near, far, zoom
        }
        self.schema.skip_added_fields(CAMERA, &mut self.r)
    }

    /// `MarkupData` [WD 7.3.10.4]: only the empty form reads past.
    fn markup_data(&mut self) -> Result<(), PrcError> {
        for _ in 0..4 {
            if self.r.unsigned_integer()? != 0 {
                return Err(PrcError::Unsupported("tree markups (PMI)"));
            }
        }
        Ok(())
    }

    fn views(&mut self) -> Result<(), PrcError> {
        if self.r.unsigned_integer()? != 0 {
            return Err(PrcError::Unsupported("tree views"));
        }
        Ok(())
    }

    /// `MISC_EntityReference` (203) [WD 7.4.4, 7.4.10; PRCRS]; `None`
    /// when it names no target.
    fn entity_reference(&mut self) -> Result<Option<Override>, PrcError> {
        self.expect_type(ENTITY_REFERENCE)?;
        let (hidden, graphics) = self.base_with_graphics()?;
        let local = self.r.unsigned_integer()?;
        let mut target = None;
        if self.r.bit()? {
            match self.r.unsigned_integer()? {
                REFERENCE_ON_PRC_BASE => {
                    let kind = self.r.unsigned_integer()?;
                    let fs = self.file_identifier()?;
                    let uid = self.r.unsigned_integer()?;
                    target = Some(Target::Entity { kind, fs, uid });
                }
                REFERENCE_ON_TOPOLOGY => target = self.reference_on_topology()?,
                t => return Err(malformed(format!("entity type {t} as reference data"))),
            }
        }
        self.schema
            .skip_added_fields(ENTITY_REFERENCE, &mut self.r)?;
        self.user_data()?;
        Ok(target.map(|target| Override {
            graphics,
            hidden,
            local,
            target,
        }))
    }

    /// `ReferenceOnTopology` (206) after its type code; `None` without a
    /// body.
    fn reference_on_topology(&mut self) -> Result<Option<Target>, PrcError> {
        let kind = self.r.unsigned_integer()?;
        if !self.r.bit()? {
            return Ok(None);
        }
        let fs = self.file_identifier()?;
        let brep = (self.r.unsigned_integer()?, self.r.unsigned_integer()?);
        let n = self.count(1, "topology reference indices")?;
        let mut indices = Vec::with_capacity(n.min(1024));
        for _ in 0..n {
            indices.push(self.r.unsigned_integer()?);
        }
        Ok(Some(Target::Topology {
            kind,
            fs,
            brep,
            indices,
        }))
    }

    /// `ASM_Filter` (320) [WD 7.3.12; PRCRS]: read past.
    fn filter(&mut self) -> Result<(), PrcError> {
        self.expect_type(FILTER)?;
        self.content_prc_ref_base()?;
        self.r.bit()?; // is_active
        self.r.bit()?; // layers inclusive
        let n = self.count(1, "filter layers")?;
        for _ in 0..n {
            self.r.unsigned_integer()?;
        }
        self.r.bit()?; // entities inclusive
        let n = self.count(1, "filter entities")?;
        for _ in 0..n {
            self.entity_reference()?;
        }
        self.schema.skip_added_fields(FILTER, &mut self.r)?;
        self.user_data()
    }

    /// `PartDefinition` (311): its drawable items and its name.
    fn part_definition(&mut self) -> Result<(Vec<Item>, Option<String>), PrcError> {
        self.expect_type(PART_DEFINITION)?;
        let (hidden, own) = self.base_with_graphics()?;
        let name = self.name.clone();
        self.vector3()?; // bounding box
        self.vector3()?;
        let n = self.count(1, "representation items")?;
        let mut items = Vec::new();
        for _ in 0..n {
            self.representation_item(&[], &[own], &[], &mut items, 0)?;
        }
        self.markup_data()?;
        self.views()?;
        self.schema
            .skip_added_fields(PART_DEFINITION, &mut self.r)?;
        self.user_data()?;
        if hidden {
            items.clear();
        }
        Ok((items, name))
    }

    /// `FileIdentifier` [WD 7.3.10.2.2]: `None` for the same structure.
    fn file_identifier(&mut self) -> Result<Option<UniqueId>, PrcError> {
        if self.r.bit()? {
            Ok(None)
        } else {
            self.unique_id().map(Some)
        }
    }

    fn product_occurrence(&mut self) -> Result<Product, PrcError> {
        self.expect_type(PRODUCT_OCCURRENCE)?;
        let (hidden, graphics) = self.base_with_graphics()?;
        let mut p = Product {
            hidden,
            graphics,
            name: self.name.clone(),
            part: self.r.unsigned_integer()?,
            ..Product::default()
        };
        p.prototype = self.r.unsigned_integer()?;
        if p.prototype != 0 {
            p.prototype_fs = self.file_identifier()?;
        }
        p.external = self.r.unsigned_integer()?;
        if p.external != 0 {
            p.external_fs = self.file_identifier()?;
        }
        let n = self.count(1, "son occurrences")?;
        for _ in 0..n {
            p.sons.push(self.r.unsigned_integer()?);
        }
        p.suppressed = self.r.character()? & SUPPRESSED != 0;
        // ProductInformation [WD 7.3.10.3].
        self.r.bit()?;
        self.r.double()?;
        self.r.character()?;
        self.r.integer()?;
        if self.r.bit()? {
            p.location = Some(self.transformation()?);
        }
        let n = self.count(1, "entity references")?;
        for _ in 0..n {
            p.overrides.extend(self.entity_reference()?);
        }
        self.markup_data()?;
        self.views()?;
        if self.r.bit()? {
            self.filter()?;
        }
        let n = self.count(1, "display filters")?;
        for _ in 0..n {
            self.filter()?;
        }
        let n = self.count(1, "scene display parameters")?;
        for _ in 0..n {
            self.scene_display_parameters()?;
        }
        self.schema
            .skip_added_fields(PRODUCT_OCCURRENCE, &mut self.r)?;
        self.user_data()?;
        Ok(p)
    }

    /// `FileStructureTree` (304) [WD 7.3.6].
    pub(crate) fn file_structure_tree(&mut self) -> Result<Tree, PrcError> {
        self.expect_type(FILE_STRUCTURE_TREE)?;
        self.content_prc_base()?;
        let n = self.count(1, "part definitions")?;
        let mut tree = Tree::default();
        for _ in 0..n {
            let (items, name) = self.part_definition()?;
            tree.parts.push(items);
            tree.part_names.push(name);
        }
        let n = self.count(1, "product occurrences")?;
        for _ in 0..n {
            tree.products.push(self.product_occurrence()?);
        }
        self.expect_type(FILE_STRUCTURE)?;
        self.content_prc_base()?;
        self.r.unsigned_integer()?; // next_available_index
        tree.root = self.r.unsigned_integer()?;
        self.schema.skip_added_fields(FILE_STRUCTURE, &mut self.r)?;
        self.schema
            .skip_added_fields(FILE_STRUCTURE_TREE, &mut self.r)?;
        self.user_data()?;
        Ok(tree)
    }

    /// `ModelFile` (301) after its schema [WD 7.3.3; ISS #724 #740]:
    /// `units_in_mm` is always present.
    pub(crate) fn model_file(&mut self) -> Result<ModelFile, PrcError> {
        self.expect_type(MODEL_FILE)?;
        self.content_prc_base()?;
        self.r.bit()?; // units_from_cad_file
        let units_in_mm = self.r.double()?;
        let n = self.count(6, "root occurrences")?;
        let mut roots = Vec::with_capacity(n);
        for _ in 0..n {
            let id = self.unique_id()?;
            let index = self.r.unsigned_integer()?;
            self.r.bit()?; // is_active
            roots.push((id, index));
        }
        Ok(ModelFile { roots, units_in_mm })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::style::{Material, Style};
    use super::*;
    use crate::Schema;
    use crate::bits::BitReader;
    use crate::testw::W;

    const FS: UniqueId = UniqueId([1, 2, 3, 4]);

    /// `ContentPRCBase`: no attributes, same name.
    fn base(w: &mut W) {
        named(w, None);
    }

    /// `ContentPRCBase` with `name`: `None` for same name, `Some(None)`
    /// for a null one.
    fn named(w: &mut W, name: Option<Option<&str>>) {
        w.uint(0);
        match name {
            None => w.bit(true),
            Some(n) => w.bit(false).string(n),
        };
    }

    /// `PRCBaseWithGraphics`; `g` is `(line style + 1, behaviour)`, `None`
    /// reusing the current graphics. `extra` writes the UInt a schema adds
    /// to type 2.
    fn graphics(w: &mut W, g: Option<(u32, u16)>, extra: bool) {
        graphics_named(w, None, g, extra);
    }

    fn graphics_named(w: &mut W, name: Option<Option<&str>>, g: Option<(u32, u16)>, extra: bool) {
        named(w, name);
        w.uint(0).uint(0).uint(7);
        match g {
            None => {
                w.bit(true);
            }
            Some((style, b)) => {
                w.bit(false)
                    .uint(0)
                    .uint(style)
                    .put(u64::from(b & 0xff), 8)
                    .put(u64::from(b >> 8), 8);
            }
        }
        if extra {
            w.uint(42);
        }
    }

    /// A translation by `t`, mirrored in x first when `mirror`.
    fn translation(w: &mut W, t: [f64; 3], mirror: bool) {
        w.uint(CARTESIAN_TRANSFORMATION)
            .put(if mirror { 0x11 } else { 0x01 }, 8);
        for c in t {
            w.double(c);
        }
        if mirror {
            w.double(-1.0).double(1.0).double(1.0);
        }
    }

    /// Four empty markup lists, then no views.
    fn no_markups_or_views(w: &mut W) {
        w.uint(0).uint(0).uint(0).uint(0).uint(0);
    }

    /// A part with one item on tessellation 1, local CS `cs`: a poly-BRep,
    /// or with `brep` a B-rep model of that `(context, body)`. Items have
    /// PRC unique id 7.
    fn part(w: &mut W, cs: u32, extra: bool, name: Option<Option<&str>>, brep: Option<(u32, u32)>) {
        w.uint(PART_DEFINITION);
        graphics_named(w, name, None, extra);
        for c in [1.0, 0.0, 0.0, -1.0, 0.0, 0.0] {
            w.double(c);
        }
        match brep {
            None => {
                w.uint(1).uint(RI_POLY_BREP_MODEL);
                graphics(w, None, extra);
                w.uint(cs).uint(1).bit(false).uint(0);
            }
            Some((context, body)) => {
                w.uint(1).uint(RI_BREP_MODEL);
                graphics(w, None, extra);
                w.uint(cs).uint(1).bit(true).uint(context).uint(body);
                w.bit(false).uint(0);
            }
        }
        no_markups_or_views(w);
        w.uint(0);
    }

    #[derive(Clone, Copy)]
    struct Occ<'a> {
        part: u32,
        sons: &'a [u32],
        behaviour: Option<(u32, u16)>,
        suppressed: bool,
        location: Option<[f64; 3]>,
        mirror: bool,
        camera: bool,
        /// `(prototype + 1, its file structure)`, `None` meaning this one.
        prototype: Option<(u32, Option<UniqueId>)>,
        /// As [`named`].
        name: Option<Option<&'a str>>,
        refs: &'a [Er<'a>],
    }

    /// A `MISC_EntityReference` with graphics `(style + 1, behaviour)`.
    #[derive(Clone, Copy)]
    enum Er<'a> {
        /// `ReferenceOnPRCBase` to item `uid`; `fs` `None` for this one.
        Item {
            g: (u32, u16),
            fs: Option<UniqueId>,
            uid: u32,
        },
        /// `ReferenceOnTopology` to faces of B-rep `(1, 1)`, same structure.
        Faces { g: (u32, u16), faces: &'a [u32] },
    }

    fn entity_reference(w: &mut W, er: &Er<'_>) {
        w.uint(ENTITY_REFERENCE);
        let g = match er {
            Er::Item { g, .. } | Er::Faces { g, .. } => *g,
        };
        graphics(w, Some(g), false);
        w.uint(0).bit(true);
        match er {
            Er::Item { fs, uid, .. } => {
                w.uint(REFERENCE_ON_PRC_BASE).uint(RI_BREP_MODEL);
                match fs {
                    None => w.bit(true),
                    Some(fs) => fs.0.iter().fold(w.bit(false), |w, &c| w.uint(c)),
                };
                w.uint(*uid);
            }
            Er::Faces { faces, .. } => {
                w.uint(REFERENCE_ON_TOPOLOGY).uint(149).bit(true).bit(true);
                w.uint(1).uint(1).uint(faces.len() as u32);
                for &f in *faces {
                    w.uint(f);
                }
            }
        }
        w.uint(0);
    }

    const OCC: Occ<'static> = Occ {
        part: 0,
        sons: &[],
        behaviour: None,
        suppressed: false,
        location: None,
        mirror: false,
        camera: false,
        prototype: None,
        name: None,
        refs: &[],
    };

    fn occurrence(w: &mut W, o: &Occ<'_>, extra: bool) {
        w.uint(PRODUCT_OCCURRENCE);
        graphics_named(w, o.name, o.behaviour, extra);
        w.uint(o.part);
        match o.prototype {
            None => w.uint(0),
            Some((index, None)) => w.uint(index).bit(true),
            Some((index, Some(fs))) => {
                w.uint(index).bit(false);
                fs.0.iter().fold(&mut *w, |w, &c| w.uint(c))
            }
        };
        w.uint(0).uint(o.sons.len() as u32);
        for &s in o.sons {
            w.uint(s);
        }
        w.put(u64::from(o.suppressed), 8);
        w.bit(false).double(1.0).put(0, 8).int(0);
        w.bit(o.location.is_some());
        if let Some(t) = o.location {
            translation(w, t, o.mirror);
        }
        w.uint(o.refs.len() as u32);
        for er in o.refs {
            entity_reference(w, er);
        }
        no_markups_or_views(w);
        w.bit(false).uint(0);
        if o.camera {
            w.uint(1).uint(SCENE_DISPLAY_PARAMETERS);
            base(w);
            w.uint(0).uint(0).uint(9).bit(false).uint(0).bit(true);
            w.uint(CAMERA);
            base(w);
            w.uint(0).uint(0).uint(10).bit(true);
            for i in 0..15 {
                w.double(f64::from(i) + 0.5);
            }
            w.bit(false).uint(0).uint(19).uint(0).uint(0).bit(false);
        } else {
            w.uint(0);
        }
        w.uint(0);
    }

    fn tree(occs: &[Occ<'_>], cs: u32, extra: bool) -> W {
        tree_with(occs, cs, extra, None, None)
    }

    fn tree_with(
        occs: &[Occ<'_>],
        cs: u32,
        extra: bool,
        part_name: Option<Option<&str>>,
        brep: Option<(u32, u32)>,
    ) -> W {
        let mut w = W::default();
        w.uint(FILE_STRUCTURE_TREE);
        base(&mut w);
        w.uint(1);
        part(&mut w, cs, extra, part_name, brep);
        w.uint(occs.len() as u32);
        for o in occs {
            occurrence(&mut w, o, extra);
        }
        w.uint(FILE_STRUCTURE);
        base(&mut w);
        w.uint(100).uint(1).uint(0);
        w
    }

    /// Globals holding one colour, material, line pattern and style, then
    /// one reference coordinate system translating by `t`.
    fn globals(t: [f64; 3]) -> W {
        let mut w = W::default();
        w.uint(FILE_STRUCTURE_GLOBALS);
        base(&mut w);
        w.uint(0)
            .double(2000.0)
            .double(40.0)
            .string(Some(""))
            .uint(0);
        w.uint(1).double(1.0).double(0.5).double(0.0);
        w.uint(0).uint(0);
        w.uint(1).uint(MATERIAL);
        base(&mut w);
        w.uint(0).uint(0).uint(11);
        for _ in 0..4 {
            w.uint(1);
        }
        for _ in 0..5 {
            w.double(0.25);
        }
        w.uint(1).uint(LINE_PATTERN);
        base(&mut w);
        w.uint(0).uint(0).uint(12);
        w.uint(2).double(1.0e6).double(0.0).double(0.0).bit(false);
        w.uint(1).uint(STYLE);
        base(&mut w);
        w.uint(0).uint(0).uint(13);
        w.double(1.0).bit(false).uint(1).bit(false).uint(1);
        w.bit(true).put(128, 8).bit(false).bit(false).bit(false);
        w.uint(0);
        w.uint(1).uint(RI_COORDINATE_SYSTEM);
        graphics(&mut w, None, false);
        w.uint(0).uint(0);
        translation(&mut w, t, false);
        w.uint(0).uint(0);
        w
    }

    fn ctx<'a, 's>(bytes: &'a [u8], schema: &'s Schema) -> Ctx<'a, 's> {
        Ctx::new(BitReader::new(bytes), schema, 8137)
    }

    fn place(occs: &[Occ<'_>], cs: u32, globals: &Globals) -> Result<Vec<Placement>, PrcError> {
        let bytes = tree(occs, cs, false).bytes();
        let t = ctx(&bytes, &Schema::default()).file_structure_tree()?;
        let mut walk = Walk::new(
            vec![(FS, &t, globals)],
            StyleAlpha::default(),
            EntityOverrides::default(),
        );
        walk.occurrence(0, 0, &IDENTITY, &[], At::ROOT)?;
        Ok(walk.out)
    }

    fn fixture_globals() -> Globals {
        let bytes = globals([0.0, 5.0, 0.0]).bytes();
        ctx(&bytes, &Schema::default()).globals().unwrap()
    }

    fn translate(t: [f64; 3]) -> Matrix {
        let mut m = IDENTITY;
        for (row, c) in m.iter_mut().zip(t) {
            row[3] = c;
        }
        m
    }

    #[test]
    fn sons_compose_locations_and_reference_systems() {
        let systems = fixture_globals();
        assert_eq!(systems.systems, [translate([0.0, 5.0, 0.0])]);
        let occs = [
            Occ {
                sons: &[1],
                location: Some([0.0, 0.0, 2.0]),
                ..OCC
            },
            Occ {
                part: 1,
                location: Some([10.0, 0.0, 0.0]),
                ..OCC
            },
        ];
        let out = place(&occs, 1, &systems).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].tessellation, 0);
        assert_eq!(out[0].matrix, translate([10.0, 5.0, 2.0]));
    }

    #[test]
    fn hidden_and_suppressed_occurrences_draw_nothing() {
        let shown = Occ { part: 1, ..OCC };
        for hide in [
            Occ {
                behaviour: Some((0, 0)),
                ..shown
            },
            Occ {
                behaviour: Some((0, SHOW | REMOVED)),
                ..shown
            },
            Occ {
                suppressed: true,
                ..shown
            },
        ] {
            let occs = [
                Occ {
                    sons: &[1, 2],
                    ..OCC
                },
                Occ {
                    behaviour: Some((0, SHOW)),
                    ..shown
                },
                hide,
            ];
            assert_eq!(place(&occs, 0, &Globals::default()).unwrap().len(), 1);
        }
    }

    #[test]
    fn a_camera_is_read_past() {
        let occs = [Occ {
            part: 1,
            camera: true,
            ..OCC
        }];
        assert_eq!(place(&occs, 0, &Globals::default()).unwrap().len(), 1);
    }

    #[test]
    fn a_schema_field_added_to_the_graphics_base_is_skipped() {
        let mut w = W::default();
        w.uint(1).uint(BASE_WITH_GRAPHICS).uint(1).uint(3);
        w.append(&tree(&[Occ { part: 1, ..OCC }], 0, true));
        let bytes = w.bytes();
        let mut r = BitReader::new(&bytes);
        let schema = Schema::read(&mut r).unwrap();
        let t = Ctx::new(r, &schema, 8137).file_structure_tree().unwrap();
        assert_eq!(t.parts[0][0].tessellation, 1);
        assert_eq!(t.root, 1);
    }

    #[test]
    fn a_cycle_is_refused() {
        let occs = [Occ { sons: &[0], ..OCC }];
        assert!(matches!(
            place(&occs, 0, &Globals::default()),
            Err(PrcError::Malformed(_))
        ));
    }

    #[test]
    fn a_missing_reference_system_is_refused() {
        let occs = [Occ { part: 1, ..OCC }];
        assert!(matches!(
            place(&occs, 1, &Globals::default()),
            Err(PrcError::Malformed(_))
        ));
    }

    fn listed(occs: &[Occ<'_>], part_name: Option<Option<&str>>) -> (usize, Vec<ModelNode>) {
        let bytes = tree_with(occs, 0, false, part_name, None).bytes();
        let t = ctx(&bytes, &Schema::default())
            .file_structure_tree()
            .unwrap();
        let globals = Globals::default();
        let mut walk = Walk::new(
            vec![(FS, &t, &globals)],
            StyleAlpha::default(),
            EntityOverrides::default(),
        );
        walk.occurrence(0, 0, &IDENTITY, &[], At::ROOT).unwrap();
        (walk.out.len(), walk.nodes)
    }

    #[test]
    fn the_tree_lists_hidden_and_suppressed_subtrees_without_drawing_them() {
        let occs = [
            Occ {
                sons: &[1, 2, 4],
                name: Some(Some("Assembly")),
                ..OCC
            },
            Occ {
                part: 1,
                name: Some(Some("Bolt")),
                ..OCC
            },
            Occ {
                part: 1,
                sons: &[3],
                suppressed: true,
                name: Some(Some("Nut")),
                ..OCC
            },
            Occ {
                part: 1,
                name: Some(Some("Washer")),
                ..OCC
            },
            Occ {
                part: 1,
                behaviour: Some((0, 0)),
                name: Some(Some("Pin")),
                ..OCC
            },
        ];
        let (drawn, nodes) = listed(&occs, None);
        assert_eq!(drawn, 1);
        let row = |n: &ModelNode| {
            (
                n.name.clone().unwrap_or_default(),
                n.parent,
                n.depth,
                n.hidden,
                n.suppressed,
                n.drawn,
                n.placements.clone(),
            )
        };
        let rows: Vec<_> = nodes.iter().map(row).collect();
        let s = String::from;
        assert_eq!(
            rows,
            [
                (s("Assembly"), None, 0, false, false, true, 0..1),
                (s("Bolt"), Some(0), 1, false, false, true, 0..1),
                (s("Nut"), Some(0), 1, false, true, false, 1..1),
                (s("Washer"), Some(2), 2, false, false, false, 1..1),
                (s("Pin"), Some(0), 1, true, false, false, 1..1),
            ]
        );
        assert!(!nodes[0].has_part && nodes[1].has_part);
        assert_eq!(nodes[3].occurrence, 3);
    }

    #[test]
    fn a_name_falls_back_to_the_prototype_then_the_part() {
        let occs = [
            Occ {
                sons: &[1, 2],
                name: Some(None),
                ..OCC
            },
            Occ {
                prototype: Some((4, None)),
                name: Some(None),
                ..OCC
            },
            Occ {
                part: 1,
                name: Some(None),
                ..OCC
            },
            Occ {
                part: 1,
                name: Some(Some("Bracket")),
                ..OCC
            },
        ];
        let from = |nodes: &[ModelNode]| -> Vec<_> {
            nodes
                .iter()
                .map(|n| (n.name.clone(), n.name_from))
                .collect()
        };
        let named = |n: &str| Some(n.to_owned());
        let (_, nodes) = listed(&occs, Some(Some("Plate")));
        assert_eq!(
            from(&nodes),
            [
                (None, NameSource::Unnamed),
                (named("Bracket"), NameSource::Prototype),
                (named("Plate"), NameSource::Part),
            ]
        );
        let (_, nodes) = listed(&occs, Some(None));
        assert_eq!(from(&nodes)[2], (None, NameSource::Unnamed));
    }

    #[test]
    fn same_name_reuses_the_current_name() {
        let occs = [
            Occ {
                sons: &[1],
                name: Some(Some("Top")),
                ..OCC
            },
            Occ { part: 1, ..OCC },
        ];
        let (_, nodes) = listed(&occs, None);
        assert_eq!(nodes[1].name.as_deref(), Some("Top"));
        assert_eq!(nodes[1].name_from, NameSource::Occurrence);
    }

    #[test]
    fn the_model_tree_indexes_the_placements() {
        let f = crate::PrcFile::parse(&assembly_prc()).unwrap();
        let ranges: Vec<_> = f
            .model_tree()
            .unwrap()
            .iter()
            .map(|n| (n.depth, n.placements.clone()))
            .collect();
        assert_eq!(ranges, [(0, 0..2), (1, 0..1), (1, 1..2)]);
    }

    #[test]
    fn the_parts_drawing_a_left_out_mesh_are_named_in_tree_order() {
        let occs = [
            Occ {
                sons: &[1, 2, 3],
                name: Some(Some("Frame")),
                ..OCC
            },
            Occ {
                part: 1,
                name: Some(Some("Bolt")),
                ..OCC
            },
            Occ { part: 1, ..OCC },
            Occ {
                part: 1,
                name: Some(None),
                ..OCC
            },
        ];
        let compressed = crate::PrcFile::parse(include_bytes!(
            "../../../../fixtures/synthetic/prc/compressed.prc"
        ))
        .unwrap();
        let tess = compressed.file_structures[0].section(crate::SectionKind::Tessellation);
        let prc = crate::PrcFile::parse(&placed(tess, &occs)).unwrap();
        let labels = crate::model::part_labels(&prc, &[(0, 0)]);
        assert_eq!(labels, ["Bolt x2", "occurrence 0:3"]);
        assert!(crate::model::part_labels(&prc, &[(0, 1)]).is_empty());
        let square = crate::assemble(&assembly_prc()).unwrap();
        assert!(square.left_out_parts.is_empty());
    }

    /// A two-occurrence assembly of the unit square: once in place, once
    /// mirrored in x and moved 5 along it. The CLI's placement fixture.
    fn assembly_prc() -> Vec<u8> {
        let square = crate::PrcFile::parse(include_bytes!(
            "../../../../fixtures/synthetic/prc/square.prc"
        ))
        .unwrap();
        let tess = square.file_structures[0].section(crate::SectionKind::Tessellation);
        let occs = [
            Occ {
                sons: &[1, 2],
                ..OCC
            },
            Occ { part: 1, ..OCC },
            Occ {
                part: 1,
                location: Some([5.0, 0.0, 0.0]),
                mirror: true,
                ..OCC
            },
        ];
        placed(tess, &occs)
    }

    /// One file structure holding `tess` and the occurrence tree `occs`.
    fn placed(tess: &[u8], occs: &[Occ]) -> Vec<u8> {
        placed_tree(tess, &tree(occs, 0, false).bytes())
    }

    /// One file structure holding `tess` and the tree section `tree`.
    fn placed_tree(tess: &[u8], tree: &[u8]) -> Vec<u8> {
        let schema = W::default().uint(0).bytes();
        let mut model = W::default();
        model
            .uint(0)
            .uint(MODEL_FILE)
            .uint(0)
            .bit(true)
            .bit(false)
            .double(1.0)
            .uint(1);
        // The id `prc_container` gives its file structure.
        for id in [5, 6, 7, 8] {
            model.uint(id);
        }
        model.uint(1).bit(true);
        crate::testw::prc_container(&schema, tree, tess, &model.bytes())
    }

    /// The square under a root with a part named directly, one named by
    /// its prototype, one by its part, one stored hidden and one
    /// suppressed: every state a model-tree panel shows.
    fn named_tree_prc() -> Vec<u8> {
        let square = crate::PrcFile::parse(include_bytes!(
            "../../../../fixtures/synthetic/prc/square.prc"
        ))
        .unwrap();
        let tess = square.file_structures[0].section(crate::SectionKind::Tessellation);
        let part = |name| Occ {
            part: 1,
            name: Some(name),
            ..OCC
        };
        let occs = [
            Occ {
                sons: &[1, 2, 3, 4, 5],
                name: Some(Some("Assembly")),
                ..OCC
            },
            part(Some("Bolt")),
            Occ {
                prototype: Some((7, None)),
                name: Some(None),
                ..OCC
            },
            part(None),
            Occ {
                behaviour: Some((0, 0)),
                ..part(Some("Pin"))
            },
            Occ {
                suppressed: true,
                ..part(Some("Nut"))
            },
            part(Some("Bracket")),
        ];
        placed_tree(
            tess,
            &tree_with(&occs, 0, false, Some(Some("Plate")), None).bytes(),
        )
    }

    #[test]
    fn the_named_tree_fixture_shows_every_listed_state() {
        let bytes = named_tree_prc();
        let f = crate::PrcFile::parse(&bytes).unwrap();
        let rows: Vec<_> = f
            .model_tree()
            .unwrap()
            .iter()
            .map(|n| (n.label(), n.name_from, n.hidden, n.suppressed, n.drawn))
            .collect();
        let s = String::from;
        use NameSource::{Occurrence as O, Part, Prototype};
        assert_eq!(
            rows,
            [
                (s("Assembly"), O, false, false, true),
                (s("Bolt"), O, false, false, true),
                (s("Bracket"), Prototype, false, false, true),
                (s("Plate"), Part, false, false, true),
                (s("Pin"), O, true, false, false),
                (s("Nut"), O, false, true, false),
            ]
        );
        let m = crate::assemble(&bytes).unwrap();
        assert_eq!(m.mesh_placements, [0, 1, 2]);
        crate::testw::check_fixture("named-tree.prc", &bytes);
    }

    #[test]
    fn draw_all_places_the_stored_hidden_and_suppressed_parts_too() {
        let bytes = named_tree_prc();
        let options = crate::AssembleOptions {
            stored_visibility: crate::StoredVisibility::DrawAll,
            ..Default::default()
        };
        let m = crate::assemble_with_options(&bytes, &options).unwrap();
        assert_eq!(m.mesh_placements, [0, 1, 2, 3, 4]);
        let rows: Vec<_> = m
            .tree
            .iter()
            .map(|n| (n.label(), n.drawn, n.placements.clone()))
            .collect();
        let s = String::from;
        assert_eq!(
            rows,
            [
                (s("Assembly"), true, 0..5),
                (s("Bolt"), true, 0..1),
                (s("Bracket"), true, 1..2),
                (s("Plate"), true, 2..3),
                (s("Pin"), false, 3..4),
                (s("Nut"), false, 4..5),
            ]
        );
    }

    #[test]
    fn the_assembly_fixture_places_and_mirrors() {
        let bytes = assembly_prc();
        let f = crate::PrcFile::parse(&bytes).unwrap();
        let p = f.placements().unwrap();
        let mut mirror = translate([5.0, 0.0, 0.0]);
        mirror[0][0] = -1.0;
        assert_eq!(p.len(), 2);
        assert_eq!((p[0].tessellation, p[0].matrix), (0, IDENTITY));
        assert_eq!((p[1].tessellation, p[1].matrix), (0, mirror));
        crate::testw::check_fixture("assembly.prc", &bytes);
    }

    #[test]
    fn normals_take_the_inverse_transpose_and_mirror_with_their_triangle() {
        let r = std::f64::consts::FRAC_1_SQRT_2;
        let mesh = crate::TriangleMesh {
            positions: vec![[0.0; 3]; 3],
            triangles: vec![[0, 1, 2]],
            normals: vec![[r, r, 0.0], [0.0, 0.0, 3.0]],
            triangle_normals: vec![[0, 0, 1]],
            ..Default::default()
        };
        // Stretching x by 2 turns the plane x + y = 0 into x + 2y = 0.
        let mut stretch = IDENTITY;
        stretch[0][0] = 2.0;
        let s = mesh.transformed(&stretch);
        let k = 1.0 / 5f64.sqrt();
        for (got, want) in s.normals[0].iter().zip([k, 2.0 * k, 0.0]) {
            assert!((got - want).abs() < 1e-12, "{:?}", s.normals[0]);
        }
        assert_eq!(s.normals[1], [0.0, 0.0, 1.0]);
        let mut mirror = IDENTITY;
        mirror[0][0] = -1.0;
        let m = mesh.transformed(&mirror);
        assert!((m.normals[0][0] + r).abs() < 1e-12 && (m.normals[0][1] - r).abs() < 1e-12);
        assert_eq!(m.triangle_normals, [[0, 1, 0]]);
        assert_eq!(m.triangles, [[0, 2, 1]]);
    }

    const RED: [f64; 3] = [1.0, 0.0, 0.0];
    const GREEN: [f64; 3] = [0.0, 1.0, 0.0];

    fn g(style: u32, bits: u16) -> Graphics {
        Graphics { style, bits, fs: 0 }
    }

    #[test]
    fn a_son_style_wins_unless_a_father_forces_his() {
        let father = FATHER_HERIT_COLOR;
        assert_eq!(resolve_style(&[g(1, 0), g(2, 0)]).style, 2, "own wins");
        assert_eq!(resolve_style(&[g(1, 0), g(0, 0)]).style, 1, "none inherits");
        assert_eq!(
            resolve_style(&[g(1, father), g(2, 0)]).style,
            1,
            "father forces"
        );
        assert_eq!(
            resolve_style(&[g(1, father), g(3, father), g(2, 0)]).style,
            1,
            "the oldest father wins"
        );
        assert_eq!(
            resolve_style(&[g(1, father), g(2, SON_HERIT_COLOR)]).style,
            2,
            "a son's heritage beats the father's"
        );
        assert_eq!(resolve_style(&[]).style, 0);
    }

    #[test]
    fn face_styles_take_part_in_inheritance() {
        let red = Some([1.0, 0.0, 0.0, 1.0]);
        let green = Some([0.0, 1.0, 0.0, 1.0]);
        let placement = |chain| Placement {
            file_structure: 0,
            tessellation: 0,
            matrix: IDENTITY,
            colour: red,
            chain,
            palettes: std::sync::Arc::from(vec![std::sync::Arc::from([None, red, green].map(
                |c| {
                    c.map(|rgba| Paint {
                        rgba,
                        alpha_unset: false,
                    })
                },
            ))]),
            skins: std::sync::Arc::from(Vec::new()),
            item_override: None,
            face_overrides: Vec::new(),
        };
        let mut mesh = crate::TriangleMesh::default();
        let item = placement(vec![g(1, 0)]);
        assert_eq!(item.triangle_colours(&mesh), None);
        mesh.triangle_graphics = vec![g(0, 0), g(2, 0)];
        assert_eq!(item.triangle_colours(&mesh), Some(vec![red, green]));
        // A parent that forces its colour wins unless the face claims it.
        let forced = placement(vec![g(1, FATHER_HERIT_COLOR)]);
        assert_eq!(forced.triangle_colours(&mesh), Some(vec![red, red]));
        mesh.triangle_graphics = vec![g(2, SON_HERIT_COLOR)];
        assert_eq!(forced.triangle_colours(&mesh), Some(vec![green]));
    }

    #[test]
    fn colour_indices_are_double_scaled() {
        let style = |is_material, index, transparency| Style {
            is_material,
            index,
            transparency,
        };
        let gl = Globals {
            colours: vec![RED, GREEN],
            materials: vec![
                Material::Plain {
                    diffuse: 4,
                    alpha: 0.5,
                },
                Material::Textured {
                    base: 1,
                    texture: 0,
                    next: 0,
                    uv: 0,
                },
            ],
            styles: vec![
                style(false, 4, None),
                style(false, 2, None),
                style(true, 1, Some(51)),
                style(true, 2, None),
            ],
            ..Globals::default()
        };
        let mul = StyleAlpha::Multiply;
        assert_eq!(
            gl.style_colour(1, mul),
            Some([0.0, 1.0, 0.0, 1.0]),
            "4 names entry 1"
        );
        assert_eq!(gl.style_colour(2, mul), None, "2 is not double-scaled");
        assert_eq!(
            gl.style_colour(3, mul),
            Some([0.0, 1.0, 0.0, 0.1]),
            "diffuse alpha x transparency"
        );
        assert_eq!(
            gl.style_colour(3, StyleAlpha::StyleWins),
            Some([0.0, 1.0, 0.0, 0.2]),
            "the style's transparency replaces the diffuse alpha"
        );
        assert_eq!(
            gl.style_colour(4, StyleAlpha::StyleWins),
            Some([0.0, 1.0, 0.0, 0.5]),
            "with no transparency the material alpha stands"
        );
        assert_eq!(
            gl.style_colour(4, mul),
            Some([0.0, 1.0, 0.0, 0.5]),
            "a texture's base material"
        );
        assert_eq!(
            gl.style_colour(4, StyleAlpha::ZeroUnset),
            Some([0.0, 1.0, 0.0, 0.5]),
            "a non-zero material alpha stands"
        );
        assert_eq!(gl.style_colour(0, mul), None);
        assert_eq!(gl.style_colour(9, mul), None);
    }

    /// The globals fixture's style 1 is colour 0 at transparency 128;
    /// an occurrence reusing the current graphics takes the one read last.
    #[test]
    fn placements_carry_the_resolved_colour() {
        let gl = fixture_globals();
        let want = Some([1.0, 0.5, 0.0, 128.0 / 255.0]);
        let occs = [
            Occ {
                sons: &[1, 2, 3],
                ..OCC
            },
            Occ {
                part: 1,
                behaviour: Some((1, SHOW)),
                ..OCC
            },
            Occ { part: 1, ..OCC },
            Occ {
                part: 1,
                behaviour: Some((0, SHOW)),
                ..OCC
            },
        ];
        let out = place(&occs, 0, &gl).unwrap();
        let colours: Vec<_> = out.iter().map(|p| p.colour).collect();
        assert_eq!(colours, [want, want, None]);
    }

    fn ref_base(w: &mut W, id: u32) {
        base(w);
        w.uint(0).uint(0).uint(id);
    }

    /// Schema, then globals with a picture, a scaled texture, red and blue,
    /// two alpha-0 materials and a texture over the blue one, and styles 1
    /// (red, transparency 255) and 2 (the texture, 128); no styles when
    /// `plain`.
    fn coloured_globals(plain: bool) -> W {
        let mut w = W::default();
        w.uint(0).uint(FILE_STRUCTURE_GLOBALS);
        base(&mut w);
        w.uint(0)
            .double(2000.0)
            .double(40.0)
            .string(Some(""))
            .uint(0);
        if plain {
            for _ in 0..9 {
                w.uint(0);
            }
            return w;
        }
        w.uint(2);
        for c in [1.0, 0.0, 0.0, 0.0, 0.0, 1.0] {
            w.double(c);
        }
        w.uint(1).uint(PICTURE);
        base(&mut w);
        w.uint(0).uint(0).uint(1).uint(1);
        w.uint(1).uint(TEXTURE_DEFINITION);
        ref_base(&mut w, 1);
        w.uint(1).put(2, 8).int(3).int(0).bit(false).uint(0);
        w.uint(1).double(1.0).uint(1).put(0, 8);
        w.int(3).double(1.0).double(1.0).double(1.0).double(1.0);
        w.int(1).int(1).int(0);
        w.put(u64::from(TEXTURE_ALPHA_TEST), 8).int(0).double(0.5);
        w.int(0).int(0).bit(true);
        w.uint(TEXTURE_TRANSFORMATION)
            .bit(false)
            .bit(false)
            .bit(true);
        w.put(0x08, 8).double(2.0);
        w.uint(3);
        for (id, diffuse) in [(2, 1), (3, 4)] {
            w.uint(MATERIAL);
            ref_base(&mut w, id);
            w.uint(0).uint(diffuse).uint(0).uint(0);
            for alpha in [0.5, 1.0, 0.0, 1.0, 1.0] {
                w.double(alpha);
            }
        }
        w.uint(TEXTURE_APPLICATION);
        ref_base(&mut w, 4);
        w.uint(2).uint(1).uint(0).uint(0);
        w.uint(0).uint(2);
        for (id, material, transparency) in [(5, 1, 255), (6, 3, 128)] {
            w.uint(STYLE);
            ref_base(&mut w, id);
            w.double(1.0).bit(false).uint(0).bit(true).uint(material);
            w.bit(true)
                .put(transparency, 8)
                .bit(false)
                .bit(false)
                .bit(false);
        }
        w.uint(0).uint(0).uint(0);
        w
    }

    /// The unit square, defined in file structure A, placed three times by
    /// prototype from file structure B, whose globals hold the colours:
    /// styles 1, 2 and none, at x 0, 2 and 4. The SolidWorks layout that
    /// colours a part with another structure's palette. The CLI's colour
    /// fixture.
    fn coloured_prc() -> Vec<u8> {
        let square = crate::PrcFile::parse(include_bytes!(
            "../../../../fixtures/synthetic/prc/square.prc"
        ))
        .unwrap();
        let tess = square.file_structures[0].section(crate::SectionKind::Tessellation);
        let a = UniqueId([5, 6, 7, 8]);
        let shown = |style, x| Occ {
            behaviour: Some((style, SHOW)),
            prototype: Some((1, Some(a))),
            location: Some([x, 0.0, 0.0]),
            ..OCC
        };
        let b_occs = [
            Occ {
                sons: &[1, 2, 3],
                ..OCC
            },
            shown(1, 0.0),
            shown(2, 2.0),
            shown(0, 4.0),
        ];
        let mut model = W::default();
        model.uint(0).uint(MODEL_FILE);
        base(&mut model);
        model.bit(false).double(1.0).uint(2);
        for (last, root) in [(8, 0), (9, 1)] {
            for c in [5, 6, 7, last] {
                model.uint(c);
            }
            model.uint(root).bit(true);
        }
        crate::testw::prc_container_n(
            &[
                [
                    &coloured_globals(true).bytes(),
                    &tree(&[Occ { part: 1, ..OCC }], 0, false).bytes(),
                    tess,
                ],
                [
                    &coloured_globals(false).bytes(),
                    &tree(&b_occs, 0, false).bytes(),
                    &[],
                ],
            ],
            &model.bytes(),
        )
    }

    #[test]
    fn a_prototype_takes_the_palette_of_the_structure_that_styled_it() {
        let bytes = coloured_prc();
        let f = crate::PrcFile::parse(&bytes).unwrap();
        let blue = 128.0 / 255.0;
        let colours = |rule| -> Vec<_> {
            let p = f.placements_with(rule).unwrap();
            assert!(p.iter().all(|p| p.file_structure == 0));
            let xs: Vec<_> = p.iter().map(|p| p.matrix[0][3]).collect();
            assert_eq!(xs, [0.0, 2.0, 4.0]);
            p.iter().map(|p| p.colour).collect()
        };
        assert_eq!(
            colours(StyleAlpha::StyleWins),
            [
                Some([1.0, 0.0, 0.0, 1.0]),
                Some([0.0, 0.0, 1.0, blue]),
                None
            ]
        );
        assert_eq!(
            colours(StyleAlpha::Multiply),
            [Some([1.0, 0.0, 0.0, 0.0]), Some([0.0, 0.0, 1.0, 0.0]), None]
        );
        let m = crate::assemble(&bytes).unwrap();
        assert_eq!(
            m.colours,
            [Some([255, 0, 0, 255]), Some([0, 0, 255, 128]), None]
        );
        crate::testw::check_fixture("coloured.prc", &bytes);
    }

    /// The unit square as B-rep `(1, 1)` in structure A, placed at x 0, 2
    /// and 4 by prototype from structure B with style 1 (red), 1 and none.
    /// The first occurrence recolours the item with style 2 (translucent
    /// blue), the second only face 0, and the third hides it: the layout a
    /// CAD export uses for its only transparency. The CLI's override
    /// fixture.
    fn overridden_prc() -> Vec<u8> {
        let square = crate::PrcFile::parse(include_bytes!(
            "../../../../fixtures/synthetic/prc/square.prc"
        ))
        .unwrap();
        let tess = square.file_structures[0].section(crate::SectionKind::Tessellation);
        let a = UniqueId([5, 6, 7, 8]);
        let shown = |style, x, refs| Occ {
            behaviour: Some((style, SHOW)),
            prototype: Some((1, Some(a))),
            location: Some([x, 0.0, 0.0]),
            refs,
            ..OCC
        };
        let blue = [Er::Item {
            g: (2, SHOW),
            fs: Some(a),
            uid: 7,
        }];
        let face = [Er::Faces {
            g: (2, SHOW),
            faces: &[0],
        }];
        let hide = [Er::Item {
            g: (0, 0),
            fs: Some(a),
            uid: 7,
        }];
        let b_occs = [
            Occ {
                sons: &[1, 2, 3],
                ..OCC
            },
            shown(1, 0.0, &blue),
            shown(1, 2.0, &face),
            shown(0, 4.0, &hide),
        ];
        let mut model = W::default();
        model.uint(0).uint(MODEL_FILE);
        base(&mut model);
        model.bit(false).double(1.0).uint(2);
        for (last, root) in [(8, 0), (9, 1)] {
            for c in [5, 6, 7, last] {
                model.uint(c);
            }
            model.uint(root).bit(true);
        }
        let a_occs = [Occ { part: 1, ..OCC }];
        crate::testw::prc_container_n(
            &[
                [
                    &coloured_globals(true).bytes(),
                    &tree_with(&a_occs, 0, false, None, Some((1, 1))).bytes(),
                    tess,
                ],
                [
                    &coloured_globals(false).bytes(),
                    &tree(&b_occs, 0, false).bytes(),
                    &[],
                ],
            ],
            &model.bytes(),
        )
    }

    #[test]
    fn an_occurrence_overrides_the_entities_its_subtree_places() {
        let bytes = overridden_prc();
        let f = crate::PrcFile::parse(&bytes).unwrap();
        let red = Some([1.0, 0.0, 0.0, 1.0]);
        let blue = Some([0.0, 0.0, 1.0, 128.0 / 255.0]);
        let walk = |scope| {
            f.textured_placements(
                StyleAlpha::default(),
                TextureRules::default(),
                scope,
                crate::StoredVisibility::Honour,
            )
            .unwrap()
            .0
        };
        let p = walk(EntityOverrides::Subtree);
        let xs: Vec<_> = p.iter().map(|p| p.matrix[0][3]).collect();
        assert_eq!(xs, [0.0, 2.0], "the third occurrence hides its square");
        let colours: Vec<_> = p.iter().map(|p| p.colour).collect();
        assert_eq!(colours, [blue, red]);
        assert_eq!(p[0].face_overrides, []);
        let faces: Vec<_> = p[1]
            .face_overrides
            .iter()
            .map(|(f, g)| (*f, g.style))
            .collect();
        assert_eq!(faces, [(0, 2)]);

        let p = walk(EntityOverrides::Everywhere);
        let colours: Vec<_> = p.iter().map(|p| p.colour).collect();
        assert_eq!(colours, [blue, blue, blue], "the first occurrence's wins");
        assert!(p.iter().all(|p| p.face_overrides.len() == 1));

        let p = walk(EntityOverrides::Ignore);
        let colours: Vec<_> = p.iter().map(|p| p.colour).collect();
        assert_eq!(colours, [red, red, None]);
        assert!(
            p.iter()
                .all(|p| p.face_overrides.is_empty() && p.item_override.is_none())
        );
        crate::testw::check_fixture("overridden.prc", &bytes);
    }

    /// Each mesh names the placement that drew it, and the tree read with
    /// the same overrides indexes those placements: hiding the third
    /// occurrence's square shifts nothing before it, and a placement whose
    /// faces differ in colour draws two meshes.
    #[test]
    fn an_assembled_mesh_names_its_placement_under_the_same_overrides() {
        let bytes = overridden_prc();
        let read = |scope| {
            let options = crate::AssembleOptions {
                entity_overrides: scope,
                ..crate::AssembleOptions::default()
            };
            let m = crate::assemble_with_options(&bytes, &options).unwrap();
            let ranges: Vec<_> = m.tree.iter().map(|n| n.placements.clone()).collect();
            (m.mesh_placements, ranges)
        };
        assert_eq!(
            read(EntityOverrides::Subtree),
            (vec![0, 1], vec![0..2, 0..1, 1..2, 2..2])
        );
        assert_eq!(
            read(EntityOverrides::Ignore),
            (vec![0, 1, 2], vec![0..3, 0..1, 1..2, 2..3])
        );
    }

    /// Globals with grey, a 2x2 raw-RGB picture in header file 1, a texture
    /// (stored-coordinate `mapping`, Replace, repeating) and a material
    /// applying it over grey, used by style 1.
    fn textured_globals(mapping: i32, next: u32) -> W {
        let mut w = W::default();
        w.uint(0).uint(FILE_STRUCTURE_GLOBALS);
        base(&mut w);
        w.uint(0)
            .double(2000.0)
            .double(40.0)
            .string(Some(""))
            .uint(0);
        w.uint(1).double(0.5).double(0.5).double(0.5);
        w.uint(1).uint(PICTURE);
        base(&mut w);
        w.uint(2).uint(1).uint(2).uint(2);
        w.uint(1).uint(TEXTURE_DEFINITION);
        ref_base(&mut w, 1);
        w.uint(1).put(2, 8).int(mapping);
        if mapping == TEXTURE_MAPPING_OPERATOR {
            w.int(0).bit(false);
        }
        w.uint(0).uint(1).double(1.0).uint(1).put(0, 8);
        w.int(2).int(0).int(0).put(0, 8);
        w.int(1).int(1).bit(false);
        w.uint(2);
        w.uint(MATERIAL);
        ref_base(&mut w, 2);
        w.uint(1).uint(1).uint(1).uint(1);
        for v in [0.5, 1.0, 1.0, 1.0, 1.0] {
            w.double(v);
        }
        w.uint(TEXTURE_APPLICATION);
        ref_base(&mut w, 3);
        w.uint(1).uint(1).uint(next).uint(1);
        w.uint(0).uint(1).uint(STYLE);
        ref_base(&mut w, 4);
        w.double(1.0).bit(false).uint(0).bit(true).uint(2);
        w.bit(false).bit(false).bit(false).bit(false);
        w.uint(0).uint(0).uint(0);
        w
    }

    /// The unit square in grey from a material whose diffuse alpha is 0.0,
    /// through a style that states no transparency: the layout CAD exports
    /// write for parts meant to be seen. The CLI's unset-alpha fixture.
    pub(crate) fn alpha_unset_prc() -> Vec<u8> {
        let mut g = W::default();
        g.uint(0).uint(FILE_STRUCTURE_GLOBALS);
        base(&mut g);
        g.uint(0)
            .double(2000.0)
            .double(40.0)
            .string(Some(""))
            .uint(0);
        g.uint(1).double(0.75).double(0.75).double(0.75);
        g.uint(0).uint(0).uint(1).uint(MATERIAL);
        ref_base(&mut g, 1);
        g.uint(1).uint(1).uint(1).uint(1);
        for v in [0.5, 0.0, 0.0, 0.0, 0.0] {
            g.double(v);
        }
        g.uint(0).uint(1).uint(STYLE);
        ref_base(&mut g, 2);
        g.double(1.0).bit(false).uint(0).bit(true).uint(1);
        g.bit(false).bit(false).bit(false).bit(false);
        g.uint(0).uint(0).uint(0);
        let square = crate::PrcFile::parse(include_bytes!(
            "../../../../fixtures/synthetic/prc/square.prc"
        ))
        .unwrap();
        let tess = square.file_structures[0].section(crate::SectionKind::Tessellation);
        let mut model = W::default();
        model.uint(0).uint(MODEL_FILE);
        base(&mut model);
        model.bit(false).double(1.0).uint(1);
        for c in [5, 6, 7, 8] {
            model.uint(c);
        }
        model.uint(1).bit(true);
        let occ = Occ {
            part: 1,
            behaviour: Some((1, SHOW)),
            ..OCC
        };
        crate::testw::prc_container_n(
            &[[&g.bytes(), &tree(&[occ], 0, false).bytes(), tess]],
            &model.bytes(),
        )
    }

    #[test]
    fn a_zero_material_alpha_under_a_bare_style_is_unset_by_default() {
        let bytes = alpha_unset_prc();
        crate::testw::check_fixture("alpha-unset.prc", &bytes);
        let grey = 191;
        let read = |rule| {
            let options = crate::AssembleOptions {
                style_alpha: rule,
                ..crate::AssembleOptions::default()
            };
            let m = crate::assemble_with_options(&bytes, &options).unwrap();
            (m.colours, m.alpha_unset)
        };
        assert_eq!(
            read(StyleAlpha::default()),
            (vec![Some([grey, grey, grey, 255])], 1)
        );
        for rule in [StyleAlpha::StyleWins, StyleAlpha::Multiply] {
            assert_eq!(
                read(rule),
                (vec![Some([grey, grey, grey, 0])], 0),
                "{rule:?}"
            );
        }
    }

    /// The unit square with stored texture coordinates, styled by the
    /// texture of [`textured_globals`]; its picture is red, green over
    /// blue, white. The CLI's texture fixture when `mapping` is 1.
    pub(crate) fn textured_prc(mapping: i32) -> Vec<u8> {
        textured_prc_with(
            &textured_globals(mapping, 0),
            &crate::tess::tests::textured_square_section(),
        )
    }

    /// [`textured_prc`]'s layout with the given globals and tessellation.
    fn textured_prc_with(globals: &W, tess: &[u8]) -> Vec<u8> {
        use std::io::Write as _;
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        e.write_all(&[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255])
            .unwrap();
        let picture = e.finish().unwrap();
        let mut model = W::default();
        model.uint(0).uint(MODEL_FILE);
        base(&mut model);
        model.bit(false).double(1.0).uint(1);
        for c in [5, 6, 7, 8] {
            model.uint(c);
        }
        // Root occurrence 0, biased by one.
        model.uint(1).bit(true);
        let occ = Occ {
            part: 1,
            behaviour: Some((1, SHOW)),
            ..OCC
        };
        crate::testw::prc_container_files(
            &[[&globals.bytes(), &tree(&[occ], 0, false).bytes(), tess]],
            &model.bytes(),
            &[&picture],
        )
    }

    #[test]
    fn a_textured_square_draws_its_picture() {
        let bytes = textured_prc(1);
        let m = crate::assemble(&bytes).unwrap();
        assert_eq!(m.unplaced, None);
        assert_eq!((m.textured, m.textures.len()), (1, 1));
        assert_eq!(m.mesh_textures, [Some(0)]);
        assert!(m.texture_notes.is_empty(), "{:?}", m.texture_notes);
        let t = &m.textures[0];
        assert_eq!((t.width, t.height, t.uv_set), (2, 2, 0));
        assert_eq!(t.function, crate::TextureFunction::Replace);
        assert_eq!(t.rgba[..4], [255, 0, 0, 255]);
        crate::testw::check_fixture("textured.prc", &bytes);
    }

    #[cfg(feature = "render")]
    #[test]
    fn a_textured_square_renders_its_picture_the_right_way_up() {
        let m = crate::assemble(&textured_prc(1)).unwrap();
        let camera = crate::Camera {
            eye: [0.5, 0.5, 10.0],
            target: [0.5, 0.5, 0.0],
            up: [0.0, 1.0, 0.0],
            projection: crate::Projection::Orthographic { height: 1.0 },
        };
        let options = crate::RenderOptions {
            width: 2,
            height: 2,
            ..crate::RenderOptions::default()
        };
        let image = crate::render_model(&m, &camera, &options).unwrap();
        // Pixel centres fall on texel centres, so filtering mixes nothing.
        let px = |x: usize, y: usize| image.rgba[(y * 2 + x) * 4..][..4].to_vec();
        // v = 0 is the picture's bottom row, the square's bottom edge.
        assert_eq!(px(0, 0), [255, 0, 0, 255], "top left");
        assert_eq!(px(1, 0), [0, 255, 0, 255], "top right");
        assert_eq!(px(0, 1), [0, 0, 255, 255], "bottom left");
        assert_eq!(px(1, 1), [255, 255, 255, 255], "bottom right");
    }

    #[test]
    fn an_undrawable_texture_draws_the_base_colour_and_says_why() {
        let m = crate::assemble(&textured_prc(TEXTURE_MAPPING_OPERATOR)).unwrap();
        assert_eq!(m.textured, 0);
        assert!(m.textures.is_empty());
        assert_eq!(m.texture_notes, [(super::why::MAPPING.to_owned(), 1)]);
        assert_eq!(m.colours, [Some([128, 128, 128, 255])]);
    }

    #[test]
    fn a_mesh_without_coordinates_or_a_later_level_is_disclosed() {
        let square = crate::PrcFile::parse(include_bytes!(
            "../../../../fixtures/synthetic/prc/square.prc"
        ))
        .unwrap();
        let bare = square.file_structures[0].section(crate::SectionKind::Tessellation);
        let m = crate::assemble(&textured_prc_with(&textured_globals(1, 0), bare)).unwrap();
        assert_eq!(m.textured, 0);
        assert_eq!(m.texture_notes, [(super::why::NO_UVS.to_owned(), 1)]);
        let uvs = crate::tess::tests::textured_square_section();
        let m = crate::assemble(&textured_prc_with(&textured_globals(1, 1), &uvs)).unwrap();
        assert_eq!(m.textured, 1, "the first level still draws");
        assert_eq!(m.texture_notes, [(super::why::MORE_LEVELS.to_owned(), 1)]);
    }
}
