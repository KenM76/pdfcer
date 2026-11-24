//! The sampled side of spot to process: image samples (ISO 32000-2
//! §8.9.5), `Indexed` lookup tables (§8.6.6.3) and axial and radial
//! shadings (§8.7.4.5.3, §8.7.4.5.4), each mapped through the space's tint
//! transform.

use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object, Stream};

use super::super::inks::{InkPlan, Planned};
use super::super::rewrite::{
    convert_samples, decode_ranges, image_samples, int, lookup_table, name, reals, shading_domain,
    shading_functions, stitch, to_byte,
};
use super::{Spaces, device_cmyk};

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
    let img = match image_samples(view, s, 4) {
        Ok(img) => img,
        Err(why) => return refuse(plan, &why),
    };
    let decode = decode_ranges(view, &s.dict, &vec![(0.0, 1.0); conv.n], img.bpc);
    let mut map = |t: &[f64]| conv.cmyk(t).map(|c| c.map(to_byte).to_vec());
    let Some(out) = convert_samples(&img, conv.n, &decode, &mut map) else {
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
    let table = lookup_table(view, items.get(3)?)?;
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
    let functions = shading_functions(view, d);
    let Some(domain) = shading_domain(view, d) else {
        return refuse(plan, "a shading's domain cannot be read".to_owned());
    };
    let mut map = |t: &[f64]| conv.cmyk(t).map(|c| c.to_vec());
    let Some(stitched) = stitch(&functions, conv.n, domain, &mut map) else {
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
