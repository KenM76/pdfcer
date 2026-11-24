//! Convert colours: move every colour a document draws in a chosen set of
//! source spaces (`DeviceRGB`, `ICCBased` RGB and grey, `Lab`, `CalRGB`,
//! `CalGray`, `DeviceGray`) into one destination profile's device space,
//! for [`crate::edit::EditSession::convert_colours`].
//!
//! pdfcer does not convert colour itself (decision 064): it finds every
//! colour, hands each value to a [`ColourConverter`] the shell supplies
//! (pdfcer-render's is backed by icccer), and writes the answers back as
//! `DeviceGray`, `DeviceRGB` or `DeviceCMYK` according to the destination
//! profile's data colour space (ICC.1:2022 §7.2.6).

mod content;
mod embed;
mod objects;
mod scope;
mod sources;

use crate::color::RenderingIntent;
use crate::object::ObjId;

pub(crate) use embed::embed_writes;
pub(crate) use objects::{ConvertPlan, PlanError, plan_convert_colours};

/// The colour engine behind [`crate::edit::EditSession::convert_colours`].
///
/// pdfcer-core finds the colours; an implementation converts them. pdfcer
/// ships one in pdfcer-render (`IcccerConverter`), backed by icccer.
pub trait ColourConverter {
    /// A transform from `source` to the ICC profile `destination` (its
    /// bytes, already checked to be a display, output, input or colour
    /// space profile whose data colour space is grey, RGB or CMYK) under
    /// `intent`, with black point compensation when `bpc` is true (never
    /// asked for under `AbsoluteColorimetric`, ISO 32000-2 §8.6.5.9).
    ///
    /// # Errors
    ///
    /// [`ConvertFailure`] when no transform can be built; every object in
    /// `source` is then left as it was and reported with the message.
    fn transform(
        &self,
        source: &ColourSource,
        destination: &[u8],
        intent: RenderingIntent,
        bpc: bool,
    ) -> Result<Box<dyn ColourTransform>, ConvertFailure>;
}

/// One built source-to-destination transform.
pub trait ColourTransform {
    /// The destination colour for `input`, given in the source's own
    /// component ranges (`Lab` L in 0..100, a and b in its `/Range`;
    /// everything else 0..1). Values are expected in 0..1; pdfcer clamps
    /// them. `None` when the value cannot be converted.
    fn convert(&self, input: &[f64]) -> Option<ConvertedColour>;

    /// A sentence the report should carry about this transform (an
    /// assumption it made, a check it could not run), or `None`.
    fn note(&self) -> Option<String> {
        None
    }
}

/// A converted colour.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ConvertedColour {
    /// The destination components, each 0..1.
    pub values: Vec<f64>,
    /// The source colour lies outside the destination's gamut, so the
    /// result is the nearest colour the destination can print or show.
    pub out_of_gamut: bool,
}

impl ConvertedColour {
    /// A converted colour of `values`.
    #[must_use]
    pub fn new(values: Vec<f64>, out_of_gamut: bool) -> Self {
        Self {
            values,
            out_of_gamut,
        }
    }
}

/// Why a [`ColourConverter`] could not build a transform.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ConvertFailure {
    message: String,
}

impl ConvertFailure {
    /// A failure saying `message`.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// The message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

/// A source colour space, as a [`ColourConverter`] receives it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ColourSource {
    /// `DeviceRGB`, or a `/DefaultRGB` that is itself `DeviceRGB`.
    DeviceRgb,
    /// `DeviceGray`.
    DeviceGray,
    /// `ICCBased` (ISO 32000-2 §8.6.5.5) with an RGB or grey profile.
    IccBased {
        /// The embedded profile's bytes.
        profile: Vec<u8>,
        /// `/N`: 1 or 3.
        components: usize,
    },
    /// `Lab` (§8.6.5.4).
    Lab {
        /// `/WhitePoint`.
        white: [f64; 3],
        /// `/Range`: a min, a max, b min, b max.
        range: [f64; 4],
    },
    /// `CalRGB` (§8.6.5.3).
    CalRgb {
        /// `/WhitePoint`.
        white: [f64; 3],
        /// `/Gamma`.
        gamma: [f64; 3],
        /// `/Matrix`, column by column as written.
        matrix: [f64; 9],
    },
    /// `CalGray` (§8.6.5.2).
    CalGray {
        /// `/WhitePoint`.
        white: [f64; 3],
        /// `/Gamma`.
        gamma: f64,
    },
}

impl ColourSource {
    /// Components a colour in this space has.
    #[must_use]
    pub fn components(&self) -> usize {
        match self {
            Self::DeviceGray | Self::CalGray { .. } => 1,
            Self::IccBased { components, .. } => *components,
            Self::DeviceRgb | Self::Lab { .. } | Self::CalRgb { .. } => 3,
        }
    }
}

/// The family a converted colour came from, for the report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum SourceFamily {
    /// `DeviceRGB`.
    DeviceRgb,
    /// `ICCBased` with an RGB profile.
    IccRgb,
    /// `CalRGB`.
    CalRgb,
    /// `Lab`.
    Lab,
    /// `DeviceGray`.
    DeviceGray,
    /// `ICCBased` with a grey profile.
    IccGray,
    /// `CalGray`.
    CalGray,
}

impl SourceFamily {
    /// The family as a PDF would name it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::DeviceRgb => "DeviceRGB",
            Self::IccRgb => "ICCBased RGB",
            Self::CalRgb => "CalRGB",
            Self::Lab => "Lab",
            Self::DeviceGray => "DeviceGray",
            Self::IccGray => "ICCBased grey",
            Self::CalGray => "CalGray",
        }
    }
}

/// Where converted colours go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColourDestination {
    /// This ICC profile's bytes.
    Profile(Vec<u8>),
    /// The profile of the document's output intent: the first
    /// `/OutputIntents` entry whose `/DestOutputProfile` is readable
    /// (ISO 32000-2 §14.11.5).
    OutputIntent,
}

/// Which rendering intent each conversion uses (ISO 32000-2 §8.6.5.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntentChoice {
    /// One intent for everything.
    Fixed(RenderingIntent),
    /// Each object's own: content colours follow the `ri` operator and
    /// `/RI` in graphics states, images their `/Intent`; lookup tables and
    /// shadings, which carry none, use `RelativeColorimetric`, the
    /// standard's default.
    ObjectsOwn,
}

/// Which part of the document [`crate::edit::EditSession::convert_colours`]
/// converts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvertScope {
    /// Everything reachable from the catalog.
    All,
    /// These pages (0-based): their content, annotation appearances and
    /// every object their resources reach. An object a page outside the
    /// list also uses changes there too, and the report counts them.
    Pages(Vec<usize>),
}

/// What [`crate::edit::EditSession::convert_colours`] converts, and how.
///
/// Build one with [`Self::new`] and set the public fields to change the
/// defaults.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ColourConversion {
    /// Where colours go.
    pub destination: ColourDestination,
    /// The rendering intent. Default `Fixed(RelativeColorimetric)`.
    pub intent: IntentChoice,
    /// Convert `DeviceRGB` (and a `/DefaultRGB` that replaces it). Default on.
    pub device_rgb: bool,
    /// Convert `ICCBased` RGB. Default on.
    pub icc_rgb: bool,
    /// Convert `Lab`. Default on.
    pub lab: bool,
    /// Convert `CalRGB`. Default on.
    pub cal_rgb: bool,
    /// Convert grey: `DeviceGray`, `CalGray` and `ICCBased` grey. Default
    /// off, because grey is usually meant to print on the black plate as it
    /// is.
    pub grey: bool,
    /// Send pure black (RGB 0 0 0, grey 0, Lab L 0 a 0 b 0) to the
    /// destination's own black (CMYK 0 0 0 1) rather than through the
    /// profile, which would give rich black. Default on.
    pub keep_pure_black: bool,
    /// Black point compensation (ISO 32000-2 §8.6.5.9). Default on; never
    /// applied under `AbsoluteColorimetric`.
    pub black_point_compensation: bool,
    /// Embed the destination profile as an output intent of this `/S`
    /// subtype (`GTS_PDFX`, `GTS_PDFA1`, ...), replacing an existing intent
    /// of the same subtype. `None` (the default) embeds nothing.
    pub embed_output_intent: Option<String>,
}

impl ColourConversion {
    /// Convert every RGB family and `Lab` to `destination` with the
    /// defaults each field names.
    #[must_use]
    pub fn new(destination: ColourDestination) -> Self {
        Self {
            destination,
            intent: IntentChoice::Fixed(RenderingIntent::RelativeColorimetric),
            device_rgb: true,
            icc_rgb: true,
            lab: true,
            cal_rgb: true,
            grey: false,
            keep_pure_black: true,
            black_point_compensation: true,
            embed_output_intent: None,
        }
    }

    /// Whether colours of `family` convert.
    #[must_use]
    pub fn selects(&self, family: SourceFamily) -> bool {
        match family {
            SourceFamily::DeviceRgb => self.device_rgb,
            SourceFamily::IccRgb => self.icc_rgb,
            SourceFamily::CalRgb => self.cal_rgb,
            SourceFamily::Lab => self.lab,
            SourceFamily::DeviceGray | SourceFamily::IccGray | SourceFamily::CalGray => self.grey,
        }
    }

    fn selects_any(&self) -> bool {
        self.device_rgb || self.icc_rgb || self.lab || self.cal_rgb || self.grey
    }
}

/// How many objects of one source family changed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SourceCount {
    /// The family.
    pub source: SourceFamily,
    /// Colour-setting operators rewritten in content streams.
    pub paints: usize,
    /// Images whose samples were converted.
    pub images: usize,
    /// `Indexed` lookup tables converted.
    pub lookups: usize,
    /// Axial and radial shadings converted.
    pub shadings: usize,
    /// Of those, how many had at least one colour outside the
    /// destination's gamut.
    pub out_of_gamut: usize,
}

/// One converted image.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ImageConversion {
    /// The image.
    pub object: ObjId,
    /// The family its samples were in.
    pub source: SourceFamily,
    /// Its `/BitsPerComponent` before; it is 8 after.
    pub bits_before: u8,
    /// Its first `/Filter` before (`DCTDecode`, `JPXDecode`, ...), or
    /// `None` when unfiltered. Every converted image is re-encoded as
    /// `FlateDecode`, so a JPEG one is no longer lossy-compressed.
    pub filter_before: Option<String>,
    /// Its encoded stream length before.
    pub bytes_before: usize,
    /// Its encoded stream length after.
    pub bytes_after: usize,
}

/// Something left in its own colour space, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Unconverted {
    /// The object left unchanged, when there is one to point at.
    pub object: Option<ObjId>,
    /// One sentence.
    pub reason: String,
}

/// What [`crate::edit::EditSession::convert_colours`] did to the output
/// intents.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum OutputIntentChange {
    /// Nothing.
    #[default]
    Unchanged,
    /// A new output intent was added.
    Added,
    /// The output intent of the same subtype was replaced; `previous` is
    /// its `/OutputConditionIdentifier`.
    Replaced {
        /// The replaced intent's identifier, or empty when it had none.
        previous: String,
    },
}

/// What [`crate::edit::EditSession::convert_colours`] changed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct ConvertColoursReport {
    /// Every object written, ascending (a new output-intent profile
    /// included).
    pub changed: Vec<ObjId>,
    /// What changed, per source family, in [`SourceFamily`] order.
    pub by_source: Vec<SourceCount>,
    /// Objects and operators with at least one out-of-gamut colour, over
    /// every family.
    pub out_of_gamut: usize,
    /// Every converted image, ascending by object.
    pub images: Vec<ImageConversion>,
    /// What was left as it was.
    pub unconverted: Vec<Unconverted>,
    /// What happened to the output intents.
    pub output_intent: OutputIntentChange,
    /// Anything else the operator should know, one sentence each.
    pub disclosures: Vec<String>,
}

/// Which counter of [`SourceCount`] a conversion adds to.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Counted {
    Paint,
    Image,
    Lookup,
    Shading,
}

impl ConvertColoursReport {
    /// True when nothing was written.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty()
    }

    /// Tallies one converted item against its source family, keeping `by_source` sorted.
    pub(crate) fn count(&mut self, source: SourceFamily, what: Counted, out_of_gamut: bool) {
        let at = match self.by_source.binary_search_by_key(&source, |c| c.source) {
            Ok(i) => i,
            Err(i) => {
                self.by_source.insert(
                    i,
                    SourceCount {
                        source,
                        paints: 0,
                        images: 0,
                        lookups: 0,
                        shadings: 0,
                        out_of_gamut: 0,
                    },
                );
                i
            }
        };
        let Some(c) = self.by_source.get_mut(at) else {
            return;
        };
        match what {
            Counted::Paint => c.paints += 1,
            Counted::Image => c.images += 1,
            Counted::Lookup => c.lookups += 1,
            Counted::Shading => c.shadings += 1,
        }
        if out_of_gamut {
            c.out_of_gamut += 1;
            self.out_of_gamut += 1;
        }
    }

    /// Record an object left unconverted, once.
    pub(crate) fn unconvert(&mut self, object: Option<ObjId>, reason: String) {
        let u = Unconverted { object, reason };
        if !self.unconverted.contains(&u) {
            self.unconverted.push(u);
        }
    }

    /// Record a disclosure once.
    pub(crate) fn disclose(&mut self, sentence: String) {
        if !self.disclosures.contains(&sentence) {
            self.disclosures.push(sentence);
        }
    }
}
