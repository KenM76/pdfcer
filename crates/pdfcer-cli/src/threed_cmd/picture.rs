//! The file `3d-render` writes: a PNG, or a JPEG when the output is named
//! `.jpg` or `.jpeg`; a 360° panorama JPEG carries the photo-sphere (GPano)
//! XMP tag photo viewers open it by.

use super::RenderThreeDArgs;
use crate::LensArg;
use std::path::Path;

/// Whether `path` names a JPEG: a `.jpg` or `.jpeg` extension, any case.
pub(crate) fn is_jpeg(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("jpg") || e.eq_ignore_ascii_case("jpeg"))
}

/// The picture size `3d-render` draws: as given; for the 360° panorama the
/// missing side of a 2:1 picture, 2048 × 1024 when neither is given;
/// otherwise 1024 wide and 768 high by default.
pub(crate) fn picture_size(lens: LensArg, width: Option<u32>, height: Option<u32>) -> (u32, u32) {
    match (lens, width, height) {
        (_, Some(w), Some(h)) => (w, h),
        (LensArg::Equirectangular, None, None) => (2048, 1024),
        (LensArg::Equirectangular, Some(w), None) => (w, (w / 2).max(1)),
        (LensArg::Equirectangular, None, Some(h)) => (h.saturating_mul(2), h),
        (_, w, h) => (w.unwrap_or(1024), h.unwrap_or(768)),
    }
}

/// The refusal for an option pair the output cannot honour, before any
/// work: `--transparent` into a JPEG, which has no alpha channel.
pub(super) fn refusal(a: &RenderThreeDArgs<'_>) -> Option<&'static str> {
    (a.transparent && is_jpeg(a.output)).then_some(
        "--transparent cannot be honoured for a JPEG, which has no alpha channel; drop the \
         flag (the background is white) or name the output .png",
    )
}

/// The encoded file for a premultiplied RGBA picture `width` × `height`.
#[cfg(feature = "3d")]
pub(super) fn encode(
    a: &RenderThreeDArgs<'_>,
    mut rgba: Vec<u8>,
    (width, height): (u32, u32),
) -> Result<Vec<u8>, String> {
    use pdfcer_render::export::{
        JpegOptions, encode_jpeg, encode_png, insert_jpeg_xmp, photo_sphere_xmp,
    };
    // The pixmap holds premultiplied alpha; glass over a transparent
    // background leaves partly transparent pixels.
    for px in rgba.chunks_exact_mut(4) {
        if let [r, g, b, alpha] = px {
            for c in [r, g, b] {
                *c = (u16::from(*c) * u16::from(*alpha)).div_ceil(255) as u8;
            }
        }
    }
    let pixmap = pdfcer_render::tiny_skia::IntSize::from_wh(width, height)
        .and_then(|size| pdfcer_render::tiny_skia::Pixmap::from_vec(rgba, size))
        .ok_or_else(|| "the rendered image has an invalid size".to_owned())?;
    let err = |e: pdfcer_render::export::ExportError| e.to_string();
    if !is_jpeg(a.output) {
        return encode_png(&pixmap, None).map_err(err);
    }
    let jpeg = encode_jpeg(&pixmap, &JpegOptions::default()).map_err(err)?;
    if a.lens == LensArg::Equirectangular {
        return insert_jpeg_xmp(&jpeg, &photo_sphere_xmp(width, height)).map_err(err);
    }
    Ok(jpeg)
}

/// The `note: photo sphere:` lines for a 360° panorama.
#[cfg(feature = "3d")]
pub(super) fn print_note(a: &RenderThreeDArgs<'_>) {
    if a.lens != LensArg::Equirectangular {
        return;
    }
    if is_jpeg(a.output) {
        println!(
            "note: photo sphere: the JPEG carries the Google Photo Sphere (GPano) XMP tag, so \
             photo viewers that read it open the picture as a panorama to look around in"
        );
    } else {
        println!(
            "note: photo sphere: the PNG is a 360-degree panorama but carries no photo-sphere \
             tag, so viewers show it flat; name the output .jpg for a tagged one"
        );
    }
    if u64::from(a.width) != 2 * u64::from(a.height) {
        println!(
            "note: photo sphere: the picture is {}x{}, not twice as wide as it is tall; viewers \
             expect 2:1 and may stretch or refuse it",
            a.width, a.height
        );
    }
}
