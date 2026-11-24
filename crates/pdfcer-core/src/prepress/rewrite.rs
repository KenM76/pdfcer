//! Pieces the colour rewrites (spot to process, convert colours) share:
//! splicing replacements into a decoded content stream, finding a page's
//! appearance streams, unpacking image samples (ISO 32000-2 §8.9.5) and
//! re-sampling a shading function as a stitched type 3 function (§7.10.3,
//! §7.10.4).

use std::collections::{BTreeMap, HashMap};

use crate::content::Operation;
use crate::function::PdfFunction;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object, Stream};
use crate::view::DocumentView;

use super::inks::Planned;

/// Segments a converted shading function is joined from.
pub(crate) const SHADING_SEGMENTS: usize = 64;
/// Largest converted image, in output bytes.
pub(crate) const MAX_OUTPUT: usize = 256 << 20;

/// One replacement in a decoded stream.
pub(crate) struct Edit {
    start: usize,
    end: usize,
    text: String,
}

/// Replace a whole operation, operands included.
pub(crate) fn edit(op: &Operation<'_>, text: String) -> Edit {
    let end = op.operator.span.start + op.operator.span.len;
    let start = op
        .operands
        .first()
        .map_or(op.operator.span.start, |t| t.span.start);
    Edit { start, end, text }
}

/// `buf` with `edits` applied, or `None` when there are none.
pub(crate) fn apply(buf: &[u8], edits: &[Edit]) -> Option<Vec<u8>> {
    if edits.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(buf.len() + edits.len() * 16);
    let mut at = 0;
    for e in edits {
        out.extend_from_slice(buf.get(at..e.start).unwrap_or_default());
        out.extend_from_slice(e.text.as_bytes());
        at = e.end;
    }
    out.extend_from_slice(buf.get(at..).unwrap_or_default());
    Some(out)
}

/// `resources/<category>/<name>`, unresolved.
pub(crate) fn resource(
    view: &DocumentView<'_>,
    resources: &Dict,
    category: &[u8],
    name: &[u8],
) -> Option<Object> {
    resources
        .get(category)
        .and_then(|o| view.resolve(o).as_dict())
        .and_then(|d| d.get(name))
        .cloned()
}

/// Every stream under a page's annotations' `/AP` `/N`, `/R` and `/D`.
pub(crate) fn appearances(view: &DocumentView<'_>, page: ObjId) -> Vec<ObjId> {
    let resolve_dict = |o: &Object| view.resolve(o).as_dict().cloned();
    let annots = view
        .graph()
        .value(page)
        .and_then(Object::as_dict)
        .and_then(|d| d.get(b"Annots"))
        .and_then(|o| view.resolve(o).as_array().map(<[Object]>::to_vec))
        .unwrap_or_default();
    let mut out = Vec::new();
    for annot in annots.iter().filter_map(&resolve_dict) {
        let Some(ap) = annot.get(b"AP").and_then(resolve_dict) else {
            continue;
        };
        for key in [b"N".as_slice(), b"R", b"D"] {
            match ap.get(key) {
                Some(Object::Reference(id))
                    if matches!(view.graph().value(*id), Some(Object::Stream(_))) =>
                {
                    out.push(*id);
                }
                Some(o) => {
                    if let Some(states) = resolve_dict(o) {
                        out.extend(states.iter().filter_map(|(_, v)| v.as_reference()));
                    }
                }
                None => {}
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Each rewritten stream's decoded bytes, re-encoded as `FlateDecode`.
pub(crate) fn flate_writes(
    view: &DocumentView<'_>,
    out: BTreeMap<ObjId, Option<Vec<u8>>>,
    writes: &mut BTreeMap<ObjId, Planned>,
) {
    for (id, bytes) in out {
        if let (Some(data), Some(Object::Stream(s))) = (bytes, view.graph().value(id)) {
            let mut dict = s.dict.clone();
            dict.insert(Name::from(b"Filter"), name(b"FlateDecode"));
            dict.remove(b"DecodeParms");
            let data = crate::filters::flate::encode(&data);
            writes.insert(id, Planned::Stream { dict, data });
        }
    }
}

/// An object's value with the plan's earlier writes applied.
pub(crate) fn current(
    view: &DocumentView<'_>,
    writes: &BTreeMap<ObjId, Planned>,
    id: ObjId,
) -> Option<Object> {
    match writes.get(&id) {
        Some(Planned::Object(o)) => Some(o.clone()),
        Some(Planned::Stream { dict, .. }) => match view.graph().value(id) {
            Some(Object::Stream(s)) => Some(Object::Stream(Stream {
                dict: dict.clone(),
                data_span: s.data_span,
            })),
            other => other.cloned(),
        },
        None => view.graph().value(id).cloned(),
    }
}

/// Keep `obj`'s new value in `plan`, with any new stream data already
/// planned for it.
pub(crate) fn keep(writes: &mut BTreeMap<ObjId, Planned>, id: ObjId, obj: Object) {
    let planned = match (writes.remove(&id), obj) {
        (Some(Planned::Stream { data, .. }), Object::Stream(s)) => {
            Planned::Stream { dict: s.dict, data }
        }
        (_, obj) => Planned::Object(obj),
    };
    writes.insert(id, planned);
}

/// A name object from its bytes.
pub(crate) fn name(n: &[u8]) -> Object {
    Object::Name(Name::from(n))
}

/// An array of reals.
pub(crate) fn reals(values: &[f64]) -> Object {
    Object::Array(values.iter().map(|v| Object::Real(*v)).collect())
}

/// A 0..1 value as a byte; out-of-range values clamp.
pub(crate) fn to_byte(v: f64) -> u8 {
    // In range after the clamp.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let b = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    b
}

/// A dictionary entry resolved to an integer, if it is one.
pub(crate) fn int(view: &DocumentView<'_>, d: &Dict, key: &[u8]) -> Option<i64> {
    d.get(key).and_then(|o| view.resolve(o).as_int())
}

/// An image's size, bit depth and decoded samples, or why it has none.
pub(crate) struct ImageSamples {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) bpc: usize,
    pub(crate) data: Vec<u8>,
}

/// Read an image's samples for conversion to `out_n` 8-bit components.
pub(crate) fn image_samples(
    view: &DocumentView<'_>,
    s: &Stream,
    out_n: usize,
) -> Result<ImageSamples, String> {
    if matches!(
        s.dict.get(b"Mask").map(|o| view.resolve(o)),
        Some(Object::Array(_))
    ) {
        return Err("masks by colour key, which names sample values in its space".to_owned());
    }
    let (Some(w), Some(h), Some(bpc)) = (
        int(view, &s.dict, b"Width"),
        int(view, &s.dict, b"Height"),
        int(view, &s.dict, b"BitsPerComponent"),
    ) else {
        return Err("lacks a readable size or bit depth".to_owned());
    };
    let (Ok(width), Ok(height)) = (usize::try_from(w), usize::try_from(h)) else {
        return Err("has a negative size".to_owned());
    };
    if !matches!(bpc, 1 | 2 | 4 | 8 | 16) {
        return Err("has an unsupported bit depth".to_owned());
    }
    width
        .checked_mul(height)
        .and_then(|p| p.checked_mul(out_n))
        .filter(|n| *n <= MAX_OUTPUT)
        .ok_or_else(|| "is too large to convert".to_owned())?;
    let data = match view
        .slice(s.data_span)
        .map(|raw| crate::filters::decode_stream(&s.dict, raw))
    {
        Some(Ok(d)) => d,
        Some(Err(e)) => return Err(format!("cannot be decoded ({e})")),
        None => return Err("has no data".to_owned()),
    };
    #[allow(clippy::cast_sign_loss)] // bpc is one of 1, 2, 4, 8, 16.
    let bpc = bpc as usize;
    Ok(ImageSamples {
        width,
        height,
        bpc,
        data,
    })
}

/// Each component's `/Decode` range (Table 87) as `(base, step)` per
/// sample unit, defaulting to `defaults` (`(min, max)` per component).
pub(crate) fn decode_ranges(
    view: &DocumentView<'_>,
    dict: &Dict,
    defaults: &[(f64, f64)],
    bpc: usize,
) -> Vec<(f64, f64)> {
    let given: Vec<f64> = dict
        .get(b"Decode")
        .and_then(|o| view.resolve(o).as_array())
        .map(|a| {
            a.iter()
                .filter_map(|o| view.resolve(o).as_number())
                .collect()
        })
        .unwrap_or_default();
    #[allow(clippy::cast_precision_loss)] // At most 65535.
    let max = ((1u32 << bpc) - 1) as f64;
    defaults
        .iter()
        .enumerate()
        .map(
            |(i, (lo, hi))| match (given.get(2 * i), given.get(2 * i + 1)) {
                (Some(a), Some(b)) => (*a, (b - a) / max),
                _ => (*lo, (hi - lo) / max),
            },
        )
        .collect()
}

/// Unpack, decode and map every pixel of `img` (`n` components), memoised
/// by sample tuple. `None` when the data is short or `map` fails.
pub(crate) fn convert_samples(
    img: &ImageSamples,
    n: usize,
    decode: &[(f64, f64)],
    map: &mut dyn FnMut(&[f64]) -> Option<Vec<u8>>,
) -> Option<Vec<u8>> {
    let (w, h, bpc) = (img.width, img.height, img.bpc);
    let row = (w * n * bpc).div_ceil(8);
    if img.data.len() < row.checked_mul(h)? {
        return None;
    }
    let mut out = Vec::new();
    let mut memo: HashMap<Vec<u16>, Vec<u8>> = HashMap::new();
    let mut key = vec![0u16; n];
    let mut values = vec![0.0; n];
    for y in 0..h {
        let line = img.data.get(y * row..(y + 1) * row)?;
        for x in 0..w {
            for (i, k) in key.iter_mut().enumerate() {
                *k = sample(line, (x * n + i) * bpc, bpc);
            }
            if let Some(px) = memo.get(key.as_slice()) {
                out.extend_from_slice(px);
                continue;
            }
            for ((t, k), (base, step)) in values.iter_mut().zip(&key).zip(decode) {
                *t = base + f64::from(*k) * step;
            }
            let px = map(&values)?;
            out.extend_from_slice(&px);
            memo.insert(key.clone(), px);
        }
    }
    Some(out)
}

/// The `bpc`-bit sample starting at bit `bit` of `line`, most significant
/// bit first (§8.9.5.2).
pub(crate) fn sample(line: &[u8], bit: usize, bpc: usize) -> u16 {
    let byte = |i: usize| line.get(i).copied().unwrap_or(0);
    match bpc {
        16 => u16::from_be_bytes([byte(bit / 8), byte(bit / 8 + 1)]),
        8 => u16::from(byte(bit / 8)),
        _ => {
            let shift = 8 - bpc - bit % 8;
            u16::from((byte(bit / 8) >> shift) & ((1u8 << bpc) - 1))
        }
    }
}

/// An `Indexed` space's lookup table bytes (§8.6.6.3).
pub(crate) fn lookup_table(view: &DocumentView<'_>, table: &Object) -> Option<Vec<u8>> {
    match view.resolve(table) {
        Object::String(b) => Some(b.clone()),
        Object::Stream(s) => view
            .slice(s.data_span)
            .and_then(|raw| crate::filters::decode_stream(&s.dict, raw).ok()),
        _ => None,
    }
}

/// A shading's `/Function`: one function or an array of them.
pub(crate) fn shading_functions(view: &DocumentView<'_>, d: &Dict) -> Vec<PdfFunction> {
    match d.get(b"Function").map(|o| view.resolve(o)) {
        Some(Object::Array(fs)) => fs
            .iter()
            .filter_map(|f| PdfFunction::load(view, f).ok())
            .collect(),
        Some(f) => PdfFunction::load(view, f).into_iter().collect(),
        None => Vec::new(),
    }
}

/// A shading's `/Domain`, defaulting to `[0 1]`.
pub(crate) fn shading_domain(view: &DocumentView<'_>, d: &Dict) -> Option<(f64, f64)> {
    let domain: Vec<f64> = d
        .get(b"Domain")
        .and_then(|o| view.resolve(o).as_array())
        .map(|a| a.iter().filter_map(Object::as_number).collect())
        .unwrap_or_else(|| vec![0.0, 1.0]);
    Some((*domain.first()?, *domain.get(1)?))
}

/// The colour the shading `functions` give at `t`: one n-output function,
/// or n one-output functions (§8.7.4.5.3).
fn colour_at(functions: &[PdfFunction], n: usize, t: f64) -> Option<Vec<f64>> {
    let out: Vec<f64> = if let [f] = functions {
        f.eval(&[t]).ok()?
    } else {
        functions
            .iter()
            .map(|f| f.eval(&[t]).ok().and_then(|o| o.first().copied()))
            .collect::<Option<_>>()?
    };
    (out.len() == n).then_some(out)
}

/// A type 3 function of [`SHADING_SEGMENTS`] linear type 2 pieces over
/// `t0..t1`, sampling the shading's `n`-component colour through `map`
/// (§7.10.3, §7.10.4).
pub(crate) fn stitch(
    functions: &[PdfFunction],
    n: usize,
    (t0, t1): (f64, f64),
    map: &mut dyn FnMut(&[f64]) -> Option<Vec<f64>>,
) -> Option<Object> {
    #[allow(clippy::cast_precision_loss)] // A small constant.
    let step = (t1 - t0) / SHADING_SEGMENTS as f64;
    let mut points = Vec::with_capacity(SHADING_SEGMENTS + 1);
    for i in 0..=SHADING_SEGMENTS {
        #[allow(clippy::cast_precision_loss)] // At most SHADING_SEGMENTS.
        let t = t0 + step * i as f64;
        points.push(map(&colour_at(functions, n, t)?)?);
    }
    let mut pieces = Vec::with_capacity(SHADING_SEGMENTS);
    let mut bounds = Vec::with_capacity(SHADING_SEGMENTS - 1);
    let mut encode = Vec::with_capacity(SHADING_SEGMENTS * 2);
    for (i, pair) in points.windows(2).enumerate() {
        let [c0, c1] = pair else { continue };
        let mut piece = Dict::default();
        piece.insert(Name::from(b"FunctionType"), Object::Integer(2));
        piece.insert(Name::from(b"Domain"), reals(&[0.0, 1.0]));
        piece.insert(Name::from(b"C0"), reals(c0));
        piece.insert(Name::from(b"C1"), reals(c1));
        piece.insert(Name::from(b"N"), Object::Integer(1));
        pieces.push(Object::Dict(piece));
        if i > 0 {
            #[allow(clippy::cast_precision_loss)] // At most SHADING_SEGMENTS.
            bounds.push(t0 + step * i as f64);
        }
        encode.extend([0.0, 1.0]);
    }
    let mut f = Dict::default();
    f.insert(Name::from(b"FunctionType"), Object::Integer(3));
    f.insert(Name::from(b"Domain"), reals(&[t0, t1]));
    f.insert(Name::from(b"Functions"), Object::Array(pieces));
    f.insert(Name::from(b"Bounds"), reals(&bounds));
    f.insert(Name::from(b"Encode"), reals(&encode));
    Some(Object::Dict(f))
}

/// Operands of `op` as numbers, `None` if any is not one.
pub(crate) fn numbers(op: &Operation<'_>) -> Option<Vec<f64>> {
    op.operands
        .iter()
        .map(|t| match &t.kind {
            crate::content::ContentTokenKind::Operand(o) => o.as_number(),
            _ => None,
        })
        .collect()
}

/// `v1 ... vn op`, each value formatted as content-stream numbers.
pub(crate) fn colour_text(values: &[f64], op: &str) -> String {
    let mut s = String::new();
    for v in values {
        s.push_str(&crate::bates::number(*v));
        s.push(' ');
    }
    s.push_str(op);
    s
}
