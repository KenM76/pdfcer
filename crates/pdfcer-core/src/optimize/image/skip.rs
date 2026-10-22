//! Why an image the settings ask to rewrite is not rewritten.

use std::fmt;

use super::ImageEncode;
use crate::object::ObjId;

/// The step a colour key blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ImageStep {
    /// Resampling to another size.
    Resample,
    /// Conversion to another colour format.
    Convert,
    /// Writing as JPEG.
    Jpeg,
}

/// Why an image is not rewritten, carried by
/// [`SkipReason::ImageUnsupported`](super::super::SkipReason::ImageUnsupported).
///
/// A shell words and groups these itself; `Display` is pdfcer's own
/// sentence, which the CLI prints after "not rewritten:".
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ImageSkip {
    /// A soft mask with `/Matte` keeps its parent's size (ISO 32000-2
    /// §11.6.5.3); it is resampled only with that parent.
    MatteMask,
    /// An image whose `/Matte` soft mask cannot be resampled with it (the
    /// mask is shared, or differs in size) keeps the mask's size.
    MatteParent,
    /// The image's `/Matte` soft mask `mask` could not be resampled with
    /// it, so neither is rewritten.
    MatteMaskFailed {
        /// The soft mask.
        mask: ObjId,
    },
    /// A colour-key masked image: its `/Mask` array matches exact sample
    /// values, which this step would change.
    ColourKey(ImageStep),
    /// A soft mask's samples are opacity, not colour, so it is not
    /// colour-converted.
    SoftMaskConvert,
    /// An image whose soft mask has `/Matte` keeps its colour space: the
    /// matte colour is in it (ISO 32000-2 §11.6.5.3).
    MatteParentConvert,
    /// The space audit has no row for the image.
    NotAudited,
    /// The colour space's number of components is unknown.
    UnknownComponents,
    /// `/Width`, `/Height` or `/BitsPerComponent` is invalid.
    InvalidSize,
    /// The image's size changed between planning and applying.
    SizeChanged,
    /// A 1-bit image or image mask is not written as JPEG.
    OneBitAsJpeg,
    /// A palette image's indices are not written as JPEG.
    PaletteAsJpeg,
    /// A bilevel or palette conversion result is not written as JPEG.
    ReducedAsJpeg,
    /// JPEG stores 1, 3 or 4 colour components; the image has another
    /// number.
    JpegComponents,
    /// The stored JPEG data disagrees with the image dictionary.
    JpegMismatch,
    /// The JPEG encoder failed, with its message.
    JpegEncode(String),
    /// `encode=lzw` on a document claiming PDF/A, which forbids LZW
    /// (ISO 19005-1 §6.1.10, -2 §6.1.7.2, -4 §6.1.6.2).
    LzwUnderPdfa,
    /// The LZW encoder failed, with its message.
    LzwEncode(String),
    /// `encode=brotli` on a document claiming PDF/A, whose filter list
    /// has no `BrotliDecode` (decision 194).
    BrotliUnderPdfa,
    /// The Brotli encoder failed, with its message.
    BrotliEncode(String),
    /// A rule or the `images` patch asked for a bilevel-only encoder
    /// (`ccitt`, `jbig2`) on an image whose output is not 1-bit,
    /// 1-component.
    NotBilevel(ImageEncode),
    /// The per-object override asked for a bilevel-only encoder on an
    /// image whose output is not 1-bit, 1-component, with no
    /// `convert=bilevel` step.
    NotBilevelOverride(ImageEncode),
    /// The encoder's filter is newer than the version the document
    /// declares (`jbig2` is PDF 1.4, `brotli` 2.0) and
    /// `version-raise=refuse`.
    VersionTooLow {
        /// The encoder asked for.
        encode: ImageEncode,
        /// The version its filter needs.
        needs: crate::PdfVersion,
        /// The version the document declares.
        document: crate::PdfVersion,
    },
    /// The encoder asked for needs a version above
    /// [`crate::optimize::OptimizeSettings::compatibility`].
    AboveCompatibility {
        /// The encoder asked for.
        encode: ImageEncode,
        /// The version its filter needs.
        needs: crate::PdfVersion,
        /// The compatibility target.
        target: crate::PdfVersion,
    },
    /// `encode=auto` found no candidate smaller than the stored stream by
    /// `min_saving_pct` percent, so the image is kept.
    AutoNotSmaller {
        /// Stored bytes.
        stored: u64,
        /// The required saving, percent.
        min_saving_pct: u8,
        /// Each candidate tried, in trial order, with its size in bytes.
        candidates: Vec<(ImageEncode, u64)>,
    },
    /// A Separation or DeviceN image has no defined grey.
    NoDefinedGrey {
        /// The colour space family, e.g. `Separation`.
        space: String,
    },
    /// The colour space could not be read, with the detail.
    BadColourSpace(String),
    /// A JPEG with unchanged pixels, asked to be JPEG at `asked`, is already
    /// at or below that quality (or its quality cannot be estimated), so
    /// re-encoding would lose detail and save little. One asked below its
    /// estimate is re-encoded instead.
    AlreadyJpeg {
        /// The source's estimated IJG quality, `None` when its quantization
        /// tables do not yield one.
        source_estimate: Option<u8>,
        /// The quality the settings asked for.
        asked: u8,
    },
}

impl fmt::Display for ImageSkip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MatteMask => f.write_str("a soft mask with /Matte keeps its parent's size"),
            Self::MatteParent => {
                f.write_str("an image whose soft mask has /Matte keeps its mask's size")
            }
            Self::MatteMaskFailed { mask } => write!(
                f,
                "its /Matte soft mask {} {} R could not be resampled with it",
                mask.num, mask.generation
            ),
            Self::ColourKey(ImageStep::Resample) => {
                f.write_str("a colour-key masked image is not resampled (keyed=skip)")
            }
            Self::ColourKey(ImageStep::Convert) => f.write_str(
                "a colour-key masked image is not colour-converted: its key matches source values",
            ),
            Self::ColourKey(ImageStep::Jpeg) => f.write_str(
                "a colour-key masked image is not written as JPEG: its key matches exact values",
            ),
            Self::SoftMaskConvert => f.write_str("a soft mask is not colour-converted"),
            Self::MatteParentConvert => {
                f.write_str("an image whose soft mask has /Matte keeps its colour space")
            }
            Self::NotAudited => f.write_str("it was not audited"),
            Self::UnknownComponents => f.write_str("its colour space's components are unknown"),
            Self::InvalidSize => f.write_str("its size or bit depth is invalid"),
            Self::SizeChanged => f.write_str("its size changed since it was planned"),
            Self::OneBitAsJpeg => f.write_str("a 1-bit image is not written as JPEG"),
            Self::PaletteAsJpeg => {
                f.write_str("a palette image's indices cannot be stored as JPEG")
            }
            Self::ReducedAsJpeg => {
                f.write_str("a bilevel or palette result is not written as JPEG")
            }
            Self::JpegComponents
            | Self::JpegMismatch
            | Self::JpegEncode(_)
            | Self::LzwUnderPdfa
            | Self::LzwEncode(_)
            | Self::BrotliUnderPdfa
            | Self::BrotliEncode(_) => self.fmt_encoder(f),
            Self::NotBilevel(_)
            | Self::NotBilevelOverride(_)
            | Self::VersionTooLow { .. }
            | Self::AboveCompatibility { .. } => self.fmt_unfit(f),
            Self::AutoNotSmaller {
                stored,
                min_saving_pct,
                candidates,
            } => auto_not_smaller(f, *stored, *min_saving_pct, candidates),
            Self::NoDefinedGrey { space } => {
                write!(f, "its {space} colour space has no defined grey")
            }
            Self::BadColourSpace(why) => f.write_str(why),
            Self::AlreadyJpeg {
                source_estimate: Some(e),
                asked,
            } => write!(
                f,
                "already JPEG at about quality {e}, at or below the {asked} asked"
            ),
            Self::AlreadyJpeg {
                source_estimate: None,
                ..
            } => f.write_str("already JPEG, at a quality that cannot be estimated"),
        }
    }
}

impl ImageSkip {
    /// The sentence for an encoder this image or version cannot take.
    fn fmt_unfit(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotBilevel(e) => {
                write!(f, "not bilevel: {e} codes 1-bit, 1-component images only")
            }
            Self::NotBilevelOverride(e) => write!(
                f,
                "{e} was asked for this object, which is not 1-bit, 1-component; add convert=bilevel"
            ),
            Self::VersionTooLow {
                encode,
                needs,
                document,
            } => write!(
                f,
                "{encode} needs PDF {}.{}; the document declares {}.{} and version-raise=refuse",
                needs.major, needs.minor, document.major, document.minor
            ),
            Self::AboveCompatibility {
                encode,
                needs,
                target,
            } => write!(
                f,
                "{encode} needs PDF {}.{}, above the compatibility target {}.{}",
                needs.major, needs.minor, target.major, target.minor
            ),
            _ => Ok(()),
        }
    }

    /// The sentence for an encoder's refusal or failure.
    fn fmt_encoder(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::JpegComponents => f.write_str("JPEG stores 1, 3 or 4 colour components"),
            Self::JpegMismatch => f.write_str("its JPEG data disagrees with its dictionary"),
            Self::JpegEncode(why) => write!(f, "the JPEG encoder failed: {why}"),
            Self::LzwUnderPdfa => f.write_str("the document claims PDF/A, which forbids LZW"),
            Self::LzwEncode(why) => write!(f, "the LZW encoder failed: {why}"),
            Self::BrotliUnderPdfa => {
                f.write_str("the document claims PDF/A, which does not allow Brotli")
            }
            Self::BrotliEncode(why) => write!(f, "the Brotli encoder failed: {why}"),
            _ => Ok(()),
        }
    }
}

fn auto_not_smaller(
    f: &mut fmt::Formatter<'_>,
    stored: u64,
    min_saving_pct: u8,
    candidates: &[(ImageEncode, u64)],
) -> fmt::Result {
    write!(f, "no auto candidate is smaller than the stored {stored} B")?;
    if min_saving_pct > 0 {
        write!(f, " by {min_saving_pct}%")?;
    }
    f.write_str(" (")?;
    for (i, (encode, bytes)) in candidates.iter().enumerate() {
        let sep = if i == 0 { "" } else { ", " };
        write!(f, "{sep}{encode} {bytes} B")?;
    }
    f.write_str("), kept")
}
