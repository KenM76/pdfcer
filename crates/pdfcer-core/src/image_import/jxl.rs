//! # JPEG XL import (ISO/IEC 18181) — frame 0, transcoded to Flate
//!
//! PDF has no standard JPEG XL filter (the `/JXLDecode` that some readers
//! accept is not in ISO 32000), so pdfcer never writes one: the image is
//! decoded and stored as `/FlateDecode`, which every reader displays.
//! Lossless JPEG XL stays lossless; a lossy one is stored as the pixels it
//! decodes to.
//!
//! | Property | Handling |
//! |---|---|
//! | Container | bare codestream (`FF 0A`) or ISOBMFF box file |
//! | Colour | gray → `/DeviceGray`, colour → `/DeviceRGB`, 8 bits |
//! | Colour encoding | not sRGB → [`ImportNotes::colour_profile_dropped`] |
//! | Alpha | first alpha channel → 8-bit `/SMask`, unpremultiplied |
//! | Orientation | applied to the pixels by the decoder |
//! | Animation | frame 0 placed |
//! | CMYK (black channel) | refused: `JXL/cmyk` |
//!
//! Every import reports [`RecompressReason::SourceCodecNotReusable`].

use super::{
    ImageFormat, ImageImportError, ImportColorSpace, ImportFilter, ImportNotes, ImportedImage,
    Orientation, PdfFeature, RecompressReason, SoftMask, check_dimensions, flate_encode,
    raise_version,
};
use crate::image_codec::ImageCodecError;
use crate::image_codec::jxl::decode_pixels;

#[cfg(all(test, feature = "jxl"))]
// Shared with the codec crate's tests; this crate uses only some of them.
#[allow(dead_code)]
#[path = "../../../pdfcer-image-codec/src/fixtures_jxl.rs"]
mod fixtures;

/// Import a `.jxl` file.
///
/// # Errors
///
/// [`ImageImportError::Unsupported`] (`JXL/cmyk`), [`ImageImportError::TooLarge`],
/// [`ImageImportError::Empty`] or [`ImageImportError::Corrupt`].
pub fn import(data: &[u8]) -> Result<ImportedImage, ImageImportError> {
    let px = decode_pixels(data).map_err(|e| match e {
        ImageCodecError::TooLarge => ImageImportError::TooLarge,
        ImageCodecError::FeatureUnsupported { feature } => {
            ImageImportError::Unsupported { feature }
        }
        other => ImageImportError::Corrupt {
            detail: other.to_string(),
        },
    })?;
    check_dimensions(px.width, px.height, u32::from(px.colour_channels) + 1, 8)?;

    let mut notes = ImportNotes {
        recompressed: Some(RecompressReason::SourceCodecNotReusable),
        colour_profile_dropped: !px.srgb,
        ..ImportNotes::default()
    };
    let soft_mask = match px.alpha {
        Some(alpha) if alpha.iter().any(|&a| a != u8::MAX) => {
            notes.alpha_to_soft_mask = true;
            raise_version(&mut notes.requires_pdf_version, PdfFeature::SoftMask);
            Some(SoftMask {
                width: px.width,
                height: px.height,
                bits_per_component: 8,
                data: flate_encode(&alpha)?,
            })
        }
        _ => None,
    };
    Ok(ImportedImage {
        format: ImageFormat::Jxl,
        width: px.width,
        height: px.height,
        bits_per_component: 8,
        color_space: if px.colour_channels == 1 {
            ImportColorSpace::DeviceGray
        } else {
            ImportColorSpace::DeviceRgb
        },
        filter: ImportFilter::Flate,
        data: flate_encode(&px.colour)?,
        soft_mask,
        color_key_mask: None,
        orientation: Orientation::Identity,
        dpi: None,
        notes,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::fixtures as fx;
    use super::*;

    fn inflate(data: &[u8]) -> Vec<u8> {
        use std::io::Read;
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(data)
            .read_to_end(&mut out)
            .unwrap();
        out
    }

    #[test]
    fn gray_imports_as_devicegray_flate_with_the_decoded_samples() {
        let img = super::super::import(fx::JXL_GRAY_4X2_BOXED).unwrap();
        assert_eq!(img.format, ImageFormat::Jxl);
        assert_eq!((img.width, img.height), (4, 2));
        assert_eq!(img.color_space, ImportColorSpace::DeviceGray);
        assert_eq!(img.filter, ImportFilter::Flate);
        assert_eq!(inflate(&img.data), fx::JXL_GRAY_4X2_SAMPLES);
        assert!(img.soft_mask.is_none());
        assert_eq!(
            img.notes.recompressed,
            Some(RecompressReason::SourceCodecNotReusable)
        );
    }

    #[test]
    fn alpha_becomes_a_soft_mask() {
        let img = super::super::import(fx::JXL_RGBA_2X2).unwrap();
        assert_eq!(img.color_space, ImportColorSpace::DeviceRgb);
        let mask = img.soft_mask.as_ref().unwrap();
        let alpha: Vec<u8> = fx::JXL_RGBA_2X2_SAMPLES
            .iter()
            .skip(3)
            .step_by(4)
            .copied()
            .collect();
        assert_eq!(inflate(&mask.data), alpha);
        assert!(img.notes.alpha_to_soft_mask);
        assert_eq!(img.notes.requires_pdf_version, Some(PdfFeature::SoftMask));
    }

    #[test]
    fn truncated_file_is_corrupt() {
        let cut = fx::JXL_RGB_2X2.get(..fx::JXL_RGB_2X2.len() / 2).unwrap();
        assert!(matches!(
            super::super::import(cut),
            Err(ImageImportError::Corrupt { .. })
        ));
    }
}
