//! Vector pages: an Enhanced Metafile played onto the printer's device
//! context in place of a bitmap blit ([MS-EMF]; `PlayEnhMetaFile`).
//!
//! The caller produces the metafile (the renderer's EMF writer) and decides
//! whether a page may go as one; this module only places it. A metafile is
//! stretched from its header frame onto the same destination rectangle a
//! bitmap of that page would fill, so a vector page and its raster fallback
//! land in the same place on the sheet.

use crate::PageBitmap;

/// The destination rectangle of `page` on the sheet, in device pixels at
/// `dpi`: `(x, y, width, height)`. 72 points to the inch is the PDF unit's
/// definition.
pub(crate) fn dest_rect(page: &PageBitmap, dpi: (i32, i32)) -> (i32, i32, i32, i32) {
    let px_x = |pt: f64| (pt * f64::from(dpi.0) / 72.0).round() as i32;
    let px_y = |pt: f64| (pt * f64::from(dpi.1) / 72.0).round() as i32;
    (
        px_x(page.placement.offset_x_pt),
        px_y(page.placement.offset_y_pt),
        px_x(page.page_pt.0 * page.placement.scale),
        px_y(page.page_pt.1 * page.placement.scale),
    )
}

/// Play `emf` onto the open page of `hdc`, stretched to `page`'s
/// destination rectangle. `page`'s pixels are not read.
///
/// # Errors
///
/// [`crate::PrintError::Metafile`] when GDI does not accept the bytes as a
/// metafile or does not play it.
#[cfg(windows)]
pub(crate) fn play_page(
    hdc: windows::Win32::Graphics::Gdi::HDC,
    emf: &[u8],
    page: &PageBitmap,
    dpi: (i32, i32),
) -> Result<(), crate::PrintError> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{DeleteEnhMetaFile, PlayEnhMetaFile, SetEnhMetaFileBits};

    let (x, y, w, h) = dest_rect(page, dpi);
    let rect = RECT {
        left: x,
        top: y,
        right: x + w,
        bottom: y + h,
    };
    // SAFETY: `emf` outlives the call; GDI copies the bytes into the handle
    // it returns, and a null handle is the documented failure.
    let handle = unsafe { SetEnhMetaFileBits(emf) };
    if handle.is_invalid() {
        return Err(crate::PrintError::Metafile);
    }
    // SAFETY: `hdc` is valid with a page open; `handle` is the live
    // metafile just created; `rect` outlives the call.
    let played = unsafe { PlayEnhMetaFile(hdc, handle, &rect) }.as_bool();
    // SAFETY: deleted exactly once, after its only use.
    unsafe {
        let _ = DeleteEnhMetaFile(Some(handle));
    }
    if played {
        Ok(())
    } else {
        Err(crate::PrintError::Metafile)
    }
}

#[cfg(all(test, windows))]
mod tests;
