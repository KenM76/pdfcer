//! The tessellation section: `FileStructureTessellation` (305) and its
//! uncompressed entities, `TESS_3D` (172) with its `TESS_Face`s (174),
//! `TESS_3D_Wire` (175) and `TESS_Markup` (176) [WD 7.3.7, 7.8].
//!
//! `TESS_3D_Compressed` (173) is read to its end [WD 7.8.9.7] so the
//! entities after it decode, and its triangles are rebuilt where the
//! traversal in `compressed` fits the arrays exactly.
//!
//! The schema runs after the fields of each concrete type read here (201,
//! 172, 174, 175, 176, 305). It does not run for the abstract levels
//! (`PRCBase`, `TESS`): the sources name no producer extending them.

use crate::PrcError;
use crate::arrays;
use crate::bits::BitReader;
use crate::schema::Schema;

const ATTRIBUTE: u32 = 201;
const FILE_STRUCTURE_TESSELLATION: u32 = 305;
const TESS_3D: u32 = 172;
const TESS_3D_COMPRESSED: u32 = 173;
const TESS_FACE: u32 = 174;
const TESS_3D_WIRE: u32 = 175;
const TESS_MARKUP: u32 = 176;

/// `origin_array` exists from this version [PRCRS; pdf-issues #705].
const ORIGIN_FROM: u32 = 7031;

/// `has_loops` exists from this file-structure authoring version [PRCRS].
const HAS_LOOPS_FROM: u32 = 7039;
/// `must_recalculate_normals` exists from this version [PRCRS].
const RECALCULATE_FROM: u32 = 7047;

/// A fan/strip count's point-count bits; bit 30 is `NORMAL_Single`
/// [WD 7.8.6.2].
const COUNT_MASK: u32 = 0x3FFF_FFFF;
const NORMAL_SINGLE: u32 = 0x4000_0000;

/// A wire header's count bits; the top four are flags [WD 7.8.7.5].
const WIRE_COUNT_MASK: u32 = 0x0FFF_FFFF;
const WIRE_IS_CLOSING: u32 = 0x1000_0000;
const WIRE_IS_CONTINUOUS: u32 = 0x2000_0000;

/// One entry of the tessellation array; a representation item's
/// `index_tessellation` selects one by position.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Tessellation {
    /// A `TESS_3D` triangle mesh.
    Mesh(TriangleMesh),
    /// A `TESS_3D_Wire`: polylines.
    Wire(Vec<Vec<[f64; 3]>>),
    /// A `TESS_Markup` (PMI drawing), read past but not decoded.
    Markup,
    /// A `TESS_3D_Compressed` mesh.
    #[non_exhaustive]
    Compressed {
        /// The entity's triangle count.
        triangles: usize,
        /// The rebuilt triangles, or `None` where the arrays do not fit
        /// the reconstruction (the one-status-per-triangle edge form, or
        /// arrays left over). The positions are rebuilt to within the
        /// entity's tolerance per step, so they carry the producer's
        /// quantisation drift; faces are not separated.
        mesh: Option<TriangleMesh>,
        /// Why `mesh` is `None`, in a sentence a shell can print; `None`
        /// when the mesh was rebuilt.
        not_rebuilt: Option<String>,
    },
}

/// A `TESS_3D` decoded to indexed triangles, in the entity's own frame (the
/// representation item's placement is not applied).
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct TriangleMesh {
    /// The coordinate array as points.
    pub positions: Vec<[f64; 3]>,
    /// Counter-clockwise triangles seen from the outside, as indices into
    /// [`Self::positions`] [WD 7.8.5.2].
    pub triangles: Vec<[u32; 3]>,
    /// `triangles` split by `TESS_Face`: face `i` is
    /// `triangles[faces[i].clone()]`.
    pub faces: Vec<std::ops::Range<usize>>,
    /// The producer asks the reader to compute normals (none are stored).
    pub normals_recalculated: bool,
    /// The stored vertex normals [WD 7.8.5.1], as the producer wrote them
    /// (not renormalised); empty when the mesh stores none or any of its
    /// normal indices is unusable.
    pub normals: Vec<[f64; 3]>,
    /// Per triangle, the index into [`Self::normals`] of each corner's
    /// normal, in the corner order of [`Self::triangles`]; empty exactly
    /// when [`Self::normals`] is.
    pub triangle_normals: Vec<[u32; 3]>,
    /// Per triangle, the graphics its face's line attributes give it
    /// [WD 7.8.6]; empty when no face carries any. Read through
    /// [`crate::Placement::triangle_colours`].
    pub(crate) triangle_graphics: Vec<crate::tree::Graphics>,
}

pub(crate) struct Ctx<'a, 's> {
    pub(crate) r: BitReader<'a>,
    pub(crate) schema: &'s Schema,
    /// The file structure's authoring version; gates version-added fields.
    pub(crate) version: u32,
    /// The current graphics: what a `same_graphics` entity reuses [WD 5.4].
    /// A fresh `Ctx` starts with none, as each section does.
    pub(crate) graphics: crate::tree::Graphics,
    /// The current name: what a `same_name` entity reuses; every entity's
    /// own name replaces it [WD 7.2.3.4].
    pub(crate) name: Option<String>,
}

impl<'a, 's> Ctx<'a, 's> {
    /// A reader at `r` with no current graphics or name: each section
    /// starts afresh.
    pub(crate) fn new(r: BitReader<'a>, schema: &'s Schema, version: u32) -> Self {
        Ctx {
            r,
            schema,
            version,
            graphics: crate::tree::Graphics::default(),
            name: None,
        }
    }
}

/// A [`PrcError::Malformed`] naming `what`.
pub(crate) fn malformed(what: String) -> PrcError {
    PrcError::Malformed(what)
}

impl Ctx<'_, '_> {
    /// Reads an entity type code; any value but `want` is malformed.
    pub(crate) fn expect_type(&mut self, want: u32) -> Result<(), PrcError> {
        let t = self.r.unsigned_integer()?;
        if t != want {
            return Err(malformed(format!("entity type {t} where {want} belongs")));
        }
        Ok(())
    }

    /// A count that must fit in the remaining data at `min_bits` per item.
    pub(crate) fn count(&mut self, min_bits: usize, what: &'static str) -> Result<usize, PrcError> {
        let n = self.r.unsigned_integer()? as usize;
        if n.saturating_mul(min_bits) > self.r.remaining() {
            return Err(PrcError::Truncated(what));
        }
        Ok(n)
    }

    fn uints(&mut self, what: &'static str) -> Result<Vec<u32>, PrcError> {
        let n = self.count(1, what)?;
        (0..n).map(|_| self.r.unsigned_integer()).collect()
    }

    fn doubles(&mut self, what: &'static str) -> Result<Vec<f64>, PrcError> {
        let n = self.count(2, what)?;
        (0..n).map(|_| self.r.double()).collect()
    }

    /// `AttributeEntry` [WD 7.4.3]: a predefined title code or a string.
    fn attribute_entry(&mut self) -> Result<(), PrcError> {
        if self.r.bit()? {
            self.r.unsigned_integer()?;
        } else {
            self.r.string()?;
        }
        Ok(())
    }

    /// `ContentPRCBase` [WD 7.2.3]: attributes, then the name; returns the
    /// entity's name, which is also the current name afterwards.
    pub(crate) fn content_prc_base(&mut self) -> Result<Option<String>, PrcError> {
        let n = self.count(1, "attributes")?;
        for _ in 0..n {
            self.expect_type(ATTRIBUTE)?;
            self.attribute_entry()?;
            let pairs = self.count(2, "attribute pairs")?;
            for _ in 0..pairs {
                self.attribute_entry()?;
                match self.r.unsigned_integer()? {
                    0 => {}
                    1 | 3 => {
                        self.r.integer()?;
                    }
                    2 => {
                        self.r.double()?;
                    }
                    4 => {
                        self.r.string()?;
                    }
                    // A 64-bit integer: high part, then low [PRCRS].
                    5 => {
                        self.r.integer()?;
                        self.r.unsigned_integer()?;
                    }
                    k => return Err(malformed(format!("attribute value kind {k}"))),
                }
            }
            self.schema.skip_added_fields(ATTRIBUTE, &mut self.r)?;
        }
        // Name: `same_name` TRUE reuses the current name [WD 7.2.3.4].
        if !self.r.bit()? {
            self.name = self.r.string()?;
        }
        Ok(self.name.clone())
    }

    /// `UserData` [WD 8.6]: a bit count, then that many opaque bits.
    pub(crate) fn user_data(&mut self) -> Result<(), PrcError> {
        let n = self.r.unsigned_integer()? as usize;
        self.r.skip_bits(n)
    }

    /// `VertexColors` [WD 7.8.7.2], for `count` points; `is_segment_color`
    /// exists only inside a wire [PRCRS].
    fn vertex_colors(&mut self, count: usize, in_wire: bool) -> Result<(), PrcError> {
        let rgba = self.r.bit()?;
        let per_segment = in_wire && self.r.bit()?;
        if self.r.bit()? {
            return Err(PrcError::Unsupported("optimised vertex colours"));
        }
        let count = if per_segment { count / 2 } else { count };
        let bytes = if rgba { 4 } else { 3 };
        if count.saturating_mul(1 + 8 * bytes) > self.r.remaining().saturating_add(8 * bytes) {
            return Err(PrcError::Truncated("vertex colours"));
        }
        for i in 0..count {
            if i == 0 || !self.r.bit()? {
                for _ in 0..bytes {
                    self.r.character()?;
                }
            }
        }
        Ok(())
    }

    /// `FileStructureTessellation` (305) [WD 7.3.7].
    pub(crate) fn file_structure_tessellation(&mut self) -> Result<Vec<Tessellation>, PrcError> {
        self.expect_type(FILE_STRUCTURE_TESSELLATION)?;
        self.content_prc_base()?;
        let n = self.count(1, "tessellations")?;
        let mut out = Vec::with_capacity(n.min(1024));
        for _ in 0..n {
            let t = self.r.unsigned_integer()?;
            out.push(match t {
                TESS_3D => Tessellation::Mesh(self.tess_3d()?),
                TESS_3D_WIRE => Tessellation::Wire(self.wire()?),
                TESS_MARKUP => {
                    self.markup()?;
                    Tessellation::Markup
                }
                TESS_3D_COMPRESSED => {
                    let (triangles, rebuilt) = self.tess_3d_compressed()?;
                    let (mesh, not_rebuilt) = match rebuilt {
                        Ok(m) => (Some(m), None),
                        Err(why) => (None, Some(why)),
                    };
                    Tessellation::Compressed {
                        triangles,
                        mesh,
                        not_rebuilt,
                    }
                }
                t => {
                    return Err(malformed(format!(
                        "entity type {t} in the tessellation array"
                    )));
                }
            });
        }
        self.schema
            .skip_added_fields(FILE_STRUCTURE_TESSELLATION, &mut self.r)?;
        self.user_data()?;
        Ok(out)
    }

    /// `ContentBaseTessData` [WD 7.8.4]: `is_calculated`, then coordinates.
    fn base_tess_data(&mut self) -> Result<Vec<f64>, PrcError> {
        self.r.bit()?;
        self.doubles("tessellation coordinates")
    }

    /// `TESS_3D_Compressed` (173) after its type code, field by field
    /// [WD 7.8.9.7; PRCRS prc.json]; returns the triangle count and the
    /// rebuilt mesh, if it rebuilds. Array
    /// sizes the WD leaves unstated follow `prc__8137__tess_3d_compressed.md`
    /// §1: `face_number` = largest face index + 1, `normal_is_reversed` is
    /// one bit per triangle, and `point_reference_array`'s compressed flag
    /// is implicit (more than three references).
    fn tess_3d_compressed(&mut self) -> Result<(usize, Result<TriangleMesh, String>), PrcError> {
        let r = &mut self.r;
        r.bit()?; // is_calculated
        r.bit()?; // has_faces
        let tol = r.double()?; // tolerance
        let mut origin = [0.0f64; 3];
        if self.version >= ORIGIN_FROM {
            for o in &mut origin {
                *o = f64::from(r.float_as_bytes()?);
            }
        }
        let point_array = arrays::compressed_integer_array(r)?; // point_array
        let edge_status = arrays::character_array(r, 2, None, false)?;
        let face_of = arrays::compressed_indice_array(r, None)?;
        let t = face_of.len();
        if edge_status.len() != t && edge_status.len() != t.saturating_mul(3) {
            return Err(malformed(format!(
                "{} edge statuses for {t} triangles",
                edge_status.len()
            )));
        }
        let faces = face_of.iter().max().map_or(0, |&m| m as usize + 1);
        let n = r.unsigned_integer()? as usize;
        let is_ref = arrays::bool_array(r, n)?;
        let references = is_ref.iter().filter(|&&b| b).count();
        let refs = arrays::compressed_indice_array(r, Some(references > 3))?;
        if refs.len() != references {
            return Err(malformed(format!(
                "{} point references for {references} flagged points",
                refs.len()
            )));
        }
        let mut stored = None;
        let recalc = r.bit()?; // must_recalculate_normals
        if recalc {
            arrays::bool_array(r, t)?; // normal_is_reversed
            r.double()?; // crease_angle
            r.character()?; // normal_recalculation_flags
        } else {
            let angle_bits = u32::from(r.character()?);
            if angle_bits > 16 {
                return Err(malformed(format!("{angle_bits}-bit normal angles")));
            }
            let n = r.unsigned_integer()? as usize;
            let binary = arrays::bool_array(r, n)?;
            let angles = arrays::short_array(r, angle_bits)?;
            let planar = arrays::bool_array(r, faces)?;
            stored = Some((angle_bits, binary, angles, planar));
        }
        if r.bit()? {
            arrays::bool_array(r, faces)?; // is_point_color_on_face
            arrays::character_array(r, 8, None, false)?; // point_color_array
        }
        let mut multi = Vec::new();
        if r.bit()? {
            multi = arrays::bool_array(r, faces)?; // is_multiple_line_attribute_on_face
        }
        let line_attributes = arrays::short_array(r, 16)?;
        if !r.bit()? {
            self.compressed_texture_parameter()?;
            if !self.r.bit()? {
                arrays::bool_array(&mut self.r, faces)?; // face_has_texture
            }
        }
        let mut behaviours = Vec::new();
        if self.r.bit()? {
            behaviours = arrays::character_array(&mut self.r, 8, None, false)?;
        }
        self.schema
            .skip_added_fields(TESS_3D_COMPRESSED, &mut self.r)?;
        let mesh = crate::compressed::reconstruct(&crate::compressed::Arrays {
            tolerance: tol,
            origin,
            points: &point_array,
            edge_status: &edge_status,
            triangles: t,
            is_reference: &is_ref,
            references: &refs,
            normals: stored.as_ref().map(|(bits, binary, angles, planar)| {
                crate::compressed::NormalArrays {
                    bits: *bits,
                    binary,
                    angles,
                    planar,
                    face_of: &face_of,
                }
            }),
        })
        .map(|m| {
            let graphics =
                compressed_graphics(&face_of, &multi, &line_attributes, &behaviours, faces);
            TriangleMesh {
                normals_recalculated: recalc,
                triangle_graphics: if graphics.len() == m.triangles.len() {
                    graphics
                } else {
                    Vec::new()
                },
                ..m
            }
        });
        Ok((t, mesh))
    }

    /// `CompressedTextureParameter` [WD 7.8.9.8-7.8.9.9; pdf-issues #729,
    /// #749].
    fn compressed_texture_parameter(&mut self) -> Result<(), PrcError> {
        let words = self.count(32, "texture data")?;
        self.r.skip_bits(words * 32)?;
        self.r.unsigned_integer()?; // last_integer_used_bit_number
        let n = self.count(5, "texture references")?;
        for _ in 0..n {
            self.r.nbits_then_unsigned()?;
        }
        self.r.double()?; // tolerance
        let n = self.count(32, "texture parameters")?;
        for _ in 0..n {
            self.r.float_as_bytes()?;
        }
        Ok(())
    }

    /// `TESS_3D` (172) after its type code [WD 7.8.5].
    fn tess_3d(&mut self) -> Result<TriangleMesh, PrcError> {
        let coords = self.base_tess_data()?;
        let _has_faces = self.r.bit()?;
        if self.version >= HAS_LOOPS_FROM {
            let _has_loops = self.r.bit()?;
        }
        let recalc = self.version >= RECALCULATE_FROM && self.r.bit()?;
        if recalc {
            let _flags = self.r.character()?;
            let _crease_angle = self.r.double()?;
        }
        let normal_coords = self.doubles("normal coordinates")?;
        let _wire_indices = self.uints("wire indices")?;
        let indices = self.uints("triangulated indices")?;
        let n_faces = self.count(1, "faces")?;
        let mut faces = Vec::with_capacity(n_faces.min(1024));
        for _ in 0..n_faces {
            self.expect_type(TESS_FACE)?;
            faces.push(self.face()?);
        }
        let _texture = self.doubles("texture coordinates")?;
        self.schema.skip_added_fields(TESS_3D, &mut self.r)?;

        let positions = points(&coords);
        let mut mesh = TriangleMesh {
            positions,
            normals_recalculated: recalc,
            ..TriangleMesh::default()
        };
        let styled = faces.iter().any(|f| !f.styles.is_empty());
        let mut corner_slots = Vec::new();
        for f in &faces {
            let start = mesh.triangles.len();
            let entities = triangulate(
                f,
                &indices,
                !recalc,
                mesh.positions.len(),
                &mut mesh.triangles,
                &mut corner_slots,
            )?;
            mesh.faces.push(start..mesh.triangles.len());
            if styled {
                face_graphics(f, &entities, &mut mesh.triangle_graphics);
            }
        }
        (mesh.normals, mesh.triangle_normals) =
            stored_normals(&normal_coords, &corner_slots, mesh.triangles.len());
        Ok(mesh)
    }

    /// `TESS_Face` (174) after its type code [WD 7.8.6].
    fn face(&mut self) -> Result<Face, PrcError> {
        let line_attributes = self.uints("face line attributes")?;
        let _start_of_wire = self.r.unsigned_integer()?;
        let _sizes_wire = self.uints("face wire sizes")?;
        let flags = self.r.unsigned_integer()?;
        let start = self.r.unsigned_integer()? as usize;
        let data = self.uints("triangulated data")?;
        let textures = self.r.unsigned_integer()? as usize;
        let mut face = Face {
            flags,
            start,
            data,
            textures,
            styles: line_attributes,
            behaviour: 0,
        };
        if self.r.bit()? {
            let n = face.point_count()?;
            self.vertex_colors(n, false)?;
        }
        if !face.styles.is_empty() {
            face.behaviour = u16::try_from(self.r.unsigned_integer()?).unwrap_or(0);
        }
        self.schema.skip_added_fields(TESS_FACE, &mut self.r)?;
        Ok(face)
    }

    /// `TESS_3D_Wire` (175) after its type code [WD 7.8.7].
    fn wire(&mut self) -> Result<Vec<Vec<[f64; 3]>>, PrcError> {
        let pts = points(&self.base_tess_data()?);
        let words = self.uints("wire indices")?;
        let mut wires: Vec<Vec<[f64; 3]>> = Vec::new();
        let mut colours = 0usize;
        if words.is_empty() {
            colours = pts.len();
            if !pts.is_empty() {
                wires.push(pts.clone());
            }
        }
        let mut it = words.iter().copied();
        while let Some(header) = it.next() {
            let n = (header & WIRE_COUNT_MASK) as usize;
            let mut wire = Vec::with_capacity(n.min(words.len()));
            for _ in 0..n {
                let idx = it
                    .next()
                    .ok_or_else(|| malformed("wire runs past its indices".into()))?;
                wire.push(point_at(&pts, idx)?);
            }
            colours += n;
            if header & WIRE_IS_CLOSING != 0 {
                colours += 1;
                if let Some(&first) = wire.first() {
                    wire.push(first);
                }
            }
            match wires.last_mut() {
                Some(prev) if header & WIRE_IS_CONTINUOUS != 0 => prev.extend(wire),
                _ => wires.push(wire),
            }
        }
        if self.r.bit()? {
            self.vertex_colors(colours, true)?;
        }
        self.schema.skip_added_fields(TESS_3D_WIRE, &mut self.r)?;
        Ok(wires)
    }

    /// `TESS_Markup` (176) after its type code [WD 7.8.8], read past.
    fn markup(&mut self) -> Result<(), PrcError> {
        self.base_tess_data()?;
        self.uints("markup codes")?;
        let n = self.count(1, "markup strings")?;
        for _ in 0..n {
            self.r.string()?;
        }
        self.r.string()?;
        self.r.character()?;
        self.schema.skip_added_fields(TESS_MARKUP, &mut self.r)
    }
}

/// The normal array and each triangle's corner normals, or both empty when
/// any slot fails to name a normal: a mesh shades from its stored normals
/// wholly or not at all.
fn stored_normals(
    coords: &[f64],
    slots: &[[u32; 3]],
    triangles: usize,
) -> (Vec<[f64; 3]>, Vec<[u32; 3]>) {
    let normals = points(coords);
    let named = |k: u32| k.is_multiple_of(3) && (k as usize / 3) < normals.len();
    if normals.is_empty() || slots.len() != triangles || !slots.iter().flatten().all(|&k| named(k))
    {
        return (Vec::new(), Vec::new());
    }
    let corners = slots.iter().map(|t| t.map(|k| k / 3)).collect();
    (normals, corners)
}

fn points(coords: &[f64]) -> Vec<[f64; 3]> {
    coords
        .chunks_exact(3)
        .map(|c| match c {
            [x, y, z] => [*x, *y, *z],
            _ => [0.0; 3],
        })
        .collect()
}

/// The point a Double-offset index names [WD 7.8.5.2: multiples of 3].
fn point_at(pts: &[[f64; 3]], idx: u32) -> Result<[f64; 3], PrcError> {
    if !idx.is_multiple_of(3) {
        return Err(malformed(format!(
            "point index {idx} is not a multiple of 3"
        )));
    }
    pts.get(idx as usize / 3)
        .copied()
        .ok_or_else(|| malformed(format!("point index {idx} past the coordinates")))
}

struct Face {
    flags: u32,
    start: usize,
    data: Vec<u32>,
    textures: usize,
    /// `line_attributes`, each a style index + 1: none = the owner's
    /// graphics, one = the whole face's, more = one per triangulation
    /// entity [WD 7.8.6].
    styles: Vec<u32>,
    /// The behaviour bits read when `styles` is not empty.
    behaviour: u16,
}

/// Append one [`Graphics`](crate::tree::Graphics) per triangle of `face`,
/// whose entities emitted `entities[i]` triangles each. Entities past the
/// face's styles inherit (style 0).
fn face_graphics(face: &Face, entities: &[usize], out: &mut Vec<crate::tree::Graphics>) {
    let g = |style: u32| crate::tree::Graphics {
        style,
        bits: face.behaviour,
        fs: 0,
    };
    let whole = match face.styles.as_slice() {
        [] => Some(0),
        [one] => Some(*one),
        _ => None,
    };
    for (i, &n) in entities.iter().enumerate() {
        let style = whole.unwrap_or_else(|| face.styles.get(i).copied().unwrap_or(0));
        out.extend(std::iter::repeat_n(g(style), n));
    }
}

/// Per-triangle graphics of a compressed tessellation [WD 7.8.9.5]: with no
/// face flagged `is_multiple_line_attribute_on_face`, `line_attributes` holds
/// one style index + 1 per face (0 = the owner's graphics) and `behaviours`
/// one behaviour byte per face. The one-per-triangle layout of a flagged face
/// is not established, so such a tessellation keeps its owner's graphics, as
/// does one whose attributes are all 0.
fn compressed_graphics(
    face_of: &[u32],
    multi: &[bool],
    line_attributes: &[i32],
    behaviours: &[i32],
    faces: usize,
) -> Vec<crate::tree::Graphics> {
    if multi.iter().any(|&b| b)
        || line_attributes.len() != faces
        || line_attributes.iter().all(|&a| a == 0)
    {
        return Vec::new();
    }
    let bits = |f: usize| {
        if behaviours.len() == faces {
            behaviours
                .get(f)
                .and_then(|&b| u16::try_from(b).ok())
                .unwrap_or(0)
        } else {
            0
        }
    };
    face_of
        .iter()
        .map(|&f| {
            let f = f as usize;
            crate::tree::Graphics {
                style: line_attributes
                    .get(f)
                    .and_then(|&a| u32::try_from(a).ok())
                    .unwrap_or(0),
                bits: bits(f),
                fs: 0,
            }
        })
        .collect()
}

/// A `used_entities_flag` block's shape [WD 7.8.5.5].
#[derive(Clone, Copy)]
enum Shape {
    Triangles,
    Fan,
    Strip,
}

/// Per block: `(bit, shape, one normal per entity, textured)`, low bit first.
const BLOCKS: [(u32, Shape, bool, bool); 12] = [
    (0x0002, Shape::Triangles, false, false),
    (0x0004, Shape::Fan, false, false),
    (0x0008, Shape::Strip, false, false),
    (0x0020, Shape::Triangles, true, false),
    (0x0040, Shape::Fan, true, false),
    (0x0080, Shape::Strip, true, false),
    (0x0200, Shape::Triangles, false, true),
    (0x0400, Shape::Fan, false, true),
    (0x0800, Shape::Strip, false, true),
    (0x2000, Shape::Triangles, true, true),
    (0x4000, Shape::Fan, true, true),
    (0x8000, Shape::Strip, true, true),
];

impl Face {
    /// Entity sizes per set block: `(block, [points per entity])`, from
    /// `TriangulatedData` [WD 7.8.6]. A triangle block is one entity of
    /// `3 × count` points; fans and strips list a vertex count each.
    fn entities(&self) -> Result<Vec<(usize, Vec<u32>)>, PrcError> {
        let mut d = self.data.iter().copied();
        let mut next = || {
            d.next()
                .ok_or_else(|| malformed("triangulated data ends early".into()))
        };
        let mut out = Vec::new();
        for (i, (bit, shape, ..)) in BLOCKS.iter().enumerate() {
            if self.flags & bit == 0 {
                continue;
            }
            let sizes = match shape {
                Shape::Triangles => vec![next()?],
                Shape::Fan | Shape::Strip => {
                    let n = next()? as usize;
                    if n > self.data.len() {
                        return Err(malformed("triangulated data ends early".into()));
                    }
                    (0..n).map(|_| next()).collect::<Result<_, _>>()?
                }
            };
            out.push((i, sizes));
        }
        Ok(out)
    }

    /// Points the face's index slots name: the vertex-colour count
    /// [ISS #820].
    fn point_count(&self) -> Result<usize, PrcError> {
        let mut n = 0usize;
        for (i, sizes) in self.entities()? {
            let tri = matches!(BLOCKS.get(i), Some((_, Shape::Triangles, ..)));
            for s in sizes {
                let s = (s & COUNT_MASK) as usize;
                n = n.saturating_add(if tri { s.saturating_mul(3) } else { s });
            }
        }
        Ok(n)
    }
}

/// Emit a face's triangles; `normals` = the index array stores normal slots,
/// in which case each triangle's corner normal slots go to `corner_normals`.
/// Returns the triangles each triangulation entity emitted, in order: one
/// entity per triangle of a triangle block, per fan, per strip [WD 7.8.6].
fn triangulate(
    face: &Face,
    indices: &[u32],
    normals: bool,
    n_points: usize,
    out: &mut Vec<[u32; 3]>,
    corner_normals: &mut Vec<[u32; 3]>,
) -> Result<Vec<usize>, PrcError> {
    let mut entities = Vec::new();
    let mut slots = indices.get(face.start..).unwrap_or(&[]).iter().copied();
    let mut take = || {
        slots
            .next()
            .ok_or_else(|| malformed("face runs past the triangulated index array".into()))
    };
    for (i, sizes) in face.entities()? {
        let Some(&(_, shape, one_normal, textured)) = BLOCKS.get(i) else {
            continue;
        };
        let t = if textured { face.textures } else { 0 };
        for word in sizes {
            let count = (word & COUNT_MASK) as usize;
            // Points in this entity, and whether each point carries its own
            // normal slot.
            let (points, per_point_normal) = match shape {
                Shape::Triangles => (count.saturating_mul(3), !one_normal),
                Shape::Fan | Shape::Strip => (count, !one_normal || word & NORMAL_SINGLE == 0),
            };
            if points > indices.len() {
                return Err(malformed(
                    "face runs past the triangulated index array".into(),
                ));
            }
            let mut verts = Vec::with_capacity(points);
            let mut vnormals = Vec::with_capacity(if normals { points } else { 0 });
            let mut current = 0;
            for k in 0..points {
                let new_entity = match shape {
                    Shape::Triangles => k % 3 == 0,
                    _ => k == 0,
                };
                let normal_here = normals && if per_point_normal { true } else { new_entity };
                if normal_here {
                    current = take()?;
                }
                if normals {
                    vnormals.push(current);
                }
                for _ in 0..t {
                    take()?;
                }
                let p = take()?;
                if p % 3 != 0 || p as usize / 3 >= n_points {
                    return Err(malformed(format!("point index {p} is not a point")));
                }
                verts.push(p / 3);
            }
            let before = out.len();
            assemble(shape, &verts, out);
            assemble(shape, &vnormals, corner_normals);
            let made = out.len() - before;
            match shape {
                Shape::Triangles => entities.extend(std::iter::repeat_n(1, made)),
                Shape::Fan | Shape::Strip => entities.push(made),
            }
        }
    }
    Ok(entities)
}

/// Append the triangles `shape` makes of `verts`.
fn assemble(shape: Shape, verts: &[u32], out: &mut Vec<[u32; 3]>) {
    match shape {
        Shape::Triangles => out.extend(verts.chunks_exact(3).filter_map(|c| match c {
            [a, b, c] => Some([*a, *b, *c]),
            _ => None,
        })),
        Shape::Fan => {
            if let Some((&c, rim)) = verts.split_first() {
                out.extend(rim.windows(2).filter_map(|w| match w {
                    [a, b] => Some([c, *a, *b]),
                    _ => None,
                }));
            }
        }
        Shape::Strip => out.extend(verts.windows(3).enumerate().filter_map(|(k, w)| match w {
            [a, b, c] if k % 2 == 0 => Some([*a, *b, *c]),
            [a, b, c] => Some([*b, *a, *c]),
            _ => None,
        })),
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
mod tests {
    use super::*;
    use crate::testw::W;

    const SQUARE: [f64; 12] = [0., 0., 0., 1., 0., 0., 1., 1., 0., 0., 1., 0.];

    /// A 305 section holding `body` (already-written entities, `n` of them).
    fn section(n: u32, body: &W) -> W {
        let mut w = W::default();
        w.uint(305).uint(0).bit(true).uint(n).append(body).uint(0);
        w
    }

    fn uints(w: &mut W, v: &[u32]) {
        w.uint(v.len() as u32);
        for &x in v {
            w.uint(x);
        }
    }

    fn square(w: &mut W, entity: u32) {
        w.uint(entity).bit(false).uint(12);
        for c in SQUARE {
            w.double(c);
        }
    }

    struct Mesh<'a> {
        recalc: bool,
        indices: &'a [u32],
        faces: &'a [(u32, u32, &'a [u32])],
        textures: u32,
    }

    /// A TESS_3D over [`SQUARE`] at authoring version `v`.
    fn tess_3d(w: &mut W, v: u32, m: &Mesh<'_>) {
        square(w, 172);
        w.bit(true);
        if v >= HAS_LOOPS_FROM {
            w.bit(false);
        }
        if v >= RECALCULATE_FROM {
            w.bit(m.recalc);
            if m.recalc {
                w.put(0, 8).double(0.5);
            }
        }
        if m.recalc {
            w.uint(0);
        } else {
            // Two normals: slot 0 is +z, slot 3 is -z.
            w.uint(6);
            for c in [0., 0., 1., 0., 0., -1.] {
                w.double(c);
            }
        }
        w.uint(0);
        uints(w, m.indices);
        w.uint(m.faces.len() as u32);
        for &(flags, start, data) in m.faces {
            w.uint(174).uint(0).uint(0).uint(0).uint(flags).uint(start);
            uints(w, data);
            w.uint(m.textures).bit(false);
        }
        w.uint(0);
    }

    fn decode(w: &W, schema: &Schema, version: u32) -> Result<Vec<Tessellation>, PrcError> {
        let bytes = w.bytes();
        Ctx::new(BitReader::new(&bytes), schema, version).file_structure_tessellation()
    }

    fn mesh(t: &Tessellation) -> &TriangleMesh {
        match t {
            Tessellation::Mesh(m) => m,
            other => panic!("not a mesh: {other:?}"),
        }
    }

    fn one(indices: &[u32], faces: &[(u32, u32, &[u32])], textures: u32) -> TriangleMesh {
        let mut b = W::default();
        tess_3d(
            &mut b,
            8137,
            &Mesh {
                recalc: false,
                indices,
                faces,
                textures,
            },
        );
        mesh(&decode(&section(1, &b), &Schema::default(), 8137).unwrap()[0]).clone()
    }

    #[test]
    fn triangle_and_fan_with_normals() {
        // One triangle (n,p x3), then a fan of 4 vertices (n,p each).
        let m = one(
            &[3, 0, 0, 3, 3, 6, 0, 0, 3, 3, 0, 6, 3, 9],
            &[(0x2 | 0x4, 0, &[1, 1, 4])],
            0,
        );
        assert_eq!(m.positions.len(), 4);
        assert_eq!(m.positions[2], [1., 1., 0.]);
        assert_eq!(m.triangles, [[0, 1, 2], [0, 1, 2], [0, 2, 3]]);
        assert_eq!(m.faces, vec![std::ops::Range { start: 0, end: 3 }]);
        assert!(!m.normals_recalculated);
        assert_eq!(m.normals, [[0., 0., 1.], [0., 0., -1.]]);
        // Each corner keeps the normal stored beside its point.
        assert_eq!(m.triangle_normals, [[1, 0, 1], [0, 1, 0], [0, 0, 1]]);
    }

    #[test]
    fn strip_normals_follow_the_alternating_winding() {
        let m = one(&[3, 0, 0, 3, 3, 6, 0, 9], &[(0x8, 0, &[1, 4])], 0);
        assert_eq!(m.triangles, [[0, 1, 2], [2, 1, 3]]);
        assert_eq!(m.triangle_normals, [[1, 0, 1], [1, 0, 0]]);
    }

    #[test]
    fn a_normal_index_past_the_array_drops_every_normal_but_no_triangle() {
        let m = one(&[0, 0, 6, 3, 0, 6], &[(0x2, 0, &[1])], 0);
        assert_eq!(m.triangles, [[0, 1, 2]]);
        assert!(m.normals.is_empty());
        assert!(m.triangle_normals.is_empty());
    }

    #[test]
    fn recalculated_normals_drop_the_normal_slots_and_strips_alternate() {
        let mut b = W::default();
        tess_3d(
            &mut b,
            8137,
            &Mesh {
                recalc: true,
                indices: &[0, 3, 6, 9],
                faces: &[(0x8, 0, &[1, 4])],
                textures: 0,
            },
        );
        let t = decode(&section(1, &b), &Schema::default(), 8137).unwrap();
        let m = mesh(&t[0]);
        assert!(m.normals_recalculated);
        assert_eq!(m.triangles, [[0, 1, 2], [2, 1, 3]]);
        assert!(m.normals.is_empty() && m.triangle_normals.is_empty());
    }

    #[test]
    fn version_gates_the_loop_and_recalculation_flags() {
        for v in [7000, 7040, 8137] {
            let mut b = W::default();
            tess_3d(
                &mut b,
                v,
                &Mesh {
                    recalc: false,
                    indices: &[0, 0, 0, 3, 0, 6],
                    faces: &[(0x2, 0, &[1])],
                    textures: 0,
                },
            );
            // A trailing wire: a misread flag desyncs it.
            wire(&mut b, false);
            let t = decode(&section(2, &b), &Schema::default(), v).unwrap();
            assert_eq!(mesh(&t[0]).triangles, [[0, 1, 2]], "version {v}");
            assert!(matches!(t[1], Tessellation::Wire(_)), "version {v}");
        }
    }

    #[test]
    fn one_normal_blocks_and_textures() {
        // 0x20: n,p,p,p. 0x40 with NORMAL_Single: n then p per vertex.
        // 0x2000 with one texture set: n,t,p,t,p,t,p.
        let idx = [
            0, 0, 3, 6, // 0x20
            3, 0, 6, 9, // 0x40 single, 3 vertices
            0, 0, 9, 2, 0, 4, 3, // 0x2000
        ];
        let m = one(
            &idx,
            &[(0x20 | 0x40 | 0x2000, 0, &[1, 1, 3 | NORMAL_SINGLE, 1])],
            1,
        );
        assert_eq!(m.triangles, [[0, 1, 2], [0, 2, 3], [3, 0, 1]]);
        // A one-normal entity gives every corner its single normal.
        assert_eq!(m.triangle_normals, [[0, 0, 0], [1, 1, 1], [0, 0, 0]]);
    }

    #[test]
    fn one_normal_fan_without_single_keeps_a_normal_per_vertex() {
        let m = one(&[0, 0, 0, 3, 0, 6, 0, 9], &[(0x40, 0, &[1, 4])], 0);
        assert_eq!(m.triangles, [[0, 1, 2], [0, 2, 3]]);
    }

    #[test]
    fn textured_fan_and_strip_carry_texture_slots_per_vertex() {
        // 0x400 then 0x800, two texture sets: n,t,t,p per vertex.
        let mut idx = Vec::new();
        for p in [0, 3, 6, 0, 3, 6, 9] {
            idx.extend([0, 0, 2, p]);
        }
        let m = one(&idx, &[(0x400 | 0x800, 0, &[1, 3, 1, 4])], 2);
        assert_eq!(m.triangles, [[0, 1, 2], [0, 1, 2], [2, 1, 3]]);
    }

    #[test]
    fn two_faces_partition_the_triangles() {
        let m = one(
            &[0, 0, 0, 3, 0, 6, 0, 0, 0, 6, 0, 9],
            &[(0x2, 0, &[1]), (0x2, 6, &[1])],
            0,
        );
        assert_eq!(m.triangles, [[0, 1, 2], [0, 2, 3]]);
        assert_eq!(m.faces, [0..1, 1..2]);
    }

    /// A wire over [`SQUARE`]: an open two-point wire continued by a closing
    /// two-point wire, then optionally per-segment RGB colours.
    fn wire(w: &mut W, colours: bool) {
        square(w, 175);
        uints(
            w,
            &[2, 0, 3, WIRE_IS_CONTINUOUS | WIRE_IS_CLOSING | 2, 6, 9],
        );
        w.bit(colours);
        if colours {
            // 5 points (4 + the implicit closing one) -> 2 segment colours.
            w.bit(false).bit(true).bit(false).put(0xff0000, 24);
            w.bit(true);
        }
    }

    #[test]
    fn wires_close_and_continue_and_colours_are_counted() {
        for colours in [false, true] {
            let mut b = W::default();
            wire(&mut b, colours);
            // A second entity: a wrong colour count would desync it.
            wire(&mut b, false);
            let t = decode(&section(2, &b), &Schema::default(), 8137).unwrap();
            for e in &t {
                let Tessellation::Wire(w) = e else {
                    panic!("not a wire")
                };
                assert_eq!(
                    w,
                    &[vec![
                        [0., 0., 0.],
                        [1., 0., 0.],
                        [1., 1., 0.],
                        [0., 1., 0.],
                        [1., 1., 0.]
                    ]]
                );
            }
        }
    }

    #[test]
    fn a_closing_wire_colours_its_implicit_point() {
        // Per-point RGB: 4 indexed points + 1 implicit = 5 colours.
        let mut b = W::default();
        square(&mut b, 175);
        uints(
            &mut b,
            &[2, 0, 3, WIRE_IS_CONTINUOUS | WIRE_IS_CLOSING | 2, 6, 9],
        );
        b.bit(true).bit(false).bit(false).bit(false).put(1, 24);
        for _ in 1..5 {
            b.bit(true);
        }
        wire(&mut b, false);
        let t = decode(&section(2, &b), &Schema::default(), 8137).unwrap();
        assert!(matches!(t[1], Tessellation::Wire(_)));
        assert_eq!(t[0], t[1]);
    }

    #[test]
    fn a_wire_index_must_name_a_point() {
        let mut b = W::default();
        square(&mut b, 175);
        uints(&mut b, &[2, 0, 4]);
        b.bit(false);
        assert!(matches!(
            decode(&section(1, &b), &Schema::default(), 8137),
            Err(PrcError::Malformed(_))
        ));
    }

    #[test]
    fn a_wire_without_indices_is_the_coordinate_polyline() {
        let mut b = W::default();
        square(&mut b, 175);
        b.uint(0).bit(true);
        // 4 implicit points -> 4 RGB colours, all "same" after the first.
        b.bit(false).bit(false).bit(false).put(1, 24);
        for _ in 1..4 {
            b.bit(true);
        }
        wire(&mut b, false);
        let t = decode(&section(2, &b), &Schema::default(), 8137).unwrap();
        let Tessellation::Wire(w) = &t[0] else {
            panic!("not a wire")
        };
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].len(), 4);
        assert!(matches!(t[1], Tessellation::Wire(_)));
    }

    #[test]
    fn face_vertex_colours_count_points_not_data() {
        // A triangle plus a fan of 4 vertices = 7 points -> 7 colours.
        let mut b = W::default();
        square(&mut b, 172);
        b.bit(true).bit(false).bit(false).uint(0).uint(0);
        uints(&mut b, &[0, 0, 0, 3, 0, 6, 0, 0, 0, 3, 0, 6, 0, 9]);
        b.uint(1)
            .uint(174)
            .uint(0)
            .uint(0)
            .uint(0)
            .uint(0x2 | 0x4)
            .uint(0);
        uints(&mut b, &[1, 1, 4]);
        b.uint(0).bit(true);
        b.bit(false).bit(false).put(0x10_2030, 24);
        for _ in 1..7 {
            b.bit(true);
        }
        b.uint(0);
        wire(&mut b, false);
        let t = decode(&section(2, &b), &Schema::default(), 8137).unwrap();
        assert_eq!(mesh(&t[0]).triangles.len(), 3);
        assert!(matches!(t[1], Tessellation::Wire(_)));
    }

    #[test]
    fn face_behaviour_follows_line_attributes() {
        let mut b = W::default();
        square(&mut b, 172);
        b.bit(true).bit(false).bit(false).uint(0).uint(0);
        uints(&mut b, &[0, 0, 0, 3, 0, 6]);
        b.uint(1).uint(174);
        uints(&mut b, &[5]);
        b.uint(0).uint(0).uint(0x2).uint(0);
        uints(&mut b, &[1]);
        b.uint(0).bit(false).uint(1).uint(0);
        wire(&mut b, false);
        let t = decode(&section(2, &b), &Schema::default(), 8137).unwrap();
        assert!(matches!(t[1], Tessellation::Wire(_)));
    }

    /// A square as one triangle plus one 4-vertex fan (two entities, three
    /// triangles), the face carrying `attrs`; behaviour bits 0x30.
    fn styled_face(attrs: &[u32]) -> TriangleMesh {
        let mut b = W::default();
        square(&mut b, 172);
        b.bit(true).bit(false).bit(false).uint(0).uint(0);
        uints(&mut b, &[0, 0, 0, 3, 0, 6, 0, 0, 0, 3, 0, 6, 0, 9]);
        b.uint(1).uint(174);
        uints(&mut b, attrs);
        b.uint(0).uint(0).uint(0x2 | 0x4).uint(0);
        uints(&mut b, &[1, 1, 4]);
        b.uint(0).bit(false);
        if !attrs.is_empty() {
            b.uint(0x30);
        }
        b.uint(0);
        wire(&mut b, false);
        let t = decode(&section(2, &b), &Schema::default(), 8137).unwrap();
        assert!(matches!(t[1], Tessellation::Wire(_)));
        mesh(&t[0]).clone()
    }

    fn styles(m: &TriangleMesh) -> Vec<(u32, u16)> {
        m.triangle_graphics
            .iter()
            .map(|g| (g.style, g.bits))
            .collect()
    }

    #[test]
    fn face_line_attributes_style_its_triangles() {
        assert!(styled_face(&[]).triangle_graphics.is_empty());
        let whole = styled_face(&[5]);
        assert_eq!(whole.triangles.len(), 3);
        assert_eq!(styles(&whole), [(5, 0x30); 3]);
        // One per entity: the triangle, then both triangles of the fan.
        let each = styled_face(&[4, 7]);
        assert_eq!(styles(&each), [(4, 0x30), (7, 0x30), (7, 0x30)]);
        // An entity styled 0 inherits.
        let short = styled_face(&[4, 0, 9]);
        assert_eq!(styles(&short), [(4, 0x30), (0, 0x30), (0, 0x30)]);
    }

    #[test]
    fn markup_is_read_past() {
        let mut b = W::default();
        b.uint(176).bit(false).uint(3);
        for c in [1., 2., 3.] {
            b.double(c);
        }
        uints(&mut b, &[6, 0]);
        b.uint(1).string(Some("R1")).string(Some("label")).put(7, 8);
        wire(&mut b, false);
        let t = decode(&section(2, &b), &Schema::default(), 8137).unwrap();
        assert_eq!(t[0], Tessellation::Markup);
        assert!(matches!(t[1], Tessellation::Wire(_)));
    }

    #[test]
    fn schema_fields_are_skipped_after_each_entity() {
        // A newer producer added one UInt to TESS_3D and one to TESS_Face.
        let mut s = W::default();
        s.uint(2)
            .uint(172)
            .uint(1)
            .uint(3)
            .uint(174)
            .uint(1)
            .uint(3);
        let sb = s.bytes();
        let schema = Schema::read(&mut BitReader::new(&sb)).unwrap();

        let mut b = W::default();
        square(&mut b, 172);
        b.bit(true).bit(false).bit(false).uint(0).uint(0);
        uints(&mut b, &[0, 0, 0, 3, 0, 6]);
        b.uint(1)
            .uint(174)
            .uint(0)
            .uint(0)
            .uint(0)
            .uint(0x2)
            .uint(0);
        uints(&mut b, &[1]);
        b.uint(0).bit(false).uint(99); // the face's added field
        b.uint(0).uint(77); // texture count, then TESS_3D's added field
        wire(&mut b, false);
        let t = decode(&section(2, &b), &schema, 8137).unwrap();
        assert_eq!(mesh(&t[0]).triangles, [[0, 1, 2]]);
        assert!(matches!(t[1], Tessellation::Wire(_)));
        // Without the schema the same bytes do not decode to the same thing.
        assert_ne!(
            decode(&section(2, &b), &Schema::default(), 8137).ok(),
            Some(t)
        );
    }

    #[test]
    fn attributes_names_and_user_data_are_read_past() {
        let mut w = W::default();
        w.uint(305).uint(1).uint(201);
        w.bit(true).uint(2); // predefined title
        w.uint(2);
        w.bit(false).string(Some("k")).uint(4).string(Some("v"));
        w.bit(true).uint(1).uint(5).int(-1).uint(9);
        w.uint(42); // the attribute's schema-added field
        w.bit(false).string(Some("tess")).uint(1);
        wire(&mut w, false);
        w.uint(5).put(0b10110, 5);
        let mut sw = W::default();
        sw.uint(1).uint(ATTRIBUTE).uint(1).uint(3);
        let sb = sw.bytes();
        let schema = Schema::read(&mut BitReader::new(&sb)).unwrap();
        let bytes = w.bytes();
        let mut ctx = Ctx::new(BitReader::new(&bytes), &schema, 8137);
        let t = ctx.file_structure_tessellation().unwrap();
        assert!(matches!(t[..], [Tessellation::Wire(_)]));
        assert_eq!(ctx.r.position(), w.len());
    }

    /// A one-triangle TESS_3D_Compressed. `full` takes every optional
    /// branch: stored normals, colours, line attributes, texture, behaviours.
    /// Otherwise `three_t` picks the 3T edge form, and the triangle rebuilds.
    fn compressed(w: &mut W, v: u32, full: bool, three_t: bool) {
        w.uint(173).bit(false).bit(true).double(0.001);
        if v >= ORIGIN_FROM {
            w.put(0, 32).put(0, 32).put(0, 32);
        }
        // point_array: nine 2-bit values.
        w.bit(false).uint(9);
        for _ in 0..9 {
            w.put(2, 8);
        }
        for _ in 0..9 {
            w.bit(false).bit(true);
        }
        // edge_status_array: the 3T form (Huffman) or the T form.
        if full {
            w.bit(true).huffman(2, 2, &[(0, 0b10, 2)], &[0, 0, 0]);
        } else if three_t {
            w.bit(false).uint(3).put(0, 8).put(0, 8).put(0, 8);
        } else {
            w.bit(false).uint(1).put(0, 8);
        }
        // triangle_face_array [0]. `full`: four references, so the
        // flagless reference array is Huffman; else none.
        w.bit(false).uint(1).put(1, 8).bit(false);
        if full {
            w.uint(4).put(0b1111, 4);
            w.huffman(6, 2, &[(1, 0b10, 2), (0, 0b11, 2)], &[1, 0, 0, 0]);
            w.put(0, 4);
        } else {
            w.uint(3).put(0, 3).uint(0);
        }
        if full {
            w.bit(false).put(10, 8).uint(3).put(0b101, 3);
            w.bit(true).huffman(10, 2, &[(5, 0b10, 2)], &[5, 5]);
            w.bit(true); // is_face_planar
            w.bit(true).bit(true).bit(false).uint(5);
            for c in [1, 255, 0, 0, 128] {
                w.put(c, 8);
            }
            w.bit(true).bit(false);
        } else {
            w.bit(true).bit(false).double(0.5).put(0, 8);
            w.bit(false).bit(false);
        }
        w.bit(false).uint(1).put(1, 8).put(0, 8); // line_attribute_array
        if full {
            w.bit(false).uint(1).put(0, 32).uint(32);
            w.uint(1).put(1, 5).bit(true).double(0.5).uint(2);
            w.put(0, 32).put(0, 32).bit(false).bit(true);
            w.bit(true).bit(false).uint(1).put(1, 8);
        } else {
            w.bit(true).bit(false);
        }
    }

    #[test]
    fn a_compressed_mesh_is_read_to_its_end() {
        for (full, three_t) in [(false, false), (false, true), (true, true)] {
            for v in [ORIGIN_FROM - 1, 8137] {
                let mut b = W::default();
                compressed(&mut b, v, full, three_t);
                tess_3d(
                    &mut b,
                    v,
                    &Mesh {
                        recalc: false,
                        indices: &[0, 0, 0, 3, 0, 6, 0, 0, 0, 3, 0, 6, 0, 9],
                        faces: &[(0x2 | 0x4, 0, &[1, 1, 4])],
                        textures: 0,
                    },
                );
                let w = section(2, &b);
                let bytes = w.bytes();
                let schema = Schema::default();
                let mut ctx = Ctx::new(BitReader::new(&bytes), &schema, v);
                let t = ctx.file_structure_tessellation().unwrap();
                assert!(
                    matches!(
                        t[..],
                        [
                            Tessellation::Compressed { triangles: 1, .. },
                            Tessellation::Mesh(_)
                        ]
                    ),
                    "full={full} v={v}: {t:?}"
                );
                let Tessellation::Compressed { mesh, .. } = &t[0] else {
                    unreachable!()
                };
                assert_eq!(
                    mesh.as_ref().map(|m| (
                        m.positions.len(),
                        m.triangles.clone(),
                        m.normals_recalculated
                    )),
                    // This branch sets must_recalculate_normals.
                    (!full && three_t).then(|| (3, vec![[0, 1, 2]], true)),
                    "full={full} three_t={three_t} v={v}"
                );
                if let Some(m) = mesh {
                    // One face, line attribute 1; `full` adds behaviour 1.
                    let bits = u16::from(full);
                    assert_eq!(
                        m.triangle_graphics,
                        vec![crate::tree::Graphics {
                            style: 1,
                            bits,
                            fs: 0
                        }],
                        "full={full} v={v}"
                    );
                }
                assert_eq!(ctx.r.position(), w.len(), "full={full} v={v}");
            }
        }
    }

    #[test]
    fn compressed_line_attributes_style_each_face() {
        let g = |style, bits| crate::tree::Graphics { style, bits, fs: 0 };
        let face_of = [0, 1, 1, 0];
        assert_eq!(
            compressed_graphics(&face_of, &[], &[2, 0], &[], 2),
            vec![g(2, 0), g(0, 0), g(0, 0), g(2, 0)]
        );
        assert_eq!(
            compressed_graphics(&face_of, &[false, false], &[2, 3], &[16, 8], 2),
            vec![g(2, 16), g(3, 8), g(3, 8), g(2, 16)]
        );
        // All inherit, a flagged face, or a count that is not one per face.
        assert!(compressed_graphics(&face_of, &[], &[0, 0], &[], 2).is_empty());
        assert!(compressed_graphics(&face_of, &[false, true], &[2, 3], &[], 2).is_empty());
        assert!(compressed_graphics(&face_of, &[], &[2], &[], 2).is_empty());
    }

    #[test]
    fn refusals_and_damage_are_errors() {
        // Edge statuses neither T nor 3T long.
        let mut c = W::default();
        c.uint(173).bit(false).bit(true).double(0.001);
        c.put(0, 32).put(0, 32).put(0, 32);
        c.bit(false).uint(0).bit(false).uint(2).put(0, 16);
        c.bit(false).uint(1).put(1, 8).bit(false);
        assert!(matches!(
            decode(&section(1, &c), &Schema::default(), 8137),
            Err(PrcError::Malformed(_))
        ));
        // Optimised vertex colours.
        let mut o = W::default();
        square(&mut o, 175);
        o.uint(0).bit(true).bit(false).bit(false).bit(true);
        assert!(matches!(
            decode(&section(1, &o), &Schema::default(), 8137),
            Err(PrcError::Unsupported(_))
        ));
        // A point index that is not a multiple of 3, and one past the end.
        for bad in [4, 12] {
            let mut b = W::default();
            tess_3d(
                &mut b,
                8137,
                &Mesh {
                    recalc: false,
                    indices: &[0, 0, 0, bad, 0, 6],
                    faces: &[(0x2, 0, &[1])],
                    textures: 0,
                },
            );
            assert!(matches!(
                decode(&section(1, &b), &Schema::default(), 8137),
                Err(PrcError::Malformed(_))
            ));
        }
        // A fan whose count claims a billion points.
        let mut b = W::default();
        tess_3d(
            &mut b,
            8137,
            &Mesh {
                recalc: false,
                indices: &[0, 0],
                faces: &[(0x4, 0, &[1, 0x3FFF_FFFF])],
                textures: 0,
            },
        );
        assert!(decode(&section(1, &b), &Schema::default(), 8137).is_err());
        // A tessellation count far past the data.
        let mut w = W::default();
        w.uint(305).uint(0).bit(true).uint(u32::MAX);
        assert!(matches!(
            decode(&w, &Schema::default(), 8137),
            Err(PrcError::Truncated(_))
        ));
        // A wrong entity type where the section's own belongs.
        let mut w = W::default();
        w.uint(306);
        assert!(matches!(
            decode(&w, &Schema::default(), 8137),
            Err(PrcError::Malformed(_))
        ));
    }

    /// A whole PRC stream: one file structure whose tessellation section
    /// holds the unit square as two triangles, and an empty schema.
    fn square_prc() -> Vec<u8> {
        let mut body = W::default();
        tess_3d(
            &mut body,
            8137,
            &Mesh {
                recalc: false,
                indices: &[0, 0, 0, 3, 0, 6, 0, 0, 0, 6, 0, 9],
                faces: &[(0x2, 0, &[2])],
                textures: 0,
            },
        );
        prc_file(&section(1, &body))
    }

    /// A one-file-structure PRC whose tessellation section is `tess`.
    fn prc_file(tess: &W) -> Vec<u8> {
        let schema = W::default().uint(0).bytes();
        crate::testw::prc_container(&schema, &[0], &tess.bytes(), &[0])
    }

    /// The CLI's `3d-mesh` fixtures under `fixtures/synthetic/prc/` are
    /// exactly what [`square_prc`], [`compressed_prc`] and
    /// [`compressed_prc_rebuilt`] build. Set
    /// `PDFCER_WRITE_FIXTURES=1` to rewrite them.
    #[test]
    fn the_prc_fixtures_are_current_and_decode() {
        let bytes = square_prc();
        let f = crate::PrcFile::parse(&bytes).unwrap();
        let t = f.file_structures[0].tessellations().unwrap();
        assert_eq!(mesh(&t[0]).triangles, [[0, 1, 2], [0, 2, 3]]);
        crate::testw::check_fixture("square.prc", &bytes);
        let bytes = compressed_prc();
        let f = crate::PrcFile::parse(&bytes).unwrap();
        let t = f.file_structures[0].tessellations().unwrap();
        assert!(matches!(
            t[..],
            [Tessellation::Compressed {
                triangles: 1,
                mesh: None,
                not_rebuilt: Some(_),
            }]
        ));
        crate::testw::check_fixture("compressed.prc", &bytes);
        let bytes = compressed_prc_rebuilt();
        let f = crate::PrcFile::parse(&bytes).unwrap();
        let t = f.file_structures[0].tessellations().unwrap();
        assert!(matches!(
            t[..],
            [Tessellation::Compressed {
                triangles: 1,
                mesh: Some(_),
                not_rebuilt: None,
            }]
        ));
        crate::testw::check_fixture("compressed_triangle.prc", &bytes);
    }

    /// One compressed-tessellation mesh, in a form not rebuilt.
    fn compressed_prc() -> Vec<u8> {
        let mut body = W::default();
        compressed(&mut body, 8137, true, true);
        prc_file(&section(1, &body))
    }

    /// One compressed-tessellation triangle that rebuilds.
    fn compressed_prc_rebuilt() -> Vec<u8> {
        let mut body = W::default();
        compressed(&mut body, 8137, false, true);
        prc_file(&section(1, &body))
    }
}
