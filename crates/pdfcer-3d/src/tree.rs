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
/// `product_behavior` SUPPRESSED [WD 7.3.10].
const SUPPRESSED: u8 = 0x01;

/// A 4×4 matrix, `m[row][col]`, acting on column vectors.
pub type Matrix = [[f64; 4]; 4];

/// The identity matrix.
pub const IDENTITY: Matrix = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// `a × b`.
pub fn multiply(a: &Matrix, b: &Matrix) -> Matrix {
    let mut m = [[0.0; 4]; 4];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..4)
                .map(|k| {
                    a.get(i).and_then(|r| r.get(k)).copied().unwrap_or(0.0)
                        * b.get(k).and_then(|r| r.get(j)).copied().unwrap_or(0.0)
                })
                .sum();
        }
    }
    m
}

/// `m × (p, 1)`, divided by the homogeneous coordinate when it is neither
/// 0 nor 1.
pub fn transform_point(m: &Matrix, p: [f64; 3]) -> [f64; 3] {
    let v = [p[0], p[1], p[2], 1.0];
    let row = |i: usize| -> f64 {
        m.get(i)
            .map_or(0.0, |r| r.iter().zip(v).map(|(a, b)| a * b).sum())
    };
    let w = row(3);
    let s = if w == 0.0 || w == 1.0 { 1.0 } else { 1.0 / w };
    [row(0) * s, row(1) * s, row(2) * s]
}

impl crate::TriangleMesh {
    /// This mesh with every position mapped through `m`; a mirroring `m`
    /// (negative determinant) reverses each triangle so the outside stays
    /// counter-clockwise.
    ///
    /// ```
    /// # use pdfcer_3d::{IDENTITY, TriangleMesh};
    /// let mut mirror = IDENTITY;
    /// mirror[0][0] = -1.0;
    /// let mut mesh = TriangleMesh::default();
    /// mesh.positions = vec![[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    /// mesh.triangles = vec![[0, 1, 2]];
    /// let placed = mesh.transformed(&mirror);
    /// assert_eq!(placed.positions[0], [-1.0, 0.0, 0.0]);
    /// assert_eq!(placed.triangles[0], [0, 2, 1]);
    /// ```
    #[must_use]
    pub fn transformed(&self, m: &Matrix) -> Self {
        let mut out = self.clone();
        for p in &mut out.positions {
            *p = transform_point(m, *p);
        }
        let a = |i: usize, j: usize| m.get(i).and_then(|r| r.get(j)).copied().unwrap_or(0.0);
        let det = a(0, 0) * (a(1, 1) * a(2, 2) - a(1, 2) * a(2, 1))
            - a(0, 1) * (a(1, 0) * a(2, 2) - a(1, 2) * a(2, 0))
            + a(0, 2) * (a(1, 0) * a(2, 1) - a(1, 1) * a(2, 0));
        if det < 0.0 {
            for t in &mut out.triangles {
                t.swap(1, 2);
            }
        }
        out
    }
}

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
}

/// A representation item that can be drawn, flattened out of any `RI_Set`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Item {
    /// `index_local_coordinate_system + 1` of the item and each enclosing
    /// set, outermost first; 0 entries dropped.
    pub(crate) local: Vec<u32>,
    /// `index_tessellation + 1`; never 0 here.
    pub(crate) tessellation: u32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Product {
    pub(crate) part: u32,
    pub(crate) prototype: u32,
    pub(crate) prototype_fs: Option<UniqueId>,
    pub(crate) external: u32,
    pub(crate) external_fs: Option<UniqueId>,
    pub(crate) sons: Vec<u32>,
    pub(crate) hidden: bool,
    pub(crate) location: Option<Matrix>,
}

/// One file structure's tree section.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Tree {
    pub(crate) parts: Vec<Vec<Item>>,
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
    /// PRC ids.
    fn content_prc_ref_base(&mut self) -> Result<(), PrcError> {
        self.content_prc_base()?;
        for _ in 0..3 {
            self.r.unsigned_integer()?;
        }
        Ok(())
    }

    /// `PRCBaseWithGraphics` [WD 7.2.4; ISS #405]; returns whether the
    /// entity's own graphics hide it (Show clear or Removed set). Inherited
    /// graphics (`same_graphics`) never hide.
    fn base_with_graphics(&mut self) -> Result<bool, PrcError> {
        self.content_prc_ref_base()?;
        let hidden = if self.r.bit()? {
            false
        } else {
            self.r.unsigned_integer()?; // layer + 1
            self.r.unsigned_integer()?; // line style + 1
            let lo = u16::from(self.r.character()?);
            let hi = u16::from(self.r.character()?);
            let bits = lo | hi << 8;
            bits & SHOW == 0 || bits & REMOVED != 0
        };
        // Schema additions to the base itself, after the graphics [PRCRS].
        self.schema
            .skip_added_fields(BASE_WITH_GRAPHICS, &mut self.r)?;
        Ok(hidden)
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
        out: &mut Vec<Item>,
        depth: usize,
    ) -> Result<(), PrcError> {
        if depth > MAX_DEPTH {
            return Err(malformed("representation sets nest too deep".into()));
        }
        let t = self.r.unsigned_integer()?;
        let hidden = self.base_with_graphics()?;
        let cs = self.r.unsigned_integer()?;
        let tess = self.r.unsigned_integer()?;
        let mut path = local.to_vec();
        if cs != 0 {
            path.push(cs);
        }
        let exact = |c: &mut Self| -> Result<(), PrcError> {
            if c.r.bit()? {
                c.r.unsigned_integer()?;
                c.r.unsigned_integer()?;
            }
            Ok(())
        };
        let mut drawable = true;
        match t {
            RI_BREP_MODEL => {
                exact(self)?;
                self.r.bit()?; // is_closed
            }
            RI_CURVE | RI_PLANE => exact(self)?,
            RI_DIRECTION => {
                if self.r.bit()? {
                    self.vector3()?;
                }
                self.vector3()?;
                drawable = false;
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
            RI_SET => {
                let n = self.count(1, "representation set")?;
                let mut members = Vec::new();
                for _ in 0..n {
                    self.representation_item(&path, &mut members, depth + 1)?;
                }
                if !hidden {
                    out.extend(members);
                }
                drawable = false;
            }
            RI_COORDINATE_SYSTEM => {
                self.transformation()?;
                drawable = false;
            }
            t => {
                return Err(malformed(format!(
                    "entity type {t} as a representation item"
                )));
            }
        }
        self.schema.skip_added_fields(t, &mut self.r)?;
        self.user_data()?;
        if drawable && !hidden && tess != 0 {
            out.push(Item {
                local: path,
                tessellation: tess,
            });
        }
        Ok(())
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

    /// `MISC_EntityReference` (203) [WD 7.4.4, 7.4.10; PRCRS].
    fn entity_reference(&mut self) -> Result<(), PrcError> {
        self.expect_type(ENTITY_REFERENCE)?;
        self.base_with_graphics()?;
        self.r.unsigned_integer()?; // local coordinate system
        if self.r.bit()? {
            match self.r.unsigned_integer()? {
                REFERENCE_ON_PRC_BASE => {
                    self.r.unsigned_integer()?;
                    if !self.r.bit()? {
                        self.unique_id()?;
                    }
                    self.r.unsigned_integer()?;
                }
                REFERENCE_ON_TOPOLOGY => {
                    self.r.unsigned_integer()?;
                    if self.r.bit()? {
                        if !self.r.bit()? {
                            self.unique_id()?;
                        }
                        self.r.unsigned_integer()?;
                        self.r.unsigned_integer()?;
                        let n = self.count(1, "topology reference indices")?;
                        for _ in 0..n {
                            self.r.unsigned_integer()?;
                        }
                    }
                }
                t => return Err(malformed(format!("entity type {t} as reference data"))),
            }
        }
        self.schema
            .skip_added_fields(ENTITY_REFERENCE, &mut self.r)?;
        self.user_data()
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

    fn part_definition(&mut self) -> Result<Vec<Item>, PrcError> {
        self.expect_type(PART_DEFINITION)?;
        let hidden = self.base_with_graphics()?;
        self.vector3()?; // bounding box
        self.vector3()?;
        let n = self.count(1, "representation items")?;
        let mut items = Vec::new();
        for _ in 0..n {
            self.representation_item(&[], &mut items, 0)?;
        }
        self.markup_data()?;
        self.views()?;
        self.schema
            .skip_added_fields(PART_DEFINITION, &mut self.r)?;
        self.user_data()?;
        if hidden {
            items.clear();
        }
        Ok(items)
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
        let mut p = Product {
            hidden: self.base_with_graphics()?,
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
        p.hidden |= self.r.character()? & SUPPRESSED != 0;
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
            self.entity_reference()?;
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

    /// `FileStructureGlobals` (303) read as far as its reference coordinate
    /// systems [WD 7.3.5, 7.3.5.2; PRCRS]; entry `i` is the matrix an item
    /// with biased local-CS index `i + 1` is placed by.
    ///
    /// Fonts, pictures, texture definitions and fill patterns are
    /// [`PrcError::Unsupported`]; colours, materials, texture applications,
    /// line patterns and styles are read past.
    pub(crate) fn coordinate_systems(&mut self) -> Result<Vec<Matrix>, PrcError> {
        self.expect_type(FILE_STRUCTURE_GLOBALS)?;
        self.content_prc_base()?;
        let n = self.count(4, "referenced file structures")?;
        for _ in 0..n {
            self.unique_id()?;
        }
        self.r.double()?; // tessellation chord-height ratio
        self.r.double()?; // tessellation angle
        self.r.string()?; // default font family
        if self.r.unsigned_integer()? != 0 {
            return Err(PrcError::Unsupported("PRC global fonts"));
        }
        let n = self.count(3, "colours")?;
        for _ in 0..n {
            self.vector3()?;
        }
        if self.r.unsigned_integer()? != 0 {
            return Err(PrcError::Unsupported("PRC global pictures"));
        }
        if self.r.unsigned_integer()? != 0 {
            return Err(PrcError::Unsupported("PRC texture definitions"));
        }
        let n = self.count(1, "materials")?;
        for _ in 0..n {
            self.material()?;
        }
        let n = self.count(1, "line patterns")?;
        for _ in 0..n {
            self.expect_type(LINE_PATTERN)?;
            self.content_prc_ref_base()?;
            let k = self.count(1, "line pattern lengths")?;
            for _ in 0..k {
                self.r.double()?;
            }
            self.r.double()?; // start offset
            self.r.bit()?; // scale
            self.schema.skip_added_fields(LINE_PATTERN, &mut self.r)?;
        }
        let n = self.count(1, "styles")?;
        for _ in 0..n {
            self.style()?;
        }
        if self.r.unsigned_integer()? != 0 {
            return Err(PrcError::Unsupported("PRC fill patterns"));
        }
        let n = self.count(1, "reference coordinate systems")?;
        let mut systems = Vec::with_capacity(n);
        for _ in 0..n {
            self.expect_type(RI_COORDINATE_SYSTEM)?;
            self.base_with_graphics()?;
            self.r.unsigned_integer()?; // local CS + 1
            self.r.unsigned_integer()?; // tessellation + 1
            systems.push(self.transformation()?);
            self.schema
                .skip_added_fields(RI_COORDINATE_SYSTEM, &mut self.r)?;
            self.user_data()?;
        }
        self.schema
            .skip_added_fields(FILE_STRUCTURE_GLOBALS, &mut self.r)?;
        self.user_data()?;
        Ok(systems)
    }

    /// One `materials` entry: `Material` (702) or `TextureApplication` (711),
    /// type-tagged [WD 7.5.4, 7.5.6; PRCRS].
    fn material(&mut self) -> Result<(), PrcError> {
        let t = self.r.unsigned_integer()?;
        self.content_prc_ref_base()?;
        match t {
            MATERIAL => {
                for _ in 0..4 {
                    self.r.unsigned_integer()?; // ambient/diffuse/emissive/specular + 1
                }
                for _ in 0..5 {
                    self.r.double()?; // shininess, then the four alphas
                }
            }
            TEXTURE_APPLICATION => {
                for _ in 0..4 {
                    self.r.unsigned_integer()?;
                }
            }
            t => return Err(malformed(format!("entity type {t} as a material"))),
        }
        self.schema.skip_added_fields(t, &mut self.r)
    }

    /// `Style` (701) [WD 7.5.3; PRCRS].
    fn style(&mut self) -> Result<(), PrcError> {
        self.expect_type(STYLE)?;
        self.content_prc_ref_base()?;
        self.r.double()?; // line width
        self.r.bit()?; // is_vpicture
        self.r.unsigned_integer()?; // pattern + 1
        self.r.bit()?; // is_material
        self.r.unsigned_integer()?; // colour or material + 1
        for _ in 0..4 {
            // transparency, then rendering parameters 1-3
            if self.r.bit()? {
                self.r.character()?;
            }
        }
        self.schema.skip_added_fields(STYLE, &mut self.r)
    }

    /// `FileStructureTree` (304) [WD 7.3.6].
    pub(crate) fn file_structure_tree(&mut self) -> Result<Tree, PrcError> {
        self.expect_type(FILE_STRUCTURE_TREE)?;
        self.content_prc_base()?;
        let n = self.count(1, "part definitions")?;
        let mut tree = Tree::default();
        for _ in 0..n {
            tree.parts.push(self.part_definition()?);
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

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The occurrence walk over every file structure's tree [WD 7.3.10.1,
/// 7.6.3.2; `prc__8137__model_tree_asm.md` §9].
pub(crate) struct Walk<'t> {
    /// Per file structure: its id, tree and reference coordinate systems.
    pub(crate) trees: Vec<(UniqueId, &'t Tree, &'t [Matrix])>,
    pub(crate) out: Vec<Placement>,
    visits: usize,
}

impl<'t> Walk<'t> {
    /// A walk over `trees`, with no placements yet.
    pub(crate) fn new(trees: Vec<(UniqueId, &'t Tree, &'t [Matrix])>) -> Self {
        Walk {
            trees,
            out: Vec::new(),
            visits: 0,
        }
    }

    fn fs(&self, id: UniqueId) -> Result<usize, PrcError> {
        self.trees
            .iter()
            .position(|t| t.0 == id)
            .ok_or_else(|| malformed("reference to an unknown file structure".into()))
    }

    fn product(&self, fs: usize, index: usize) -> Result<&'t Product, PrcError> {
        self.trees
            .get(fs)
            .and_then(|t| t.1.products.get(index))
            .ok_or_else(|| malformed(format!("product occurrence {index} does not exist")))
    }

    /// Draws occurrence `index` of structure `fs` under `father`.
    pub(crate) fn occurrence(
        &mut self,
        fs: usize,
        index: usize,
        father: &Matrix,
        depth: usize,
    ) -> Result<(), PrcError> {
        self.visits += 1;
        if depth > MAX_DEPTH || self.visits > MAX_VISITS {
            return Err(malformed(
                "the occurrence tree is too deep or too large".into(),
            ));
        }
        let p = self.product(fs, index)?;
        if p.hidden {
            return Ok(());
        }
        let m = match &p.location {
            Some(l) => multiply(father, l),
            None => *father,
        };
        // The part and sons, falling back to the prototype chain's for
        // whichever this occurrence leaves empty [WD 7.3.10.1].
        let (mut part, mut sons) = ((fs, p.part), (fs, &p.sons));
        let mut proto = (fs, p.prototype, p.prototype_fs);
        let mut hops = 0;
        while proto.1 != 0 && (part.1 == 0 || sons.1.is_empty()) {
            hops += 1;
            if hops > MAX_DEPTH {
                return Err(malformed("prototype chain too long".into()));
            }
            let pfs = match proto.2 {
                Some(id) => self.fs(id)?,
                None => proto.0,
            };
            let q = self.product(pfs, proto.1 as usize - 1)?;
            if part.1 == 0 {
                part = (pfs, q.part);
            }
            if sons.1.is_empty() {
                sons = (pfs, &q.sons);
            }
            proto = (pfs, q.prototype, q.prototype_fs);
        }
        if part.1 != 0 {
            self.part(part.0, part.1 as usize - 1, &m)?;
        }
        for &s in sons.1 {
            self.occurrence(sons.0, s as usize, &m, depth + 1)?;
        }
        if p.external != 0 {
            let efs = match p.external_fs {
                Some(id) => self.fs(id)?,
                None => fs,
            };
            self.occurrence(efs, p.external as usize - 1, &m, depth + 1)?;
        }
        Ok(())
    }

    fn part(&mut self, fs: usize, index: usize, m: &Matrix) -> Result<(), PrcError> {
        let (_, tree, systems) = self
            .trees
            .get(fs)
            .copied()
            .ok_or_else(|| malformed("file structure out of range".into()))?;
        let items = tree
            .parts
            .get(index)
            .ok_or_else(|| malformed(format!("part definition {index} does not exist")))?;
        for item in items {
            let mut placed = *m;
            for &cs in &item.local {
                let l = systems
                    .get(cs as usize - 1)
                    .ok_or_else(|| malformed(format!("coordinate system {cs} does not exist")))?;
                placed = multiply(&placed, l);
            }
            self.out.push(Placement {
                file_structure: fs,
                tessellation: item.tessellation as usize - 1,
                matrix: placed,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::Schema;
    use crate::bits::BitReader;
    use crate::testw::W;

    const FS: UniqueId = UniqueId([1, 2, 3, 4]);

    /// `ContentPRCBase`: no attributes, same name.
    fn base(w: &mut W) {
        w.uint(0).bit(true);
    }

    /// `PRCBaseWithGraphics`; `behaviour` `None` inherits the father's.
    /// `extra` writes the UInt a schema adds to type 2.
    fn graphics(w: &mut W, behaviour: Option<u16>, extra: bool) {
        base(w);
        w.uint(0).uint(0).uint(7);
        match behaviour {
            None => {
                w.bit(true);
            }
            Some(b) => {
                w.bit(false)
                    .uint(0)
                    .uint(0)
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

    /// A part with one poly-BRep item on tessellation 1, local CS `cs`.
    fn part(w: &mut W, cs: u32, extra: bool) {
        w.uint(PART_DEFINITION);
        graphics(w, None, extra);
        for c in [1.0, 0.0, 0.0, -1.0, 0.0, 0.0] {
            w.double(c);
        }
        w.uint(1).uint(RI_POLY_BREP_MODEL);
        graphics(w, None, extra);
        w.uint(cs).uint(1).bit(false).uint(0);
        no_markups_or_views(w);
        w.uint(0);
    }

    #[derive(Clone, Copy)]
    struct Occ<'a> {
        part: u32,
        sons: &'a [u32],
        behaviour: Option<u16>,
        suppressed: bool,
        location: Option<[f64; 3]>,
        mirror: bool,
        camera: bool,
    }

    const OCC: Occ<'static> = Occ {
        part: 0,
        sons: &[],
        behaviour: None,
        suppressed: false,
        location: None,
        mirror: false,
        camera: false,
    };

    fn occurrence(w: &mut W, o: &Occ<'_>, extra: bool) {
        w.uint(PRODUCT_OCCURRENCE);
        graphics(w, o.behaviour, extra);
        w.uint(o.part).uint(0).uint(0).uint(o.sons.len() as u32);
        for &s in o.sons {
            w.uint(s);
        }
        w.put(u64::from(o.suppressed), 8);
        w.bit(false).double(1.0).put(0, 8).int(0);
        w.bit(o.location.is_some());
        if let Some(t) = o.location {
            translation(w, t, o.mirror);
        }
        w.uint(0);
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
        let mut w = W::default();
        w.uint(FILE_STRUCTURE_TREE);
        base(&mut w);
        w.uint(1);
        part(&mut w, cs, extra);
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
        Ctx {
            r: BitReader::new(bytes),
            schema,
            version: 8137,
        }
    }

    fn place(occs: &[Occ<'_>], cs: u32, systems: &[Matrix]) -> Result<Vec<Placement>, PrcError> {
        let bytes = tree(occs, cs, false).bytes();
        let t = ctx(&bytes, &Schema::default()).file_structure_tree()?;
        let mut walk = Walk::new(vec![(FS, &t, systems)]);
        walk.occurrence(0, 0, &IDENTITY, 0)?;
        Ok(walk.out)
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
        let bytes = globals([0.0, 5.0, 0.0]).bytes();
        let systems = ctx(&bytes, &Schema::default())
            .coordinate_systems()
            .unwrap();
        assert_eq!(systems, [translate([0.0, 5.0, 0.0])]);
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
                behaviour: Some(0),
                ..shown
            },
            Occ {
                behaviour: Some(SHOW | REMOVED),
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
                    behaviour: Some(SHOW),
                    ..shown
                },
                hide,
            ];
            assert_eq!(place(&occs, 0, &[]).unwrap().len(), 1);
        }
    }

    #[test]
    fn a_camera_is_read_past() {
        let occs = [Occ {
            part: 1,
            camera: true,
            ..OCC
        }];
        assert_eq!(place(&occs, 0, &[]).unwrap().len(), 1);
    }

    #[test]
    fn a_schema_field_added_to_the_graphics_base_is_skipped() {
        let mut w = W::default();
        w.uint(1).uint(BASE_WITH_GRAPHICS).uint(1).uint(3);
        w.append(&tree(&[Occ { part: 1, ..OCC }], 0, true));
        let bytes = w.bytes();
        let mut r = BitReader::new(&bytes);
        let schema = Schema::read(&mut r).unwrap();
        let t = Ctx {
            r,
            schema: &schema,
            version: 8137,
        }
        .file_structure_tree()
        .unwrap();
        assert_eq!(t.parts[0][0].tessellation, 1);
        assert_eq!(t.root, 1);
    }

    #[test]
    fn a_cycle_is_refused() {
        let occs = [Occ { sons: &[0], ..OCC }];
        assert!(matches!(place(&occs, 0, &[]), Err(PrcError::Malformed(_))));
    }

    #[test]
    fn a_missing_reference_system_is_refused() {
        let occs = [Occ { part: 1, ..OCC }];
        assert!(matches!(place(&occs, 1, &[]), Err(PrcError::Malformed(_))));
    }

    /// A two-occurrence assembly of the unit square: once in place, once
    /// mirrored in x and moved 5 along it. The CLI's placement fixture.
    fn assembly_prc() -> Vec<u8> {
        let square =
            crate::PrcFile::parse(include_bytes!("../../../fixtures/synthetic/prc/square.prc"))
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
        crate::testw::prc_container(
            &schema,
            &tree(&occs, 0, false).bytes(),
            tess,
            &model.bytes(),
        )
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
}
