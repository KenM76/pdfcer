//! Write a free-form triangle mesh as a `/ShadingType 4` stream
//! (§8.7.4.5.5, Table 84).
//!
//! The record layout is fixed and byte-aligned: an 8-bit flag, 32-bit
//! coordinates and 8- or 16-bit components, so every vertex ends on a byte
//! boundary and `MSH12`'s padding never arises. `/Decode` spans the data's
//! own bounds, which spends every code on values the mesh uses.

use super::decode::MAX_COMPONENTS;
use super::{MAX_TRIANGLES, MeshError, MeshTriangle};
use crate::object::{Dict, Name, Object};

const BITS_PER_COORDINATE: u32 = 32;
const BITS_PER_FLAG: u8 = 8;

/// What [`write_free_form_mesh`] writes besides the vertices.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshWriteOptions {
    color_space: Object,
    components: usize,
    bits_per_component: u8,
    background: Option<Vec<f64>>,
    bbox: Option<[f64; 4]>,
    anti_alias: bool,
    function: Option<Object>,
    cap: usize,
}

impl MeshWriteOptions {
    /// Write vertices of `components` colour values in `color_space`, the
    /// `/ColorSpace` object written verbatim (a family name, an array, or a
    /// reference to one). 16-bit components, no `Background`, `BBox`,
    /// `AntiAlias` or `Function`, and the [`MAX_TRIANGLES`](super::MAX_TRIANGLES)
    /// cap.
    #[must_use]
    pub const fn new(color_space: Object, components: usize) -> Self {
        Self {
            color_space,
            components,
            bits_per_component: 16,
            background: None,
            bbox: None,
            anti_alias: false,
            function: None,
            cap: MAX_TRIANGLES,
        }
    }

    /// `BitsPerComponent` for the colour fields: 8 or 16.
    #[must_use]
    pub const fn with_bits_per_component(mut self, bits: u8) -> Self {
        self.bits_per_component = bits;
        self
    }

    /// `/Background`, one value per colour-space component.
    #[must_use]
    pub fn with_background(mut self, background: Vec<f64>) -> Self {
        self.background = Some(background);
        self
    }

    /// `/BBox` as `[llx lly urx ury]` in the shading's target space.
    #[must_use]
    pub const fn with_bbox(mut self, bbox: [f64; 4]) -> Self {
        self.bbox = Some(bbox);
        self
    }

    /// `/AntiAlias`.
    #[must_use]
    pub const fn with_anti_alias(mut self, anti_alias: bool) -> Self {
        self.anti_alias = anti_alias;
        self
    }

    /// `/Function`, written verbatim. Each vertex then carries one
    /// parametric `t` (`MSH14`), and `components` is ignored for the data.
    #[must_use]
    pub fn with_function(mut self, function: Object) -> Self {
        self.function = Some(function);
        self
    }

    /// Decline past `cap` triangles (clamped to the module cap).
    #[must_use]
    pub fn with_triangle_cap(mut self, cap: usize) -> Self {
        self.cap = cap.min(MAX_TRIANGLES);
        self
    }

    fn fields(&self) -> usize {
        if self.function.is_some() {
            1
        } else {
            self.components
        }
    }
}

/// What [`write_free_form_mesh`] produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct MeshWriteReport {
    /// Triangles in the stream.
    pub triangles: usize,
    /// Vertex records written.
    pub vertices: usize,
    /// Triangles written as one vertex continuing the previous triangle
    /// (edge flag 1 or 2) rather than three.
    pub shared_edges: usize,
    /// `BitsPerComponent` used.
    pub bits_per_component: u8,
}

/// A written mesh shading: the stream dictionary and its encoded bytes.
///
/// `dict` carries `/Length` and `/Filter /FlateDecode`; store the pair as
/// one indirect stream object (`MSH1`: a mesh shading is a stream).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct WrittenMesh {
    /// The shading dictionary, stream entries included.
    pub dict: Dict,
    /// The stream's bytes, as `dict`'s `/Filter` encodes them.
    pub data: Vec<u8>,
    /// Counts.
    pub report: MeshWriteReport,
}

/// Write `triangles` as a type 4 shading, in the given (paint) order.
///
/// A triangle sharing two quantised vertices with its predecessor's
/// continuable edge is written as one vertex with flag 1 or 2 (`MSH17`);
/// vertex order within a triangle does not affect how it paints.
///
/// # Errors
///
/// - [`MeshError::Empty`] for no triangles; [`MeshError::TooManyTriangles`]
///   past the cap, with no output.
/// - [`MeshError::NonFinite`] / [`MeshError::ComponentMismatch`] naming the
///   first bad triangle; [`MeshError::RangeTooWide`] when a value span
///   overflows.
/// - [`MeshError::ColorSpaceMismatch`] when the colour space is `Indexed`,
///   `Pattern`, an unknown family, or declares a component count other
///   than `components`; [`MeshError::BadComponentCount`],
///   [`MeshError::BadOutputBits`], [`MeshError::BadBackground`].
///
/// # Example
///
/// ```
/// use pdfcer_core::mesh_shading::{write_free_form_mesh, MeshVertex, MeshWriteOptions};
/// use pdfcer_core::object::{Name, Object};
///
/// let v = |x: f64, y: f64| MeshVertex { xy: [x, y], color: vec![0.0, 0.2, 0.9, 0.0] };
/// let opts = MeshWriteOptions::new(Object::Name(Name::from(b"DeviceCMYK")), 4);
/// let mesh = write_free_form_mesh(&[[v(0.0, 0.0), v(10.0, 0.0), v(0.0, 10.0)]], &opts)?;
/// assert_eq!(mesh.dict.get(b"ShadingType").and_then(Object::as_int), Some(4));
/// # Ok::<(), pdfcer_core::mesh_shading::MeshError>(())
/// ```
pub fn write_free_form_mesh(
    triangles: &[MeshTriangle],
    options: &MeshWriteOptions,
) -> Result<WrittenMesh, MeshError> {
    let fields = options.fields();
    validate(triangles, options, fields)?;
    let ranges = bounds(triangles, fields)?;
    let bpc = options.bits_per_component;
    let quantised: Vec<[Vec<u32>; 3]> = triangles
        .iter()
        .map(|t| t.clone().map(|v| quantise(&v.xy, &v.color, &ranges, bpc)))
        .collect();
    let mut report = MeshWriteReport {
        triangles: triangles.len(),
        bits_per_component: bpc,
        ..MeshWriteReport::default()
    };
    let raw = encode_records(&quantised, bpc, &mut report);
    let (data, filtered) = match zlib(&raw) {
        Some(z) => (z, true),
        None => (raw, false),
    };
    let dict = shading_dict(options, &ranges, data.len(), filtered);
    Ok(WrittenMesh { dict, data, report })
}

fn validate(
    triangles: &[MeshTriangle],
    options: &MeshWriteOptions,
    fields: usize,
) -> Result<(), MeshError> {
    if !matches!(options.bits_per_component, 8 | 16) {
        return Err(MeshError::BadOutputBits(options.bits_per_component));
    }
    if options.components == 0 || options.components > MAX_COMPONENTS {
        return Err(MeshError::BadComponentCount(options.components));
    }
    if declared_components(&options.color_space)
        .is_some_and(|n| n.is_none_or(|n| n != options.components))
    {
        return Err(MeshError::ColorSpaceMismatch {
            components: options.components,
        });
    }
    if let Some(bg) = &options.background
        && (bg.len() != options.components || !bg.iter().all(|c| c.is_finite()))
    {
        return Err(MeshError::BadBackground);
    }
    if triangles.is_empty() {
        return Err(MeshError::Empty);
    }
    if triangles.len() > options.cap {
        return Err(MeshError::TooManyTriangles { cap: options.cap });
    }
    for (i, t) in triangles.iter().enumerate() {
        for v in t {
            if v.color.len() != fields {
                return Err(MeshError::ComponentMismatch {
                    triangle: i,
                    expected: fields,
                    found: v.color.len(),
                });
            }
            if !v.xy.iter().chain(&v.color).all(|c| c.is_finite()) {
                return Err(MeshError::NonFinite { triangle: i });
            }
        }
    }
    Ok(())
}

/// The component count a colour-space object declares.
///
/// `None`: not checkable here (an `ICCBased` stream or a reference), so the
/// caller's count is trusted. `Some(None)`: a mesh cannot use it
/// (`Indexed`, `Pattern` — `MSH-A4`, §8.7.4.5.1) or the family is unknown.
fn declared_components(space: &Object) -> Option<Option<usize>> {
    let (family, rest) = match space {
        Object::Name(n) => (n.as_bytes(), &[][..]),
        Object::Array(a) => match a.split_first() {
            Some((Object::Name(n), rest)) => (n.as_bytes(), rest),
            _ => return Some(None),
        },
        Object::Reference(_) => return None,
        _ => return Some(None),
    };
    Some(match family {
        b"DeviceGray" | b"CalGray" | b"Separation" => Some(1),
        b"DeviceRGB" | b"CalRGB" | b"Lab" => Some(3),
        b"DeviceCMYK" => Some(4),
        b"DeviceN" => rest.first().and_then(Object::as_array).map(<[Object]>::len),
        b"ICCBased" => return None,
        _ => None,
    })
}

/// `[lo, hi]` per field (`x`, `y`, then colour); a constant field gets
/// `[v, v + 1]`, so its code 0 decodes to `v` exactly.
fn bounds(triangles: &[MeshTriangle], fields: usize) -> Result<Vec<[f64; 2]>, MeshError> {
    let mut ranges = vec![[f64::INFINITY, f64::NEG_INFINITY]; 2 + fields];
    for v in triangles.iter().flatten() {
        for (r, &x) in ranges.iter_mut().zip(v.xy.iter().chain(&v.color)) {
            *r = [r[0].min(x), r[1].max(x)];
        }
    }
    for r in &mut ranges {
        if !(r[1] - r[0]).is_finite() {
            return Err(MeshError::RangeTooWide);
        }
        if r[1] <= r[0] {
            r[1] = r[0] + 1.0;
            if r[1] <= r[0] {
                // `v + 1 == v` at this magnitude; widen proportionally.
                r[1] = r[0] + r[0].abs() * f64::EPSILON * 2.0;
            }
        }
    }
    Ok(ranges)
}

/// `MSH11` inverted, rounded to nearest: `[x, y, c1 … cn]` as codes.
fn quantise(xy: &[f64; 2], color: &[f64], ranges: &[[f64; 2]], bpc: u8) -> Vec<u32> {
    let code = |value: f64, range: [f64; 2], bits: u32| -> u32 {
        let max = f64::from(u32::MAX >> (32 - bits));
        let t = ((value - range[0]) / (range[1] - range[0])).clamp(0.0, 1.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to 0..=max
        let q = (t * max).round() as u32;
        q
    };
    xy.iter()
        .zip(ranges)
        .map(|(&v, &r)| code(v, r, BITS_PER_COORDINATE))
        .chain(
            color
                .iter()
                .zip(ranges.iter().skip(2))
                .map(|(&v, &r)| code(v, r, u32::from(bpc))),
        )
        .collect()
}

/// The records: a flag-0 triangle is three vertices; one continuing the
/// previous triangle's `vb vc` (flag 1) or `va vc` (flag 2) edge is one.
fn encode_records(tris: &[[Vec<u32>; 3]], bpc: u8, report: &mut MeshWriteReport) -> Vec<u8> {
    let mut out = Vec::new();
    let mut prev: Option<[Vec<u32>; 3]> = None;
    for t in tris {
        let shared = prev.as_ref().and_then(|p| continuation(p, t));
        let state = if let Some((flag, new, next)) = shared {
            push_vertex(&mut out, flag, new, bpc);
            report.vertices += 1;
            report.shared_edges += 1;
            next
        } else {
            for v in t {
                push_vertex(&mut out, 0, v, bpc);
            }
            report.vertices += 3;
            t.clone()
        };
        prev = Some(state);
    }
    out
}

/// A flag, the new vertex, and the decoder's next `(va, vb, vc)`.
type Continuation<'a> = (u8, &'a Vec<u32>, [Vec<u32>; 3]);

/// Whether `t` continues `prev` through an available edge: the flag, the
/// new vertex, and the decoder's next `(va, vb, vc)`.
fn continuation<'a>(prev: &[Vec<u32>; 3], t: &'a [Vec<u32>; 3]) -> Option<Continuation<'a>> {
    let [a, b, c] = prev;
    for (flag, (e0, e1)) in [(1u8, (b, c)), (2u8, (a, c))] {
        let others: Vec<&Vec<u32>> = t.iter().filter(|v| *v != e0 && *v != e1).collect();
        let has_edge = t.contains(e0) && t.contains(e1);
        if let (true, [new]) = (has_edge && e0 != e1, others.as_slice()) {
            return Some((flag, new, [e0.clone(), e1.clone(), (*new).clone()]));
        }
    }
    None
}

fn push_vertex(out: &mut Vec<u8>, flag: u8, codes: &[u32], bpc: u8) {
    out.push(flag);
    for (i, &c) in codes.iter().enumerate() {
        if i < 2 {
            out.extend_from_slice(&c.to_be_bytes());
        } else if bpc == 8 {
            out.push(u8::try_from(c).unwrap_or(u8::MAX));
        } else {
            out.extend_from_slice(&u16::try_from(c).unwrap_or(u16::MAX).to_be_bytes());
        }
    }
}

fn zlib(raw: &[u8]) -> Option<Vec<u8>> {
    use std::io::Write as _;
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    z.write_all(raw).ok()?;
    z.finish().ok()
}

fn shading_dict(
    options: &MeshWriteOptions,
    ranges: &[[f64; 2]],
    length: usize,
    filtered: bool,
) -> Dict {
    let int = |v: i64| Object::Integer(v);
    let mut d = Dict::new();
    d.insert(Name::from(b"ShadingType"), int(4));
    d.insert(Name::from(b"ColorSpace"), options.color_space.clone());
    if let Some(bg) = &options.background {
        d.insert(Name::from(b"Background"), reals(bg));
    }
    if let Some(bbox) = options.bbox {
        d.insert(Name::from(b"BBox"), reals(&bbox));
    }
    if options.anti_alias {
        d.insert(Name::from(b"AntiAlias"), Object::Boolean(true));
    }
    d.insert(Name::from(b"BitsPerCoordinate"), int(32));
    d.insert(
        Name::from(b"BitsPerComponent"),
        int(i64::from(options.bits_per_component)),
    );
    d.insert(Name::from(b"BitsPerFlag"), int(i64::from(BITS_PER_FLAG)));
    let flat: Vec<f64> = ranges.iter().flatten().copied().collect();
    d.insert(Name::from(b"Decode"), reals(&flat));
    if let Some(f) = &options.function {
        d.insert(Name::from(b"Function"), f.clone());
    }
    if filtered {
        d.insert(
            Name::from(b"Filter"),
            Object::Name(Name::from(b"FlateDecode")),
        );
    }
    d.insert(
        Name::from(b"Length"),
        int(i64::try_from(length).unwrap_or(i64::MAX)),
    );
    d
}

fn reals(values: &[f64]) -> Object {
    Object::Array(values.iter().map(|&v| Object::Real(v)).collect())
}
