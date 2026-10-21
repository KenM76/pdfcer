//! Reading an image's colour space for conversion and turning its samples
//! into 8-bit values in that space: `/Decode` applied (ISO 32000-2
//! §8.9.5.2), palette entries looked up (§8.6.6.3), and grey computed by
//! the device formulas of §10.4.2.2 and §10.4.2.3.

use crate::deskew::Raster;
use crate::filters;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};
use crate::view::DocumentView;

/// How colour values relate to grey.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Family {
    Gray,
    Rgb,
    Cmyk,
    /// `Separation` or `DeviceN`: a palette may sit over it, grey may not.
    Tint,
}

/// A colour space samples can be written in, as an `/Indexed` base or
/// converted from.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Base {
    pub(crate) family: Family,
    pub(crate) components: u32,
    /// Family name for the report.
    pub(crate) name: String,
    /// CIE-based (`ICCBased`, `CalGray`, `CalRGB`): a device formula over
    /// it is not colour-managed.
    pub(crate) cie: bool,
    /// The colour-space object as written, for an `/Indexed` base.
    pub(crate) object: Object,
}

/// An image's colour, resolved for conversion.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SourceColour {
    pub(crate) base: Base,
    /// An `/Indexed` source's `(hival, lookup)`; the lookup holds at least
    /// `(hival + 1) x components` bytes.
    pub(crate) palette: Option<(u32, Vec<u8>)>,
}

/// The formula grey is computed by from `family`, when it is computed.
pub(crate) const fn gray_formula(family: Family) -> Option<&'static str> {
    match family {
        Family::Rgb => Some("ISO 32000-2 10.4.2.2: 0.3 R + 0.59 G + 0.11 B"),
        Family::Cmyk => Some("ISO 32000-2 10.4.2.3: 1 - min(1, 0.3 C + 0.59 M + 0.11 Y + K)"),
        Family::Gray | Family::Tint => None,
    }
}

/// Resolve the image's `/ColorSpace`, or say why it cannot be converted.
pub(crate) fn resolve(view: &DocumentView<'_>, dict: &Dict) -> Result<SourceColour, String> {
    let written = dict
        .get(b"ColorSpace")
        .ok_or_else(|| "it has no colour space".to_owned())?;
    let cs = view.resolve(written);
    if let Object::Array(a) = cs
        && matches!(a.first(), Some(Object::Name(n)) if matches!(n.as_bytes(), b"Indexed" | b"I"))
    {
        return indexed(view, a);
    }
    Ok(SourceColour {
        base: base(view, written)?,
        palette: None,
    })
}

fn indexed(view: &DocumentView<'_>, a: &[Object]) -> Result<SourceColour, String> {
    let (Some(base_obj), Some(hival), Some(lookup)) = (a.get(1), a.get(2), a.get(3)) else {
        return Err("its /Indexed colour space is incomplete".to_owned());
    };
    let base = base(view, base_obj)?;
    let hival = match view.resolve(hival) {
        Object::Integer(n) if (0..=255).contains(n) => u32::try_from(*n).unwrap_or(0),
        _ => return Err("its /Indexed hival is not 0 to 255".to_owned()),
    };
    let mut bytes = match view.resolve(lookup) {
        Object::String(s) => s.clone(),
        Object::Stream(st) => view
            .slice(st.data_span)
            .and_then(|raw| filters::decode_stream(&st.dict, raw).ok())
            .ok_or_else(|| "its /Indexed lookup stream cannot be read".to_owned())?,
        _ => return Err("its /Indexed lookup is not a string or stream".to_owned()),
    };
    let need = (hival as usize + 1) * base.components as usize;
    if bytes.len() < need {
        return Err("its /Indexed lookup is shorter than hival needs".to_owned());
    }
    bytes.truncate(need);
    Ok(SourceColour {
        base,
        palette: Some((hival, bytes)),
    })
}

/// A non-indexed colour space whose components run 0 to 1.
fn base(view: &DocumentView<'_>, written: &Object) -> Result<Base, String> {
    let cs = view.resolve(written);
    let (head, params) = match cs {
        Object::Name(n) => (n.as_bytes(), None),
        Object::Array(a) => match a.first() {
            Some(Object::Name(n)) => (n.as_bytes(), a.get(1).map(|p| view.resolve(p))),
            _ => return Err("its colour space is malformed".to_owned()),
        },
        _ => return Err("its colour space is malformed".to_owned()),
    };
    let make = |family, components, cie| Base {
        family,
        components,
        name: String::from_utf8_lossy(head).into_owned(),
        cie,
        object: written.clone(),
    };
    Ok(match head {
        b"DeviceGray" | b"G" => make(Family::Gray, 1, false),
        b"DeviceRGB" | b"RGB" => make(Family::Rgb, 3, false),
        b"DeviceCMYK" | b"CMYK" => make(Family::Cmyk, 4, false),
        b"CalGray" => make(Family::Gray, 1, true),
        b"CalRGB" => make(Family::Rgb, 3, true),
        b"ICCBased" => {
            let n = icc_components(params)?;
            let family = match n {
                1 => Family::Gray,
                3 => Family::Rgb,
                _ => Family::Cmyk,
            };
            make(family, n, true)
        }
        b"Separation" => make(Family::Tint, 1, false),
        b"DeviceN" => match params {
            Some(Object::Array(names)) if !names.is_empty() && names.len() <= 32 => {
                make(Family::Tint, u32::try_from(names.len()).unwrap_or(1), false)
            }
            _ => return Err("its DeviceN colour space is malformed".to_owned()),
        },
        other => {
            return Err(format!(
                "its {} colour space is not converted",
                String::from_utf8_lossy(other)
            ));
        }
    })
}

/// `/N` of an `ICCBased` stream whose `/Range`, if any, is 0 to 1.
fn icc_components(params: Option<&Object>) -> Result<u32, String> {
    let Some(Object::Stream(s)) = params else {
        return Err("its ICCBased colour space has no profile stream".to_owned());
    };
    let n = match s.dict.get(b"N") {
        Some(Object::Integer(n @ (1 | 3 | 4))) => u32::try_from(*n).unwrap_or(1),
        _ => return Err("its ICCBased /N is not 1, 3 or 4".to_owned()),
    };
    if let Some(Object::Array(r)) = s.dict.get(b"Range") {
        let unit = r
            .chunks(2)
            .all(|p| matches!(p, [lo, hi] if number(lo) == Some(0.0) && number(hi) == Some(1.0)));
        if !unit {
            return Err("its ICCBased /Range is not 0 to 1".to_owned());
        }
    }
    Ok(n)
}

fn number(o: &Object) -> Option<f64> {
    match o {
        Object::Integer(n) => Some(*n as f64),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}

/// The image's samples, laid out as `raster`, as 8-bit values in
/// `colour`'s base space, one byte per component, rows unpadded: `/Decode`
/// (`decode`, pairs per raster component) applied and palette entries
/// looked up.
pub(crate) fn base_values(
    raster: &Raster,
    samples: &[u8],
    colour: &SourceColour,
    decode: Option<&[f64]>,
) -> Vec<u8> {
    let max = raster.max();
    let luts: Vec<Vec<u8>> = (0..raster.components as usize)
        .map(|c| {
            let pair = decode.and_then(|d| d.get(2 * c..2 * c + 2));
            lut(max, pair, colour.palette.as_ref().map(|(h, _)| *h))
        })
        .collect();
    let n = colour.base.components as usize;
    let pixels = raster.width as usize * raster.height as usize;
    let mut out = Vec::with_capacity(pixels.saturating_mul(n));
    for y in 0..raster.height {
        for x in 0..raster.width {
            for (c, table) in luts.iter().enumerate() {
                let raw = raster.get(samples, x, y, c as u32) as usize;
                let v = table.get(raw).copied().unwrap_or(0);
                match &colour.palette {
                    Some((_, lookup)) => {
                        let at = usize::from(v) * n;
                        out.extend_from_slice(lookup.get(at..at + n).unwrap_or(&[]));
                    }
                    None => out.push(v),
                }
            }
        }
    }
    out
}

/// Raw sample value to output byte: a palette index clamped to `hival`, or
/// a component scaled 0 to 255, through the `[Dmin, Dmax]` decode pair.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)] // every value is clamped into u8 range first
fn lut(max: u32, pair: Option<&[f64]>, hival: Option<u32>) -> Vec<u8> {
    let top = f64::from(max.max(1));
    let (lo, hi) = match (pair, hival) {
        (Some([lo, hi]), _) => (*lo, *hi),
        (_, Some(_)) => (0.0, top),
        _ => (0.0, 1.0),
    };
    (0..=max)
        .map(|raw| {
            let v = lo + f64::from(raw) * (hi - lo) / top;
            match hival {
                Some(h) => v.round().clamp(0.0, f64::from(h)) as u8,
                None => (v.clamp(0.0, 1.0) * 255.0).round() as u8,
            }
        })
        .collect()
}

/// One grey byte per pixel from `values` in `family` (one byte per
/// component, `n` components).
pub(crate) fn to_gray(values: &[u8], family: Family, n: usize) -> Vec<u8> {
    values
        .chunks_exact(n.max(1))
        .map(|p| {
            let w = |i: usize| u32::from(p.get(i).copied().unwrap_or(0));
            let g = match family {
                Family::Rgb => (30 * w(0) + 59 * w(1) + 11 * w(2) + 50) / 100,
                Family::Cmyk => {
                    let ink = (30 * w(0) + 59 * w(1) + 11 * w(2) + 100 * w(3)).min(25_500);
                    255 - (ink + 50) / 100
                }
                Family::Gray | Family::Tint => w(0),
            };
            u8::try_from(g).unwrap_or(u8::MAX)
        })
        .collect()
}

/// `/Decode` as numbers, when every entry is one.
pub(crate) fn decode_array(dict: &Dict) -> Option<Vec<f64>> {
    match dict.get(b"Decode") {
        Some(Object::Array(a)) => a.iter().map(number).collect(),
        _ => None,
    }
}
