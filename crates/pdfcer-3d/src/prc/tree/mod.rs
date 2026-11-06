//! The tree section (`FileStructureTree`, 304) and the model file (301):
//! the product-occurrence tree that places each tessellation in the model
//! [WD 7.3.3-7.3.11, 7.6; PRCRS prc.json]. Field order and index biasing
//! follow the spec RAG's `threed/prc__8137__model_tree_asm.md`,
//! `prc__8137__transformations.md` and `prc__8137__representation_items.md`.
//!
//! Markups (PMI), views, scene lights and clipping planes are read past
//! (`markup`, `scene`); only how many markups and views each node carries
//! is kept.

use crate::PrcError;
use crate::prc::container::UniqueId;
use crate::prc::tess::{Ctx, malformed};

mod geometry;
mod markup;
mod node;
mod overrides;
mod scene;
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
    /// Face indices an entity reference hides, sorted; their triangles are
    /// not drawn ([`Self::shown`]).
    pub(crate) hidden_faces: Vec<u32>,
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
    pub(crate) notes: Notes,
    /// Its scene display parameters' lights.
    pub(crate) lights: Vec<crate::model::light::SceneLights>,
    /// Its views' scene display parameters.
    pub(crate) view_lights: Vec<crate::model::light::StoredView>,
}

/// How many markups and views an occurrence or part definition carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Notes {
    pub(crate) markups: usize,
    pub(crate) views: usize,
}

/// A part definition's notes and its views' scene display parameters.
type PartNotes = (Notes, Vec<crate::model::light::StoredView>);

/// One file structure's tree section.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Tree {
    pub(crate) parts: Vec<Vec<Item>>,
    /// Each part definition's name, parallel to `parts`.
    pub(crate) part_names: Vec<Option<String>>,
    /// Each part definition's markup and view counts, parallel to `parts`.
    pub(crate) part_notes: Vec<Notes>,
    /// Each part definition's views' scene display parameters, parallel to
    /// `parts`.
    pub(crate) part_view_lights: Vec<Vec<crate::model::light::StoredView>>,
    pub(crate) products: Vec<Product>,
    /// `index_product_occurrence + 1` of this structure's root.
    pub(crate) root: u32,
    /// A representation item or an entity reference names a local
    /// coordinate system: placing needs the globals' reference systems.
    pub(crate) names_cs: bool,
}

/// The model file's roots: file structure id and biased occurrence index.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ModelFile {
    pub(crate) roots: Vec<(UniqueId, u32)>,
    pub(crate) units_in_mm: f64,
    pub(crate) units_from_cad_file: bool,
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
        self.skip_added(BASE_WITH_GRAPHICS)?;
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
        self.skip_added(t)?;
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
        self.skip_added(t)?;
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

    /// `MISC_EntityReference` (203) [WD 7.4.4, 7.4.10; PRCRS]; `None`
    /// when it names no target.
    fn entity_reference(&mut self) -> Result<Option<Override>, PrcError> {
        self.expect_type(ENTITY_REFERENCE)?;
        let o = self.entity_reference_body()?;
        self.skip_added(ENTITY_REFERENCE)?;
        self.user_data()?;
        Ok(o)
    }

    /// `ContentEntityReference` [WD 7.4.10]: graphics, local coordinate
    /// system, optional reference data.
    fn entity_reference_body(&mut self) -> Result<Option<Override>, PrcError> {
        let (hidden, graphics) = self.base_with_graphics()?;
        let local = self.r.unsigned_integer()?;
        self.names_cs |= local != 0;
        let target = self.reference_data()?;
        Ok(target.map(|target| Override {
            graphics,
            hidden,
            local,
            target,
        }))
    }

    /// An optional `ReferenceOnPRCBase` (205) or `ReferenceOnTopology`
    /// (206) behind a presence bit.
    fn reference_data(&mut self) -> Result<Option<Target>, PrcError> {
        if !self.r.bit()? {
            return Ok(None);
        }
        match self.r.unsigned_integer()? {
            REFERENCE_ON_PRC_BASE => {
                let kind = self.r.unsigned_integer()?;
                let fs = self.file_identifier()?;
                let uid = self.r.unsigned_integer()?;
                Ok(Some(Target::Entity { kind, fs, uid }))
            }
            REFERENCE_ON_TOPOLOGY => self.reference_on_topology(),
            t => Err(malformed(format!("entity type {t} as reference data"))),
        }
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
        self.skip_added(FILTER)?;
        self.user_data()
    }

    /// `PartDefinition` (311): its drawable items, its name, and its notes
    /// and views' lights.
    fn part_definition(&mut self) -> Result<(Vec<Item>, Option<String>, PartNotes), PrcError> {
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
        let markups = self.markup_data()?;
        let (views, lit) = self.views()?;
        self.skip_added(PART_DEFINITION)?;
        self.user_data()?;
        if hidden {
            items.clear();
        }
        Ok((items, name, (Notes { markups, views }, lit)))
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
        let markups = self.markup_data()?;
        let (views, lit) = self.views()?;
        p.notes = Notes { markups, views };
        p.view_lights = lit;
        if self.r.bit()? {
            self.filter()?;
        }
        let n = self.count(1, "display filters")?;
        for _ in 0..n {
            self.filter()?;
        }
        let n = self.count(1, "scene display parameters")?;
        for _ in 0..n {
            let s = self.scene_display_parameters()?;
            p.lights.push(s);
        }
        self.skip_added(PRODUCT_OCCURRENCE)?;
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
            let (items, name, (notes, lit)) = self.part_definition()?;
            tree.parts.push(items);
            tree.part_names.push(name);
            tree.part_notes.push(notes);
            tree.part_view_lights.push(lit);
        }
        let n = self.count(1, "product occurrences")?;
        for _ in 0..n {
            tree.products.push(self.product_occurrence()?);
        }
        self.expect_type(FILE_STRUCTURE)?;
        self.content_prc_base()?;
        self.r.unsigned_integer()?; // next_available_index
        tree.root = self.r.unsigned_integer()?;
        self.skip_added(FILE_STRUCTURE)?;
        self.skip_added(FILE_STRUCTURE_TREE)?;
        self.user_data()?;
        tree.names_cs = self.names_cs || tree.parts.iter().flatten().any(|i| !i.local.is_empty());
        Ok(tree)
    }

    /// `ModelFile` (301) after its schema [WD 7.3.3; ISS #724 #740]:
    /// `units_in_mm` is always present.
    pub(crate) fn model_file(&mut self) -> Result<ModelFile, PrcError> {
        self.expect_type(MODEL_FILE)?;
        self.content_prc_base()?;
        let units_from_cad_file = self.r.bit()?;
        let units_in_mm = self.r.double()?;
        let n = self.count(6, "root occurrences")?;
        let mut roots = Vec::with_capacity(n);
        for _ in 0..n {
            let id = self.unique_id()?;
            let index = self.r.unsigned_integer()?;
            self.r.bit()?; // is_active
            roots.push((id, index));
        }
        Ok(ModelFile {
            roots,
            units_in_mm,
            units_from_cad_file,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests;
