//! Performing resolved image settings on one image XObject: decide the
//! output size from the binding placement, decode the generic filter chain,
//! resample the packed samples, re-encode as `/FlateDecode`, and keep the
//! result only when it passes the never-larger and `min-saving` guards.

use super::{ImageEncode, ResampleMethod, ResolvedImage};
use crate::deskew::Raster;
use crate::filters::{self, flate};
use crate::image_resample::resample_with;
use crate::object::{Dict, Name, Object};
use crate::ocr::upscale::Kernel;
use crate::optimize::SkipReason;
use crate::optimize::streams::{GENERIC, chain};
use crate::space_audit::{ImageClass, ImagePlacement, ImageSpace};

/// What one image's downsampling did, for the report.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ImageChange {
    /// The settings that produced it, each with its source.
    pub settings: ResolvedImage,
    /// Pixel size before, `(width, height)`.
    pub size_before: (u32, u32),
    /// Pixel size after.
    pub size_after: (u32, u32),
    /// The placement that limited the reduction: the one drawn largest,
    /// at the lowest effective ppi.
    pub binding: Option<ImagePlacement>,
    /// That placement's effective ppi after resampling.
    pub binding_ppi_after: Option<f64>,
}

/// A resample to redo on a stream an earlier step of the run rewrote.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Job {
    src: (u32, u32),
    out: (u32, u32),
    components: u32,
    bpc: u32,
    indexed: bool,
    kernel: Option<Kernel>,
    level: u8,
    min_saving_pct: u8,
}

/// What resolved settings ask of one image.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Outcome {
    /// Nothing: no downsampling and the encoding kept.
    Unchanged,
    /// No downsampling; recompress losslessly at this level.
    Lossless(u8),
    /// Downsample.
    Resample(Job, Box<ImageChange>),
}

fn int(dict: &Dict, key: &[u8]) -> Option<u32> {
    match dict.get(key) {
        Some(Object::Integer(n)) => u32::try_from(*n).ok(),
        _ => None,
    }
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
    } else if matches!(dict.get(b"Mask"), Some(Object::Array(_))) {
        Some("a colour-key masked image is not resampled")
    } else {
        None
    }
}

/// Decide what `settings` do to the image stored under `dict`, audited as
/// `space`. `blocker` is [`mask_blocker`]'s answer.
pub(crate) fn decide(
    settings: &ResolvedImage,
    dict: &Dict,
    space: Option<&ImageSpace>,
    blocker: Option<&'static str>,
) -> Result<Outcome, SkipReason> {
    let lossless = match settings.encode.value {
        ImageEncode::Keep => Outcome::Unchanged,
        ImageEncode::Flate { level } => Outcome::Lossless(level),
    };
    let (Some(ppi), Some(trigger)) = (settings.ppi, settings.trigger()) else {
        return Ok(lossless);
    };
    if !settings.resamples() {
        return Ok(lossless);
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
    if out == (w, h) {
        return Ok(lossless);
    }
    if let Some(why) = blocker {
        return Err(SkipReason::ImageUnsupported(why.to_owned()));
    }
    let (components, bpc) = sample_format(dict, space)?;
    let indexed = space.class() == ImageClass::Indexed;
    let job = Job {
        src: (w, h),
        out,
        components,
        bpc,
        indexed,
        kernel: kernel(settings.method.value),
        level: match settings.encode.value {
            ImageEncode::Flate { level } => level,
            ImageEncode::Keep => 9,
        },
        min_saving_pct: settings.min_saving_pct.value,
    };
    let binding = space.binding_placement().cloned();
    let binding_ppi_after = binding.as_ref().map(|b| {
        let sx = f64::from(out.0) / f64::from(w);
        let sy = f64::from(out.1) / f64::from(h);
        (b.ppi_x * sx).min(b.ppi_y * sy)
    });
    let change = ImageChange {
        settings: *settings,
        size_before: (w, h),
        size_after: out,
        binding,
        binding_ppi_after,
    };
    Ok(Outcome::Resample(job, Box::new(change)))
}

/// The image's `(components, bits per component)`, when this pass can
/// decode and resample it.
fn sample_format(dict: &Dict, space: &ImageSpace) -> Result<(u32, u32), SkipReason> {
    let (w, h) = (space.width, space.height);
    for name in chain(dict) {
        if !GENERIC.contains(&name.as_bytes()) {
            return Err(SkipReason::UnsupportedFilter(name));
        }
    }
    let (components, bpc) = if space.image_mask {
        (1, 1)
    } else {
        let c = space.components.ok_or_else(|| {
            SkipReason::ImageUnsupported("its colour space's components are unknown".to_owned())
        })?;
        (u32::from(c), int(dict, b"BitsPerComponent").unwrap_or(0))
    };
    if !matches!(bpc, 1 | 2 | 4 | 8 | 16)
        || (int(dict, b"Width"), int(dict, b"Height")) != (Some(w), Some(h))
    {
        return Err(SkipReason::ImageUnsupported(
            "its size or bit depth is invalid".to_owned(),
        ));
    }
    Ok((components, bpc))
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

/// Perform `job` on the image stored as `raw` under `dict`.
pub(crate) fn run(job: &Job, dict: &Dict, raw: &[u8]) -> Result<(Dict, Vec<u8>), SkipReason> {
    if (int(dict, b"Width"), int(dict, b"Height")) != (Some(job.src.0), Some(job.src.1)) {
        return Err(SkipReason::ImageUnsupported(
            "its size changed since it was planned".to_owned(),
        ));
    }
    let samples =
        filters::decode_stream(dict, raw).map_err(|e| SkipReason::Undecodable(e.to_string()))?;
    let raster = Raster {
        width: job.src.0,
        height: job.src.1,
        components: job.components,
        bpc: job.bpc,
        paper_is_ones: true,
        indexed: job.indexed,
    };
    let need = raster.row_bytes().saturating_mul(job.src.1 as usize);
    if samples.len() < need {
        return Err(SkipReason::Undecodable(format!(
            "{} sample bytes, {need} expected",
            samples.len()
        )));
    }
    let resampled = resample_with(&raster, &samples, job.out.0, job.out.1, job.kernel);
    let data = flate::encode_with_level(&resampled, u32::from(job.level));
    let (before, after) = (raw.len() as u64, data.len() as u64);
    if after >= before {
        return Err(SkipReason::NotSmaller { before, after });
    }
    if after * 100 > before * u64::from(100 - job.min_saving_pct.min(99)) {
        return Err(SkipReason::BelowMinSaving {
            before,
            after,
            pct: job.min_saving_pct,
        });
    }
    let mut out = dict.clone();
    out.remove(b"DecodeParms");
    out.remove(b"DP");
    let set = |d: &mut Dict, k: &[u8], v: Object| {
        d.insert(Name::from(k), v);
    };
    set(&mut out, b"Width", Object::Integer(i64::from(job.out.0)));
    set(&mut out, b"Height", Object::Integer(i64::from(job.out.1)));
    set(
        &mut out,
        b"Filter",
        Object::Name(Name::from(b"FlateDecode")),
    );
    set(
        &mut out,
        b"Length",
        Object::Integer(i64::try_from(data.len()).unwrap_or(i64::MAX)),
    );
    Ok((out, data))
}
