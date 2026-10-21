//! Image optimisation settings (decision 192): the [`ImagePatch`] an
//! `image(...)` action carries, and the fold that resolves every matching
//! action into one [`ResolvedImage`] naming where each value came from.

pub(crate) mod encode;
pub(crate) mod jpeg_quality;
pub(crate) mod pipeline;

use std::fmt;

use super::StreamAction;
pub use crate::jpeg_write::Chroma;

/// How an image is resampled when it is downsampled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ResampleMethod {
    /// Never resample.
    Off,
    /// Mean over each output pixel's footprint (Acrobat's "average").
    Average,
    /// The nearest source pixel, no blending.
    Subsample,
    /// Cubic convolution, a = -0.5.
    #[default]
    Bicubic,
    /// Three-lobe Lanczos: sharper, more ringing.
    Lanczos,
}

impl ResampleMethod {
    /// Every method, in spelling order.
    pub const ALL: [Self; 5] = [
        Self::Off,
        Self::Average,
        Self::Subsample,
        Self::Bicubic,
        Self::Lanczos,
    ];

    /// The profile spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Average => "average",
            Self::Subsample => "subsample",
            Self::Bicubic => "bicubic",
            Self::Lanczos => "lanczos",
        }
    }

    /// The method spelt `name`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.as_str() == name)
    }
}

impl fmt::Display for ResampleMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How an image's samples are stored after optimisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ImageEncode {
    /// Keep the stored encoding. Resampled samples are written as
    /// `/FlateDecode` level 9 from a generic-filter source and as JPEG from
    /// a `/DCTDecode` one.
    Keep,
    /// `/FlateDecode` at zlib `level` (0 to 9). Lossless.
    Flate {
        /// zlib level, 0 to 9.
        level: u8,
    },
    /// `/DCTDecode` (JPEG) with the patch's `quality`, `chroma` and
    /// `progressive`. Lossy.
    Jpeg,
}

impl fmt::Display for ImageEncode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Keep => f.write_str("keep"),
            Self::Flate { level } => write!(f, "flate-{level}"),
            Self::Jpeg => f.write_str("jpeg"),
        }
    }
}

/// The settings one `image(...)` action changes; `None` leaves a field as
/// an earlier action set it.
///
/// Spelling: `image(method=bicubic,ppi=150,above=225,encode=jpeg,quality=high,chroma=420,progressive,min-saving=10)`,
/// any subset, any order, spaces allowed when parsing. `reset` first clears
/// every field an earlier action set. Downsampling needs a `ppi`; `method`
/// defaults to bicubic and `above` (the trigger) to 1.5 x `ppi`. `quality`
/// is 1 to 100 or a [`QUALITY_LEVELS`] name; it applies when the output is
/// JPEG and defaults to 80.
///
/// ```
/// use pdfcer_core::optimize::{ImagePatch, ResampleMethod, StreamAction};
///
/// let action = StreamAction::parse("image(ppi=150, method=average)")?;
/// let StreamAction::Image(patch) = action else { unreachable!() };
/// assert_eq!((patch.ppi, patch.method), (Some(150), Some(ResampleMethod::Average)));
/// assert_eq!(action.to_string(), "image(method=average,ppi=150)");
/// # Ok::<(), String>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct ImagePatch {
    /// Clear every field earlier actions set before applying this one.
    pub reset: bool,
    /// Resampling method.
    pub method: Option<ResampleMethod>,
    /// Target resolution in pixels per inch at the binding placement.
    pub ppi: Option<u16>,
    /// Resample only images whose binding resolution is above this.
    pub above: Option<u16>,
    /// Output encoding.
    pub encode: Option<ImageEncode>,
    /// JPEG quality, 1 to 100 (IJG scale).
    pub quality: Option<u8>,
    /// JPEG chroma subsampling.
    pub chroma: Option<Chroma>,
    /// Progressive rather than baseline JPEG.
    pub progressive: Option<bool>,
    /// Keep the stored form unless the result is at least this many percent
    /// smaller (0 to 99).
    pub min_saving_pct: Option<u8>,
}

/// The named JPEG qualities `quality=` accepts (decision 192).
pub const QUALITY_LEVELS: [(&str, u8); 5] = [
    ("minimum", 20),
    ("low", 40),
    ("medium", 60),
    ("high", 80),
    ("maximum", 95),
];

impl ImagePatch {
    /// A patch that changes nothing.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            reset: false,
            method: None,
            ppi: None,
            above: None,
            encode: None,
            quality: None,
            chroma: None,
            progressive: None,
            min_saving_pct: None,
        }
    }

    /// Parse the `image(...)` spelling.
    ///
    /// # Errors
    ///
    /// A message naming the bad key or value: an unknown or repeated key,
    /// `ppi` or `above` of 0, `above` below `ppi`, a flate level above 9, a
    /// `quality` outside 1 to 100 or a `min-saving` above 99.
    pub fn parse(text: &str) -> Result<Self, String> {
        let inner = text
            .trim()
            .strip_prefix("image(")
            .and_then(|t| t.strip_suffix(')'))
            .ok_or_else(|| format!("expected image(key=value,...), found {text:?}"))?;
        let mut patch = Self::new();
        let mut seen: Vec<&str> = Vec::new();
        for item in inner.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let (key, value) = item
                .split_once('=')
                .map_or((item, ""), |(k, v)| (k.trim(), v.trim()));
            if seen.contains(&key) {
                return Err(format!("image(): duplicate key {key:?}"));
            }
            seen.push(key);
            patch.set(key, value)?;
        }
        patch.validate()?;
        Ok(patch)
    }

    fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        let bad = || format!("image(): bad value {value:?} for {key}");
        match key {
            "reset" if value.is_empty() || value == "true" => self.reset = true,
            "method" => {
                self.method = Some(ResampleMethod::from_name(value).ok_or_else(|| {
                    format!(
                        "image(): unknown method {value:?}; expected off, average, subsample, bicubic or lanczos"
                    )
                })?);
            }
            "ppi" => self.ppi = Some(value.parse().map_err(|_| bad())?),
            "above" => self.above = Some(value.parse().map_err(|_| bad())?),
            "encode" => self.encode = Some(parse_encode(value)?),
            "quality" => self.quality = Some(parse_quality(value)?),
            "chroma" => {
                self.chroma = Some(Chroma::from_name(value).ok_or_else(|| {
                    format!("image(): unknown chroma {value:?}; expected auto, 444 or 420")
                })?);
            }
            "progressive" => {
                self.progressive = Some(match value {
                    "" | "true" => true,
                    "false" => false,
                    _ => return Err(bad()),
                });
            }
            "min-saving" => self.min_saving_pct = Some(value.parse().map_err(|_| bad())?),
            _ => {
                return Err(format!(
                    "image(): unknown key {key:?}; expected reset, method, ppi, above, encode, quality, chroma, progressive or min-saving"
                ));
            }
        }
        Ok(())
    }

    /// Refuse values no image could use.
    ///
    /// # Errors
    ///
    /// As [`Self::parse`].
    pub fn validate(&self) -> Result<(), String> {
        if self.ppi == Some(0) || self.above == Some(0) {
            return Err("image(): ppi and above must be at least 1".to_owned());
        }
        if let (Some(ppi), Some(above)) = (self.ppi, self.above)
            && above < ppi
        {
            return Err(format!("image(): above={above} is below ppi={ppi}"));
        }
        if matches!(self.encode, Some(ImageEncode::Flate { level }) if level > 9) {
            return Err("image(): flate level must be 0 to 9".to_owned());
        }
        if self.quality.is_some_and(|q| !(1..=100).contains(&q)) {
            return Err("image(): quality must be 1 to 100".to_owned());
        }
        if self.min_saving_pct.is_some_and(|p| p > 99) {
            return Err("image(): min-saving must be 0 to 99".to_owned());
        }
        Ok(())
    }
}

fn parse_encode(value: &str) -> Result<ImageEncode, String> {
    match value {
        "keep" => Ok(ImageEncode::Keep),
        "flate" => Ok(ImageEncode::Flate { level: 9 }),
        "jpeg" => Ok(ImageEncode::Jpeg),
        _ => value
            .strip_prefix("flate-")
            .and_then(|n| n.parse::<u8>().ok())
            .filter(|n| *n <= 9)
            .map(|level| ImageEncode::Flate { level })
            .ok_or_else(|| {
                format!(
                    "image(): unknown encode {value:?}; expected keep, flate, flate-0..9 or jpeg"
                )
            }),
    }
}

fn parse_quality(value: &str) -> Result<u8, String> {
    QUALITY_LEVELS
        .iter()
        .find(|(name, _)| *name == value)
        .map(|(_, q)| *q)
        .or_else(|| value.parse().ok())
        .ok_or_else(|| {
            format!(
                "image(): bad quality {value:?}; expected 1 to 100 or minimum, low, medium, high, maximum"
            )
        })
}

impl fmt::Display for ImagePatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<String> = Vec::new();
        if self.reset {
            parts.push("reset".to_owned());
        }
        if let Some(m) = self.method {
            parts.push(format!("method={m}"));
        }
        if let Some(p) = self.ppi {
            parts.push(format!("ppi={p}"));
        }
        if let Some(a) = self.above {
            parts.push(format!("above={a}"));
        }
        if let Some(e) = self.encode {
            parts.push(format!("encode={e}"));
        }
        if let Some(q) = self.quality {
            parts.push(format!("quality={q}"));
        }
        if let Some(c) = self.chroma {
            parts.push(format!("chroma={c}"));
        }
        if let Some(p) = self.progressive {
            parts.push(format!("progressive={p}"));
        }
        if let Some(s) = self.min_saving_pct {
            parts.push(format!("min-saving={s}"));
        }
        write!(f, "image({})", parts.join(","))
    }
}

/// Which setting supplied a resolved value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SettingSource {
    /// No action set it; pdfcer's default (decision 192's table).
    BuiltIn,
    /// The default `streams` action.
    Streams,
    /// The rule at this index of [`OptimizeSettings::rules`](super::OptimizeSettings::rules).
    Rule(usize),
    /// The per-object override.
    Object,
}

impl fmt::Display for SettingSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BuiltIn => f.write_str("default"),
            Self::Streams => f.write_str("streams"),
            Self::Rule(n) => write!(f, "rule {}", n + 1),
            Self::Object => f.write_str("object override"),
        }
    }
}

/// A resolved value and the setting it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Sourced<T> {
    /// The value.
    pub value: T,
    /// Where it came from.
    pub source: SettingSource,
}

const fn sourced<T>(value: T, source: SettingSource) -> Sourced<T> {
    Sourced { value, source }
}

/// Every image setting for one stream after the precedence fold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ResolvedImage {
    /// Resampling method.
    pub method: Sourced<ResampleMethod>,
    /// Target ppi; `None` means no downsampling.
    pub ppi: Option<Sourced<u16>>,
    /// The trigger as set; [`Self::trigger`] applies the default.
    pub above: Option<Sourced<u16>>,
    /// Output encoding.
    pub encode: Sourced<ImageEncode>,
    /// JPEG quality as set; a JPEG source caps it at its own estimate.
    pub quality: Sourced<u8>,
    /// JPEG chroma subsampling.
    pub chroma: Sourced<Chroma>,
    /// Progressive JPEG.
    pub progressive: Sourced<bool>,
    /// The `min-saving` guard, percent.
    pub min_saving_pct: Sourced<u8>,
}

impl ResolvedImage {
    fn base(encode: Sourced<ImageEncode>) -> Self {
        Self {
            method: sourced(ResampleMethod::Bicubic, SettingSource::BuiltIn),
            ppi: None,
            above: None,
            encode,
            quality: sourced(80, SettingSource::BuiltIn),
            chroma: sourced(Chroma::Auto, SettingSource::BuiltIn),
            progressive: sourced(false, SettingSource::BuiltIn),
            min_saving_pct: sourced(0, SettingSource::BuiltIn),
        }
    }

    /// Whether these settings downsample at all.
    #[must_use]
    pub fn resamples(&self) -> bool {
        self.ppi.is_some() && self.method.value != ResampleMethod::Off
    }

    /// The binding resolution above which an axis is resampled: `above`,
    /// else 1.5 x `ppi`, never below `ppi` (a later `ppi` can exceed an
    /// earlier `above`).
    #[must_use]
    pub fn trigger(&self) -> Option<f64> {
        let ppi = f64::from(self.ppi?.value);
        Some(
            self.above
                .map_or(ppi * 1.5, |a| f64::from(a.value))
                .max(ppi),
        )
    }

    /// The settings as one patch, every resolved field set.
    #[must_use]
    pub fn to_patch(&self) -> ImagePatch {
        ImagePatch {
            reset: false,
            method: Some(self.method.value),
            ppi: self.ppi.map(|p| p.value),
            above: self.above.map(|a| a.value),
            encode: Some(self.encode.value),
            quality: Some(self.quality.value),
            chroma: Some(self.chroma.value),
            progressive: Some(self.progressive.value),
            min_saving_pct: Some(self.min_saving_pct.value),
        }
    }

    fn merge(&mut self, p: &ImagePatch, src: SettingSource, base: Sourced<ImageEncode>) {
        if p.reset {
            *self = Self::base(base);
        }
        if let Some(v) = p.method {
            self.method = sourced(v, src);
        }
        if let Some(v) = p.ppi {
            self.ppi = Some(sourced(v, src));
        }
        if let Some(v) = p.above {
            self.above = Some(sourced(v, src));
        }
        if let Some(v) = p.encode {
            self.encode = sourced(v, src);
        }
        if let Some(v) = p.quality {
            self.quality = sourced(v, src);
        }
        if let Some(v) = p.chroma {
            self.chroma = sourced(v, src);
        }
        if let Some(v) = p.progressive {
            self.progressive = sourced(v, src);
        }
        if let Some(v) = p.min_saving_pct {
            self.min_saving_pct = sourced(v, src);
        }
    }
}

/// What a stream's matching actions fold to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Resolved {
    /// The last whole-state action: what a non-image stream gets.
    pub(crate) base: StreamAction,
    /// The image settings, when an `image(...)` action followed `base`.
    pub(crate) image: Option<ResolvedImage>,
}

/// Fold `actions` in precedence order (lowest first). `leave` and
/// `recompress-N` replace the whole state; `image(...)` merges field by
/// field over an encode taken from the state it lands on.
pub(crate) fn fold(actions: impl IntoIterator<Item = (StreamAction, SettingSource)>) -> Resolved {
    let mut base = (StreamAction::Leave, SettingSource::BuiltIn);
    let mut image: Option<ResolvedImage> = None;
    for (action, src) in actions {
        match action {
            StreamAction::Image(patch) => {
                let encode = match base.0 {
                    StreamAction::Recompress { level } => {
                        sourced(ImageEncode::Flate { level }, base.1)
                    }
                    _ => sourced(ImageEncode::Keep, base.1),
                };
                image
                    .get_or_insert_with(|| ResolvedImage::base(encode))
                    .merge(&patch, src, encode);
            }
            other => {
                base = (other, src);
                image = None;
            }
        }
    }
    Resolved {
        base: base.0,
        image,
    }
}

#[cfg(test)]
mod pipeline_jpeg_tests;
#[cfg(test)]
mod pipeline_tests;
#[cfg(test)]
mod tests;
