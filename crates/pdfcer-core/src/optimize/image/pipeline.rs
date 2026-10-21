//! Deciding what resolved image settings do to one image XObject: the
//! output size from the binding placement, the source kind, and the target
//! encoding. [`super::encode::run`] performs the result.

use super::{ImageEncode, ResampleMethod, ResolvedImage};
use crate::jpeg_write::JpegOptions;
use crate::object::{Dict, Object};
use crate::ocr::upscale::Kernel;
use crate::optimize::SkipReason;
use crate::optimize::streams::{GENERIC, chain};
use crate::space_audit::{ImageClass, ImagePlacement, ImageSpace};

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
    /// `/FlateDecode` at this level.
    Flate(u8),
    /// `/DCTDecode`.
    Jpeg(JpegOptions),
}

/// A planned rewrite, redone on a stream an earlier step of the run
/// rewrote.
#[derive(Debug, Clone, Copy, PartialEq)]
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

fn unsupported(why: &str) -> SkipReason {
    SkipReason::ImageUnsupported(why.to_owned())
}

/// Why an image's masking keeps it from being resampled on its own, if it
/// does: a `/Matte` soft mask must keep its parent's size (ISO 32000-2
/// §11.6.5.3), and blending a colour-key image invents fringe colours that
/// no longer match the key. `smask` is the image's resolved `/SMask`.
pub(crate) fn mask_blocker(dict: &Dict, smask: Option<&Dict>) -> Option<&'static str> {
    if dict.contains_key(b"Matte") {
        Some("a soft mask with /Matte keeps its parent's size")
    } else if smask.is_some_and(|m| m.contains_key(b"Matte")) {
        Some("an image whose soft mask has /Matte keeps its mask's size")
    } else if colour_keyed(dict) {
        Some("a colour-key masked image is not resampled")
    } else {
        None
    }
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
/// audited as `space`. `blocker` is [`mask_blocker`]'s answer.
pub(crate) fn decide(
    settings: &ResolvedImage,
    dict: &Dict,
    raw: &[u8],
    space: Option<&ImageSpace>,
    blocker: Option<&'static str>,
) -> Result<Outcome, SkipReason> {
    let resized = target_size(settings, space)?;
    let encode = settings.encode.value;
    let source = match (resized, encode) {
        (None, ImageEncode::Keep) => return Ok(Outcome::Unchanged),
        (None, ImageEncode::Flate { level }) => return Ok(Outcome::Lossless(level)),
        _ => source_kind(dict)?,
    };
    if resized.is_none() && source == Source::Dct {
        // An existing JPEG whose pixels are unchanged is left untouched.
        return Ok(Outcome::Unchanged);
    }
    if resized.is_some()
        && let Some(why) = blocker
    {
        return Err(unsupported(why));
    }
    let space = space.ok_or_else(|| unsupported("it was not audited"))?;
    let jpeg = matches!(encode, ImageEncode::Jpeg)
        || (encode == ImageEncode::Keep && source == Source::Dct);
    let (components, bpc) = sample_format(dict, space, source, jpeg)?;
    let (target, quality) = if jpeg {
        let (opts, q) = jpeg_options(settings, source, raw);
        (Target::Jpeg(opts), Some(q))
    } else {
        let level = match encode {
            ImageEncode::Flate { level } => level,
            _ => 9,
        };
        (Target::Flate(level), None)
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
    };
    let change = change(settings, space, &job, quality);
    Ok(Outcome::Rewrite(job, Box::new(change)))
}

/// The output size when `settings` resample `space`, else `None`.
fn target_size(
    settings: &ResolvedImage,
    space: Option<&ImageSpace>,
) -> Result<Option<(u32, u32)>, SkipReason> {
    let (Some(ppi), Some(trigger)) = (settings.ppi, settings.trigger()) else {
        return Ok(None);
    };
    if !settings.resamples() {
        return Ok(None);
    }
    let space = space
        .filter(|s| !s.placements.is_empty())
        .ok_or(SkipReason::Unplaced)?;
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
        Target::Flate(level) => (ImageEncode::Flate { level }, job.bpc),
        Target::Jpeg(_) => (ImageEncode::Jpeg, 8),
    };
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
            .ok_or_else(|| unsupported("its colour space's components are unknown"))?;
        (u32::from(c), int(dict, b"BitsPerComponent").unwrap_or(0))
    };
    if !matches!(bpc, 1 | 2 | 4 | 8 | 16)
        || (source == Source::Dct && bpc != 8)
        || (int(dict, b"Width"), int(dict, b"Height")) != (Some(w), Some(h))
    {
        return Err(unsupported("its size or bit depth is invalid"));
    }
    if jpeg && let Some(why) = jpeg_blocker(dict, space, components, bpc) {
        return Err(unsupported(why));
    }
    Ok((components, bpc))
}

/// Why this image cannot be written as JPEG, if it cannot.
fn jpeg_blocker(
    dict: &Dict,
    space: &ImageSpace,
    components: u32,
    bpc: u32,
) -> Option<&'static str> {
    if space.image_mask || bpc == 1 {
        Some("a 1-bit image is not written as JPEG")
    } else if space.class() == ImageClass::Indexed {
        Some("a palette image's indices cannot be stored as JPEG")
    } else if colour_keyed(dict) {
        Some("a colour-key masked image is not written as JPEG: its key matches exact values")
    } else if !matches!(components, 1 | 3 | 4) {
        Some("JPEG stores 1, 3 or 4 colour components")
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
