//! Which colour spaces convert, the destination they convert to, and the
//! memoised transforms between them.

use std::collections::HashMap;

use crate::color::RenderingIntent;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::view::DocumentView;

use super::{
    ColourConversion, ColourConverter, ColourSource, ColourTransform, ConvertColoursReport,
    IntentChoice, SourceFamily,
};

/// The destination profile and the device space its colours are written in.
pub(crate) struct Destination {
    pub(crate) profile: Vec<u8>,
    /// Components: 1, 3 or 4.
    pub(crate) n: usize,
}

impl Destination {
    /// Read a profile's header: the `acsp` signature (ICC.1:2022 §7.2.9),
    /// a class that maps device colours (§7.2.5) and a grey, RGB or CMYK
    /// data colour space (§7.2.6).
    pub(crate) fn from_profile(profile: Vec<u8>) -> Result<Self, String> {
        if profile.len() < 128 || profile.get(36..40) != Some(b"acsp".as_slice()) {
            return Err("the destination is not an ICC profile".to_owned());
        }
        if let Some(class @ (b"link" | b"abst" | b"nmcl")) = profile.get(12..16) {
            return Err(format!(
                "the destination profile's class is '{}', which does not describe a device",
                String::from_utf8_lossy(class)
            ));
        }
        let n = match profile.get(16..20) {
            Some(b"GRAY") => 1,
            Some(b"RGB ") => 3,
            Some(b"CMYK") => 4,
            other => {
                return Err(format!(
                    "the destination profile's colour space is '{}'; only grey, RGB and CMYK are written",
                    String::from_utf8_lossy(other.unwrap_or_default()).trim()
                ));
            }
        };
        Ok(Self { profile, n })
    }

    /// The device space colours are written in.
    pub(crate) fn space(&self) -> &'static [u8] {
        match self.n {
            1 => b"DeviceGray",
            3 => b"DeviceRGB",
            _ => b"DeviceCMYK",
        }
    }

    /// The content operator that sets a colour in [`Self::space`].
    pub(crate) fn operator(&self, stroke: bool) -> &'static str {
        match (self.n, stroke) {
            (1, false) => "g",
            (1, true) => "G",
            (3, false) => "rg",
            (3, true) => "RG",
            (_, false) => "k",
            (_, true) => "K",
        }
    }

    /// The destination's own black.
    pub(crate) fn black(&self) -> Vec<f64> {
        match self.n {
            1 => vec![0.0],
            3 => vec![0.0; 3],
            _ => vec![0.0, 0.0, 0.0, 1.0],
        }
    }
}

/// One classified source space.
pub(crate) struct Source {
    pub(crate) family: SourceFamily,
    pub(crate) space: ColourSource,
    /// Each component's range: `Lab`'s L 0..100 and its `/Range`; 0..1 for
    /// everything else.
    pub(crate) ranges: Vec<(f64, f64)>,
}

impl Source {
    /// The space's initial colour (ISO 32000-2 §8.6.8): every component 0,
    /// clamped into its range.
    pub(crate) fn initial(&self) -> Vec<f64> {
        self.ranges
            .iter()
            .map(|(lo, hi)| 0.0f64.clamp(*lo, *hi))
            .collect()
    }

    fn pure_black(&self, input: &[f64]) -> bool {
        input.iter().all(|v| *v == 0.0)
    }
}

/// A transform memo entry: built, or why not.
type Built = Result<Box<dyn ColourTransform>, String>;

/// Classifies spaces once each and converts colours through memoised
/// transforms, recording what it could not do in [`Self::report`].
pub(crate) struct Registry<'a, 'd> {
    pub(crate) view: &'a DocumentView<'d>,
    pub(crate) conversion: &'a ColourConversion,
    converter: &'a dyn ColourConverter,
    pub(crate) dest: Destination,
    memo: Vec<(Object, Option<usize>)>,
    pub(crate) sources: Vec<Source>,
    transforms: HashMap<(usize, RenderingIntent), Built>,
    pub(crate) report: ConvertColoursReport,
}

impl<'a, 'd> Registry<'a, 'd> {
    /// An empty registry for one conversion run.
    pub(crate) fn new(
        view: &'a DocumentView<'d>,
        conversion: &'a ColourConversion,
        converter: &'a dyn ColourConverter,
        dest: Destination,
    ) -> Self {
        Self {
            view,
            conversion,
            converter,
            dest,
            memo: Vec::new(),
            sources: Vec::new(),
            transforms: HashMap::new(),
            report: ConvertColoursReport::default(),
        }
    }

    /// The intent to use for an object whose own is `own`.
    pub(crate) fn intent(&self, own: RenderingIntent) -> RenderingIntent {
        match self.conversion.intent {
            IntentChoice::Fixed(i) => i,
            IntentChoice::ObjectsOwn => own,
        }
    }

    /// The source index for colour space `cs`, or `None` when it does not
    /// convert. In content, a device space is first replaced by the
    /// `/DefaultRGB` or `/DefaultGray` of `resources` (ISO 32000-2
    /// §8.6.5.6).
    pub(crate) fn lookup(
        &mut self,
        cs: &Object,
        resources: Option<&Dict>,
        at: Option<ObjId>,
    ) -> Option<usize> {
        let view = self.view;
        let mut resolved = view.resolve(cs).clone();
        if let (Some(res), Object::Name(n)) = (resources, &resolved) {
            let default: &[u8] = match n.as_bytes() {
                b"DeviceRGB" | b"RGB" => b"DefaultRGB",
                b"DeviceGray" | b"G" => b"DefaultGray",
                _ => b"",
            };
            if let Some(d) = super::super::rewrite::resource(view, res, b"ColorSpace", default) {
                resolved = view.resolve(&d).clone();
            }
        }
        if let Some((_, hit)) = self.memo.iter().find(|(o, _)| *o == resolved) {
            return *hit;
        }
        let hit = match self.classify(&resolved) {
            Ok(Some(source)) if self.conversion.selects(source.family) => {
                self.sources.push(source);
                Some(self.sources.len() - 1)
            }
            Ok(_) => None,
            Err(why) => {
                self.report.unconvert(cs.as_reference().or(at), why);
                None
            }
        };
        self.memo.push((resolved, hit));
        hit
    }

    /// What family `cs` is in, `Ok(None)` for one that never converts.
    fn classify(&self, cs: &Object) -> Result<Option<Source>, String> {
        let unit = |n: usize| vec![(0.0, 1.0); n];
        let source = |family, space, ranges| {
            Ok(Some(Source {
                family,
                space,
                ranges,
            }))
        };
        match cs {
            Object::Name(n) => match n.as_bytes() {
                b"DeviceRGB" | b"RGB" => {
                    source(SourceFamily::DeviceRgb, ColourSource::DeviceRgb, unit(3))
                }
                b"DeviceGray" | b"G" => {
                    source(SourceFamily::DeviceGray, ColourSource::DeviceGray, unit(1))
                }
                _ => Ok(None),
            },
            Object::Array(items) => self.classify_array(items),
            _ => Ok(None),
        }
    }

    fn classify_array(&self, items: &[Object]) -> Result<Option<Source>, String> {
        let view = self.view;
        let head = items.first().and_then(|o| view.resolve(o).as_name());
        let param = items.get(1).map(|o| view.resolve(o));
        match head.map(Name::as_bytes) {
            Some(b"ICCBased") => self.icc(param),
            Some(b"Lab") => Ok(param.and_then(Object::as_dict).map(|d| lab(view, d))),
            Some(b"CalRGB") => Ok(param.and_then(Object::as_dict).map(|d| cal_rgb(view, d))),
            Some(b"CalGray") => Ok(param.and_then(Object::as_dict).map(|d| cal_gray(view, d))),
            Some(b"Separation" | b"DeviceN") => {
                let alt = items.get(2).map(|a| self.classify(view.resolve(a)));
                if let Some(Ok(Some(s))) = alt
                    && self.conversion.selects(s.family)
                {
                    return Err(format!(
                        "a spot ink with a {} alternate was left as a spot",
                        s.family.name()
                    ));
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    /// `[/ICCBased stream]` (§8.6.5.5): RGB and grey profiles convert; one
    /// byte-equal to the destination is already there.
    fn icc(&self, param: Option<&Object>) -> Result<Option<Source>, String> {
        let Some(Object::Stream(s)) = param else {
            return Err("an ICCBased space has no profile stream".to_owned());
        };
        let profile = self
            .view
            .slice(s.data_span)
            .and_then(|raw| crate::filters::decode_stream(&s.dict, raw).ok())
            .ok_or_else(|| "an ICCBased profile cannot be decoded".to_owned())?;
        if profile == self.dest.profile {
            return Ok(None);
        }
        let (family, n) = match profile.get(16..20) {
            Some(b"RGB ") => (SourceFamily::IccRgb, 3),
            Some(b"GRAY") => (SourceFamily::IccGray, 1),
            _ => return Ok(None),
        };
        let ranges = ranges_of(self.view, &s.dict, n).unwrap_or_else(|| vec![(0.0, 1.0); n]);
        let space = ColourSource::IccBased {
            profile,
            components: n,
        };
        Ok(Some(Source {
            family,
            space,
            ranges,
        }))
    }

    /// Convert `input` (source `src`'s components) under `intent`; `None`
    /// when the transform cannot be built or the value cannot be converted.
    /// The `bool` is out of gamut.
    pub(crate) fn convert(
        &mut self,
        src: usize,
        input: &[f64],
        intent: RenderingIntent,
        at: Option<ObjId>,
    ) -> Option<(Vec<f64>, bool)> {
        let source = self.sources.get(src)?;
        if input.len() != source.ranges.len() {
            return None;
        }
        if self.conversion.keep_pure_black && source.pure_black(input) {
            return Some((self.dest.black(), false));
        }
        let family = source.family;
        let n = self.dest.n;
        let built = self.transform(src, intent)?;
        let Some(out) = built.convert(input).filter(|c| c.values.len() == n) else {
            let why = format!("a {} colour could not be converted", family.name());
            self.report.unconvert(at, why);
            return None;
        };
        let values = out.values.iter().map(|v| v.clamp(0.0, 1.0)).collect();
        Some((values, out.out_of_gamut))
    }

    fn transform(&mut self, src: usize, intent: RenderingIntent) -> Option<&dyn ColourTransform> {
        let source = self.sources.get(src)?;
        let bpc = self.conversion.black_point_compensation
            && intent != RenderingIntent::AbsoluteColorimetric;
        if !self.transforms.contains_key(&(src, intent)) {
            let built = self
                .converter
                .transform(&source.space, &self.dest.profile, intent, bpc)
                .map_err(|e| {
                    format!(
                        "{} colours could not be converted: {}",
                        source.family.name(),
                        e.message()
                    )
                });
            match &built {
                Ok(t) => {
                    if let Some(note) = t.note() {
                        self.report.disclose(note);
                    }
                }
                Err(why) => self.report.unconvert(None, why.clone()),
            }
            self.transforms.insert((src, intent), built);
        }
        self.transforms.get(&(src, intent))?.as_deref().ok()
    }
}

/// A space dictionary's `/WhitePoint`, defaulting to D50 when unreadable.
fn white(view: &DocumentView<'_>, d: &Dict) -> [f64; 3] {
    let v = numbers(view, d, b"WhitePoint");
    match v.as_slice() {
        [x, y, z] => [*x, *y, *z],
        _ => [0.9642, 1.0, 0.8249],
    }
}

fn numbers(view: &DocumentView<'_>, d: &Dict, key: &[u8]) -> Vec<f64> {
    d.get(key)
        .and_then(|o| view.resolve(o).as_array())
        .map(|a| {
            a.iter()
                .filter_map(|o| view.resolve(o).as_number())
                .collect()
        })
        .unwrap_or_default()
}

/// An `ICCBased` stream's `/Range`, when it has a full one.
fn ranges_of(view: &DocumentView<'_>, d: &Dict, n: usize) -> Option<Vec<(f64, f64)>> {
    let v = numbers(view, d, b"Range");
    (v.len() == 2 * n).then(|| v.as_chunks::<2>().0.iter().map(|[a, b]| (*a, *b)).collect())
}

/// `Lab` (§8.6.5.4): `/Range` defaults to `[-100 100 -100 100]`.
fn lab(view: &DocumentView<'_>, d: &Dict) -> Source {
    let r = numbers(view, d, b"Range");
    let range = match r.as_slice() {
        [a0, a1, b0, b1] => [*a0, *a1, *b0, *b1],
        _ => [-100.0, 100.0, -100.0, 100.0],
    };
    Source {
        family: SourceFamily::Lab,
        space: ColourSource::Lab {
            white: white(view, d),
            range,
        },
        ranges: vec![(0.0, 100.0), (range[0], range[1]), (range[2], range[3])],
    }
}

/// `CalRGB` (§8.6.5.3): `/Gamma` defaults to `[1 1 1]`, `/Matrix` to
/// identity.
fn cal_rgb(view: &DocumentView<'_>, d: &Dict) -> Source {
    let g = numbers(view, d, b"Gamma");
    let gamma = match g.as_slice() {
        [r, g, b] => [*r, *g, *b],
        _ => [1.0; 3],
    };
    let m = numbers(view, d, b"Matrix");
    let matrix: [f64; 9] = m
        .as_slice()
        .try_into()
        .unwrap_or([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
    Source {
        family: SourceFamily::CalRgb,
        space: ColourSource::CalRgb {
            white: white(view, d),
            gamma,
            matrix,
        },
        ranges: vec![(0.0, 1.0); 3],
    }
}

/// `CalGray` (§8.6.5.2): `/Gamma` defaults to 1.
fn cal_gray(view: &DocumentView<'_>, d: &Dict) -> Source {
    let gamma = d
        .get(b"Gamma")
        .and_then(|o| view.resolve(o).as_number())
        .unwrap_or(1.0);
    Source {
        family: SourceFamily::CalGray,
        space: ColourSource::CalGray {
            white: white(view, d),
            gamma,
        },
        ranges: vec![(0.0, 1.0)],
    }
}

/// The document's chosen output intent's decoded profile
/// ([`crate::prepress::output_intent`]), the one the renderer also uses.
pub(crate) fn output_intent_profile(view: &DocumentView<'_>) -> Option<Vec<u8>> {
    crate::prepress::output_intent(view).map(|i| i.profile.to_vec())
}
