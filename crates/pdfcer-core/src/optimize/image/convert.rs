//! The colour conversions an `image(...)` action can ask for (decision 192,
//! Pass 616.4 slice C), and what one did.

use std::fmt;

use super::colour::{self, Family, SourceColour, gray_formula};
use super::reduce;
use crate::deskew::Raster;
use crate::image_resample::resample_with;
use crate::object::{Dict, Name, Object};
use crate::ocr::upscale::Kernel;
use crate::space_audit::{ImageClass, ImageSpace};
use crate::view::DocumentView;

/// The colour form an image is converted to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ImageConvert {
    /// Keep the colour space.
    #[default]
    Keep,
    /// One-component `/DeviceGray` by the ISO 32000-2 §10.4.2.2 / §10.4.2.3
    /// formulas. An image already one component is left as it is.
    Gray,
    /// 1-bit `/DeviceGray`: converted to grey, then thresholded or
    /// dithered by [`Threshold`].
    Bilevel,
    /// A palette (`/Indexed`, §8.6.6.3) over the image's own colour space,
    /// at most `colors` entries, chosen by median cut.
    Indexed,
}

impl ImageConvert {
    /// Every target, in spelling order.
    pub const ALL: [Self; 4] = [Self::Keep, Self::Gray, Self::Bilevel, Self::Indexed];

    /// The profile spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Gray => "gray",
            Self::Bilevel => "bilevel",
            Self::Indexed => "indexed",
        }
    }

    /// The target spelt `name`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.as_str() == name)
    }
}

impl fmt::Display for ImageConvert {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How grey becomes black and white for [`ImageConvert::Bilevel`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Threshold {
    /// Otsu's threshold for this image's histogram; the value used is
    /// reported.
    #[default]
    Otsu,
    /// Grey values above this (0 to 255) become white.
    Fixed(u8),
    /// Floyd-Steinberg error diffusion at the midpoint.
    Dither,
}

impl Threshold {
    /// Parse `otsu`, `dither` or a number 0 to 255.
    ///
    /// # Errors
    ///
    /// A message naming the bad value.
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "otsu" => Ok(Self::Otsu),
            "dither" => Ok(Self::Dither),
            _ => value.parse().map(Self::Fixed).map_err(|_| {
                format!("image(): bad threshold {value:?}; expected otsu, dither or 0 to 255")
            }),
        }
    }
}

impl fmt::Display for Threshold {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Otsu => f.write_str("otsu"),
            Self::Fixed(n) => write!(f, "{n}"),
            Self::Dither => f.write_str("dither"),
        }
    }
}

/// What converting one image's colour did, for the report.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Conversion {
    /// The target.
    pub to: ImageConvert,
    /// The colour-space family converted from (`DeviceRGB`, `ICCBased`, …;
    /// an `/Indexed` source names its base).
    pub from: String,
    /// The formula used, when colour values were computed rather than
    /// copied; `None` for a palette over the image's own colour space.
    pub formula: Option<&'static str>,
    /// The source was CIE-based (`ICCBased`, `CalGray`, `CalRGB`) and was
    /// converted by device formula: not colour-managed.
    pub not_colour_managed: bool,
    /// The grey threshold used for a bilevel result; `None` when dithered.
    pub threshold: Option<u8>,
    /// For a palette result, the entries written.
    pub colors: Option<u16>,
    /// The palette holds every colour the image had, so nothing was lost.
    pub exact: bool,
}

impl Conversion {
    /// Whether the conversion lost information.
    #[must_use]
    pub const fn is_lossy(&self) -> bool {
        !self.exact
    }
}

/// The palette sizes `colors=` accepts.
pub const COLORS_RANGE: std::ops::RangeInclusive<u16> = 2..=256;

/// A planned conversion, carried by the job so a redo converts the same way.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ConvertJob {
    pub(crate) to: ImageConvert,
    pub(crate) threshold: Threshold,
    pub(crate) colors: u16,
    pub(crate) colour: SourceColour,
    pub(crate) decode: Option<Vec<f64>>,
}

impl ConvertJob {
    /// The report entry before the pixels are seen: what the run finds
    /// (threshold, palette size, exactness) is filled by [`Converted`].
    pub(crate) fn conversion(&self) -> Conversion {
        let base = &self.colour.base;
        let to_gray = matches!(self.to, ImageConvert::Gray | ImageConvert::Bilevel);
        let formula = if to_gray {
            gray_formula(base.family)
        } else {
            None
        };
        Conversion {
            to: self.to,
            from: base.name.clone(),
            formula,
            not_colour_managed: to_gray && base.cie,
            threshold: None,
            colors: None,
            exact: self.to == ImageConvert::Gray && base.family == Family::Gray && !base.cie,
        }
    }

    /// The colour space the output is written in, by report name.
    pub(crate) fn space_name(&self) -> String {
        match self.to {
            ImageConvert::Indexed => format!("Indexed {}", self.colour.base.name),
            _ => "DeviceGray".to_owned(),
        }
    }
}

/// The plan for `settings`' conversion of the image under `dict`:
/// `Ok(None)` when there is nothing to convert, `Err` with the reason it
/// cannot be. `blocker` is [`super::pipeline::convert_blocker`]'s answer.
pub(crate) fn plan(
    settings: &super::ResolvedImage,
    view: &DocumentView<'_>,
    dict: &Dict,
    space: Option<&ImageSpace>,
    blocker: Option<&'static str>,
) -> Result<Option<ConvertJob>, String> {
    let to = settings.convert.value;
    if to == ImageConvert::Keep {
        return Ok(None);
    }
    let space = space.ok_or_else(|| "it was not audited".to_owned())?;
    let indexed = space.class() == ImageClass::Indexed;
    let tint = matches!(space.color_space.as_deref(), Some("Separation" | "DeviceN"));
    let one = space.components == Some(1) && !indexed && !tint;
    let noop = space.image_mask
        || (to == ImageConvert::Gray && one)
        || (to == ImageConvert::Bilevel && one && space.bits_per_component == Some(1));
    if noop {
        return Ok(None);
    }
    if let Some(why) = blocker {
        return Err(why.to_owned());
    }
    let colour = colour::resolve(view, dict)?;
    if to != ImageConvert::Indexed && colour.base.family == Family::Tint {
        return Err(format!(
            "its {} colour space has no defined grey",
            colour.base.name
        ));
    }
    let colors = settings.colors.value;
    if to == ImageConvert::Indexed
        && colour
            .palette
            .as_ref()
            .is_some_and(|(h, _)| *h < u32::from(colors))
    {
        return Ok(None);
    }
    Ok(Some(ConvertJob {
        to,
        threshold: settings.threshold.value,
        colors,
        decode: colour::decode_array(dict),
        colour,
    }))
}

/// Converted samples, ready to encode.
pub(crate) struct Converted {
    /// Packed rows at `bpc`; 8-bit grey is one byte per pixel.
    pub(crate) data: Vec<u8>,
    pub(crate) components: u32,
    pub(crate) bpc: u32,
    /// The `/ColorSpace` to write.
    pub(crate) color_space: Object,
    /// The report entry, run results filled in.
    pub(crate) conversion: Conversion,
}

/// Convert `samples` (laid out as `raster`) by `job`, resampling to `out`
/// with `kernel` before any reduction to fewer values.
pub(crate) fn apply(
    job: &ConvertJob,
    raster: &Raster,
    samples: &[u8],
    out: (u32, u32),
    kernel: Option<Kernel>,
) -> Converted {
    let values = colour::base_values(raster, samples, &job.colour, job.decode.as_deref());
    let base = &job.colour.base;
    let mut conversion = job.conversion();
    let resize = |v: Vec<u8>, components: u32| {
        if out == (raster.width, raster.height) {
            return v;
        }
        let r = Raster {
            width: raster.width,
            height: raster.height,
            components,
            bpc: 8,
            paper_is_ones: true,
            indexed: false,
        };
        resample_with(&r, &v, out.0, out.1, kernel)
    };
    let gray_space = Object::Name(Name::from(&b"DeviceGray"[..]));
    if job.to == ImageConvert::Indexed {
        let n = base.components as usize;
        let q = reduce::quantise(&resize(values, base.components), n, usize::from(job.colors));
        let entries = q.palette.len() / n.max(1);
        let bpc = reduce::index_bits(entries);
        conversion.colors = u16::try_from(entries).ok();
        conversion.exact = q.exact;
        let hival = i64::try_from(entries.saturating_sub(1)).unwrap_or(0);
        let color_space = Object::Array(vec![
            Object::Name(Name::from(&b"Indexed"[..])),
            base.object.clone(),
            Object::Integer(hival),
            Object::String(q.palette),
        ]);
        return Converted {
            data: reduce::pack(&q.indices, out.0, bpc),
            components: 1,
            bpc,
            color_space,
            conversion,
        };
    }
    let gray = resize(
        colour::to_gray(&values, base.family, base.components as usize),
        1,
    );
    let (data, bpc) = match (job.to, job.threshold) {
        (ImageConvert::Bilevel, Threshold::Dither) => (reduce::dither(&gray, out.0), 1),
        (ImageConvert::Bilevel, t) => {
            let at = match t {
                Threshold::Fixed(n) => n,
                _ => reduce::otsu(&gray),
            };
            conversion.threshold = Some(at);
            (reduce::threshold(&gray, out.0, at), 1)
        }
        _ => (gray, 8),
    };
    Converted {
        data,
        components: 1,
        bpc,
        color_space: gray_space,
        conversion,
    }
}
