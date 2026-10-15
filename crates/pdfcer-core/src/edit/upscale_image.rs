//! Enlarge a placed image's samples in place (pdfcer-gui request G198), the
//! twin of [`EditSession::deskew_image`]. The resampler is
//! [`crate::image_resample`].

use super::deskew::{Target, do_operator};
use super::{CommandKind, EditError, EditSession};
use crate::object::ObjId;
use crate::ocr::upscale::{MAX_OUTPUT_PIXELS, Upscale};

/// What [`EditSession::upscale_image`] did.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ImageUpscale {
    /// The new, enlarged image XObject.
    pub image_id: ObjId,
    /// The `/XObject` resource name the page's content now invokes.
    pub resource_name: Vec<u8>,
    /// The image XObject no longer drawn at this place. It stays in the
    /// file: other placements may draw it, and an incremental save keeps
    /// its bytes.
    pub replaced: ObjId,
    /// The old `(width, height)` in samples.
    pub old_size: (u32, u32),
    /// The new `(width, height)` in samples.
    pub new_size: (u32, u32),
    /// The encoded size of the old image stream, in bytes.
    pub old_stream_bytes: usize,
    /// The encoded size of the new one (`FlateDecode`).
    pub new_stream_bytes: usize,
    /// What the operator should know (rule 4): the pixels were
    /// interpolated, and anything about the colour space or resources that
    /// changed.
    pub notes: Vec<String>,
}

impl EditSession {
    /// **Enlarge a placed image's samples** by `upscale`'s factor and
    /// kernel, every colour channel, keeping its colour space, `/Decode` and
    /// bit depth — one undo entry ([`CommandKind::UpscaleImage`]).
    ///
    /// Each output sample is the kernel's weighted sum rounded and clamped
    /// to the sample range; at 1 bit that is a threshold at one half, so a
    /// bilevel scan stays bilevel. `/Indexed` images take the nearest
    /// sample (palette indices cannot be blended). The result is a **new**
    /// `FlateDecode` image XObject drawn by a `Do` that replaces this
    /// placement's own; the CTM is unchanged, so the image keeps its place
    /// and size on the page. Other placements of the old XObject keep
    /// drawing it. Fold a run over many pages with
    /// [`Self::coalesce_last_same`].
    ///
    /// # Errors
    ///
    /// - [`EditError::ImageUpscaleUnsupported`] for an identity factor, a
    ///   result over [`MAX_OUTPUT_PIXELS`], and every image
    ///   [`Self::deskew_image`] refuses as [`EditError::DeskewUnsupported`]
    ///   (inline, `/SMask` or `/Mask`, embedded alpha, undecodable).
    /// - [`EditError::ReplaceImageOnOther`] when the object is a path, text
    ///   or a form XObject.
    /// - [`crate::vector::VectorEditError::ObjectOutOfRange`],
    ///   [`EditError::PageOutOfRange`], [`EditError::VectorEditNoContents`],
    ///   [`EditError::DocumentEncrypted`],
    ///   [`EditError::CertificationForbidsChange`],
    ///   [`EditError::ObjectCreationWouldExposeHiddenObjects`],
    ///   [`EditError::ObjectNumbersExhausted`].
    pub fn upscale_image(
        &mut self,
        page_index: usize,
        object_index: usize,
        upscale: Upscale,
    ) -> Result<ImageUpscale, EditError> {
        if upscale.is_identity() {
            return Err(unsupported("a factor of 1 changes nothing"));
        }
        let t = self
            .deskew_target(page_index, object_index)
            .map_err(as_upscale_error)?;
        let old_size = (t.raster.width, t.raster.height);
        let new_size = checked_size(&t, upscale)?;
        let samples = crate::image_resample::resample_samples(
            &t.raster,
            &t.decoded.samples,
            new_size.0,
            new_size.1,
            upscale.filter(),
        );
        let mut prior = Vec::new();
        let (image_id, new_stream_bytes, mut notes) =
            self.stage_resampled(&t, &samples, new_size, "upscale:", &mut prior)?;
        let resource_name = self.repoint_image_placement(
            CommandKind::UpscaleImage,
            page_index,
            t.placed.bytes,
            image_id,
            prior,
            do_operator,
        )?;
        notes.insert(0, resample_note(&t, upscale, old_size, new_size));
        Ok(ImageUpscale {
            image_id,
            resource_name,
            replaced: t.id,
            old_size,
            new_size,
            old_stream_bytes: t.stream.data_span.len,
            new_stream_bytes,
            notes,
        })
    }
}

fn checked_size(t: &Target, upscale: Upscale) -> Result<(u32, u32), EditError> {
    let (w, h) = upscale.output_size(t.raster.width, t.raster.height);
    if (w as usize).saturating_mul(h as usize) > MAX_OUTPUT_PIXELS {
        return Err(unsupported(&format!(
            "{w}x{h} would exceed {MAX_OUTPUT_PIXELS} pixels"
        )));
    }
    Ok((w, h))
}

fn resample_note(t: &Target, upscale: Upscale, old: (u32, u32), new: (u32, u32)) -> String {
    let how = if t.raster.indexed {
        "copied to the nearest sample (indexed colour)".to_owned()
    } else {
        format!("interpolated ({})", upscale.filter())
    };
    format!(
        "upscale: the image's pixels were {how} from {}x{} to {}x{}; no detail was added",
        old.0, old.1, new.0, new.1
    )
}

fn as_upscale_error(err: EditError) -> EditError {
    match err {
        EditError::DeskewUnsupported { reason } => EditError::ImageUpscaleUnsupported { reason },
        other => other,
    }
}

fn unsupported(reason: &str) -> EditError {
    EditError::ImageUpscaleUnsupported {
        reason: reason.to_owned(),
    }
}
