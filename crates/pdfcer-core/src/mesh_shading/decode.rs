//! Decode a type 4–7 mesh stream to authored colour components
//! (§8.7.4.5.5–.8).

use super::{
    DecodedMesh, MAX_TRIANGLES, MeshError, MeshGeometry, MeshPatch, MeshTriangle, MeshVertex,
};
use crate::object::{Dict, Object};
use crate::settings::MeshPatchPadding;

/// Records (vertices or patches) read from one stream at most; the
/// renderer's own ceiling.
const MAX_RECORDS: usize = 1_000_000;

/// Colour fields per vertex at most. DeviceN allows 32 colorants
/// (Annex C, Table C.1); nothing a mesh can name has more.
pub(super) const MAX_COMPONENTS: usize = 32;

/// The stream-format entries of a mesh shading dictionary (Tables 82–86),
/// validated.
///
/// Build it with [`MeshStreamFormat::from_dict`]; the fields are then
/// guaranteed legal, which is what lets [`decode_mesh`] index nothing it
/// has not checked.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct MeshStreamFormat {
    /// 4, 5, 6 or 7.
    pub shading_type: u8,
    /// `BitsPerCoordinate` (`MSH4`).
    pub bits_per_coordinate: u32,
    /// `BitsPerComponent` (`MSH5`).
    pub bits_per_component: u32,
    /// `BitsPerFlag` (`MSH6`); 0 on type 5, which has none (`MSH7`).
    pub bits_per_flag: u32,
    /// Type 5 `VerticesPerRow`; 0 on the other types.
    pub vertices_per_row: usize,
    /// `/Decode` as `[x, y, c1 … cn]` ranges (`MSH10`).
    pub decode: Vec<[f64; 2]>,
    /// Colour fields per vertex: the space's components, or 1 when
    /// [`Self::parametric`] (`MSH14`).
    pub components: usize,
    /// The dictionary has a `/Function`, so each colour field is a `t`.
    pub parametric: bool,
    /// How type 6/7 patch records are padded (`MSH-A1`); the renderer's
    /// setting of the same name.
    pub patch_padding: MeshPatchPadding,
}

impl MeshStreamFormat {
    /// Read and validate the stream-format entries of a shading
    /// dictionary.
    ///
    /// `space_components` is the colour space's component count, which the
    /// caller resolves (a `/ColorSpace` may be an indirect `ICCBased`
    /// stream this function cannot see). Entries must be direct; an
    /// indirect number reads as absent.
    ///
    /// # Errors
    ///
    /// The [`MeshError`] naming the first illegal entry: not a mesh type,
    /// an illegal bit width, a short or non-finite `/Decode`, a type 5
    /// `VerticesPerRow` below 2, or a component count outside 1–32.
    pub fn from_dict(
        dict: &Dict,
        space_components: usize,
        patch_padding: MeshPatchPadding,
    ) -> Result<Self, MeshError> {
        let int = |key: &[u8]| dict.get(key).and_then(Object::as_int);
        let ty = int(b"ShadingType").unwrap_or(0);
        let shading_type = u8::try_from(ty)
            .ok()
            .filter(|t| (4..=7).contains(t))
            .ok_or(MeshError::NotAMesh(ty))?;
        let bpco = int(b"BitsPerCoordinate").unwrap_or(0);
        let bits_per_coordinate = legal_width(bpco, &[1, 2, 4, 8, 12, 16, 24, 32])
            .ok_or(MeshError::BadBitsPerCoordinate(bpco))?;
        let bpcp = int(b"BitsPerComponent").unwrap_or(0);
        let bits_per_component =
            legal_width(bpcp, &[1, 2, 4, 8, 12, 16]).ok_or(MeshError::BadBitsPerComponent(bpcp))?;
        let bits_per_flag = if shading_type == 5 {
            0
        } else {
            let bpf = int(b"BitsPerFlag").unwrap_or(0);
            legal_width(bpf, &[2, 4, 8]).ok_or(MeshError::BadBitsPerFlag(bpf))?
        };
        let vertices_per_row = if shading_type == 5 {
            int(b"VerticesPerRow")
                .and_then(|v| usize::try_from(v).ok())
                .filter(|v| (2..=MAX_RECORDS).contains(v))
                .ok_or(MeshError::BadVerticesPerRow)?
        } else {
            0
        };
        let parametric = dict.contains_key(b"Function");
        let components = if parametric { 1 } else { space_components };
        if components == 0 || components > MAX_COMPONENTS {
            return Err(MeshError::BadComponentCount(components));
        }
        let decode = decode_ranges(dict.get(b"Decode"), components)?;
        Ok(Self {
            shading_type,
            bits_per_coordinate,
            bits_per_component,
            bits_per_flag,
            vertices_per_row,
            decode,
            components,
            parametric,
            patch_padding,
        })
    }

    fn vertex_bits(&self, with_flag: bool) -> usize {
        let flag = if with_flag { self.bits_per_flag } else { 0 } as usize;
        flag + 2 * self.bits_per_coordinate as usize
            + self.components * self.bits_per_component as usize
    }
}

fn legal_width(value: i64, legal: &[u32]) -> Option<u32> {
    u32::try_from(value).ok().filter(|v| legal.contains(v))
}

/// `MSH10`: `4 + 2n` finite numbers; extra entries are ignored, as the
/// renderer ignores them.
fn decode_ranges(entry: Option<&Object>, components: usize) -> Result<Vec<[f64; 2]>, MeshError> {
    let needed = 4 + 2 * components;
    let numbers: Vec<f64> = entry
        .and_then(Object::as_array)
        .unwrap_or_default()
        .iter()
        .map_while(|o| o.as_number().filter(|n| n.is_finite()))
        .take(needed)
        .collect();
    if numbers.len() < needed {
        return Err(MeshError::BadDecode {
            needed,
            found: numbers.len(),
        });
    }
    Ok(numbers.as_chunks::<2>().0.to_vec())
}

/// Decode a mesh stream's (already filter-decoded) bytes.
///
/// Stream ends that stop part-way through a record keep every complete
/// record and set [`DecodedMesh::truncated`] (`MSH-N2`), as the renderer
/// does, so the geometry here is the geometry the renderer paints.
///
/// # Errors
///
/// [`MeshError::Empty`] when no complete triangle or patch decodes,
/// [`MeshError::FirstPatchNotNew`] for a type 6/7 stream whose first flag
/// is nonzero, and [`MeshError::TooManyTriangles`] past
/// [`MAX_TRIANGLES`](super::MAX_TRIANGLES) (types 4/5) or one million
/// patches — never a partial mesh past a cap.
pub fn decode_mesh(data: &[u8], format: &MeshStreamFormat) -> Result<DecodedMesh, MeshError> {
    let mut r = BitReader::new(data);
    let mut truncated = false;
    let geometry = match format.shading_type {
        4 => MeshGeometry::Triangles(free_form(&mut r, format, &mut truncated)?),
        5 => MeshGeometry::Triangles(lattice(&mut r, format, &mut truncated)?),
        _ => MeshGeometry::Patches(patches(&mut r, format, &mut truncated)?),
    };
    Ok(DecodedMesh {
        shading_type: format.shading_type,
        components: format.components,
        parametric: format.parametric,
        geometry,
        truncated,
    })
}

fn read_vertex(r: &mut BitReader<'_>, f: &MeshStreamFormat) -> Option<MeshVertex> {
    let xy = read_point(r, f)?;
    let color = read_color(r, f)?;
    Some(MeshVertex { xy, color })
}

fn read_point(r: &mut BitReader<'_>, f: &MeshStreamFormat) -> Option<[f64; 2]> {
    let bits = f.bits_per_coordinate;
    let x = decode_field(r.read(bits)?, bits, *f.decode.first()?);
    let y = decode_field(r.read(bits)?, bits, *f.decode.get(1)?);
    Some([x, y])
}

fn read_color(r: &mut BitReader<'_>, f: &MeshStreamFormat) -> Option<Vec<f64>> {
    let bits = f.bits_per_component;
    let mut out = Vec::with_capacity(f.components);
    for range in f.decode.iter().skip(2).take(f.components) {
        out.push(decode_field(r.read(bits)?, bits, *range));
    }
    Some(out)
}

/// Type 4 (`MSH15`–`MSH20`): flag 0 starts a triangle from three
/// vertices; flag 1 continues with `(vb, vc, new)`, flag 2 with
/// `(va, vc, new)`.
fn free_form(
    r: &mut BitReader<'_>,
    f: &MeshStreamFormat,
    truncated: &mut bool,
) -> Result<Vec<MeshTriangle>, MeshError> {
    let record_bits = f.vertex_bits(true);
    let mut tris: Vec<MeshTriangle> = Vec::new();
    let mut state: Option<MeshTriangle> = None;
    let next = |r: &mut BitReader<'_>| -> Option<(u64, MeshVertex)> {
        let flag = r.read(f.bits_per_flag)? & 3;
        let v = read_vertex(r, f)?;
        r.align();
        Some((flag, v))
    };
    while r.remaining() >= record_bits {
        if tris.len() >= MAX_TRIANGLES {
            return Err(MeshError::TooManyTriangles { cap: MAX_TRIANGLES });
        }
        let Some((flag, v)) = next(r) else {
            *truncated = true;
            break;
        };
        let tri = match (flag, state.take()) {
            (0, _) => {
                let (Some((_, b)), Some((_, c))) = (next(r), next(r)) else {
                    *truncated = true;
                    break;
                };
                [v, b, c]
            }
            (1, Some([_, b, c])) => [b, c, v],
            (2, Some([a, _, c])) => [a, c, v],
            (1 | 2, None) => {
                *truncated = true;
                continue;
            }
            _ => {
                *truncated = true;
                break;
            }
        };
        tris.push(tri.clone());
        state = Some(tri);
    }
    if r.remaining() > 0 && r.remaining() < record_bits {
        *truncated = true;
    }
    if tris.is_empty() {
        return Err(MeshError::Empty);
    }
    Ok(tris)
}

/// Type 5 (`MSH21`, `MSH22`): rows of `VerticesPerRow` vertices, two
/// triangles per lattice cell.
fn lattice(
    r: &mut BitReader<'_>,
    f: &MeshStreamFormat,
    truncated: &mut bool,
) -> Result<Vec<MeshTriangle>, MeshError> {
    let k = f.vertices_per_row;
    let record_bits = f.vertex_bits(false);
    let mut verts: Vec<MeshVertex> = Vec::new();
    while r.remaining() >= record_bits && verts.len() < MAX_RECORDS {
        let Some(v) = read_vertex(r, f) else {
            *truncated = true;
            break;
        };
        r.align();
        verts.push(v);
    }
    if r.remaining() > 0 && r.remaining() < record_bits || verts.len() >= MAX_RECORDS {
        *truncated = true;
    }
    if k == 0 || !verts.len().is_multiple_of(k) {
        *truncated = true;
    }
    let rows: Vec<&[MeshVertex]> = verts.chunks_exact(k.max(1)).collect();
    if rows.len() < 2 {
        return Err(MeshError::Empty);
    }
    let mut tris = Vec::new();
    for pair in rows.windows(2) {
        let [row, below] = pair else { continue };
        for (top, bottom) in row.windows(2).zip(below.windows(2)) {
            let ([a, b], [c, d]) = (top, bottom) else {
                continue;
            };
            if tris.len() + 2 > MAX_TRIANGLES {
                return Err(MeshError::TooManyTriangles { cap: MAX_TRIANGLES });
            }
            tris.push([a.clone(), b.clone(), c.clone()]);
            tris.push([b.clone(), c.clone(), d.clone()]);
        }
    }
    Ok(tris)
}

/// Stream order of a patch's control points as `(i, j)` (`MSH29`): the
/// boundary counterclockwise from `p00`, then type 7's internal cycle.
const PATCH_ORDER: [(usize, usize); 16] = [
    (0, 0),
    (0, 1),
    (0, 2),
    (0, 3),
    (1, 3),
    (2, 3),
    (3, 3),
    (3, 2),
    (3, 1),
    (3, 0),
    (2, 0),
    (1, 0),
    (1, 1),
    (1, 2),
    (2, 2),
    (2, 1),
];

/// Types 6 and 7 (Tables 85/86). A nonzero flag reuses one of the previous
/// patch's three available edges as the new `u = 0` edge, with the two
/// corner colours at its ends; internal points are never inherited.
fn patches(
    r: &mut BitReader<'_>,
    f: &MeshStreamFormat,
    truncated: &mut bool,
) -> Result<Vec<MeshPatch>, MeshError> {
    let tensor = f.shading_type == 7;
    let mut out: Vec<MeshPatch> = Vec::new();
    while let Some(flag) = r.read(f.bits_per_flag) {
        if out.len() >= MAX_RECORDS {
            return Err(MeshError::TooManyTriangles { cap: MAX_TRIANGLES });
        }
        let flag = flag & 3;
        let prev = out.last();
        if flag != 0 && prev.is_none() {
            return Err(MeshError::FirstPatchNotNew);
        }
        let Some(patch) = read_patch(r, f, flag, tensor, prev) else {
            *truncated = true;
            break;
        };
        if f.patch_padding == MeshPatchPadding::PerRecord {
            r.align();
        }
        out.push(patch);
    }
    if r.remaining() > 0 {
        *truncated = true;
    }
    if out.is_empty() {
        return Err(MeshError::Empty);
    }
    Ok(out)
}

fn read_patch(
    r: &mut BitReader<'_>,
    f: &MeshStreamFormat,
    flag: u64,
    tensor: bool,
    prev: Option<&MeshPatch>,
) -> Option<MeshPatch> {
    let read_from = if flag == 0 { 0 } else { 4 };
    let read_to = if tensor { 16 } else { 12 };
    let mut p = [[[0.0f64; 2]; 4]; 4];
    for &(i, j) in PATCH_ORDER.iter().take(read_to).skip(read_from) {
        *p.get_mut(i)?.get_mut(j)? = read_point(r, f)?;
    }
    let first_colour = if flag == 0 { 0 } else { 2 };
    let mut corner: [Vec<f64>; 4] = Default::default();
    for slot in corner.iter_mut().skip(first_colour) {
        *slot = read_color(r, f)?;
    }
    if let (true, Some(prev)) = (flag != 0, prev) {
        let (src, c0, c1): ([(usize, usize); 4], usize, usize) = match flag {
            1 => ([(0, 3), (1, 3), (2, 3), (3, 3)], 1, 2),
            2 => ([(3, 3), (3, 2), (3, 1), (3, 0)], 2, 3),
            _ => ([(3, 0), (2, 0), (1, 0), (0, 0)], 3, 0),
        };
        for (j, (si, sj)) in src.into_iter().enumerate() {
            *p.get_mut(0)?.get_mut(j)? = *prev.p.get(si)?.get(sj)?;
        }
        corner[0] = prev.corner.get(c0)?.clone();
        corner[1] = prev.corner.get(c1)?.clone();
    }
    if !tensor {
        coons_internals(&mut p);
    }
    Some(MeshPatch { p, corner })
}

/// `MSH30`: a Coons patch's internal points, §8.7.4.5.8's four `1/9 (…)`
/// equations — one template rotated by a corner.
fn coons_internals(g: &mut [[[f64; 2]; 4]; 4]) {
    let f = |pts: [[f64; 2]; 8]| -> [f64; 2] {
        let [a, b, c, d, e, h, i, j] = pts;
        let axis = |a: f64, b: f64, c: f64, d: f64, e: f64, h: f64, i: f64, j: f64| {
            (-4.0 * a + 6.0 * (b + c) - 2.0 * (d + e) + 3.0 * (h + i) - j) / 9.0
        };
        [
            axis(a[0], b[0], c[0], d[0], e[0], h[0], i[0], j[0]),
            axis(a[1], b[1], c[1], d[1], e[1], h[1], i[1], j[1]),
        ]
    };
    let [c0, c1, c2, c3] = *g;
    let (p00, p01, p02, p03) = (c0[0], c0[1], c0[2], c0[3]);
    let (p10, p13) = (c1[0], c1[3]);
    let (p20, p23) = (c2[0], c2[3]);
    let (p30, p31, p32, p33) = (c3[0], c3[1], c3[2], c3[3]);
    g[1][1] = f([p00, p01, p10, p03, p30, p31, p13, p33]);
    g[1][2] = f([p03, p02, p13, p00, p33, p32, p10, p30]);
    g[2][1] = f([p30, p31, p20, p33, p00, p01, p23, p03]);
    g[2][2] = f([p33, p32, p23, p30, p03, p02, p20, p00]);
}

/// `MSH11`: `y = Dmin + x × (Dmax − Dmin) / (2ⁿ − 1)`, with `n` this
/// field's own width.
fn decode_field(raw: u64, bits: u32, range: [f64; 2]) -> f64 {
    let max = (1u64 << bits) - 1;
    #[allow(clippy::cast_precision_loss)] // widths are at most 32 bits; exact in f64
    let frac = raw as f64 / max as f64;
    frac.mul_add(range[1] - range[0], range[0])
}

/// MSB-first bit reader over a mesh stream.
struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> BitReader<'a> {
    const fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    const fn remaining(&self) -> usize {
        (self.data.len() * 8).saturating_sub(self.pos)
    }

    fn read(&mut self, bits: u32) -> Option<u64> {
        if bits == 0 || bits > 32 || self.remaining() < bits as usize {
            return None;
        }
        let mut value: u64 = 0;
        for _ in 0..bits {
            let &byte = self.data.get(self.pos >> 3)?;
            value = (value << 1) | u64::from((byte >> (7 - (self.pos & 7))) & 1);
            self.pos += 1;
        }
        Some(value)
    }

    /// `MSH12`: pad bits after a vertex are ignored.
    const fn align(&mut self) {
        self.pos = self.pos.div_ceil(8) * 8;
    }
}
