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
use crate::tess_face::{
    Charge, Corners, Face, face_graphics, points, stored_normals, stored_uvs, triangle_elements,
    triangulate,
};
use crate::tess_wire::{split_like, unflagged, wire_colour_count, wire_polylines};

/// Lenient-read reasons, each `defect: what is left out`.
pub(crate) const PARTIAL_POINT: &str =
    "a point missing a coordinate: the point and every triangle or wire using it are left out";
pub(crate) const WIDE_BEHAVIOUR: &str = "a face behaviour wider than 16 bits: the face is left out";

/// A `TESS_3D`'s fields, read whole, that its triangles are built from.
struct Tess3d {
    coords: Vec<f64>,
    recalc: bool,
    normal_coords: Vec<f64>,
    indices: Vec<u32>,
    faces: Vec<Face>,
    texture: Vec<f64>,
}

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

/// A `TESS_3D_Wire` [WD 7.8.7]: its polylines, each point in the
/// tessellation's own coordinates, a closing wire ending on a repeat of its
/// first point and a continuous one already joined to the wire before it.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct WireTessellation {
    /// The polylines; one of fewer than two points draws nothing.
    pub polylines: Vec<Vec<[f64; 3]>>,
    /// Each point's 8-bit RGBA colour, parallel to `polylines` (a segment
    /// colour [WD 7.8.7.2] given to both its points, RGB read opaque), or
    /// empty when the wire has none or their count does not match.
    pub colours: Vec<Vec<[u8; 4]>>,
}

/// One entry of the tessellation array; a representation item's
/// `index_tessellation` selects one by position.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Tessellation {
    /// A `TESS_3D` triangle mesh.
    Mesh(TriangleMesh),
    /// A `TESS_3D_Wire`: polylines and their colours.
    Wire(WireTessellation),
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
        /// Whether `mesh` is a best fit: the first geometry a deep search
        /// found that consumes every array, where another might as well.
        /// Such a mesh is disclosed as inferred.
        best_fit: bool,
        /// When `mesh` is `None`: the triangles that decoded regardless,
        /// drawn only under [`MeshFit::Partial`](crate::MeshFit::Partial)
        /// or [`MeshFit::Continue`](crate::MeshFit::Continue) and disclosed.
        /// `None` when not even a seed triangle decodes.
        partial: Option<Box<crate::PartialMesh>>,
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
    /// Per triangle, its face index as the compressed form stores it;
    /// empty for a mesh read face by face, whose [`Self::faces`] say it.
    pub(crate) triangle_faces: Vec<u32>,
    /// The stored texture coordinates as (u, v) pairs [WD 7.8.5.1]; empty
    /// when the mesh stores none.
    pub uvs: Vec<[f64; 2]>,
    /// Per texture-coordinate set, per triangle, the index into
    /// [`Self::uvs`] of each corner's coordinate, in the corner order of
    /// [`Self::triangles`]; each set is as long as [`Self::triangles`], a
    /// triangle whose face stores no coordinate for the set `None`. Empty
    /// when [`Self::uvs`] is.
    pub triangle_uvs: Vec<Vec<Option<[u32; 3]>>>,
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
    /// The last `ContentPRCRefBase`'s PRC unique id [WD 7.2.3.2].
    pub(crate) uid: u32,
    /// Schema steps the reading pass has taken; a pass over several
    /// sections carries it from one reader to the next.
    pub(crate) schema_steps: u64,
    /// Decoded elements the file structure's readers have built so far,
    /// charged against [`crate::MAX_DECODED_ELEMENTS`].
    pub(crate) elements: u64,
    /// An entity reference read so far names a local coordinate system
    /// [WD 7.4.10], so the globals' reference systems are needed.
    pub(crate) names_cs: bool,
    /// Damage contained to one point or face is left out and noted in
    /// `notes` instead of refusing the tessellation.
    pub(crate) lenient: bool,
    /// One reason per unit left out while `lenient`.
    pub(crate) notes: Vec<&'static str>,
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
            uid: 0,
            schema_steps: 0,
            elements: 0,
            names_cs: false,
            lenient: false,
            notes: Vec::new(),
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

    /// The schema's added fields for `entity_type`, charged to this pass.
    pub(crate) fn skip_added(&mut self, entity_type: u32) -> Result<(), PrcError> {
        self.schema
            .skip_added_fields_within(entity_type, &mut self.r, &mut self.schema_steps)
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
            self.skip_added(ATTRIBUTE)?;
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
    /// exists only inside a wire [PRCRS]. One colour per point, a segment
    /// colour repeated for both of its points.
    fn vertex_colors(&mut self, count: usize, in_wire: bool) -> Result<Vec<[u8; 4]>, PrcError> {
        let rgba = self.r.bit()?;
        let per_segment = in_wire && self.r.bit()?;
        if self.r.bit()? {
            return Err(PrcError::Unsupported("optimised vertex colours"));
        }
        let points = count;
        let count = if per_segment { count / 2 } else { count };
        let bytes = if rgba { 4 } else { 3 };
        // The first colour is stored in full; each later one may be just
        // its reuse bit.
        let least = match count {
            0 => 0,
            n => (n - 1).saturating_add(8 * bytes),
        };
        if least > self.r.remaining() {
            return Err(PrcError::Truncated("vertex colours"));
        }
        let mut out: Vec<[u8; 4]> = Vec::with_capacity(count.min(1 << 16));
        let mut last = [0, 0, 0, 255];
        for i in 0..count {
            if i == 0 || !self.r.bit()? {
                for c in last.iter_mut().take(bytes) {
                    *c = self.r.character()?;
                }
            }
            out.push(last);
            if per_segment {
                out.push(last);
            }
        }
        // An odd point count leaves the last point to the last segment.
        if per_segment && count > 0 && out.len() < points {
            out.push(last);
        }
        Ok(out)
    }

    /// `FileStructureTessellation` (305) [WD 7.3.7].
    pub(crate) fn file_structure_tessellation(&mut self) -> Result<Vec<Tessellation>, PrcError> {
        let n = self.tessellation_count()?;
        let mut out = Vec::with_capacity(n.min(1024));
        for _ in 0..n {
            out.push(self.tessellation()??);
        }
        self.tessellation_end()?;
        Ok(out)
    }

    /// The section's leading fields; returns its tessellation count.
    pub(crate) fn tessellation_count(&mut self) -> Result<usize, PrcError> {
        self.expect_type(FILE_STRUCTURE_TESSELLATION)?;
        self.content_prc_base()?;
        self.count(1, "tessellations")
    }

    /// The section's trailing fields, after its last tessellation.
    pub(crate) fn tessellation_end(&mut self) -> Result<(), PrcError> {
        self.skip_added(FILE_STRUCTURE_TESSELLATION)?;
        self.user_data()
    }

    /// One tessellation entry. Entities are not length-prefixed, so the
    /// outer error means the reader no longer knows where the next entry
    /// starts; the inner one is damage found after the entity was read
    /// whole, which leaves the reader at the next entry.
    pub(crate) fn tessellation(&mut self) -> Result<Result<Tessellation, PrcError>, PrcError> {
        let t = self.r.unsigned_integer()?;
        Ok(match t {
            TESS_3D => self.tess_3d()?.map(Tessellation::Mesh),
            TESS_3D_WIRE => self.wire()?.map(Tessellation::Wire),
            TESS_MARKUP => {
                self.markup()?;
                Ok(Tessellation::Markup)
            }
            TESS_3D_COMPRESSED => {
                let (triangles, rebuilt) = self.tess_3d_compressed()?;
                let (mesh, not_rebuilt, best_fit, partial) = match rebuilt {
                    Ok((m, best)) => (Some(m), None, best, None),
                    Err(e) => (None, Some(e.why), false, e.partial),
                };
                Ok(Tessellation::Compressed {
                    triangles,
                    mesh,
                    not_rebuilt,
                    best_fit,
                    partial,
                })
            }
            t => {
                return Err(malformed(format!(
                    "entity type {t} in the tessellation array"
                )));
            }
        })
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
    fn tess_3d_compressed(&mut self) -> Result<(usize, crate::compressed::Rebuilt), PrcError> {
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
        crate::elements::charge(&mut self.elements, t as u64)?;
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
        self.skip_added(TESS_3D_COMPRESSED)?;
        let graphics = compressed_graphics(&face_of, &multi, &line_attributes, &behaviours, faces);
        // A partial mesh holds a prefix of the entity's triangles; the
        // per-triangle arrays are cut to it.
        let per_face = |m: TriangleMesh, recalc: bool| {
            let n = m.triangles.len();
            TriangleMesh {
                normals_recalculated: recalc,
                triangle_faces: face_of.get(..n).map(<[u32]>::to_vec).unwrap_or_default(),
                triangle_graphics: graphics.get(..n).map(<[_]>::to_vec).unwrap_or_default(),
                ..m
            }
        };
        let mesh = crate::compressed::rebuild(&crate::compressed::Arrays {
            tolerance: tol,
            origin,
            points: &point_array,
            edge_status: &edge_status,
            triangles: t,
            is_reference: &is_ref,
            references: &refs,
            ortho_turned: false,
            legacy_orient: false,
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
        .map(|(m, best)| (per_face(m, recalc), best))
        .map_err(|mut e| {
            if let Some(p) = e.partial.as_mut() {
                let m = std::mem::take(&mut p.mesh);
                let recalc = recalc || m.normals.is_empty();
                p.mesh = per_face(m, recalc);
            }
            e
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
    fn tess_3d(&mut self) -> Result<Result<TriangleMesh, PrcError>, PrcError> {
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
        let texture = self.doubles("texture coordinates")?;
        self.skip_added(TESS_3D)?;
        Ok(self.build_mesh(&Tess3d {
            coords,
            recalc,
            normal_coords,
            indices,
            faces,
            texture,
        }))
    }

    /// A `TESS_3D` read whole, triangulated. While lenient, a damaged face
    /// keeps an empty range and a partial last point takes every triangle
    /// naming it out with it.
    fn build_mesh(&mut self, t: &Tess3d) -> Result<TriangleMesh, PrcError> {
        let (positions, partial) = self.whole_points(&t.coords, "position")?;
        let whole = positions.len();
        let mut mesh = TriangleMesh {
            positions,
            normals_recalculated: t.recalc,
            ..TriangleMesh::default()
        };
        let styled = t.faces.iter().any(|f| !f.styles.is_empty());
        let mut corners = Corners::default();
        // Every triangle carries a corner triple per texture set any face
        // uses, so each is charged for what it holds.
        let sets = t.faces.iter().map(Face::texture_sets).max().unwrap_or(0);
        let per_triangle = triangle_elements(!t.recalc, sets, styled);
        for f in &t.faces {
            let start = mesh.triangles.len();
            if f.damaged {
                mesh.faces.push(start..start);
                continue;
            }
            let entities = triangulate(
                f,
                &t.indices,
                !t.recalc,
                whole + usize::from(partial),
                &mut mesh.triangles,
                &mut corners,
                Charge {
                    spent: &mut self.elements,
                    per_triangle,
                },
            )?;
            mesh.faces.push(start..mesh.triangles.len());
            if styled {
                face_graphics(f, &entities, &mut mesh.triangle_graphics);
            }
        }
        (mesh.normals, mesh.triangle_normals) =
            stored_normals(&t.normal_coords, &corners.normals, mesh.triangles.len());
        (mesh.uvs, mesh.triangle_uvs) = stored_uvs(&t.texture, corners.textures);
        if partial {
            let lost = u32::try_from(whole).unwrap_or(u32::MAX);
            let keep: Vec<bool> = mesh.triangles.iter().map(|t| !t.contains(&lost)).collect();
            mesh = crate::tess_face::keep_triangles(&mesh, &keep);
        }
        Ok(mesh)
    }

    /// `coords` as whole points. A partial last point is an error unless
    /// lenient, when it is left off, noted, and `true` says so.
    fn whole_points(
        &mut self,
        coords: &[f64],
        what: &str,
    ) -> Result<(Vec<[f64; 3]>, bool), PrcError> {
        let cut = coords.len() % 3;
        if cut == 0 || !self.lenient {
            return Ok((points(coords, what)?, false));
        }
        self.notes.push(PARTIAL_POINT);
        let whole = coords.get(..coords.len() - cut).unwrap_or(&[]);
        Ok((points(whole, what)?, true))
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
            damaged: false,
        };
        if self.r.bit()? {
            let n = face.point_count()?;
            self.vertex_colors(n, false)?;
        }
        if !face.styles.is_empty() {
            // Behaviour bits are 16 wide [WD 7.2.4.2: two Characters]; a
            // wider value would lose its inherit flags if cut, so a lenient
            // read leaves the face out rather than guess them.
            let b = self.r.unsigned_integer()?;
            match u16::try_from(b) {
                Ok(bits) => face.behaviour = bits,
                Err(_) if self.lenient => {
                    face.damaged = true;
                    self.notes.push(WIDE_BEHAVIOUR);
                }
                Err(_) => {
                    return Err(malformed(format!(
                        "face behaviour {b:#x} wider than 16 bits"
                    )));
                }
            }
        }
        self.skip_added(TESS_FACE)?;
        Ok(face)
    }

    /// `TESS_3D_Wire` (175) after its type code [WD 7.8.7].
    fn wire(&mut self) -> Result<Result<WireTessellation, PrcError>, PrcError> {
        let coords = self.base_tess_data()?;
        let (pts, partial) = self.whole_points(&coords, "wire")?;
        let words = self.uints("wire indices")?;
        // Strict reads fail here, before the colours, as they always have;
        // a lenient one reads past the colours so the next entry decodes.
        let built = match wire_polylines(&pts, partial, &words) {
            Err(e) if !self.lenient => return Err(e),
            b => b,
        };
        let colours = wire_colour_count(&words, pts.len() + usize::from(partial));
        let mut flat = if self.r.bit()? {
            self.vertex_colors(colours, true)?
        } else {
            Vec::new()
        };
        self.skip_added(TESS_3D_WIRE)?;
        if words.is_empty() {
            flat.truncate(pts.len());
        }
        Ok(built.map(|(wires, bad)| {
            let colours = split_like(&wires, flat);
            WireTessellation {
                colours: unflagged(colours, &bad),
                polylines: unflagged(wires, &bad),
            }
        }))
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
        self.skip_added(TESS_MARKUP)
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

#[cfg(test)]
#[allow(clippy::unreachable)] // tests: an unreachable arm failing IS the report
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
pub(crate) mod tests {
    use super::*;
    use crate::tess_face::NORMAL_SINGLE;
    use crate::tess_wire::{WIRE_IS_CLOSING, WIRE_IS_CONTINUOUS};
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
        coordinates(w, entity, &SQUARE);
    }

    /// An entity header over `coords` (any length, whole points or not).
    fn coordinates(w: &mut W, entity: u32, coords: &[f64]) {
        w.uint(entity).bit(false).uint(coords.len() as u32);
        for &c in coords {
            w.double(c);
        }
    }

    /// Two normals: slot 0 is +z, slot 3 is -z.
    const NORMALS: [f64; 6] = [0., 0., 1., 0., 0., -1.];

    struct Mesh<'a> {
        recalc: bool,
        indices: &'a [u32],
        faces: &'a [(u32, u32, &'a [u32])],
        textures: u32,
        uvs: &'a [f64],
    }

    /// A TESS_3D over [`SQUARE`] at authoring version `v`.
    fn tess_3d(w: &mut W, v: u32, m: &Mesh<'_>) {
        tess_3d_over(w, v, m, &SQUARE, &NORMALS);
    }

    /// [`tess_3d`] over the given positions and (unless recalculated)
    /// normals.
    fn tess_3d_over(w: &mut W, v: u32, m: &Mesh<'_>, coords: &[f64], normals: &[f64]) {
        coordinates(w, 172, coords);
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
            w.uint(normals.len() as u32);
            for &c in normals {
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
        w.uint(m.uvs.len() as u32);
        for &c in m.uvs {
            w.double(c);
        }
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
                uvs: &[],
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
                uvs: &[],
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
                    uvs: &[],
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

    /// The unit square as two textured triangles (one set; per vertex a
    /// normal, a texture slot, a point), its corners at the picture's
    /// corners.
    pub(crate) fn textured_square_section() -> Vec<u8> {
        let mut body = W::default();
        tess_3d(
            &mut body,
            8137,
            &Mesh {
                recalc: false,
                indices: &[0, 0, 0, 0, 2, 3, 0, 4, 6, 0, 0, 0, 0, 4, 6, 0, 6, 9],
                faces: &[(0x200, 0, &[2])],
                textures: 1,
                uvs: &[0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0],
            },
        );
        section(1, &body).bytes()
    }

    #[test]
    fn a_textured_mesh_keeps_each_corners_coordinates() {
        let bytes = textured_square_section();
        let t = Ctx::new(BitReader::new(&bytes), &Schema::default(), 8137)
            .file_structure_tessellation()
            .unwrap();
        let m = mesh(&t[0]);
        assert_eq!(m.triangles, [[0, 1, 2], [0, 2, 3]]);
        assert_eq!(m.uvs, [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        assert_eq!(m.triangle_uvs, [vec![Some([0, 1, 2]), Some([0, 2, 3])]]);
        // An odd slot names no pair: that triangle has no coordinates.
        let mut b = W::default();
        tess_3d(
            &mut b,
            8137,
            &Mesh {
                recalc: false,
                indices: &[0, 1, 0, 0, 2, 3, 0, 4, 6],
                faces: &[(0x200, 0, &[1])],
                textures: 1,
                uvs: &[0.0; 6],
            },
        );
        let t = decode(&section(1, &b), &Schema::default(), 8137).unwrap();
        let m = mesh(&t[0]);
        assert_eq!(m.triangles, [[0, 1, 2]]);
        assert_eq!(m.triangle_uvs, [vec![None]]);
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

    /// One triangle with normals, over `coords` and `normals`.
    fn one_over(coords: &[f64], normals: &[f64]) -> Result<Vec<Tessellation>, PrcError> {
        let m = Mesh {
            recalc: false,
            indices: &[3, 0, 0, 3, 3, 6],
            faces: &[(0x2, 0, &[1])],
            textures: 0,
            uvs: &[],
        };
        let mut b = W::default();
        tess_3d_over(&mut b, 8137, &m, coords, normals);
        decode(&section(1, &b), &Schema::default(), 8137)
    }

    #[test]
    fn partial_position_and_wire_points_are_malformed() {
        for extra in [1, 2] {
            let coords: Vec<f64> = SQUARE
                .iter()
                .copied()
                .chain([0.0; 2])
                .take(12 + extra)
                .collect();
            let got = one_over(&coords, &NORMALS);
            assert!(matches!(got, Err(PrcError::Malformed(_))), "{got:?}");
            let mut b = W::default();
            coordinates(&mut b, 175, &coords);
            b.uint(0).bit(false);
            let got = decode(&section(1, &b), &Schema::default(), 8137);
            assert!(matches!(got, Err(PrcError::Malformed(_))), "{got:?}");
        }
        assert!(one_over(&SQUARE, &NORMALS).is_ok());
    }

    #[test]
    fn a_partial_normal_triple_drops_every_stored_normal() {
        let whole = one_over(&SQUARE, &NORMALS).unwrap();
        assert_eq!(mesh(&whole[0]).normals.len(), 2);
        let normals: Vec<f64> = NORMALS.iter().copied().chain([0.0]).collect();
        let t = one_over(&SQUARE, &normals).unwrap();
        let m = mesh(&t[0]);
        assert_eq!(m.triangles, [[0, 1, 2]]);
        assert!(m.normals.is_empty());
        assert!(m.triangle_normals.is_empty());
    }

    #[test]
    fn faces_reusing_one_index_range_are_charged_per_face() {
        // F faces, each a run of k triangles over the same k-triangle range.
        const F: u64 = 5;
        const K: u64 = 40;
        let indices: Vec<u32> = [3, 0, 0, 3, 3, 6].repeat(K as usize);
        let sizes = [K as u32];
        let faces = vec![(0x2, 0, &sizes[..]); F as usize];
        let m = Mesh {
            recalc: false,
            indices: &indices,
            faces: &faces,
            textures: 0,
            uvs: &[],
        };
        let mut b = W::default();
        tess_3d(&mut b, 8137, &m);
        let bytes = section(1, &b).bytes();
        let schema = Schema::default();
        let run = |already: u64| {
            let mut ctx = Ctx::new(BitReader::new(&bytes), &schema, 8137);
            ctx.elements = already;
            let got = ctx.file_structure_tessellation();
            (got, ctx.elements)
        };
        let (got, spent) = run(0);
        assert_eq!(mesh(&got.unwrap()[0]).triangles.len() as u64, F * K);
        // Each triangle holds its corners and its normal-slot corners.
        let cost = F * K * 4;
        assert_eq!(spent, cost);
        // Room for exactly that fits; one element less is refused.
        let room = crate::MAX_DECODED_ELEMENTS - cost;
        assert!(run(room).0.is_ok());
        assert_eq!(
            run(room + 1).0.unwrap_err(),
            PrcError::TooManyElements {
                limit: crate::MAX_DECODED_ELEMENTS
            }
        );
        // A lenient read contains damage, never a ceiling.
        let mut ctx = Ctx::new(BitReader::new(&bytes), &schema, 8137);
        ctx.elements = room + 1;
        ctx.lenient = true;
        let n = ctx.tessellation_count().unwrap();
        let mut model = crate::AssembledModel::default();
        let mut flagged = crate::mesh_decode::Flagged::default();
        let got = crate::mesh_decode::read_entries(&mut ctx, (0, n), &mut model, &mut flagged);
        assert_eq!(
            got.unwrap_err(),
            PrcError::TooManyElements {
                limit: crate::MAX_DECODED_ELEMENTS
            }
        );
        assert_eq!(model.damaged, 0);
    }

    /// An empty textured fan declaring `textures` texture sets.
    fn empty_fan(textures: u32) -> Result<Vec<Tessellation>, PrcError> {
        let mut b = W::default();
        tess_3d(
            &mut b,
            8137,
            &Mesh {
                recalc: false,
                indices: &[0],
                faces: &[(0x400, 0, &[1, 0])],
                textures,
                uvs: &[],
            },
        );
        decode(&section(1, &b), &Schema::default(), 8137)
    }

    #[test]
    fn too_many_texture_sets_are_refused_before_anything_is_allocated() {
        match empty_fan(u32::MAX) {
            Err(PrcError::Malformed(why)) => {
                assert!(why.contains("4294967295 texture-coordinate sets"), "{why}");
            }
            other => panic!("not refused: {other:?}"),
        }
        let at_cap = crate::tess_face::MAX_TEXTURE_SETS as u32;
        assert!(empty_fan(at_cap).is_ok());
        assert!(matches!(empty_fan(at_cap + 1), Err(PrcError::Malformed(_))));
    }

    #[test]
    fn reused_textured_faces_are_charged_for_their_corner_arrays() {
        // F faces, each one triangle with 8 texture sets, over one range.
        const F: u64 = 6;
        let sets = crate::tess_face::MAX_TEXTURE_SETS;
        let vertex = |p: u32| {
            let mut v = vec![0];
            v.extend(std::iter::repeat_n(0, sets));
            v.push(p);
            v
        };
        let indices: Vec<u32> = [0, 3, 6].into_iter().flat_map(vertex).collect();
        let faces = vec![(0x200, 0, &[1][..]); F as usize];
        let m = Mesh {
            recalc: false,
            indices: &indices,
            faces: &faces,
            textures: sets as u32,
            uvs: &[0.0, 0.0],
        };
        let mut b = W::default();
        tess_3d(&mut b, 8137, &m);
        let bytes = section(1, &b).bytes();
        let schema = Schema::default();
        let run = |already: u64| {
            let mut ctx = Ctx::new(BitReader::new(&bytes), &schema, 8137);
            ctx.elements = already;
            let got = ctx.file_structure_tessellation();
            (got, ctx.elements)
        };
        let (got, spent) = run(0);
        let t = got.unwrap();
        assert_eq!(mesh(&t[0]).triangles.len() as u64, F);
        assert_eq!(mesh(&t[0]).triangle_uvs.len(), sets);
        // Corners 2, normal slots 2, 2 per texture set: 20 per triangle.
        let cost = F * (2 + 2 + 2 * sets as u64);
        assert_eq!(spent, cost);
        let room = crate::MAX_DECODED_ELEMENTS - cost;
        assert!(run(room).0.is_ok());
        assert_eq!(
            run(room + 1).0.unwrap_err(),
            PrcError::TooManyElements {
                limit: crate::MAX_DECODED_ELEMENTS
            }
        );
        // A real textured model still opens from an empty allowance.
        let square = textured_square_section();
        assert!(
            Ctx::new(BitReader::new(&square), &Schema::default(), 8137)
                .file_structure_tessellation()
                .is_ok()
        );
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
            for (i, e) in t.iter().enumerate() {
                let Tessellation::Wire(w) = e else {
                    panic!("not a wire")
                };
                // Two red segments cover four points; the odd fifth takes
                // the last segment's colour.
                let red = vec![[255, 0, 0, 255]; 5];
                let want: Vec<Vec<[u8; 4]>> = if colours && i == 0 {
                    vec![red]
                } else {
                    Vec::new()
                };
                assert_eq!(w.colours, want);
                assert_eq!(
                    w.polylines,
                    [vec![
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
        let (Tessellation::Wire(a), Tessellation::Wire(b)) = (&t[0], &t[1]) else {
            panic!("not wires")
        };
        assert_eq!(a.polylines, b.polylines);
        assert_eq!(a.colours, [vec![[0, 0, 1, 255]; 5]]);
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
        assert_eq!(w.polylines.len(), 1);
        assert_eq!(w.polylines[0].len(), 4);
        assert!(matches!(t[1], Tessellation::Wire(_)));
    }

    #[test]
    fn a_tessellation_ending_in_reused_vertex_colours_reads() {
        // A four-point RGBA polyline, the last entity: one colour stored in
        // full, three reusing it, then only the section's user data.
        let mut b = W::default();
        square(&mut b, 175);
        b.uint(0).bit(true);
        b.bit(true).bit(false).bit(false).put(0x1020_3040, 32);
        for _ in 1..4 {
            b.bit(true);
        }
        let t = decode(&section(1, &b), &Schema::default(), 8137).unwrap();
        let Tessellation::Wire(w) = &t[0] else {
            panic!("not a wire")
        };
        assert_eq!(w.polylines[0].len(), 4);

        // A first colour cut short still cannot fit.
        let mut b = W::default();
        square(&mut b, 175);
        b.uint(0).bit(true);
        b.bit(true).bit(false).bit(false).put(0x10_2030, 24);
        assert!(matches!(
            decode(&section(1, &b), &Schema::default(), 8137),
            Err(PrcError::Truncated(_))
        ));
    }

    #[test]
    fn programs_on_one_stream_share_one_step_allowance() {
        // For 1_500_000 { {} } after each wire: about 3M steps, under the
        // 2^22 ceiling once, over it twice.
        let mut sw = W::default();
        sw.uint(1).uint(TESS_3D_WIRE).uint(5);
        for t in [15, 26, 1_500_000, 19, 21] {
            sw.uint(t);
        }
        let sb = sw.bytes();
        let schema = Schema::read(&mut BitReader::new(&sb)).unwrap();
        let wires = |n| {
            let mut b = W::default();
            for _ in 0..n {
                wire(&mut b, false);
            }
            decode(&section(n, &b), &schema, 8137)
        };
        assert_eq!(wires(1).unwrap().len(), 1);
        match wires(2) {
            Err(PrcError::Malformed(m)) => assert!(m.contains("step ceiling"), "{m}"),
            other => panic!("not refused: {other:?}"),
        }
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
        let t = styled_face_with(attrs, 0x30).unwrap();
        assert!(matches!(t[1], Tessellation::Wire(_)));
        mesh(&t[0]).clone()
    }

    /// [`styled_face`]'s section with the face behaviour written as `bits`.
    fn styled_face_with(attrs: &[u32], bits: u32) -> Result<Vec<Tessellation>, PrcError> {
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
            b.uint(bits);
        }
        b.uint(0);
        wire(&mut b, false);
        decode(&section(2, &b), &Schema::default(), 8137)
    }

    #[test]
    fn a_face_behaviour_wider_than_16_bits_is_malformed() {
        // SonHeritColor (0x0008) with bit 16 set: cut to u16 it would read
        // as a plain inherit flag.
        assert!(matches!(
            styled_face_with(&[5], 0x1_0000 | 0x0008),
            Err(PrcError::Malformed(_))
        ));
        assert!(styled_face_with(&[5], 0xffff).is_ok());
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
                        uvs: &[],
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
                    uvs: &[],
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
                uvs: &[],
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
                uvs: &[],
            },
        );
        prc_file(&section(1, &body))
    }

    /// [`square_prc`] plus a red wire along its diagonal, (0,0,0)–(1,1,0):
    /// a `TESS_3D_Wire` of two indexed points and one segment colour.
    fn wired_prc() -> Vec<u8> {
        let mut body = W::default();
        tess_3d(
            &mut body,
            8137,
            &Mesh {
                recalc: false,
                indices: &[0, 0, 0, 3, 0, 6, 0, 0, 0, 6, 0, 9],
                faces: &[(0x2, 0, &[2])],
                textures: 0,
                uvs: &[],
            },
        );
        square(&mut body, 175);
        uints(&mut body, &[2, 0, 6]);
        body.bit(true)
            .bit(false)
            .bit(true)
            .bit(false)
            .put(0xff0000, 24);
        prc_file(&section(2, &body))
    }

    /// `fixtures/synthetic/prc/wired.prc` is [`wired_prc`], and draws its
    /// wire red over the grey square.
    #[test]
    fn the_wired_fixture_is_current_and_its_wire_is_drawn() {
        let bytes = wired_prc();
        let f = crate::PrcFile::parse(&bytes).unwrap();
        let t = f.file_structures[0].tessellations().unwrap();
        let Tessellation::Wire(w) = &t[1] else {
            panic!("not a wire")
        };
        assert_eq!(w.polylines, [vec![[0., 0., 0.], [1., 1., 0.]]]);
        assert_eq!(w.colours, [vec![[255, 0, 0, 255]; 2]]);
        crate::testw::check_fixture("wired.prc", &bytes);
        let model = crate::assemble(&bytes).unwrap();
        assert_eq!((model.wires, model.lines.len()), (1, 1));
        #[cfg(feature = "render")]
        {
            let bounds = crate::Bounds::of(&model.meshes).unwrap();
            let camera =
                crate::Camera::fit(&bounds, [0., 0., -1.], [0., 1., 0.], false, 1.0).unwrap();
            let options = crate::RenderOptions {
                width: 50,
                height: 50,
                ..crate::RenderOptions::default()
            };
            let image = crate::render_model(&model, &camera, &options).unwrap();
            let px = |x: usize, y: usize| &image.rgba[(y * 50 + x) * 4..][..4];
            // The diagonal runs bottom left to top right through the centre.
            assert!((10..40).all(|x| px(x, 49 - x) == [255, 0, 0, 255]));
            assert_ne!(px(25, 20), [255, 0, 0, 255]);
        }
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
                best_fit: false,
                partial: None,
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
                best_fit: false,
                partial: None,
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

    /// Copies of the one-turn mesh in `best_fit.prc`. Each needs two
    /// inverted choices, so seven need more than the unique search's
    /// deepest stage combines, and no walk retry repairs one.
    const BEST_FIT_COPIES: u32 = 7;

    /// `best_fit.prc`: one compressed mesh, [`BEST_FIT_COPIES`] separate
    /// copies of the four-triangle planar mesh whose apex needs its frame
    /// turned, that only [`crate::compressed::rebuild`]'s best-fit search
    /// rebuilds.
    fn best_fit_prc() -> Vec<u8> {
        let one = [0i64, 0, 0, 4, 0, 0, 2, 4, 0, 0, 0, 0, -4, -4, 2];
        let (mut pts, mut st, mut face_of, mut is_ref, mut refs) =
            (vec![], vec![], vec![], vec![], vec![]);
        let (mut angles, mut planar) = (vec![], vec![]);
        for k in 0..BEST_FIT_COPIES {
            pts.extend_from_slice(&one);
            st.extend_from_slice(&[1, 3, 1, 0]);
            face_of.extend([k; 4]);
            is_ref.extend([false, false, false, false, false, true]);
            refs.push(2 + 5 * k);
            angles.extend([470, 724]);
            planar.push(true);
        }
        st.resize(3 * face_of.len(), 0);
        let mut w = W::default();
        w.uint(173).bit(false).bit(true).double(0.5);
        w.put(0, 32).put(0, 32).put(0, 32);
        vbn_array(&mut w, &pts, 4);
        w.bit(false).uint(st.len() as u32);
        for &x in &st {
            w.put(x as u64, 8);
        }
        w.bit(false);
        indices(&mut w, &face_of, 2, None);
        w.uint(is_ref.len() as u32);
        for &x in &is_ref {
            w.bit(x);
        }
        indices(&mut w, &refs, 4, Some(&[(4, 0b10, 2), (0, 0b11, 2)]));
        w.bit(false).put(10, 8).uint(2 * angles.len() as u32);
        for _ in 0..2 * angles.len() {
            w.bit(false);
        }
        w.bit(false).uint(angles.len() as u32);
        for &a in &angles {
            w.put(a & 0xff, 8).put(a >> 8, 8);
        }
        for &x in &planar {
            w.bit(x);
        }
        w.bit(false).bit(false);
        w.bit(false).uint(1).put(1, 8).put(0, 8); // line_attribute_array
        w.bit(true).bit(false);
        prc_file(&section(1, &w))
    }

    /// A `CompressedIntegerArray` of `v`, every value `width` bits wide.
    fn vbn_array(w: &mut W, v: &[i64], width: u64) {
        w.bit(false).uint(v.len() as u32);
        for _ in v {
            w.put(width, 8);
        }
        for &x in v {
            w.bit(x < 0).put(x.unsigned_abs(), width as u32 - 1);
        }
    }

    /// A `CompressedIndiceArray` of `v`, every delta `width` bits wide: its
    /// width differences uncompressed, or Huffman-coded by `leaves`.
    fn indices(w: &mut W, v: &[u32], width: i32, leaves: Option<&[(u32, u32, u32)]>) {
        let diffs: Vec<i32> = (0..v.len())
            .map(|i| if i == 0 { width } else { 0 })
            .collect();
        match leaves {
            Some(l) => {
                w.huffman(6, 2, l, &diffs);
            }
            None => {
                w.uint(v.len() as u32);
                for &d in &diffs {
                    w.put(d as u64, 8);
                }
            }
        }
        let mut prev = 0;
        for &x in v {
            w.bit(false).put(u64::from(x - prev), width as u32 - 1);
            prev = x;
        }
    }

    /// `best_fit.prc` rebuilds only by best fit: drawn by default and
    /// counted, left out with the strict-setting reason under
    /// [`crate::MeshFit::Unique`].
    #[test]
    fn the_best_fit_fixture_is_current_and_rebuilt_only_by_best_fit() {
        let bytes = best_fit_prc();
        let f = crate::PrcFile::parse(&bytes).unwrap();
        let t = f.file_structures[0].tessellations().unwrap();
        let [
            Tessellation::Compressed {
                triangles,
                mesh: Some(m),
                best_fit: true,
                ..
            },
        ] = &t[..]
        else {
            panic!("{t:?}");
        };
        assert_eq!((*triangles, m.triangles.len()), (28, 28));
        crate::testw::check_fixture("best_fit.prc", &bytes);
        let model = crate::assemble(&bytes).unwrap();
        assert_eq!((model.rebuilt, model.best_fit, model.compressed), (1, 1, 0));
        let strict = crate::AssembleOptions {
            mesh_fit: crate::MeshFit::Unique,
            ..Default::default()
        };
        let err = crate::assemble_with_options(&bytes, &strict).unwrap_err();
        assert!(
            err.to_string()
                .contains("strict mesh-fit setting draws unique fits only"),
            "{err}"
        );
    }

    /// `partial.prc`: one compressed mesh of four triangles on one face;
    /// the fourth puts a third triangle on an edge, so no fit rebuilds it,
    /// and three decode before that.
    fn partial_prc() -> Vec<u8> {
        let pts = [0i64, 0, 0, 4, 0, 0, -2, 4, 0, 2, -6, 2];
        let mut st = vec![3u8, 3];
        st.resize(12, 0);
        let is_ref = [false, false, false, false, true, true];
        let mut w = W::default();
        w.uint(173).bit(false).bit(true).double(0.5);
        w.put(0, 32).put(0, 32).put(0, 32);
        vbn_array(&mut w, &pts, 4);
        w.bit(false).uint(st.len() as u32);
        for &x in &st {
            w.put(u64::from(x), 8);
        }
        w.bit(false);
        indices(&mut w, &[0, 0, 0, 0], 2, None);
        w.uint(is_ref.len() as u32);
        for &x in &is_ref {
            w.bit(x);
        }
        indices(&mut w, &[0, 1], 2, None);
        w.bit(true); // must_recalculate_normals
        for _ in 0..4 {
            w.bit(false); // normal_is_reversed
        }
        w.double(0.5).put(0, 8);
        w.bit(false).bit(false);
        w.bit(false).uint(1).put(1, 8).put(0, 8); // line_attribute_array
        w.bit(true).bit(false);
        prc_file(&section(1, &w))
    }

    /// `partial.prc` rebuilds by no fit: left out by default, drawn in
    /// part under [`crate::MeshFit::Partial`] (the sound prefix) and
    /// [`crate::MeshFit::Continue`] (the whole decode), each disclosed.
    #[test]
    fn the_partial_fixture_is_current_and_drawn_only_in_part() {
        let bytes = partial_prc();
        let f = crate::PrcFile::parse(&bytes).unwrap();
        let t = f.file_structures[0].tessellations().unwrap();
        let [
            Tessellation::Compressed {
                triangles: 4,
                mesh: None,
                best_fit: false,
                partial: Some(p),
                ..
            },
        ] = &t[..]
        else {
            panic!("{t:?}");
        };
        assert_eq!((p.sound, p.total, p.mesh.triangles.len()), (3, 4, 4));
        assert_eq!(p.mesh.triangle_faces, vec![0; 4]);
        assert_eq!(p.sound_mesh().triangle_faces, vec![0; 3]);
        crate::testw::check_fixture("partial.prc", &bytes);
        assert!(matches!(
            crate::assemble(&bytes),
            Err(crate::AssembleError::CompressedOnly { .. })
        ));
        for (fit, n) in [(crate::MeshFit::Partial, 3), (crate::MeshFit::Continue, 4)] {
            let options = crate::AssembleOptions {
                mesh_fit: fit,
                ..Default::default()
            };
            let model = crate::assemble_with_options(&bytes, &options).unwrap();
            assert_eq!(model.meshes.len(), 1, "{fit:?}");
            assert_eq!(model.meshes[0].triangles.len(), n, "{fit:?}");
            assert_eq!(
                (model.partial, model.partial_triangles, model.compressed),
                (1, (n, 4), 0),
                "{fit:?}"
            );
            assert_eq!(
                model.partial_why,
                vec![("an edge is shared by more than two triangles".to_owned(), 1)]
            );
            assert!(model.skipped_why.is_empty() && model.left_out_parts.is_empty());
        }
    }

    /// `bytes` refused under a strict fit with `Malformed`, and assembled
    /// under the default (lenient) fit with one damaged unit for `why`.
    fn salvaged(bytes: &[u8], why: &str) -> crate::AssembledModel {
        let strict = crate::AssembleOptions {
            mesh_fit: crate::MeshFit::Unique,
            ..Default::default()
        };
        let err = crate::assemble_with_options(bytes, &strict).unwrap_err();
        assert!(
            matches!(err, crate::AssembleError::Prc(PrcError::Malformed(_))),
            "{err:?}"
        );
        let m = crate::assemble(bytes).unwrap();
        assert_eq!(m.damaged, 1, "{:?}", m.damaged_why);
        assert_eq!(m.damaged_why.len(), 1);
        assert!(m.damaged_why[0].0.contains(why), "{:?}", m.damaged_why);
        assert!(m.partial_why.is_empty() && m.skipped_why.is_empty());
        m
    }

    #[test]
    fn a_partial_mesh_point_leaves_out_the_triangles_using_it() {
        let coords: Vec<f64> = SQUARE.iter().copied().chain([0.0]).collect();
        let m = Mesh {
            recalc: false,
            indices: &[3, 0, 0, 3, 3, 6, 3, 0, 3, 6, 0, 12],
            faces: &[(0x2, 0, &[2])],
            textures: 0,
            uvs: &[],
        };
        let mut b = W::default();
        tess_3d_over(&mut b, 8137, &m, &coords, &NORMALS);
        wire(&mut b, false);
        let model = salvaged(&prc_file(&section(2, &b)), PARTIAL_POINT);
        assert_eq!(model.triangles, 1);
        assert_eq!(model.wires, 1);
    }

    #[test]
    fn a_partial_wire_point_leaves_out_the_polyline_using_it() {
        let coords: Vec<f64> = SQUARE.iter().copied().chain([0.0; 2]).collect();
        let mut b = W::default();
        tess_3d(
            &mut b,
            8137,
            &Mesh {
                recalc: false,
                indices: &[3, 0, 0, 3, 3, 6],
                faces: &[(0x2, 0, &[1])],
                textures: 0,
                uvs: &[],
            },
        );
        coordinates(&mut b, 175, &coords);
        uints(&mut b, &[2, 0, 3, 2, 6, 12]);
        b.bit(false);
        let model = salvaged(&prc_file(&section(2, &b)), PARTIAL_POINT);
        assert_eq!((model.triangles, model.wires), (1, 1));
        assert_eq!(model.lines.len(), 1);
        assert_eq!(model.lines[0].points.len(), 2);
    }

    /// Two one-triangle faces over [`SQUARE`]; the second's behaviour is
    /// written as `bits`.
    fn two_faces(bits: u32) -> Vec<u8> {
        let mut b = W::default();
        square(&mut b, 172);
        b.bit(true).bit(false).bit(false).uint(0).uint(0);
        uints(&mut b, &[0, 0, 0, 3, 0, 6, 0, 0, 0, 6, 0, 9]);
        b.uint(2);
        for (start, bits) in [(0, 0x30), (6, bits)] {
            b.uint(174);
            uints(&mut b, &[5]);
            b.uint(0).uint(0).uint(0x2).uint(start);
            uints(&mut b, &[1]);
            b.uint(0).bit(false).uint(bits);
        }
        b.uint(0);
        prc_file(&section(1, &b))
    }

    #[test]
    fn a_face_behaviour_wider_than_16_bits_leaves_out_that_face() {
        let whole = crate::assemble(&two_faces(0x30)).unwrap();
        assert_eq!((whole.triangles, whole.damaged), (2, 0));
        let model = salvaged(&two_faces(0x1_0000 | 0x0008), WIDE_BEHAVIOUR);
        assert_eq!(model.triangles, 1);
    }

    #[test]
    fn a_huffman_prefix_pair_leaves_out_its_tessellation() {
        let mut b = W::default();
        tess_3d(
            &mut b,
            8137,
            &Mesh {
                recalc: false,
                indices: &[3, 0, 0, 3, 3, 6],
                faces: &[(0x2, 0, &[1])],
                textures: 0,
                uvs: &[],
            },
        );
        // A compressed tessellation whose edge-status table stores `10`
        // and `100`: the reader loses its place there.
        b.uint(173).bit(false).bit(true).double(0.001);
        b.put(0, 32).put(0, 32).put(0, 32);
        b.bit(false).uint(9);
        for _ in 0..9 {
            b.put(2, 8);
        }
        for _ in 0..9 {
            b.bit(false).bit(true);
        }
        b.bit(true)
            .huffman(2, 2, &[(0, 0b10, 2), (1, 0b100, 3)], &[0, 1, 0]);
        let model = salvaged(&prc_file(&section(2, &b)), "prefix of another");
        assert_eq!(model.triangles, 1);
        assert_eq!(model.compressed, 0);
    }

    #[test]
    fn a_model_file_end_past_the_data_is_read_to_the_data_end() {
        let mut bytes = square_prc();
        let len = bytes.len() as u32;
        let at = (0..120)
            .find(|&i| bytes[i..i + 4] == len.to_le_bytes())
            .unwrap();
        bytes[at..at + 4].copy_from_slice(&(len + 1).to_le_bytes());
        let model = salvaged(&bytes, crate::mesh_decode::SHORT_FILE);
        assert_eq!(model.triangles, 2);
    }
}
