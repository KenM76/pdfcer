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

mod compressed;
pub(crate) mod face;
pub(crate) mod wire;

use crate::PrcError;
use crate::prc::bits::BitReader;
use crate::prc::schema::Schema;
use crate::prc::tess::face::{
    Charge, Corners, Face, face_graphics, points, stored_normals, stored_uvs, triangle_elements,
    triangulate,
};
use crate::prc::tess::wire::{split_like, unflagged, wire_colour_count, wire_polylines};

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
    pub(crate) triangle_graphics: Vec<crate::prc::tree::Graphics>,
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
    pub(crate) graphics: crate::prc::tree::Graphics,
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
            graphics: crate::prc::tree::Graphics::default(),
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
            mesh = crate::prc::tess::face::keep_triangles(&mesh, &keep);
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

#[cfg(test)]
#[allow(clippy::unreachable)] // tests: an unreachable arm failing IS the report
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
pub(crate) mod tests;
