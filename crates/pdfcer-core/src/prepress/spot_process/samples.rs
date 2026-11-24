//! The sampled side of spot to process: image samples (ISO 32000-2
//! §8.9.5), `Indexed` lookup tables (§8.6.6.3) and axial and radial
//! shadings (§8.7.4.5.3, §8.7.4.5.4), each mapped through the space's tint
//! transform.

use std::collections::HashMap;

use crate::function::PdfFunction;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object, Stream};

use super::super::inks::{InkPlan, Planned};
use super::{Conversion, Spaces, device_cmyk};

/// Segments a converted shading function is joined from.
pub(super) const SHADING_SEGMENTS: usize = 64;
/// Largest converted image, in output bytes.
const MAX_OUTPUT: usize = 256 << 20;

fn name(n: &[u8]) -> Object {
    Object::Name(Name::from(n))
}

fn reals(values: &[f64]) -> Object {
    Object::Array(values.iter().map(|v| Object::Real(*v)).collect())
}

fn to_byte(v: f64) -> u8 {
    // In range: `v` is clamped to 0..1 by `Conversion::cmyk`.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let b = (v * 255.0).round() as u8;
    b
}

fn int(view: &crate::view::DocumentView<'_>, d: &Dict, key: &[u8]) -> Option<i64> {
    d.get(key).and_then(|o| view.resolve(o).as_int())
}

/// Convert an image drawn in a converted space to 8-bit `DeviceCMYK`, or
/// `None` when its space does not convert or it is refused.
pub(super) fn image(
    spaces: &mut Spaces<'_, '_>,
    id: ObjId,
    s: &Stream,
    plan: &mut InkPlan,
) -> Option<Planned> {
    let cs = s.dict.get(b"ColorSpace")?.clone();
    let c = spaces.lookup(&cs, Some(id), plan)?;
    let view = spaces.view;
    let conv = spaces.conversions.get(c)?;
    let ink = conv.ink().to_vec();
    let refuse = |plan: &mut InkPlan, why: &str| {
        plan.report
            .refuse(Some(id), &ink, format!("the image {why}"));
        None
    };
    if matches!(
        s.dict.get(b"Mask").map(|o| view.resolve(o)),
        Some(Object::Array(_))
    ) {
        return refuse(
            plan,
            "masks by colour key, which names sample values in the spot",
        );
    }
    let (Some(w), Some(h), Some(bpc)) = (
        int(view, &s.dict, b"Width"),
        int(view, &s.dict, b"Height"),
        int(view, &s.dict, b"BitsPerComponent"),
    ) else {
        return refuse(plan, "lacks a readable size or bit depth");
    };
    let (Ok(w), Ok(h)) = (usize::try_from(w), usize::try_from(h)) else {
        return refuse(plan, "has a negative size");
    };
    if !matches!(bpc, 1 | 2 | 4 | 8 | 16) {
        return refuse(plan, "has an unsupported bit depth");
    }
    let Some(out_len) = w
        .checked_mul(h)
        .and_then(|p| p.checked_mul(4))
        .filter(|n| *n <= MAX_OUTPUT)
    else {
        return refuse(plan, "is too large to convert");
    };
    let data = match view
        .slice(s.data_span)
        .map(|raw| crate::filters::decode_stream(&s.dict, raw))
    {
        Some(Ok(d)) => d,
        Some(Err(e)) => return refuse(plan, &format!("cannot be decoded ({e})")),
        None => return refuse(plan, "has no data"),
    };
    #[allow(clippy::cast_sign_loss)] // bpc is one of 1, 2, 4, 8, 16.
    let bpc = bpc as usize;
    let decode = decode_ranges(view, &s.dict, conv.n, bpc);
    let Some(out) = convert_samples(conv, &data, w, h, bpc, &decode, out_len) else {
        return refuse(
            plan,
            "data is shorter than its size, or its tint transform fails",
        );
    };
    let mut dict = s.dict.clone();
    dict.insert(Name::from(b"ColorSpace"), device_cmyk());
    dict.insert(Name::from(b"BitsPerComponent"), Object::Integer(8));
    dict.insert(Name::from(b"Filter"), name(b"FlateDecode"));
    dict.remove(b"Decode");
    dict.remove(b"DecodeParms");
    Some(Planned::Stream {
        dict,
        data: crate::filters::flate::encode(&out),
    })
}

/// Each component's `/Decode` range (Table 87), defaulting to 0..1.
fn decode_ranges(
    view: &crate::view::DocumentView<'_>,
    dict: &Dict,
    n: usize,
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
    (0..n)
        .map(|i| match (given.get(2 * i), given.get(2 * i + 1)) {
            (Some(a), Some(b)) => (*a, (b - a) / max),
            _ => (0.0, 1.0 / max),
        })
        .collect()
}

/// Unpack, decode and convert every pixel, memoised by sample tuple.
fn convert_samples(
    conv: &Conversion,
    data: &[u8],
    w: usize,
    h: usize,
    bpc: usize,
    decode: &[(f64, f64)],
    out_len: usize,
) -> Option<Vec<u8>> {
    let n = conv.n;
    let row = (w * n * bpc).div_ceil(8);
    if data.len() < row.checked_mul(h)? {
        return None;
    }
    let mut out = Vec::with_capacity(out_len);
    let mut memo: HashMap<Vec<u16>, [u8; 4]> = HashMap::new();
    let mut key = vec![0u16; n];
    let mut tints = vec![0.0; n];
    for y in 0..h {
        let line = data.get(y * row..(y + 1) * row)?;
        for x in 0..w {
            for (i, k) in key.iter_mut().enumerate() {
                *k = sample(line, (x * n + i) * bpc, bpc);
            }
            if let Some(px) = memo.get(key.as_slice()) {
                out.extend_from_slice(px);
                continue;
            }
            for ((t, k), (base, step)) in tints.iter_mut().zip(&key).zip(decode) {
                *t = base + f64::from(*k) * step;
            }
            let px = conv.cmyk(&tints)?.map(to_byte);
            memo.insert(key.clone(), px);
            out.extend_from_slice(&px);
        }
    }
    Some(out)
}

/// The `bpc`-bit sample starting at bit `bit` of `line`, most significant
/// bit first (§8.9.5.2).
fn sample(line: &[u8], bit: usize, bpc: usize) -> u16 {
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

/// `[/Indexed base hival lookup]` over a converted base, as
/// `[/Indexed /DeviceCMYK hival lookup']`; `None` for any other array.
pub(super) fn indexed(
    spaces: &mut Spaces<'_, '_>,
    items: &[Object],
    at: Option<ObjId>,
    plan: &mut InkPlan,
) -> Option<Object> {
    let view = spaces.view;
    let head = items.first().and_then(|o| view.resolve(o).as_name())?;
    if head.as_bytes() != b"Indexed" {
        return None;
    }
    let c = spaces.lookup(items.get(1)?, at, plan)?;
    let conv = spaces.conversions.get(c)?;
    let hival = usize::try_from(view.resolve(items.get(2)?).as_int()?).ok()?;
    let table = match view.resolve(items.get(3)?) {
        Object::String(b) => b.clone(),
        Object::Stream(s) => view
            .slice(s.data_span)
            .and_then(|raw| crate::filters::decode_stream(&s.dict, raw).ok())?,
        _ => return None,
    };
    let mut out = Vec::with_capacity((hival + 1) * 4);
    for entry in 0..=hival {
        let bytes = table.get(entry * conv.n..(entry + 1) * conv.n);
        let Some(cmyk) = bytes.and_then(|b| {
            let tints: Vec<f64> = b.iter().map(|v| f64::from(*v) / 255.0).collect();
            conv.cmyk(&tints)
        }) else {
            let why = "its indexed lookup table is short or its tint transform fails".to_owned();
            plan.report.refuse(at, conv.ink(), why);
            return None;
        };
        out.extend(cmyk.map(to_byte));
    }
    Some(Object::Array(vec![
        name(b"Indexed"),
        device_cmyk(),
        items.get(2)?.clone(),
        Object::String(out),
    ]))
}

/// Convert an axial or radial shading in place; true when it changed.
pub(super) fn shading(
    spaces: &mut Spaces<'_, '_>,
    d: &mut Dict,
    at: Option<ObjId>,
    plan: &mut InkPlan,
) -> bool {
    let view = spaces.view;
    let Some(cs) = d.get(b"ColorSpace").cloned() else {
        return false;
    };
    let Some(c) = spaces.lookup(&cs, at, plan) else {
        return false;
    };
    let Some(conv) = spaces.conversions.get(c) else {
        return false;
    };
    let kind = int(view, d, b"ShadingType").unwrap_or(0);
    let refuse = |plan: &mut InkPlan, why: String| {
        plan.report.refuse(at, conv.ink(), why);
        false
    };
    if !matches!(kind, 2 | 3) {
        return refuse(
            plan,
            format!(
                "a type {kind} shading paints in it; only axial and radial shadings are converted"
            ),
        );
    }
    let functions: Vec<PdfFunction> = match d.get(b"Function").map(|o| view.resolve(o)) {
        Some(Object::Array(fs)) => fs
            .iter()
            .filter_map(|f| PdfFunction::load(view, f).ok())
            .collect(),
        Some(f) => PdfFunction::load(view, f).into_iter().collect(),
        None => Vec::new(),
    };
    let domain: Vec<f64> = d
        .get(b"Domain")
        .and_then(|o| view.resolve(o).as_array())
        .map(|a| a.iter().filter_map(Object::as_number).collect())
        .unwrap_or_else(|| vec![0.0, 1.0]);
    let (Some(t0), Some(t1)) = (domain.first().copied(), domain.get(1).copied()) else {
        return refuse(plan, "a shading's domain cannot be read".to_owned());
    };
    let Some(stitched) = stitch(conv, &functions, t0, t1) else {
        return refuse(plan, "a shading's function cannot be evaluated".to_owned());
    };
    if let Some(bg) = d
        .get(b"Background")
        .and_then(|o| view.resolve(o).as_array())
    {
        let tints: Vec<f64> = bg.iter().filter_map(Object::as_number).collect();
        match conv.cmyk(&tints).filter(|_| tints.len() == conv.n) {
            Some(cmyk) => d.insert(Name::from(b"Background"), reals(&cmyk)),
            None => {
                d.remove(b"Background");
            }
        }
    }
    d.insert(Name::from(b"ColorSpace"), device_cmyk());
    d.insert(Name::from(b"Function"), stitched);
    true
}

/// The tints the shading `functions` give at `t`: one n-output function, or
/// n one-output functions (§8.7.4.5.3).
fn tints_at(functions: &[PdfFunction], n: usize, t: f64) -> Option<Vec<f64>> {
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
/// `t0..t1`, sampling the shading through the tint transform (§7.10.3,
/// §7.10.4).
fn stitch(conv: &Conversion, functions: &[PdfFunction], t0: f64, t1: f64) -> Option<Object> {
    #[allow(clippy::cast_precision_loss)] // A small constant.
    let step = (t1 - t0) / SHADING_SEGMENTS as f64;
    let mut points = Vec::with_capacity(SHADING_SEGMENTS + 1);
    for i in 0..=SHADING_SEGMENTS {
        #[allow(clippy::cast_precision_loss)] // At most SHADING_SEGMENTS.
        let t = t0 + step * i as f64;
        points.push(conv.cmyk(&tints_at(functions, conv.n, t)?)?);
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
