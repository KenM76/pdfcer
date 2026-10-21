//! Performing a planned [`Job`]: decode the source, convert to 8 bits for
//! JPEG or convert its colour, resample, encode, and keep the result only
//! when it passes the never-larger and `min-saving` guards.

use super::Conversion;
use super::convert;
use super::pipeline::{Job, Source, Target};
use crate::deskew::Raster;
use crate::filters::{self, flate};
use crate::image_codec::{CodecColorModel, decode_image_view};
use crate::image_resample::resample_with;
use crate::jpeg_write::{self, unpack_to_bytes};
use crate::object::{Dict, Name, Object};
use crate::optimize::SkipReason;
use crate::view::DocumentView;

/// A performed job.
pub(crate) struct Written {
    pub(crate) dict: Dict,
    pub(crate) data: Vec<u8>,
    /// Bits per component written.
    pub(crate) bpc: u32,
    /// The conversion's report entry, run results filled in.
    pub(crate) conversion: Option<Conversion>,
}

/// Perform `job` on the image stored as `raw` under `dict`; `view`
/// resolves the dictionary's indirect entries for a JPEG decode.
pub(crate) fn run(
    job: &Job,
    view: &DocumentView<'_>,
    dict: &Dict,
    raw: &[u8],
) -> Result<Written, SkipReason> {
    let int = |k: &[u8]| match dict.get(k) {
        Some(Object::Integer(n)) => u32::try_from(*n).ok(),
        _ => None,
    };
    if (int(b"Width"), int(b"Height")) != (Some(job.src.0), Some(job.src.1)) {
        return Err(SkipReason::ImageUnsupported(
            "its size changed since it was planned".to_owned(),
        ));
    }
    let (samples, bpc) = source_samples(job, view, dict, raw)?;
    let raster = Raster {
        width: job.src.0,
        height: job.src.1,
        components: job.components,
        bpc,
        paper_is_ones: true,
        indexed: job.indexed,
    };
    let (pixels, components, bpc, colour) = match &job.convert {
        Some(c) => {
            let done = convert::apply(c, &raster, &samples, job.out, job.kernel);
            let space = Some((done.color_space, done.conversion));
            (done.data, done.components, done.bpc, space)
        }
        None if job.out == job.src => (samples, job.components, bpc, None),
        None => {
            let resized = resample_with(&raster, &samples, job.out.0, job.out.1, job.kernel);
            (resized, job.components, bpc, None)
        }
    };
    let (data, filter, bpc) = match job.target {
        Target::Flate(level) => (
            flate::encode_with_level(&pixels, u32::from(level)),
            &b"FlateDecode"[..],
            bpc,
        ),
        Target::Jpeg(opts) => {
            let n = u8::try_from(components).unwrap_or(0);
            let data = jpeg_write::encode(&pixels, job.out.0, job.out.1, n, opts)
                .map_err(SkipReason::ImageUnsupported)?;
            (data, &b"DCTDecode"[..], 8)
        }
    };
    guard(job, raw.len() as u64, data.len() as u64)?;
    let mut out = output_dict(dict, job.out, filter, bpc, data.len());
    let conversion = colour.map(|(space, conversion)| {
        out.remove(b"Decode");
        out.remove(b"D");
        out.insert(Name::from(&b"ColorSpace"[..]), space);
        conversion
    });
    Ok(Written {
        dict: out,
        data,
        bpc,
        conversion,
    })
}

/// The never-larger and `min-saving` guards.
fn guard(job: &Job, before: u64, after: u64) -> Result<(), SkipReason> {
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
    Ok(())
}

/// `dict` describing the new samples. `/Decode` and `/ColorSpace` are kept:
/// the samples keep their meaning, including an inverted CMYK `/Decode`.
fn output_dict(dict: &Dict, size: (u32, u32), filter: &[u8], bpc: u32, len: usize) -> Dict {
    let mut out = dict.clone();
    out.remove(b"DecodeParms");
    out.remove(b"DP");
    let mut set = |k: &[u8], v: Object| {
        out.insert(Name::from(k), v);
    };
    set(b"Width", Object::Integer(i64::from(size.0)));
    set(b"Height", Object::Integer(i64::from(size.1)));
    set(b"Filter", Object::Name(Name::from(filter)));
    if !matches!(dict.get(b"ImageMask"), Some(Object::Boolean(true))) {
        set(b"BitsPerComponent", Object::Integer(i64::from(bpc)));
    }
    set(
        b"Length",
        Object::Integer(i64::try_from(len).unwrap_or(i64::MAX)),
    );
    out
}

/// The source's samples and their bit depth: as stored for a generic
/// source written back as Flate, else one byte per component with no row
/// padding (sub-byte depths scaled onto 0..=255, 16 bits to the high byte).
fn source_samples(
    job: &Job,
    view: &DocumentView<'_>,
    dict: &Dict,
    raw: &[u8],
) -> Result<(Vec<u8>, u32), SkipReason> {
    let (w, h) = job.src;
    let flat_len = (w as usize)
        .saturating_mul(h as usize)
        .saturating_mul(job.components as usize);
    let short = |got: usize, need: usize| {
        SkipReason::Undecodable(format!("{got} sample bytes, {need} expected"))
    };
    match job.source {
        Source::Generic => {
            let packed = filters::decode_stream(dict, raw)
                .map_err(|e| SkipReason::Undecodable(e.to_string()))?;
            let raster = Raster {
                width: w,
                height: h,
                components: job.components,
                bpc: job.bpc,
                paper_is_ones: true,
                indexed: job.indexed,
            };
            let stride = raster.row_bytes();
            let need = stride.saturating_mul(h as usize);
            if packed.len() < need {
                return Err(short(packed.len(), need));
            }
            if matches!(job.target, Target::Flate(_)) || job.convert.is_some() {
                return Ok((packed, job.bpc));
            }
            let bits = u8::try_from(job.bpc).unwrap_or(8);
            let n = job.components as usize;
            Ok((unpack_to_bytes(&packed, w, h, n, bits, stride), 8))
        }
        Source::Dct => {
            let coded = decode_image_view(view, dict, raw, false)
                .map_err(|e| SkipReason::Undecodable(e.to_string()))?;
            let model_ok = matches!(
                coded.color_model,
                CodecColorModel::Gray
                    | CodecColorModel::Rgb
                    | CodecColorModel::Untransformed3
                    | CodecColorModel::Cmyk
            );
            if (coded.width, coded.height) != (w, h)
                || u32::from(coded.components) != job.components
                || !model_ok
            {
                return Err(SkipReason::ImageUnsupported(
                    "its JPEG data disagrees with its dictionary".to_owned(),
                ));
            }
            let mut samples = coded.samples;
            if samples.len() < flat_len {
                return Err(short(samples.len(), flat_len));
            }
            samples.truncate(flat_len);
            Ok((samples, 8))
        }
    }
}
