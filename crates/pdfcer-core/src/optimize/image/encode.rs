//! Performing a planned [`Job`]: decode the source, convert to 8 bits for
//! JPEG or convert its colour, resample, encode, and keep the result only
//! when it passes the never-larger and `min-saving` guards.

use super::convert;
use super::pipeline::{Job, Source, Target};
use super::predict::{self, Codec};
use super::{Conversion, ImageSkip};
use crate::deskew::Raster;
use crate::filters;
use crate::image_codec::{self, CodecColorModel, decode_image_view};
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
    /// Whether the samples were PNG-predicted.
    pub(crate) predicted: bool,
    /// For LZW output, the Flate level 9 size of the same samples.
    pub(crate) flate_bytes: Option<u64>,
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
        return Err(SkipReason::ImageUnsupported(ImageSkip::SizeChanged));
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
    let Encoded {
        data,
        filter,
        bpc,
        parms,
        flate_bytes,
    } = encode(job, &pixels, (components, bpc))?;
    guard(job, raw.len() as u64, data.len() as u64)?;
    let mut out = output_dict(dict, job.out, filter, bpc, data.len());
    let predicted = parms.is_some() && job.target != Target::Ccitt;
    if let Some(p) = parms {
        out.insert(Name::from(&b"DecodeParms"[..]), Object::Dict(p));
    }
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
        predicted,
        flate_bytes,
    })
}

/// Samples encoded for the job's target.
struct Encoded {
    data: Vec<u8>,
    filter: &'static [u8],
    bpc: u32,
    parms: Option<Dict>,
    flate_bytes: Option<u64>,
}

fn encode(job: &Job, pixels: &[u8], (components, bpc): (u32, u32)) -> Result<Encoded, SkipReason> {
    let shape = (components, bpc, job.out.0);
    let failed =
        |e: filters::FilterError| SkipReason::ImageUnsupported(ImageSkip::LzwEncode(e.to_string()));
    let predicted = |codec, predictor| predict::compress(pixels, shape, codec, predictor);
    let plain = |data, filter| Encoded {
        data,
        filter,
        bpc,
        parms: None,
        flate_bytes: None,
    };
    Ok(match job.target {
        Target::Flate { level, predictor } => {
            let done = predicted(Codec::Flate(level), predictor).map_err(failed)?;
            Encoded {
                parms: done.parms,
                ..plain(done.data, b"FlateDecode")
            }
        }
        Target::Lzw { predictor } => {
            let done = predicted(Codec::Lzw, predictor).map_err(failed)?;
            let flate = predicted(Codec::Flate(9), predictor).map_err(failed)?;
            Encoded {
                parms: done.parms,
                flate_bytes: Some(flate.data.len() as u64),
                ..plain(done.data, b"LZWDecode")
            }
        }
        Target::Jpeg(opts) => {
            let n = u8::try_from(components).unwrap_or(0);
            let data = jpeg_write::encode(pixels, job.out.0, job.out.1, n, opts)
                .map_err(|e| SkipReason::ImageUnsupported(ImageSkip::JpegEncode(e)))?;
            Encoded {
                bpc: 8,
                ..plain(data, b"DCTDecode")
            }
        }
        Target::RunLength => plain(filters::runlength::encode(pixels), b"RunLengthDecode"),
        Target::Ccitt => ccitt(job, pixels, (components, bpc))?,
        Target::Jbig2 => jbig2(job, pixels, (components, bpc))?,
    })
}

/// One lossless generic region with no globals, so no `/DecodeParms`.
/// The codec codes a PDF 1 as JBIG2 white, so `/Decode` stays as it is.
fn jbig2(job: &Job, pixels: &[u8], format: (u32, u32)) -> Result<Encoded, SkipReason> {
    if format != (1, 1) {
        return Err(SkipReason::ImageUnsupported(ImageSkip::NotBilevel(
            super::ImageEncode::Jbig2,
        )));
    }
    let (w, h) = job.out;
    Ok(Encoded {
        data: image_codec::jbig2::encode_generic(pixels, w, h),
        filter: b"JBIG2Decode",
        bpc: 1,
        parms: None,
        flate_bytes: None,
    })
}

/// Group 4 in whichever polarity codes smaller (ISO 32000-1 §7.4.6
/// Table 11): `/BlackIs1 true` with a 1 bit coded black, else `/BlackIs1`
/// omitted with a 1 bit coded white. Decoded samples equal `pixels` both
/// ways, so `/Decode` and `/ImageMask` stay as they are.
fn ccitt(job: &Job, pixels: &[u8], format: (u32, u32)) -> Result<Encoded, SkipReason> {
    if format != (1, 1) {
        return Err(SkipReason::ImageUnsupported(ImageSkip::NotBilevel(
            super::ImageEncode::Ccitt,
        )));
    }
    let (w, h) = job.out;
    let black = image_codec::ccitt::encode_g4(pixels, w, h, true);
    let white = image_codec::ccitt::encode_g4(pixels, w, h, false);
    let black_is_1 = black.len() < white.len();
    let mut parms = Dict::new();
    let mut set = |k: &[u8], v: Object| {
        parms.insert(Name::from(k), v);
    };
    set(b"K", Object::Integer(-1));
    set(b"Columns", Object::Integer(i64::from(w)));
    set(b"Rows", Object::Integer(i64::from(h)));
    if black_is_1 {
        set(b"BlackIs1", Object::Boolean(true));
    }
    Ok(Encoded {
        data: if black_is_1 { black } else { white },
        filter: b"CCITTFaxDecode",
        bpc: 1,
        parms: Some(parms),
        flate_bytes: None,
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
            if matches!(
                job.target,
                Target::Flate { .. }
                    | Target::RunLength
                    | Target::Lzw { .. }
                    | Target::Ccitt
                    | Target::Jbig2
            ) || job.convert.is_some()
            {
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
                return Err(SkipReason::ImageUnsupported(ImageSkip::JpegMismatch));
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
