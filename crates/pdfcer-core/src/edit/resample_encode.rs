//! How a resampled image's samples are re-encoded: in the source's codec
//! family by default, so a straightened JPEG or fax scan does not grow
//! several-fold under `FlateDecode` (pdfcer request X101).

use super::deskew::Target;
use crate::image_codec::Codec;
use crate::object::Dict;
use crate::optimize::image::predict::{self, Predictor};

/// How [`super::EditSession::deskew_image_with`] encodes the new image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum DeskewEncoding {
    /// The source's codec family: `DCTDecode` from `DCTDecode`, at the
    /// quality its quantisation tables imply (90 when they cannot be read);
    /// CCITT Group 4 from a 1-bit `CCITTFaxDecode` or `JBIG2Decode` image;
    /// otherwise `FlateDecode`, PNG-predicted where that is smaller.
    #[default]
    SameFamily,
    /// `FlateDecode`, PNG-predicted (`/Predictor 15`) where that is smaller.
    /// Lossless.
    Flate,
    /// `DCTDecode` at IJG `quality`, clamped to 1–100. An image JPEG cannot
    /// hold — not 8 bits per component, not 1 or 3 components, indexed, or
    /// an image mask — is written as [`Self::Flate`], with a note.
    Dct {
        /// IJG quality, 1 to 100.
        quality: u8,
    },
}

/// The quality a `DCTDecode` source gets when its tables cannot be read.
const DEFAULT_QUALITY: u8 = 90;

/// Encoded samples, ready for [`crate::redact_image::Allocator::encoded_stream`].
pub(super) struct Coded {
    pub(super) data: Vec<u8>,
    pub(super) filter: &'static [u8],
    pub(super) parms: Option<Dict>,
    pub(super) notes: Vec<String>,
}

/// `samples` (a `width x height` image in `t`'s layout) encoded as
/// `encoding` asks. `raw` is `t`'s encoded stream data, read for the source
/// quality. Notes are prefixed `prefix`.
pub(super) fn encode(
    t: &Target,
    raw: &[u8],
    samples: &[u8],
    (width, height): (u32, u32),
    encoding: DeskewEncoding,
    prefix: &str,
) -> Coded {
    let d = &t.decoded;
    let quality = match encoding {
        DeskewEncoding::Dct { quality } => Some(quality.clamp(1, 100)),
        DeskewEncoding::SameFamily if d.codec == Some(Codec::Dct) => Some(source_quality(t, raw)),
        _ => None,
    };
    if let Some(quality) = quality {
        return match dct(t, samples, (width, height), quality) {
            Ok(mut coded) => {
                coded.notes.push(format!(
                    "{prefix} the image was re-encoded as JPEG at quality {quality}, which is lossy"
                ));
                coded
            }
            Err(why) => {
                let mut coded = flate(t, samples, width);
                coded.notes.push(format!(
                    "{prefix} the image was written FlateDecode, not DCTDecode: {why}"
                ));
                coded
            }
        };
    }
    let bilevel = d.bpc == 1 && d.components == 1;
    let fax = matches!(d.codec, Some(Codec::Ccitt | Codec::Jbig2));
    if encoding == DeskewEncoding::SameFamily && bilevel && fax {
        let (data, parms) = crate::optimize::image::encode::g4_smaller(samples, width, height);
        return Coded {
            data,
            filter: b"CCITTFaxDecode",
            parms: Some(parms),
            notes: Vec::new(),
        };
    }
    flate(t, samples, width)
}

/// The IJG quality `t`'s codestream was written at, when `DCTDecode` is its
/// only filter and its tables match a scaled standard table.
fn source_quality(t: &Target, raw: &[u8]) -> u8 {
    let only_dct = match t.stream.dict.get(b"Filter") {
        Some(crate::object::Object::Name(n)) => matches!(n.as_bytes(), b"DCTDecode" | b"DCT"),
        Some(crate::object::Object::Array(a)) => {
            a.len() == 1
                && matches!(a.first(), Some(crate::object::Object::Name(n))
                    if matches!(n.as_bytes(), b"DCTDecode" | b"DCT"))
        }
        _ => false,
    };
    only_dct
        .then(|| crate::optimize::image::jpeg_quality::estimate_jpeg_quality(raw))
        .flatten()
        .unwrap_or(DEFAULT_QUALITY)
}

/// A baseline JPEG of `samples`, or why the image cannot be one.
fn dct(t: &Target, samples: &[u8], (w, h): (u32, u32), quality: u8) -> Result<Coded, String> {
    let d = &t.decoded;
    let is_mask = matches!(
        t.stream.dict.get(b"ImageMask"),
        Some(crate::object::Object::Boolean(true))
    );
    if d.bpc != 8 || !matches!(d.components, 1 | 3) || t.raster.indexed || is_mask {
        return Err(format!(
            "JPEG holds 8-bit grey or RGB, and this image has {} component(s) at {} bits{}",
            d.components,
            d.bpc,
            if t.raster.indexed || is_mask {
                ", indexed or a mask"
            } else {
                ""
            }
        ));
    }
    let opts = crate::jpeg_write::JpegOptions {
        quality,
        chroma: crate::jpeg_write::Chroma::Auto,
        progressive: false,
        optimize_huffman: true,
    };
    #[allow(clippy::cast_possible_truncation)] // 1 or 3, checked above
    let components = d.components as u8;
    let data = crate::jpeg_write::encode(samples, w, h, components, opts)?;
    Ok(Coded {
        data,
        filter: b"DCTDecode",
        parms: None,
        notes: Vec::new(),
    })
}

/// `samples` PNG-predicted under zlib level 9, or plain `FlateDecode`,
/// whichever is smaller.
fn flate(t: &Target, samples: &[u8], width: u32) -> Coded {
    let d = &t.decoded;
    let layout = (d.components, d.bpc, width);
    let plain = crate::filters::flate::encode(samples);
    let (data, parms) =
        match predict::compress(samples, layout, predict::Codec::Flate(9), Predictor::Auto) {
            Ok(f) if f.data.len() < plain.len() => (f.data, f.parms),
            _ => (plain, None),
        };
    Coded {
        data,
        filter: b"FlateDecode",
        parms,
        notes: Vec::new(),
    }
}
