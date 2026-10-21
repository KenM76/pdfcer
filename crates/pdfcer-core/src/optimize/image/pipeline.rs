//! Deciding what resolved image settings do to one image XObject: the
//! output size from the binding placement, the source kind, and the target
//! encoding. [`super::encode::run`] performs the result.

use super::convert::{self, ConvertJob};
use super::{
    Conversion, ImageConvert, ImageEncode, ImageSkip, ImageStep, KeyedResample, Predictor,
    ResampleMethod, ResolvedImage, sourced,
};
use crate::jpeg_write::JpegOptions;
use crate::object::{Dict, ObjId, Object};
use crate::ocr::upscale::Kernel;
use crate::optimize::SkipReason;
use crate::optimize::streams::{GENERIC, chain};
use crate::space_audit::{ImageClass, ImagePlacement, ImageSpace};
use crate::view::DocumentView;

/// What one image's rewrite did, for the report.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ImageChange {
    /// The settings that produced it, each with its source.
    pub settings: ResolvedImage,
    /// Pixel size before, `(width, height)`.
    pub size_before: (u32, u32),
    /// Pixel size after; equal to `size_before` when only the encoding
    /// changed.
    pub size_after: (u32, u32),
    /// The placement that limited the reduction: the one drawn largest,
    /// at the lowest effective ppi. `None` when nothing was resampled.
    pub binding: Option<ImagePlacement>,
    /// That placement's effective ppi after resampling.
    pub binding_ppi_after: Option<f64>,
    /// The encoding written: `Flate` or `Jpeg`, never `Keep`.
    pub encoded_as: ImageEncode,
    /// Bits per component before and after.
    pub bpc: (u8, u8),
    /// For JPEG output, the quality written and why.
    pub jpeg: Option<JpegQuality>,
    /// The colour conversion, when the settings asked for one that applied.
    pub conversion: Option<Conversion>,
    /// The colour space before and after, by family name (`DeviceRGB`,
    /// `Indexed DeviceRGB`, …); equal when not converted.
    pub color_space: (String, String),
    /// Whether the Flate output was PNG-predicted (`/Predictor 15`).
    pub predicted: bool,
    /// For a `/Matte` soft mask, the parent image it was resampled with:
    /// its size follows the parent's, not its own settings (ISO 32000-2
    /// §11.6.5.3 requires the two sizes to be equal).
    pub resampled_with: Option<ObjId>,
}

impl ImageChange {
    /// Whether the rewrite lost information: it resampled, wrote JPEG,
    /// narrowed the bit depth, or converted lossily. An exact palette at
    /// the same size is lossless.
    #[must_use]
    pub fn is_lossy(&self) -> bool {
        self.size_after != self.size_before
            || self.encoded_as == ImageEncode::Jpeg
            || self
                .conversion
                .as_ref()
                .map_or(self.bpc.1 < self.bpc.0, Conversion::is_lossy)
    }
}

impl ImageChange {
    /// Fill in what performing the job found, returning its stream.
    pub(crate) fn record(&mut self, w: super::encode::Written) -> (Dict, Vec<u8>) {
        self.bpc.1 = u8::try_from(w.bpc).unwrap_or(u8::MAX);
        self.conversion = w.conversion;
        self.predicted = w.predicted;
        (w.dict, w.data)
    }
}

/// The JPEG quality an image was written at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct JpegQuality {
    /// The quality the settings asked for.
    pub asked: u8,
    /// The quality written: `asked`, capped at `source_estimate`.
    pub used: u8,
    /// A JPEG source's estimated IJG quality; `None` for a non-JPEG source
    /// or tables that match no IJG scaling.
    pub source_estimate: Option<u8>,
}

/// How the source's samples are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    /// Generic lossless filters only.
    Generic,
    /// `/DCTDecode` last, generic filters before it.
    Dct,
}

/// What the samples are written as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    /// `/FlateDecode` at `level`, predicted as `predictor` says.
    Flate { level: u8, predictor: Predictor },
    /// `/DCTDecode`.
    Jpeg(JpegOptions),
}

/// A planned rewrite, redone on a stream an earlier step of the run
/// rewrote.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Job {
    pub(crate) src: (u32, u32),
    pub(crate) out: (u32, u32),
    pub(crate) components: u32,
    pub(crate) bpc: u32,
    pub(crate) indexed: bool,
    pub(crate) source: Source,
    pub(crate) target: Target,
    pub(crate) kernel: Option<Kernel>,
    pub(crate) min_saving_pct: u8,
    pub(crate) convert: Option<ConvertJob>,
}

/// What the caller knows about one image beyond its own dictionary.
pub(crate) struct Context<'a, 'v> {
    pub(crate) view: &'a DocumentView<'v>,
    /// [`mask_blocker`]'s answer.
    pub(crate) blocker: Option<ImageSkip>,
    /// [`convert_blocker`]'s answer.
    pub(crate) convert_blocker: Option<ImageSkip>,
    /// Set for a `/Matte` soft mask whose parent was resampled.
    pub(crate) lockstep: Option<Lockstep>,
}

/// A `/Matte` soft mask resampled to its parent's new size.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Lockstep {
    pub(crate) parent: ObjId,
    pub(crate) size: (u32, u32),
    /// The mask's settings, with the parent's method and no conversion.
    pub(crate) settings: ResolvedImage,
}

/// What resolved settings ask of one image.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Outcome {
    /// Nothing: no downsampling and the encoding kept.
    Unchanged,
    /// No downsampling; recompress losslessly at this level.
    Lossless(u8),
    /// Rewrite the pixels: downsample, re-encode, or both.
    Rewrite(Job, Box<ImageChange>),
}

fn int(dict: &Dict, key: &[u8]) -> Option<u32> {
    match dict.get(key) {
        Some(Object::Integer(n)) => u32::try_from(*n).ok(),
        _ => None,
    }
}

const fn unsupported(why: ImageSkip) -> SkipReason {
    SkipReason::ImageUnsupported(why)
}

/// Why an image's masking keeps it from being resampled on its own, if it
/// does: a `/Matte` soft mask must keep its parent's size (ISO 32000-2
/// §11.6.5.3), and blending a colour-key image invents fringe colours that
/// no longer match the key. `smask` is the image's resolved `/SMask`;
/// `lockstep` says that mask is resampled with the image, so its `/Matte`
/// does not block.
pub(crate) fn mask_blocker(dict: &Dict, smask: Option<&Dict>, lockstep: bool) -> Option<ImageSkip> {
    if dict.contains_key(b"Matte") {
        Some(ImageSkip::MatteMask)
    } else if !lockstep && smask.is_some_and(|m| m.contains_key(b"Matte")) {
        Some(ImageSkip::MatteParent)
    } else if colour_keyed(dict) {
        Some(ImageSkip::ColourKey(ImageStep::Resample))
    } else {
        None
    }
}

/// Why an image's colours cannot be converted, if they cannot: a soft
/// mask's samples are opacity, not colour; a `/Matte` colour is in its
/// parent's colour space (ISO 32000-2 §11.6.5.3); a colour key matches
/// source sample values. `is_soft_mask` says another image names it as
/// its `/SMask`.
pub(crate) fn convert_blocker(
    dict: &Dict,
    smask: Option<&Dict>,
    is_soft_mask: bool,
) -> Option<ImageSkip> {
    if is_soft_mask || dict.contains_key(b"Matte") {
        Some(ImageSkip::SoftMaskConvert)
    } else if smask.is_some_and(|m| m.contains_key(b"Matte")) {
        Some(ImageSkip::MatteParentConvert)
    } else if colour_keyed(dict) {
        Some(ImageSkip::ColourKey(ImageStep::Convert))
    } else {
        None
    }
}

/// The settings a resize runs with, or why masking forbids it. A
/// colour-key blocker gives way under `keyed=subsample`, forcing a
/// nearest-pixel resample sourced from `keyed`.
fn unblocked(
    settings: &ResolvedImage,
    resized: bool,
    ctx: &Context<'_, '_>,
) -> Result<ResolvedImage, SkipReason> {
    let mut settings = *settings;
    let Some(why) = ctx
        .blocker
        .as_ref()
        .filter(|_| resized && ctx.lockstep.is_none())
    else {
        return Ok(settings);
    };
    if *why != ImageSkip::ColourKey(ImageStep::Resample)
        || settings.keyed.value != KeyedResample::Subsample
    {
        return Err(unsupported(why.clone()));
    }
    settings.method = sourced(ResampleMethod::Subsample, settings.keyed.source);
    Ok(settings)
}

fn colour_keyed(dict: &Dict) -> bool {
    matches!(dict.get(b"Mask"), Some(Object::Array(_)))
}

/// The source kind, or the filter that keeps it from being decoded here.
fn source_kind(dict: &Dict) -> Result<Source, SkipReason> {
    let names = chain(dict);
    let mut source = Source::Generic;
    for (i, name) in names.iter().enumerate() {
        if name == "DCTDecode" && i + 1 == names.len() {
            source = Source::Dct;
        } else if !GENERIC.contains(&name.as_bytes()) {
            return Err(SkipReason::UnsupportedFilter(name.clone()));
        }
    }
    Ok(source)
}

/// Decide what `settings` do to the image stored as `raw` under `dict`,
/// audited as `space`.
pub(crate) fn decide(
    settings: &ResolvedImage,
    dict: &Dict,
    raw: &[u8],
    space: Option<&ImageSpace>,
    ctx: &Context<'_, '_>,
) -> Result<Outcome, SkipReason> {
    let resized = match &ctx.lockstep {
        Some(l) => space.and_then(|s| (l.size != (s.width, s.height)).then_some(l.size)),
        None => target_size(settings, space)?,
    };
    let convert = convert::plan(
        settings,
        ctx.view,
        dict,
        space,
        ctx.convert_blocker.as_ref(),
    )
    .map_err(SkipReason::ImageUnsupported)?;
    let encode = settings.encode.value;
    let source = match (resized, encode, &convert) {
        (None, ImageEncode::Keep, None) => return Ok(Outcome::Unchanged),
        (None, ImageEncode::Flate { level }, None) if !predictable(settings, dict, space) => {
            return Ok(Outcome::Lossless(level));
        }
        _ => source_kind(dict)?,
    };
    if resized.is_none()
        && convert.is_none()
        && source == Source::Dct
        && let Some(left) = already_jpeg(settings, raw)?
    {
        return Ok(left);
    }
    let settings = &unblocked(settings, resized.is_some(), ctx)?;
    let space = space.ok_or(unsupported(ImageSkip::NotAudited))?;
    let to = convert.as_ref().map(|c| c.to);
    let reduced = matches!(to, Some(ImageConvert::Bilevel | ImageConvert::Indexed));
    let jpeg = match encode {
        ImageEncode::Jpeg => true,
        ImageEncode::Keep => source == Source::Dct && !reduced,
        _ => false,
    };
    if jpeg && reduced {
        return Err(unsupported(ImageSkip::ReducedAsJpeg));
    }
    let (components, bpc) = sample_format(dict, space, source, jpeg && convert.is_none())?;
    let (target, quality) = if jpeg {
        let (opts, q) = jpeg_options(settings, source, raw);
        (Target::Jpeg(opts), Some(q))
    } else {
        let level = match encode {
            ImageEncode::Flate { level } => level,
            _ => 9,
        };
        let predictor = settings.predictor.value;
        (Target::Flate { level, predictor }, None)
    };
    let (w, h) = (space.width, space.height);
    let job = Job {
        src: (w, h),
        out: resized.unwrap_or((w, h)),
        components,
        bpc,
        indexed: space.class() == ImageClass::Indexed,
        source,
        target,
        kernel: kernel(settings.method.value),
        min_saving_pct: settings.min_saving_pct.value,
        convert,
    };
    let mut change = change(settings, space, &job, quality);
    change.resampled_with = ctx.lockstep.map(|l| l.parent);
    Ok(Outcome::Rewrite(job, Box::new(change)))
}

/// Whether a same-size Flate recompression goes through the sample
/// pipeline so it can be PNG-predicted: a generic-filter source, audited,
/// with a sample format the pipeline reads. Otherwise the stream is only
/// re-deflated.
fn predictable(settings: &ResolvedImage, dict: &Dict, space: Option<&ImageSpace>) -> bool {
    settings.predictor.value != Predictor::Off
        && matches!(source_kind(dict), Ok(Source::Generic))
        && space.is_some_and(|s| sample_format(dict, s, Source::Generic, false).is_ok())
}

/// The output size when `settings` resample `space`, else `None`.
fn target_size(
    settings: &ResolvedImage,
    space: Option<&ImageSpace>,
) -> Result<Option<(u32, u32)>, SkipReason> {
    if !settings.resamples() {
        return Ok(None);
    }
    let Some(space) = space.filter(|s| !s.placements.is_empty()) else {
        return match (settings.max_pixels, space) {
            (Some(max), Some(s)) => Ok(capped(s.width, s.height, max.value)),
            (None, _) if settings.ppi.is_none() => Ok(None),
            _ => Err(SkipReason::Unplaced),
        };
    };
    let (Some(ppi), Some(trigger)) = (settings.ppi, settings.trigger()) else {
        return Ok(None);
    };
    let (w, h) = (space.width, space.height);
    let target = f64::from(ppi.value);
    let axis = |len: u32, binding: Option<f64>| match binding {
        Some(b) if b > trigger && len > 1 => scaled(len, target / b),
        _ => len,
    };
    let out = (
        axis(w, space.binding_ppi_x()),
        axis(h, space.binding_ppi_y()),
    );
    Ok((out != (w, h)).then_some(out))
}

/// `w` x `h` scaled by one factor to at most `max` pixels, if it has more.
/// An axis that would fall below one pixel stays at one and the other takes
/// the remaining budget, so the result never exceeds `max`.
pub(super) fn capped(w: u32, h: u32, max: u32) -> Option<(u32, u32)> {
    let pixels = f64::from(w) * f64::from(h);
    if pixels <= f64::from(max) {
        return None;
    }
    let k = (f64::from(max) / pixels).sqrt();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // in 1..=len
    let axis = |len: u32| (f64::from(len) * k).floor().clamp(1.0, f64::from(len)) as u32;
    let (a, b) = (axis(w), axis(h));
    let b = b.min(max / a).max(1);
    Some((a.min(max / b).max(1), b))
}

/// A JPEG whose pixels are unchanged is re-encoded only when JPEG is asked at
/// a quality below the source's estimate; otherwise it is left as it is,
/// silently for `encode=keep` and with a reason for `encode=jpeg`.
fn already_jpeg(settings: &ResolvedImage, raw: &[u8]) -> Result<Option<Outcome>, SkipReason> {
    let asked = settings.quality.value;
    let source_estimate = super::jpeg_quality::estimate_jpeg_quality(raw);
    match (settings.encode.value, source_estimate) {
        (ImageEncode::Jpeg, Some(e)) if asked < e => Ok(None),
        (ImageEncode::Jpeg, _) => Err(unsupported(ImageSkip::AlreadyJpeg {
            source_estimate,
            asked,
        })),
        _ => Ok(Some(Outcome::Unchanged)),
    }
}

/// The JPEG options, the quality capped at a JPEG source's estimate.
fn jpeg_options(
    settings: &ResolvedImage,
    source: Source,
    raw: &[u8],
) -> (JpegOptions, JpegQuality) {
    let asked = settings.quality.value;
    let source_estimate = match source {
        Source::Dct => super::jpeg_quality::estimate_jpeg_quality(raw),
        Source::Generic => None,
    };
    let used = source_estimate.map_or(asked, |e| asked.min(e));
    let opts = JpegOptions {
        quality: used,
        chroma: settings.chroma.value,
        progressive: settings.progressive.value,
        optimize_huffman: true,
    };
    let quality = JpegQuality {
        asked,
        used,
        source_estimate,
    };
    (opts, quality)
}

fn change(
    settings: &ResolvedImage,
    space: &ImageSpace,
    job: &Job,
    jpeg: Option<JpegQuality>,
) -> ImageChange {
    let (w, h) = job.src;
    let resized = job.out != job.src;
    let binding = space.binding_placement().filter(|_| resized).cloned();
    let binding_ppi_after = binding.as_ref().map(|b| {
        let sx = f64::from(job.out.0) / f64::from(w);
        let sy = f64::from(job.out.1) / f64::from(h);
        (b.ppi_x * sx).min(b.ppi_y * sy)
    });
    let (encoded_as, bpc_after) = match job.target {
        Target::Flate { .. } if job.convert.is_some() => (encoded(job.target), 8),
        Target::Flate { .. } => (encoded(job.target), job.bpc),
        Target::Jpeg(_) => (ImageEncode::Jpeg, 8),
    };
    let before = space
        .color_space
        .clone()
        .unwrap_or_else(|| "none".to_owned());
    let after = job
        .convert
        .as_ref()
        .map_or_else(|| before.clone(), ConvertJob::space_name);
    let narrow = |b: u32| u8::try_from(b).unwrap_or(u8::MAX);
    ImageChange {
        settings: *settings,
        size_before: job.src,
        size_after: job.out,
        binding,
        binding_ppi_after,
        encoded_as,
        bpc: (narrow(job.bpc), narrow(bpc_after)),
        jpeg,
        conversion: job.convert.as_ref().map(ConvertJob::conversion),
        color_space: (before, after),
        predicted: false,
        resampled_with: None,
    }
}

const fn encoded(target: Target) -> ImageEncode {
    match target {
        Target::Flate { level, .. } => ImageEncode::Flate { level },
        Target::Jpeg(_) => ImageEncode::Jpeg,
    }
}

/// The image's `(components, bits per component)`, when it can be decoded
/// and, for `jpeg`, written as JPEG.
fn sample_format(
    dict: &Dict,
    space: &ImageSpace,
    source: Source,
    jpeg: bool,
) -> Result<(u32, u32), SkipReason> {
    let (w, h) = (space.width, space.height);
    let (components, bpc) = if space.image_mask {
        (1, 1)
    } else {
        let c = space
            .components
            .ok_or(unsupported(ImageSkip::UnknownComponents))?;
        (u32::from(c), int(dict, b"BitsPerComponent").unwrap_or(0))
    };
    if !matches!(bpc, 1 | 2 | 4 | 8 | 16)
        || (source == Source::Dct && bpc != 8)
        || (int(dict, b"Width"), int(dict, b"Height")) != (Some(w), Some(h))
    {
        return Err(unsupported(ImageSkip::InvalidSize));
    }
    if jpeg && let Some(why) = jpeg_blocker(dict, space, components, bpc) {
        return Err(unsupported(why));
    }
    Ok((components, bpc))
}

/// Why this image cannot be written as JPEG, if it cannot.
fn jpeg_blocker(dict: &Dict, space: &ImageSpace, components: u32, bpc: u32) -> Option<ImageSkip> {
    if space.image_mask || bpc == 1 {
        Some(ImageSkip::OneBitAsJpeg)
    } else if space.class() == ImageClass::Indexed {
        Some(ImageSkip::PaletteAsJpeg)
    } else if colour_keyed(dict) {
        Some(ImageSkip::ColourKey(ImageStep::Jpeg))
    } else if !matches!(components, 1 | 3 | 4) {
        Some(ImageSkip::JpegComponents)
    } else {
        None
    }
}

#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)] // the product is below `len`, which fits u32
fn scaled(len: u32, factor: f64) -> u32 {
    ((f64::from(len) * factor).round() as u32).clamp(1, len)
}

const fn kernel(method: ResampleMethod) -> Option<Kernel> {
    match method {
        ResampleMethod::Average => Some(Kernel::Average),
        ResampleMethod::Lanczos => Some(Kernel::Lanczos),
        ResampleMethod::Subsample => None,
        _ => Some(Kernel::Bicubic),
    }
}
