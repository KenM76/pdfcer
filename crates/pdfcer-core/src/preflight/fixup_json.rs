//! A profile rule's `fixups` member: each [`Fixup`] as a JSON object.
//!
//! ```json
//! "fixups": [{"fixup": "downsample-images", "above_ppi": 300, "to_ppi": 300,
//!             "method": "bicubic"}]
//! ```
//!
//! Parameters are listed on [`super::Profile::from_json`]. ICC bytes are never
//! written: a conversion's profile is always the one supplied at apply time.

use pdfcer_json::Json;

use super::fixup::Fixup;
use super::profile::{number, quote};
use super::{PdfuaPart, PdfxLevel, ProfileError};
use crate::color::RenderingIntent;
use crate::edit::{FixPdfuaOptions, PdfuaFixup};
use crate::optimize::ResampleMethod;
use crate::prepress::{
    ColourConversion, ColourDestination, InkSelection, IntentChoice, PdfxConversion,
    PdfxOutputIntent,
};
use crate::transparency::FlattenOptions;

const INTENTS: [RenderingIntent; 4] = [
    RenderingIntent::AbsoluteColorimetric,
    RenderingIntent::RelativeColorimetric,
    RenderingIntent::Saturation,
    RenderingIntent::Perceptual,
];

const PDFUA_STEPS: [PdfuaFixup; 5] = [
    PdfuaFixup::Language,
    PdfuaFixup::DisplayDocTitle,
    PdfuaFixup::TabOrder,
    PdfuaFixup::Artifacts,
    PdfuaFixup::Identification,
];

/// `, "fixups": [...]` for a rule, or nothing when it has none.
pub(super) fn fixups_json(fixups: &[Fixup]) -> String {
    if fixups.is_empty() {
        return String::new();
    }
    let items: Vec<String> = fixups.iter().map(fixup_json).collect();
    format!(", \"fixups\": [{}]", items.join(", "))
}

fn fixup_json(fixup: &Fixup) -> String {
    let params = match fixup {
        Fixup::FlattenTransparency(o) => flatten_json(o),
        Fixup::FixHairlines { min_width_pt } => format!(", \"min_width_pt\": {min_width_pt}"),
        Fixup::AliasInk { from, to } => {
            format!(", \"from\": {}, \"to\": {}", quote(from), quote(to))
        }
        Fixup::SpotToProcess { keep } => format!(", \"keep\": {}", strings_json(keep)),
        Fixup::ConvertColours(c) => colours_json(c),
        Fixup::ConvertToPdfx { level, conversion } => pdfx_json(*level, conversion),
        Fixup::FixPdfua {
            part,
            options,
            step,
        } => {
            let mut s = format!(", \"part\": {}", part.number());
            push_opt(&mut s, "lang", options.lang.as_deref());
            push_opt(&mut s, "title", options.title.as_deref());
            push_opt(&mut s, "step", step.map(PdfuaFixup::as_str));
            s
        }
        Fixup::DownsampleImages {
            above_ppi,
            to_ppi,
            method,
        } => format!(
            ", \"above_ppi\": {above_ppi}, \"to_ppi\": {to_ppi}, \"method\": \"{}\"",
            method.as_str()
        ),
    };
    format!("{{\"fixup\": \"{}\"{params}}}", fixup.name())
}

fn flatten_json(o: &FlattenOptions) -> String {
    format!(
        ", \"keep_text\": {}, \"keep_strokes\": {}, \"raster_ppi\": {}",
        o.keep_text, o.keep_strokes, o.raster_ppi
    )
}

fn colours_json(c: &ColourConversion) -> String {
    let destination = match c.destination {
        ColourDestination::Profile(_) => "profile",
        ColourDestination::OutputIntent => "output-intent",
    };
    let mut s = format!(
        ", \"destination\": \"{destination}\", \"intent\": \"{}\", \"device_rgb\": {}, \
         \"icc_rgb\": {}, \"lab\": {}, \"cal_rgb\": {}, \"grey\": {}, \"keep_pure_black\": {}, \
         \"black_point_compensation\": {}",
        intent_name(c.intent),
        c.device_rgb,
        c.icc_rgb,
        c.lab,
        c.cal_rgb,
        c.grey,
        c.keep_pure_black,
        c.black_point_compensation
    );
    push_opt(
        &mut s,
        "embed_output_intent",
        c.embed_output_intent.as_deref(),
    );
    s
}

fn pdfx_json(level: PdfxLevel, c: &PdfxConversion) -> String {
    let mut s = format!(", \"level\": \"{}\"", level.as_str());
    push_opt(&mut s, "condition", c.intent.condition.as_deref());
    push_opt(&mut s, "registry_name", c.intent.registry_name.as_deref());
    push_opt(&mut s, "info", c.intent.info.as_deref());
    push_opt(&mut s, "title", c.title.as_deref());
    if let Some(t) = c.trapped {
        s.push_str(&format!(", \"trapped\": {t}"));
    }
    match &c.spot_to_process {
        Some(InkSelection::All) => s.push_str(", \"spot_to_process\": \"all\""),
        Some(InkSelection::Only(names)) => {
            s.push_str(&format!(", \"spot_to_process\": {}", strings_json(names)));
        }
        None => {}
    }
    s.push_str(&format!(
        ", \"flatten\": {{{}}}, \"intent\": \"{}\", \"infer_trim_boxes\": {}, \
         \"require_conforms\": {}, \"keep_partial\": {}",
        &flatten_json(&c.flatten)[2..],
        intent_name(c.rendering_intent),
        c.infer_trim_boxes,
        c.require_conforms,
        c.keep_partial
    ));
    s
}

fn intent_name(intent: IntentChoice) -> &'static str {
    match intent {
        IntentChoice::Fixed(i) => i.name(),
        IntentChoice::ObjectsOwn => "objects-own",
    }
}

fn strings_json(names: &[String]) -> String {
    let quoted: Vec<String> = names.iter().map(|n| quote(n)).collect();
    format!("[{}]", quoted.join(", "))
}

fn push_opt(s: &mut String, key: &str, value: Option<&str>) {
    if let Some(v) = value {
        s.push_str(&format!(", \"{key}\": {}", quote(v)));
    }
}

/// A rule's `fixups` member; absent means none.
pub(super) fn parse_fixups(item: &Json, rule: &str) -> Result<Vec<Fixup>, ProfileError> {
    let Some(value) = item.get("fixups") else {
        return Ok(Vec::new());
    };
    let bad = |why: String| ProfileError::BadValue {
        rule: rule.to_owned(),
        why,
    };
    let items = value
        .as_array()
        .ok_or_else(|| bad("fixups must be an array".into()))?;
    items
        .iter()
        .map(|f| {
            let name = f
                .get("fixup")
                .and_then(Json::as_str)
                .ok_or_else(|| bad("each fixup needs a `fixup` name".into()))?;
            parse_fixup(name, f).map_err(|why| bad(format!("fixup `{name}`: {why}")))
        })
        .collect()
}

fn parse_fixup(name: &str, f: &Json) -> Result<Fixup, String> {
    Ok(match name {
        "flatten-transparency" => Fixup::FlattenTransparency(parse_flatten(f)?),
        "fix-hairlines" => Fixup::FixHairlines {
            min_width_pt: number(f, "min_width_pt", |v| v > 0.0)
                .ok_or("min_width_pt must be a number above 0")?,
        },
        "alias-ink" => Fixup::AliasInk {
            from: text(f, "from").ok_or("from must be an ink name")?,
            to: text(f, "to").ok_or("to must be an ink name")?,
        },
        "spot-to-process" => Fixup::SpotToProcess {
            keep: match f.get("keep") {
                None => Vec::new(),
                Some(v) => strings(v).ok_or("keep must be an array of ink names")?,
            },
        },
        "convert-colours" => Fixup::ConvertColours(parse_colours(f)?),
        "convert-to-pdfx" => {
            let level = text(f, "level")
                .and_then(|l| PdfxLevel::from_name(&l))
                .ok_or("level must be a PDF/X level")?;
            let conversion = Box::new(parse_pdfx(f)?);
            Fixup::ConvertToPdfx { level, conversion }
        }
        "fix-pdfua" => parse_pdfua(f)?,
        "downsample-images" => parse_downsample(f)?,
        _ => return Err(format!("unknown; known: {}", Fixup::NAMES.join(", "))),
    })
}

fn parse_flatten(f: &Json) -> Result<FlattenOptions, String> {
    let mut o = FlattenOptions::default();
    o.keep_text = flag(f, "keep_text", o.keep_text)?;
    o.keep_strokes = flag(f, "keep_strokes", o.keep_strokes)?;
    if f.get("raster_ppi").is_some() {
        o.raster_ppi = number(f, "raster_ppi", |v| {
            (FlattenOptions::MIN_PPI..=FlattenOptions::MAX_PPI).contains(&v)
        })
        .ok_or("raster_ppi must be a number from 36 to 2400")?;
    }
    Ok(o)
}

fn parse_colours(f: &Json) -> Result<ColourConversion, String> {
    let destination = match text(f, "destination").as_deref() {
        None | Some("output-intent") => ColourDestination::OutputIntent,
        Some("profile") => ColourDestination::Profile(Vec::new()),
        Some(_) => return Err("destination must be output-intent or profile".into()),
    };
    let mut c = ColourConversion::new(destination);
    if let Some(i) = f.get("intent") {
        c.intent = parse_intent(i)?;
    }
    c.device_rgb = flag(f, "device_rgb", c.device_rgb)?;
    c.icc_rgb = flag(f, "icc_rgb", c.icc_rgb)?;
    c.lab = flag(f, "lab", c.lab)?;
    c.cal_rgb = flag(f, "cal_rgb", c.cal_rgb)?;
    c.grey = flag(f, "grey", c.grey)?;
    c.keep_pure_black = flag(f, "keep_pure_black", c.keep_pure_black)?;
    c.black_point_compensation = flag(f, "black_point_compensation", c.black_point_compensation)?;
    c.embed_output_intent = text(f, "embed_output_intent");
    Ok(c)
}

fn parse_pdfx(f: &Json) -> Result<PdfxConversion, String> {
    let mut intent = PdfxOutputIntent::new(Vec::new());
    intent.condition = text(f, "condition");
    intent.registry_name = text(f, "registry_name");
    intent.info = text(f, "info");
    let mut c = PdfxConversion::new(intent);
    c.title = text(f, "title");
    c.trapped = match f.get("trapped") {
        None => None,
        Some(v) => Some(v.as_bool().ok_or("trapped must be true or false")?),
    };
    c.spot_to_process = match f.get("spot_to_process") {
        None => None,
        Some(v) if v.as_str() == Some("all") => Some(InkSelection::All),
        Some(v) => Some(InkSelection::Only(
            strings(v).ok_or("spot_to_process must be \"all\" or an array of ink names")?,
        )),
    };
    if let Some(fl) = f.get("flatten") {
        c.flatten = parse_flatten(fl)?;
    }
    if let Some(i) = f.get("intent") {
        c.rendering_intent = parse_intent(i)?;
    }
    c.infer_trim_boxes = flag(f, "infer_trim_boxes", c.infer_trim_boxes)?;
    c.require_conforms = flag(f, "require_conforms", c.require_conforms)?;
    c.keep_partial = flag(f, "keep_partial", c.keep_partial)?;
    Ok(c)
}

fn parse_pdfua(f: &Json) -> Result<Fixup, String> {
    let part = f
        .get("part")
        .and_then(Json::as_u32)
        .and_then(|n| u8::try_from(n).ok())
        .and_then(PdfuaPart::from_number)
        .ok_or("part must be 1 or 2")?;
    let options = FixPdfuaOptions {
        lang: text(f, "lang"),
        title: text(f, "title"),
        ..FixPdfuaOptions::default()
    };
    let step = match text(f, "step") {
        None => None,
        Some(s) => Some(PDFUA_STEPS.into_iter().find(|p| p.as_str() == s).ok_or(
            "step must be language, display-doc-title, tab-order, artifacts or identification",
        )?),
    };
    Ok(Fixup::FixPdfua {
        part,
        options,
        step,
    })
}

fn parse_downsample(f: &Json) -> Result<Fixup, String> {
    let ppi = |key: &str| {
        f.get(key)
            .and_then(Json::as_u32)
            .and_then(|v| u16::try_from(v).ok())
            .filter(|&v| v > 0)
    };
    let above_ppi = ppi("above_ppi").ok_or("above_ppi must be a whole number from 1 to 65535")?;
    let to_ppi = match f.get("to_ppi") {
        None => above_ppi,
        Some(_) => ppi("to_ppi").ok_or("to_ppi must be a whole number from 1 to 65535")?,
    };
    if to_ppi > above_ppi {
        return Err("to_ppi must not be above above_ppi".into());
    }
    let method = match text(f, "method") {
        None => ResampleMethod::Bicubic,
        Some(m) => ResampleMethod::from_name(&m)
            .filter(|&m| m != ResampleMethod::Off)
            .ok_or("method must be average, subsample, bicubic or lanczos")?,
    };
    Ok(Fixup::DownsampleImages {
        above_ppi,
        to_ppi,
        method,
    })
}

fn parse_intent(v: &Json) -> Result<IntentChoice, String> {
    let name = v.as_str().ok_or("intent must be a name")?;
    if name == "objects-own" {
        return Ok(IntentChoice::ObjectsOwn);
    }
    INTENTS
        .into_iter()
        .find(|i| i.name() == name)
        .map(IntentChoice::Fixed)
        .ok_or_else(|| {
            "intent must be objects-own, AbsoluteColorimetric, RelativeColorimetric, \
             Saturation or Perceptual"
                .into()
        })
}

fn text(f: &Json, key: &str) -> Option<String> {
    f.get(key).and_then(Json::as_str).map(str::to_owned)
}

fn strings(v: &Json) -> Option<Vec<String>> {
    v.as_array()?
        .iter()
        .map(|s| s.as_str().map(str::to_owned))
        .collect()
}

fn flag(f: &Json, key: &str, default: bool) -> Result<bool, String> {
    match f.get(key) {
        None => Ok(default),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| format!("{key} must be true or false")),
    }
}
