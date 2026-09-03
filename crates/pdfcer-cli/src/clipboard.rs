//! # The OS clipboard — `pdfcer copy-page` (`Pass 248.2`)
//!
//! Places one page on the **Windows** clipboard in every format the
//! operator's target applications read, so that a paste lands as **editable
//! vectors** in Word / PowerPoint / Excel (Microsoft 365) and Inkscape, and
//! as an **alpha raster** everywhere else. The operator's words
//! (2026-09-03): *"copy and paste anything to other software — like copy and
//! paste vector graphics into word or inkscape"*.
//!
//! ## Why this lives in the CLI and not in the engine
//!
//! `pdfcer-core` and `pdfcer-render` may not touch a windowing system
//! (`ARCHITECTURE.md` §3 — the invariant that keeps the WASM fork a shell
//! swap), and the clipboard is one. So the engine produces the BYTES —
//! SVG ([`pdfcer_render::svg`]), PNG with alpha ([`pdfcer_render::export`]),
//! the page as a one-page PDF (`pageops::extract`) — and a **shell** places
//! them. This module is that shell half for the CLI; `pdfcer-gui` carries
//! its own against the same engine calls (channel note, 2026-09-03).
//!
//! ## The formats, in placement order, and who reads each
//!
//! Sourced in `docs/clipboard-interop-survey.md` §7 (application source at
//! pinned revisions, 2026-09-03). A reader "typically retrieves … the first
//! format it recognizes", so the order IS the design:
//!
//! | # | format | payload | reaches |
//! |---|---|---|---|
//! | 1 | registered `"image/svg+xml"` | UTF-8 SVG **plus one trailing NUL** — byte-for-byte what Chromium ≥ M127 writes and what Microsoft validated Office against | Word/PowerPoint/Excel as an editable SVG graphic; Inkscape (its 2nd preference, above EMF and PDF); LibreOffice ≥ 25.2; browsers |
//! | 2 | registered `"PNG"` | the PNG file bytes, straight alpha, DPI in `pHYs` | Office's preferred raster, Paint.NET, GIMP, Inkscape, LibreOffice, Firefox/Chromium, Snip & Sketch |
//! | 3 | `CF_DIBV5` | `BITMAPV5HEADER` + 32 bpp `BI_BITFIELDS` BGRA, premultiplied, top-down | readers older than the `"PNG"` convention; Windows synthesises `CF_DIB`/`CF_BITMAP` from it |
//! | 4 | registered `"application/pdf"` | the page as a one-page PDF | Inkscape (7th preference; it imports it through its PDF dialog); nobody else on Windows |
//!
//! **Not placed:** `CF_UNICODETEXT` carrying the SVG source (a text-first
//! reader would paste XML as text), `image/x-inkscape-svg` (Inkscape-internal
//! semantics), `CF_ENHMETAFILE` (only LibreOffice 24.x needs it; an EMF
//! writer is a follow-on, not assumed).
//!
//! ## Contracts
//!
//! - **One transaction.** Open, empty, set every format, close — through
//!   `clipboard-win`'s RAII guard, so a failure part-way leaves either the
//!   old contents or the new, never half.
//! - **The bytes are the engine's.** Nothing here re-renders or re-encodes;
//!   the SVG is `export_svg`'s output verbatim (plus the NUL), the PNG is
//!   `encode_png`'s, the DIB is built from the same pixmap.
//! - **Disclosure is the caller's.** [`Placed`] reports which formats went
//!   on; the CLI prints them and the SVG's own tally, because a paste that
//!   arrives as a picture where the operator expected vectors must be
//!   explicable from the terminal.
//!
//! ## Failure modes
//!
//! [`ClipboardError`]: the clipboard could not be opened (another process
//! holds it — Windows serialises access; `new_attempts` retries), a format
//! name could not be registered, or a `SetClipboardData` failed. Each names
//! the format so the operator knows what did and did not land.

// On a non-Windows target nothing here is reached except the refusing
// `place`: the payload fields, `dib_v5` and `svg_payload` exist so the CLI's
// call site compiles identically everywhere, and are dead there BY DESIGN.
// Allowed for that target only, so a Windows build still flags real rot.
#![cfg_attr(not(windows), allow(dead_code))]

use pdfcer_render::tiny_skia::Pixmap;

/// What to place. Every field optional so a caller can place a subset.
#[derive(Debug, Default, Clone)]
pub struct ClipboardPayload {
    /// The SVG document (no XML declaration needed).
    pub svg: Option<String>,
    /// PNG file bytes, straight alpha.
    pub png: Option<Vec<u8>>,
    /// The raster the PNG was made from, for `CF_DIBV5`. Premultiplied, as
    /// `tiny_skia` stores it — which is what `CF_DIBV5` readers assume.
    pub pixmap: Option<Pixmap>,
    /// Pixels per metre for the DIB header (`dpi / 0.0254`), or 0.
    pub pixels_per_metre: u32,
    /// A one-page PDF.
    pub pdf: Option<Vec<u8>>,
}

/// What landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    /// Format names in placement order: `image/svg+xml`, `PNG`, `CF_DIBV5`,
    /// `application/pdf` — whichever the payload carried.
    pub formats: Vec<&'static str>,
}

/// Why a placement failed.
#[derive(Debug, thiserror::Error)]
pub enum ClipboardError {
    /// Windows serialises clipboard access; another process held it for
    /// longer than the retry budget.
    #[error("could not open the clipboard (another application is holding it)")]
    Open,
    /// `RegisterClipboardFormat` refused a name — not expected for the
    /// four names this module uses.
    #[error("could not register the clipboard format {0:?}")]
    Register(&'static str),
    /// `SetClipboardData` failed for a format; earlier formats are on the
    /// clipboard, this one and later ones are not.
    #[error("could not place {0:?} on the clipboard")]
    Set(&'static str),
    /// Not Windows. Constructed only by the non-Windows `place`, so on
    /// Windows the variant is deliberately dead: the error type is the
    /// same on every target so the CLI's match arms are too.
    #[cfg_attr(windows, allow(dead_code))]
    #[error("the OS clipboard is reachable on Windows only in this build")]
    Unsupported,
}

/// `CF_DIBV5` bytes for a premultiplied RGBA pixmap: a `BITMAPV5HEADER`
/// (124 bytes) followed by 32 bpp BGRA rows, **top-down** (negative height),
/// `BI_BITFIELDS` with explicit masks, sRGB colour space.
///
/// Premultiplied BGRA is what Chromium writes
/// (`CreateDIBV5ImageDataFromN32SkBitmap`) and what Mozilla settled on
/// reading; a straight-alpha DIB would look wrong in exactly the readers
/// that fall back to this format. The `"PNG"` entry, placed first, carries
/// straight alpha unambiguously, which is why it comes first.
#[must_use]
pub fn dib_v5(pixmap: &Pixmap, pixels_per_metre: u32) -> Vec<u8> {
    let (w, h) = (pixmap.width(), pixmap.height());
    let row_bytes = w as usize * 4;
    let image_bytes = row_bytes * h as usize;
    let mut out = Vec::with_capacity(124 + image_bytes);
    let u32le = |v: u32| v.to_le_bytes();
    let i32le = |v: i32| v.to_le_bytes();
    out.extend_from_slice(&u32le(124)); // bV5Size
    out.extend_from_slice(&i32le(w as i32)); // bV5Width
    out.extend_from_slice(&i32le(-(h as i32))); // bV5Height: negative = top-down
    out.extend_from_slice(&1u16.to_le_bytes()); // bV5Planes
    out.extend_from_slice(&32u16.to_le_bytes()); // bV5BitCount
    out.extend_from_slice(&u32le(3)); // bV5Compression = BI_BITFIELDS
    out.extend_from_slice(&u32le(image_bytes as u32)); // bV5SizeImage
    out.extend_from_slice(&i32le(pixels_per_metre as i32)); // bV5XPelsPerMeter
    out.extend_from_slice(&i32le(pixels_per_metre as i32)); // bV5YPelsPerMeter
    out.extend_from_slice(&u32le(0)); // bV5ClrUsed
    out.extend_from_slice(&u32le(0)); // bV5ClrImportant
    out.extend_from_slice(&u32le(0x00FF_0000)); // bV5RedMask
    out.extend_from_slice(&u32le(0x0000_FF00)); // bV5GreenMask
    out.extend_from_slice(&u32le(0x0000_00FF)); // bV5BlueMask
    out.extend_from_slice(&u32le(0xFF00_0000)); // bV5AlphaMask
    out.extend_from_slice(&u32le(0x7352_4742)); // bV5CSType = LCS_sRGB ('sRGB')
    out.extend_from_slice(&[0u8; 36]); // bV5Endpoints (CIEXYZTRIPLE), unused for sRGB
    out.extend_from_slice(&u32le(0)); // bV5GammaRed
    out.extend_from_slice(&u32le(0)); // bV5GammaGreen
    out.extend_from_slice(&u32le(0)); // bV5GammaBlue
    out.extend_from_slice(&u32le(4)); // bV5Intent = LCS_GM_IMAGES
    out.extend_from_slice(&u32le(0)); // bV5ProfileData
    out.extend_from_slice(&u32le(0)); // bV5ProfileSize
    out.extend_from_slice(&u32le(0)); // bV5Reserved
    debug_assert_eq!(out.len(), 124);
    for px in pixmap.pixels() {
        out.extend_from_slice(&[px.blue(), px.green(), px.red(), px.alpha()]);
    }
    out
}

/// The SVG payload exactly as Chromium writes it: UTF-8 plus one NUL.
#[must_use]
pub fn svg_payload(svg: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(svg.len() + 1);
    v.extend_from_slice(svg.as_bytes());
    v.push(0);
    v
}

/// Place the payload on the OS clipboard in one transaction.
///
/// # Errors
///
/// [`ClipboardError`] — see the type.
#[cfg(windows)]
pub fn place(payload: &ClipboardPayload) -> Result<Placed, ClipboardError> {
    use clipboard_win::{Clipboard, formats, raw};

    // Windows serialises clipboard access across processes; ten attempts
    // is clipboard-win's own convention for "wait for the other one".
    let _guard = Clipboard::new_attempts(10).map_err(|_| ClipboardError::Open)?;
    raw::empty().map_err(|_| ClipboardError::Open)?;
    let mut placed = Vec::new();

    if let Some(svg) = &payload.svg {
        let id = raw::register_format("image/svg+xml")
            .ok_or(ClipboardError::Register("image/svg+xml"))?;
        raw::set_without_clear(id.get(), &svg_payload(svg))
            .map_err(|_| ClipboardError::Set("image/svg+xml"))?;
        placed.push("image/svg+xml");
    }
    if let Some(png) = &payload.png {
        let id = raw::register_format("PNG").ok_or(ClipboardError::Register("PNG"))?;
        raw::set_without_clear(id.get(), png).map_err(|_| ClipboardError::Set("PNG"))?;
        placed.push("PNG");
    }
    if let Some(pixmap) = &payload.pixmap {
        raw::set_without_clear(formats::CF_DIBV5, &dib_v5(pixmap, payload.pixels_per_metre))
            .map_err(|_| ClipboardError::Set("CF_DIBV5"))?;
        placed.push("CF_DIBV5");
    }
    if let Some(pdf) = &payload.pdf {
        let id = raw::register_format("application/pdf")
            .ok_or(ClipboardError::Register("application/pdf"))?;
        raw::set_without_clear(id.get(), pdf)
            .map_err(|_| ClipboardError::Set("application/pdf"))?;
        placed.push("application/pdf");
    }
    Ok(Placed { formats: placed })
}

/// Not Windows: nothing can be placed. The CLI prints the error and points
/// at `export-image`, which writes the same bytes to files.
#[cfg(not(windows))]
pub fn place(_payload: &ClipboardPayload) -> Result<Placed, ClipboardError> {
    Err(ClipboardError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dib_v5_header_is_124_bytes_top_down_bitfields_bgra() {
        let mut p = Pixmap::new(2, 1).unwrap();
        p.fill(pdfcer_render::tiny_skia::Color::from_rgba8(255, 0, 0, 255));
        let d = dib_v5(&p, 3780);
        assert_eq!(d.len(), 124 + 8);
        assert_eq!(u32::from_le_bytes([d[0], d[1], d[2], d[3]]), 124);
        assert_eq!(i32::from_le_bytes([d[4], d[5], d[6], d[7]]), 2);
        assert_eq!(i32::from_le_bytes([d[8], d[9], d[10], d[11]]), -1);
        assert_eq!(u16::from_le_bytes([d[14], d[15]]), 32);
        assert_eq!(u32::from_le_bytes([d[16], d[17], d[18], d[19]]), 3);
        // BGRA: red pixel = 00 00 FF FF
        assert_eq!(&d[124..128], &[0, 0, 255, 255]);
    }

    #[test]
    fn svg_payload_is_nul_terminated_utf8() {
        assert_eq!(svg_payload("<svg/>"), b"<svg/>\0");
    }
}
