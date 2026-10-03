//! Pixels already in memory, as an [`ImportedImage`] with no file encoding.

use super::{
    ImageFormat, ImageImportError, ImportColorSpace, ImportFilter, ImportNotes, ImportedImage,
    Orientation, PdfFeature, RecompressReason, SoftMask, check_dimensions, flate_encode,
    raise_version,
};

impl ImportedImage {
    /// An 8-bit `/DeviceRGB` image from straight (not premultiplied) RGBA
    /// samples, row-major from the top-left, four bytes per pixel.
    ///
    /// Both halves are written as plain `/FlateDecode`. An alpha channel that
    /// is 255 everywhere produces **no** `/SMask` and no notes. Any other
    /// alpha becomes an 8-bit soft mask of the same size, with the notes a
    /// PNG of the same pixels gets from [`import`](super::import):
    /// [`RecompressReason::AlphaSplit`], `alpha_to_soft_mask` and the
    /// soft-mask PDF version (§8.9.5 Table 89 gives an image one colour
    /// space, so opacity travels in a separate `/SMask`).
    ///
    /// `format` is [`ImageFormat::Pixels`]; there is no `dpi` and the
    /// orientation is [`Orientation::Identity`].
    ///
    /// # Errors
    ///
    /// - [`ImageImportError::Empty`] — a zero width or height.
    /// - [`ImageImportError::TooLarge`] — over the ceilings [`import`](super::import)
    ///   enforces on a decoded file.
    /// - [`ImageImportError::BufferSize`] — `rgba` is not exactly
    ///   `width × height × 4` bytes.
    /// - [`ImageImportError::Compress`] — deflate failed.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::image_import::{ImageFormat, ImportedImage};
    ///
    /// let opaque = ImportedImage::from_rgba8(2, 1, &[255, 0, 0, 255, 0, 0, 255, 255])?;
    /// assert_eq!(opaque.format, ImageFormat::Pixels);
    /// assert!(opaque.soft_mask.is_none());
    ///
    /// let half = ImportedImage::from_rgba8(1, 1, &[0, 0, 0, 128])?;
    /// assert!(half.soft_mask.is_some());
    /// assert!(half.notes.alpha_to_soft_mask);
    /// # Ok::<(), pdfcer_core::image_import::ImageImportError>(())
    /// ```
    pub fn from_rgba8(width: u32, height: u32, rgba: &[u8]) -> Result<Self, ImageImportError> {
        check_dimensions(width, height, 4, 8)?;
        let pixels = width as usize * height as usize;
        let expected = pixels * 4;
        if rgba.len() != expected {
            return Err(ImageImportError::BufferSize {
                width,
                height,
                expected,
                actual: rgba.len(),
            });
        }
        let mut rgb = Vec::with_capacity(pixels * 3);
        let mut alpha = Vec::with_capacity(pixels);
        for px in rgba.as_chunks::<4>().0 {
            rgb.extend_from_slice(&px[..3]);
            alpha.push(px[3]);
        }
        let mut notes = ImportNotes::default();
        let soft_mask = if alpha.iter().all(|&a| a == 255) {
            None
        } else {
            notes.recompressed = Some(RecompressReason::AlphaSplit);
            notes.alpha_to_soft_mask = true;
            raise_version(&mut notes.requires_pdf_version, PdfFeature::SoftMask);
            Some(SoftMask {
                width,
                height,
                bits_per_component: 8,
                data: flate_encode(&alpha)?,
            })
        };
        Ok(Self {
            format: ImageFormat::Pixels,
            width,
            height,
            bits_per_component: 8,
            color_space: ImportColorSpace::DeviceRgb,
            filter: ImportFilter::Flate,
            data: flate_encode(&rgb)?,
            soft_mask,
            color_key_mask: None,
            orientation: Orientation::Identity,
            dpi: None,
            notes,
        })
    }
}
