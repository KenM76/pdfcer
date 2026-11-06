//! `TESS_3D_Compressed` (173) [WD 7.8.9.7]: its fields, read to the
//! entity's end, and the triangles [`crate::prc::compressed`] rebuilds.

use super::{Ctx, ORIGIN_FROM, TESS_3D_COMPRESSED, TriangleMesh, malformed};
use crate::PrcError;
use crate::prc::arrays;
use crate::prc::compressed::{Arrays, NormalArrays, Rebuilt, rebuild};
use crate::prc::tree::Graphics;

/// Stored normals: angle bits, `is_binary`, the angles and `is_planar`.
type StoredNormals = (u32, Vec<bool>, Vec<i32>, Vec<bool>);

/// The fields of one `TESS_3D_Compressed` the rebuild uses.
struct Fields {
    tolerance: f64,
    origin: [f64; 3],
    points: Vec<i64>,
    edge_status: Vec<i32>,
    face_of: Vec<u32>,
    is_reference: Vec<bool>,
    references: Vec<u32>,
    recalc: bool,
    stored: Option<StoredNormals>,
    graphics: Vec<Graphics>,
}

impl Ctx<'_, '_> {
    /// `TESS_3D_Compressed` (173) after its type code, field by field
    /// [WD 7.8.9.7; PRCRS prc.json]; returns the triangle count and the
    /// rebuilt mesh, if it rebuilds. Array
    /// sizes the WD leaves unstated follow `prc__8137__tess_3d_compressed.md`
    /// §1: `face_number` = largest face index + 1, `normal_is_reversed` is
    /// one bit per triangle, and `point_reference_array`'s compressed flag
    /// is implicit (more than three references).
    pub(super) fn tess_3d_compressed(&mut self) -> Result<(usize, Rebuilt), PrcError> {
        let f = self.compressed_fields()?;
        let t = f.face_of.len();
        // A partial mesh holds a prefix of the entity's triangles; the
        // per-triangle arrays are cut to it.
        let per_face = |m: TriangleMesh, recalc: bool| {
            let n = m.triangles.len();
            TriangleMesh {
                normals_recalculated: recalc,
                triangle_faces: f.face_of.get(..n).map(<[u32]>::to_vec).unwrap_or_default(),
                triangle_graphics: f.graphics.get(..n).map(<[_]>::to_vec).unwrap_or_default(),
                ..m
            }
        };
        let recalc = f.recalc;
        let mesh = rebuild(&Arrays {
            tolerance: f.tolerance,
            origin: f.origin,
            points: &f.points,
            edge_status: &f.edge_status,
            triangles: t,
            is_reference: &f.is_reference,
            references: &f.references,
            ortho_turned: false,
            legacy_orient: false,
            normals: f
                .stored
                .as_ref()
                .map(|(bits, binary, angles, planar)| NormalArrays {
                    bits: *bits,
                    binary,
                    angles,
                    planar,
                    face_of: &f.face_of,
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

    /// Every field of the entity, in file order, through its added schema
    /// fields.
    fn compressed_fields(&mut self) -> Result<Fields, PrcError> {
        let r = &mut self.r;
        r.bit()?; // is_calculated
        r.bit()?; // has_faces
        let tolerance = r.double()?; // tolerance
        let mut origin = [0.0f64; 3];
        if self.version >= ORIGIN_FROM {
            for o in &mut origin {
                *o = f64::from(r.float_as_bytes()?);
            }
        }
        let points = arrays::compressed_integer_array(r)?; // point_array
        let edge_status = arrays::character_array(r, 2, None, false)?;
        let face_of = arrays::compressed_indice_array(r, None)?;
        let t = face_of.len();
        crate::prc::elements::charge(&mut self.elements, t as u64)?;
        let r = &mut self.r;
        if edge_status.len() != t && edge_status.len() != t.saturating_mul(3) {
            return Err(malformed(format!(
                "{} edge statuses for {t} triangles",
                edge_status.len()
            )));
        }
        let faces = face_of.iter().max().map_or(0, |&m| m as usize + 1);
        let n = r.unsigned_integer()? as usize;
        let is_reference = arrays::bool_array(r, n)?;
        let flagged = is_reference.iter().filter(|&&b| b).count();
        let references = arrays::compressed_indice_array(r, Some(flagged > 3))?;
        if references.len() != flagged {
            return Err(malformed(format!(
                "{} point references for {flagged} flagged points",
                references.len()
            )));
        }
        let recalc = self.r.bit()?; // must_recalculate_normals
        let stored = self.compressed_normals(recalc, t, faces)?;
        let graphics = self.compressed_face_attributes(&face_of, faces)?;
        Ok(Fields {
            tolerance,
            origin,
            points,
            edge_status,
            face_of,
            is_reference,
            references,
            recalc,
            stored,
            graphics,
        })
    }

    /// The normal fields after `must_recalculate_normals`: recalculation
    /// parameters, or the stored normals.
    fn compressed_normals(
        &mut self,
        recalc: bool,
        t: usize,
        faces: usize,
    ) -> Result<Option<StoredNormals>, PrcError> {
        let r = &mut self.r;
        if recalc {
            arrays::bool_array(r, t)?; // normal_is_reversed
            r.double()?; // crease_angle
            r.character()?; // normal_recalculation_flags
            return Ok(None);
        }
        let angle_bits = u32::from(r.character()?);
        if angle_bits > 16 {
            return Err(malformed(format!("{angle_bits}-bit normal angles")));
        }
        let n = r.unsigned_integer()? as usize;
        let binary = arrays::bool_array(r, n)?;
        let angles = arrays::short_array(r, angle_bits)?;
        let planar = arrays::bool_array(r, faces)?;
        Ok(Some((angle_bits, binary, angles, planar)))
    }

    /// Colours, line attributes, textures and behaviours, through the
    /// entity's added schema fields; returns the per-triangle graphics.
    fn compressed_face_attributes(
        &mut self,
        face_of: &[u32],
        faces: usize,
    ) -> Result<Vec<Graphics>, PrcError> {
        let r = &mut self.r;
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
        Ok(compressed_graphics(
            face_of,
            &multi,
            &line_attributes,
            &behaviours,
            faces,
        ))
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
}

/// Per-triangle graphics of a compressed tessellation [WD 7.8.9.5]: with no
/// face flagged `is_multiple_line_attribute_on_face`, `line_attributes` holds
/// one style index + 1 per face (0 = the owner's graphics) and `behaviours`
/// one behaviour byte per face. The one-per-triangle layout of a flagged face
/// is not established, so such a tessellation keeps its owner's graphics, as
/// does one whose attributes are all 0.
pub(super) fn compressed_graphics(
    face_of: &[u32],
    multi: &[bool],
    line_attributes: &[i32],
    behaviours: &[i32],
    faces: usize,
) -> Vec<Graphics> {
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
            Graphics {
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
