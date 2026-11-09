//! `JXLDecode` — JPEG XL (ISO/IEC 18181) image data, a **non-standard**
//! filter.
//!
//! No part of ISO 32000-1/2 names JPEG XL, and no PDF extension registers
//! it. `/JXLDecode` is the de-facto name used by PDFium (CL 142070) and
//! PdfCraft (PR #5648); pdfcer reads it, sets
//! [`CodecNotes::jxl_nonstandard`] on every image so the shells disclose
//! it, and never writes it. Spec RAG: `PDF_Spec/filters/filter__jxl.md`.
//!
//! The dictionary rules are the `JPXDecode` ones (Table 89), because the
//! codestream is self-describing in the same way: see
//! [`Codec::is_self_describing`]. Frame 0 only; animation is ignored.
//! Colour is delivered as 8-bit gray or RGB in the codestream's own
//! encoding with its ICC profile attached (synthesised by the decoder
//! for enum encodings; sRGB when an XYB image's profile cannot be
//! expressed without a CMS). CMYK (a black extra channel) is refused by
//! name. Alpha is the image's first alpha channel, gated by
//! `/SMaskInData` when present, and otherwise used unless the dictionary
//! carries its own `/SMask`.

use jxl_oxide::color::{ColourEncoding, ColourSpace, TransferFunction, WhitePoint};
use jxl_oxide::{AllocTracker, JxlImage, NullCms};

use super::{
    Codec, CodecColorModel, CodecNotes, CodedImage, ImageCodecError, MAX_IMAGE_DIMENSION,
    MAX_IMAGE_PIXELS, MAX_IMAGE_SAMPLE_BYTES,
};
use pdfcer_model::graph::ObjectGraph;
use pdfcer_model::object::{Dict, Object};
use pdfcer_model::view::DocumentView;

/// Ceiling on the decoder's own allocations, enforced by its
/// [`AllocTracker`]: four colour-plus-alpha `f32` planes at
/// [`MAX_IMAGE_PIXELS`], the same budget as the JPX decoder.
const MAX_WORKING_BYTES: u64 = MAX_IMAGE_PIXELS * 4 * 4;

/// Bare-codestream signature (ISO/IEC 18181-1 §6.1).
const CODESTREAM_SIGNATURE: [u8; 2] = [0xFF, 0x0A];

/// ISOBMFF container signature box (ISO/IEC 18181-2 §3).
const CONTAINER_SIGNATURE: [u8; 12] = [
    0, 0, 0, 0x0C, b'J', b'X', b'L', b' ', 0x0D, 0x0A, 0x87, 0x0A,
];

/// Is `data` a JPEG XL codestream or container?
#[must_use]
pub fn is_jxl(data: &[u8]) -> bool {
    data.starts_with(&CODESTREAM_SIGNATURE) || data.starts_with(&CONTAINER_SIGNATURE)
}

/// Pixels of a decoded JPEG XL image, before any PDF interpretation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct JxlPixels {
    pub width: u32,
    pub height: u32,
    /// 1 (gray) or 3 (RGB).
    pub colour_channels: u8,
    /// Interleaved 8-bit colour samples, unpremultiplied.
    pub colour: Vec<u8>,
    /// One 8-bit alpha sample per pixel, when the image has alpha.
    pub alpha: Option<Vec<u8>>,
    /// The ICC profile the colour samples are in.
    pub icc_profile: Vec<u8>,
    /// The samples are sRGB (or sRGB-transfer D65 gray), so a device
    /// colour space carries them without a profile.
    pub srgb: bool,
}

/// Decode frame 0 of a JPEG XL file to 8-bit pixels.
///
/// # Errors
///
/// [`ImageCodecError::TooLarge`] past the pixel or working-set ceilings;
/// `FeatureUnsupported { "JXL/cmyk" }` for an image with a black channel;
/// [`ImageCodecError::Corrupt`] for anything the decoder rejects.
pub fn decode_pixels(data: &[u8]) -> Result<JxlPixels, ImageCodecError> {
    if !is_jxl(data) {
        return Err(corrupt("not a JPEG XL codestream or container"));
    }
    let tracker =
        AllocTracker::with_limit(usize::try_from(MAX_WORKING_BYTES).unwrap_or(usize::MAX));
    let mut image = JxlImage::builder()
        .alloc_tracker(tracker)
        .read(data)
        .map_err(|e| corrupt_owned(e.to_string()))?;
    image.set_cms(NullCms);

    let header = read_header(&image)?;
    let (colour_channels, has_alpha) = (header.colour_channels, header.has_alpha);
    let channels = usize::from(colour_channels) + usize::from(has_alpha);

    let render = image
        .render_frame(0)
        .map_err(|e| corrupt_owned(e.to_string()))?;
    let mut stream = render.stream();
    // Orientation is applied by the stream, so its geometry is the one
    // the samples are laid out in.
    let (width, height) = (stream.width(), stream.height());
    if stream.channels() as usize != channels {
        return Err(corrupt("decoded channel count disagrees with the header"));
    }
    let len = (width as usize)
        .saturating_mul(height as usize)
        .saturating_mul(channels);
    let mut interleaved = vec![0u8; len];
    if stream.write_to_buffer(&mut interleaved) != len {
        return Err(corrupt("decoder produced fewer samples than declared"));
    }

    let (colour, alpha) = split_alpha(
        &interleaved,
        usize::from(colour_channels),
        has_alpha,
        header.associated,
    );
    Ok(JxlPixels {
        width,
        height,
        colour_channels,
        colour,
        alpha,
        icc_profile: image.rendered_icc(),
        srgb: header.srgb,
    })
}

/// What the image header says, after the ceilings and refusals.
struct Header {
    colour_channels: u8,
    has_alpha: bool,
    associated: bool,
    srgb: bool,
}

/// Enforce the ceilings and the CMYK refusal from the header alone, before
/// any pixel is decoded.
fn read_header(image: &JxlImage) -> Result<Header, ImageCodecError> {
    let (width, height) = (image.width(), image.height());
    let pixels = u64::from(width).saturating_mul(u64::from(height));
    if width == 0 || height == 0 {
        return Err(corrupt("zero image dimension"));
    }
    if width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION || pixels > MAX_IMAGE_PIXELS {
        return Err(ImageCodecError::TooLarge);
    }
    let format = image.pixel_format();
    if format.has_black() {
        return Err(ImageCodecError::FeatureUnsupported {
            feature: "JXL/cmyk",
        });
    }
    let colour_channels: u8 = if format.is_grayscale() { 1 } else { 3 };
    let has_alpha = format.has_alpha();
    let channels = u64::from(colour_channels) + u64::from(has_alpha);
    if pixels.saturating_mul(channels) > MAX_IMAGE_SAMPLE_BYTES as u64 {
        return Err(ImageCodecError::TooLarge);
    }
    if image.num_loaded_keyframes() == 0 {
        return Err(corrupt("no complete frame"));
    }
    let metadata = &image.image_header().metadata;
    let srgb = match &metadata.colour_encoding {
        ColourEncoding::Enum(e) if e.colour_space == ColourSpace::Grey => {
            e.white_point == WhitePoint::D65 && e.tf == TransferFunction::Srgb
        }
        enc => enc.is_srgb(),
    };
    let associated = metadata
        .ec_info
        .iter()
        .find_map(|ec| ec.alpha_associated())
        .unwrap_or(false);
    Ok(Header {
        colour_channels,
        has_alpha,
        associated,
        srgb,
    })
}

/// Separate interleaved colour+alpha, unpremultiplying associated alpha
/// (ISO/IEC 18181-1 `alpha_associated`) so the colour is straight, as a
/// PDF soft mask expects.
fn split_alpha(
    interleaved: &[u8],
    colour_channels: usize,
    has_alpha: bool,
    associated: bool,
) -> (Vec<u8>, Option<Vec<u8>>) {
    if !has_alpha {
        return (interleaved.to_vec(), None);
    }
    let stride = colour_channels + 1;
    let pixels = interleaved.len() / stride;
    let mut colour = Vec::with_capacity(pixels * colour_channels);
    let mut alpha = Vec::with_capacity(pixels);
    for px in interleaved.chunks_exact(stride) {
        let (c, a) = px.split_at(colour_channels);
        let a = a.first().copied().unwrap_or(u8::MAX);
        for &v in c {
            colour.push(if associated && a > 0 && a < u8::MAX {
                ((u32::from(v) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8
            } else {
                v
            });
        }
        alpha.push(a);
    }
    (colour, Some(alpha))
}

/// Decode a `/JXLDecode` image stream (see the module docs).
///
/// `dict` is read for `/SMaskInData`, `/SMask` and the geometry
/// disagreement counter only.
pub(super) fn decode(
    doc: &DocumentView<'_>,
    data: &[u8],
    dict: &Dict,
    notes: &mut CodecNotes,
) -> Result<CodedImage, ImageCodecError> {
    notes.jxl_nonstandard = true;
    let px = decode_pixels(data)?;
    let int = |key: &[u8]| {
        dict.get(key)
            .map(|o| doc.resolve(o))
            .and_then(Object::as_int)
    };
    let use_alpha = match int(b"SMaskInData") {
        Some(0) => false,
        Some(_) => true,
        None => !dict.contains_key(b"SMask"),
    };
    let differs =
        |key: &[u8], actual: u32| int(key).is_some_and(|v| u32::try_from(v) != Ok(actual));
    notes.geometry_mismatch |= differs(b"Width", px.width) || differs(b"Height", px.height);

    Ok(CodedImage {
        samples: px.colour,
        codec: Some(Codec::Jxl),
        width: px.width,
        height: px.height,
        components: px.colour_channels,
        bits_per_component: 8,
        color_model: if px.colour_channels == 1 {
            CodecColorModel::Gray
        } else {
            CodecColorModel::Rgb
        },
        icc_profile: Some(px.icc_profile),
        embedded_alpha: if use_alpha { px.alpha } else { None },
        notes: *notes,
    })
}

fn corrupt(detail: &'static str) -> ImageCodecError {
    corrupt_owned(detail.to_owned())
}

fn corrupt_owned(detail: String) -> ImageCodecError {
    ImageCodecError::Corrupt {
        codec: Codec::Jxl,
        detail,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn both_signatures_are_recognised_and_nothing_else() {
        assert!(is_jxl(&[0xFF, 0x0A, 0x00]));
        assert!(is_jxl(&CONTAINER_SIGNATURE));
        assert!(!is_jxl(&[0xFF, 0xD8, 0xFF]));
        assert!(!is_jxl(&[]));
    }

    #[test]
    fn associated_alpha_is_unpremultiplied() {
        // Half-transparent mid-grey, premultiplied: 64 at alpha 128.
        let (c, a) = split_alpha(&[64, 128, 255, 255], 1, true, true);
        assert_eq!(c, [128, 255]);
        assert_eq!(a.unwrap(), [128, 255]);
        let (c, _) = split_alpha(&[64, 128], 1, true, false);
        assert_eq!(c, [64]);
    }

    #[test]
    fn garbage_is_corrupt_not_a_panic() {
        for data in [
            &b""[..],
            b"\xFF\x0A",
            b"\xFF\x0A\xFF\xFF\xFF\xFF",
            &CONTAINER_SIGNATURE,
        ] {
            assert!(decode_pixels(data).is_err());
        }
    }
}
