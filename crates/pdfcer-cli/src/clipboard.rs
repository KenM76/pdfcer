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
//! | 1b | `CF_ENHMETAFILE` (`Pass 248.4`) | an [MS-EMF] metafile from `pdfcer_render::emf`, placed as a GDI handle via `SetEnhMetaFileBits` | LibreOffice 24.x (its ONLY vector route on Windows), Office *Paste Special → Picture (Enhanced Metafile)*, Visio/CorelDRAW/CAD; Inkscape ranks it 5th, after the SVG |
//! | 2 | registered `"PNG"` | the PNG file bytes, straight alpha, DPI in `pHYs` | Office's preferred raster, Paint.NET, GIMP, Inkscape, LibreOffice, Firefox/Chromium, Snip & Sketch |
//! | 3 | `CF_DIBV5` | `BITMAPV5HEADER` + 32 bpp `BI_BITFIELDS` BGRA, premultiplied, top-down | readers older than the `"PNG"` convention; Windows synthesises `CF_DIB`/`CF_BITMAP` from it |
//! | 4 | registered `"application/pdf"` | the page as a one-page PDF | Inkscape (7th preference; it imports it through its PDF dialog); nobody else on Windows |
//!
//! **Not placed:** `CF_UNICODETEXT` carrying the SVG source (a text-first
//! reader would paste XML as text), `image/x-inkscape-svg` (Inkscape-internal
//! semantics).
//!
//! ## Contracts
//!
//! - **All or nothing, as a reader sees it.** Every payload is built before
//!   the clipboard opens, so a refusal (an oversized DIB) leaves the old
//!   contents. Then open, empty, set every format, close; if a set fails,
//!   the clipboard is emptied again before the error returns, so no reader
//!   ever pastes a partial copy (the old contents are gone by then — Windows
//!   has no rollback). The setter is the [`ClipboardSink`] trait so a test
//!   can force a later format to fail.
//! - **Opening waits.** Windows serialises clipboard access; [`place`]
//!   retries [`OPEN_ATTEMPTS`] times with [`OPEN_WAIT`] between tries.
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
//! holds it), the raster is too large for `CF_DIBV5`'s header fields
//! (refused, never truncated), a format name could not be registered, or a
//! `SetClipboardData` failed. Each names the format or field.

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
    /// An Enhanced Metafile (`Pass 248.4`) -- placed as `CF_ENHMETAFILE`,
    /// which is a GDI HANDLE rather than an `HGLOBAL`, so it goes through
    /// `SetEnhMetaFileBits` + `SetClipboardData` directly (clipboard-win
    /// has no metafile setter and its generic path would hand GDI the
    /// wrong handle type).
    pub emf: Option<Vec<u8>>,
}

/// What landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    /// Format names in placement order: `image/svg+xml`, `CF_ENHMETAFILE`,
    /// `PNG`, `CF_DIBV5`, `application/pdf` — whichever the payload carried.
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
    /// names this module uses. The clipboard was emptied again.
    #[error(
        "could not register the clipboard format {0:?}; the clipboard was emptied, nothing was copied"
    )]
    Register(&'static str),
    /// `SetClipboardData` failed for a format; the clipboard was emptied
    /// again, so no format of this placement is on it.
    #[error(
        "could not place {0:?} on the clipboard; the clipboard was emptied, nothing was copied"
    )]
    Set(&'static str),
    /// A set failed AND the follow-up empty failed: a partial copy may be on
    /// the clipboard.
    #[error(
        "could not place {0:?} on the clipboard, and could not empty it afterwards; it may hold a partial copy"
    )]
    NotCleared(&'static str),
    /// The page raster's sizes do not fit `BITMAPV5HEADER`'s fields; the
    /// payload names the field. Refused before the clipboard was opened.
    #[error(
        "the page raster is too large to copy as a bitmap: {0}; nothing was copied (lower --dpi)"
    )]
    DibTooLarge(&'static str),
    /// The assembled `BITMAPV5HEADER` was not 124 bytes (an internal
    /// defect). Refused before the clipboard was opened.
    #[error("internal error: the bitmap header came out {0} bytes, not 124; nothing was copied")]
    DibHeaderLength(usize),
    /// Not Windows. Constructed only by the non-Windows `place`, so on
    /// Windows the variant is deliberately dead: the error type is the
    /// same on every target so the CLI's match arms are too.
    #[cfg_attr(windows, allow(dead_code))]
    #[error("the OS clipboard is reachable on Windows only in this build")]
    Unsupported,
}

/// `bV5Size`: the length of a `BITMAPV5HEADER`.
const DIB_V5_SIZE_FIELD: u32 = 124;

/// Length of a `BITMAPV5HEADER` in bytes (`bV5Size`, as a `usize`).
pub const DIB_V5_HEADER_LEN: usize = DIB_V5_SIZE_FIELD as usize;

/// `CF_UNICODETEXT` (WinUser.h), stated here so entries build on every target.
pub const CF_UNICODETEXT: u32 = 13;

/// `CF_DIBV5` (WinUser.h).
pub const CF_DIBV5: u32 = 17;

/// `CF_DIBV5` bytes for a premultiplied RGBA pixmap: a `BITMAPV5HEADER`
/// (124 bytes) followed by 32 bpp BGRA rows, **top-down** (negative height),
/// `BI_BITFIELDS` with explicit masks, sRGB colour space.
///
/// Premultiplied BGRA is what Chromium writes
/// (`CreateDIBV5ImageDataFromN32SkBitmap`) and what Mozilla settled on
/// reading; a straight-alpha DIB would look wrong in exactly the readers
/// that fall back to this format. The `"PNG"` entry, placed first, carries
/// straight alpha unambiguously, which is why it comes first.
///
/// # Errors
///
/// See [`dib_v5_header`]: a raster whose sizes do not fit the header's
/// fields is refused, never truncated.
pub fn dib_v5(pixmap: &Pixmap, pixels_per_metre: u32) -> Result<Vec<u8>, ClipboardError> {
    let (header, image_len) = dib_v5_header(pixmap.width(), pixmap.height(), pixels_per_metre)?;
    let total = DIB_V5_HEADER_LEN
        .checked_add(image_len)
        .ok_or(ClipboardError::DibTooLarge(
            "bV5SizeImage exceeds this platform's address space",
        ))?;
    let mut out = Vec::new();
    out.try_reserve_exact(total)
        .map_err(|_| ClipboardError::DibTooLarge("not enough memory for the bitmap"))?;
    out.extend_from_slice(&header);
    for px in pixmap.pixels() {
        out.extend_from_slice(&[px.blue(), px.green(), px.red(), px.alpha()]);
    }
    Ok(out)
}

/// The `BITMAPV5HEADER` for a `width` x `height` 32 bpp top-down raster, and
/// the byte length of the pixel rows that follow it.
///
/// Every field is computed with checked arithmetic and converted with
/// `try_from`; the header is returned as a fixed `[u8; 124]`, so its length
/// cannot differ from `bV5Size` in any build profile.
///
/// # Errors
///
/// [`ClipboardError::DibTooLarge`] when `width`, `height` or
/// `pixels_per_metre` exceeds `i32::MAX` (the signed `bV5Width`,
/// `bV5Height`, `bV5XPelsPerMeter` fields), or the image exceeds `u32::MAX`
/// bytes (`bV5SizeImage`). [`ClipboardError::DibHeaderLength`] if the
/// assembled fields do not total 124 bytes — unreachable unless this
/// function is edited wrongly, and checked rather than asserted so a release
/// build refuses instead of placing a malformed header.
pub fn dib_v5_header(
    width: u32,
    height: u32,
    pixels_per_metre: u32,
) -> Result<([u8; DIB_V5_HEADER_LEN], usize), ClipboardError> {
    let too_large = ClipboardError::DibTooLarge;
    let w = i32::try_from(width).map_err(|_| too_large("width exceeds 2147483647 pixels"))?;
    let neg_h = i32::try_from(height)
        .ok()
        .and_then(i32::checked_neg)
        .ok_or(too_large("height exceeds 2147483647 pixels"))?;
    let ppm = i32::try_from(pixels_per_metre)
        .map_err(|_| too_large("resolution exceeds 2147483647 pixels per metre"))?;
    let image_bytes = u64::from(width)
        .checked_mul(4)
        .and_then(|row| row.checked_mul(u64::from(height)))
        .and_then(|n| u32::try_from(n).ok())
        .ok_or(too_large("the image exceeds 4 GiB (bV5SizeImage)"))?;
    let image_len = usize::try_from(image_bytes)
        .map_err(|_| too_large("bV5SizeImage exceeds this platform's address space"))?;

    let mut v = Vec::with_capacity(DIB_V5_HEADER_LEN);
    let u32s = |vals: &[u32], v: &mut Vec<u8>| {
        vals.iter()
            .for_each(|x| v.extend_from_slice(&x.to_le_bytes()))
    };
    u32s(&[DIB_V5_SIZE_FIELD], &mut v); // bV5Size
    v.extend_from_slice(&w.to_le_bytes()); // bV5Width
    v.extend_from_slice(&neg_h.to_le_bytes()); // bV5Height: negative = top-down
    v.extend_from_slice(&1u16.to_le_bytes()); // bV5Planes
    v.extend_from_slice(&32u16.to_le_bytes()); // bV5BitCount
    u32s(&[3, image_bytes], &mut v); // bV5Compression = BI_BITFIELDS, bV5SizeImage
    v.extend_from_slice(&ppm.to_le_bytes()); // bV5XPelsPerMeter
    v.extend_from_slice(&ppm.to_le_bytes()); // bV5YPelsPerMeter
    // bV5ClrUsed, bV5ClrImportant, R/G/B/A masks, bV5CSType = LCS_sRGB.
    u32s(
        &[
            0,
            0,
            0x00FF_0000,
            0x0000_FF00,
            0x0000_00FF,
            0xFF00_0000,
            0x7352_4742,
        ],
        &mut v,
    );
    v.extend_from_slice(&[0u8; 36]); // bV5Endpoints (CIEXYZTRIPLE), unused for sRGB
    // Gamma R/G/B, bV5Intent = LCS_GM_IMAGES, ProfileData, ProfileSize, Reserved.
    u32s(&[0, 0, 0, 4, 0, 0, 0], &mut v);
    let header = <[u8; DIB_V5_HEADER_LEN]>::try_from(v.as_slice())
        .map_err(|_| ClipboardError::DibHeaderLength(v.len()))?;
    Ok((header, image_len))
}

/// The SVG payload exactly as Chromium writes it: UTF-8 plus one NUL.
#[must_use]
pub fn svg_payload(svg: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(svg.len() + 1);
    v.extend_from_slice(svg.as_bytes());
    v.push(0);
    v
}

/// `CF_UNICODETEXT` bytes: UTF-16LE plus one NUL code unit.
#[must_use]
pub fn unicode_text_payload(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// How a [`ClipboardEntry`]'s format is identified to Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A registered format; the entry's `name` is registered at set time.
    Registered,
    /// A predefined `CF_*` format number; bytes go in an `HGLOBAL`.
    Standard(u32),
    /// `CF_ENHMETAFILE`: the bytes become a GDI metafile handle.
    EnhMetafile,
}

/// One format, fully built before the clipboard is opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardEntry {
    /// The name reported in [`Placed`] and in errors (and registered, for
    /// [`EntryKind::Registered`]).
    pub name: &'static str,
    /// How Windows identifies the format.
    pub kind: EntryKind,
    /// The payload bytes.
    pub bytes: Vec<u8>,
}

impl ClipboardEntry {
    /// A registered-format entry.
    #[must_use]
    pub fn registered(name: &'static str, bytes: Vec<u8>) -> Self {
        Self {
            name,
            kind: EntryKind::Registered,
            bytes,
        }
    }
}

/// Every entry the payload carries, in placement order, built in full —
/// so a refusal (an oversized DIB) happens before the clipboard is touched.
///
/// # Errors
///
/// [`dib_v5`]'s refusals.
pub fn entries(payload: &ClipboardPayload) -> Result<Vec<ClipboardEntry>, ClipboardError> {
    let mut out = Vec::new();
    if let Some(svg) = &payload.svg {
        out.push(ClipboardEntry::registered(
            "image/svg+xml",
            svg_payload(svg),
        ));
    }
    if let Some(emf) = &payload.emf {
        let kind = EntryKind::EnhMetafile;
        out.push(ClipboardEntry {
            name: "CF_ENHMETAFILE",
            kind,
            bytes: emf.clone(),
        });
    }
    if let Some(png) = &payload.png {
        out.push(ClipboardEntry::registered("PNG", png.clone()));
    }
    if let Some(pixmap) = &payload.pixmap {
        let bytes = dib_v5(pixmap, payload.pixels_per_metre)?;
        out.push(ClipboardEntry {
            name: "CF_DIBV5",
            kind: EntryKind::Standard(CF_DIBV5),
            bytes,
        });
    }
    if let Some(pdf) = &payload.pdf {
        out.push(ClipboardEntry::registered("application/pdf", pdf.clone()));
    }
    Ok(out)
}

/// The two clipboard operations a placement needs, so a test can stand in
/// for Windows and force a later format to fail.
pub trait ClipboardSink {
    /// `EmptyClipboard`.
    ///
    /// # Errors
    ///
    /// The clipboard could not be emptied.
    fn empty(&mut self) -> Result<(), ClipboardError>;
    /// Set one format without clearing the others.
    ///
    /// # Errors
    ///
    /// [`ClipboardError::Register`] or [`ClipboardError::Set`].
    fn set(&mut self, entry: &ClipboardEntry) -> Result<(), ClipboardError>;
}

/// Empty the (already open) clipboard and set every entry — all or nothing
/// as seen by a reader: if any set fails, the clipboard is emptied again
/// before the error returns, so no partial copy is left.
///
/// # Errors
///
/// [`ClipboardError::Open`] if the first empty fails; the failing set's
/// error once the clipboard is emptied again; [`ClipboardError::NotCleared`]
/// if that second empty also fails.
pub fn fill(
    sink: &mut impl ClipboardSink,
    entries: &[ClipboardEntry],
) -> Result<Placed, ClipboardError> {
    sink.empty().map_err(|_| ClipboardError::Open)?;
    let mut formats = Vec::with_capacity(entries.len());
    for entry in entries {
        if let Err(err) = sink.set(entry) {
            return Err(match sink.empty() {
                Ok(()) => err,
                Err(_) => ClipboardError::NotCleared(entry.name),
            });
        }
        formats.push(entry.name);
    }
    Ok(Placed { formats })
}

/// How many times to try `OpenClipboard`, and the wait between tries
/// (about 270 ms in all). `clipboard-win` 5.4 has no timed retry —
/// `new_attempts` loops back to back — so the wait is here.
pub const OPEN_ATTEMPTS: u32 = 10;
/// See [`OPEN_ATTEMPTS`].
pub const OPEN_WAIT: std::time::Duration = std::time::Duration::from_millis(30);

/// Call `open` up to `attempts` times, calling `sleep(wait)` between tries.
/// `None` when every try failed.
pub fn open_with_wait<T, E>(
    attempts: u32,
    wait: std::time::Duration,
    mut open: impl FnMut() -> Result<T, E>,
    mut sleep: impl FnMut(std::time::Duration),
) -> Option<T> {
    for attempt in 0..attempts {
        if attempt > 0 {
            sleep(wait);
        }
        if let Ok(guard) = open() {
            return Some(guard);
        }
    }
    None
}

/// Place the payload on the OS clipboard, all or nothing.
///
/// Every payload (including the DIB) is built before the clipboard is
/// opened, so a refusal leaves the operator's previous clipboard intact.
///
/// # Errors
///
/// [`ClipboardError`] — see the type.
#[cfg(windows)]
pub fn place(payload: &ClipboardPayload) -> Result<Placed, ClipboardError> {
    place_entries(&entries(payload)?)
}

/// Not Windows: nothing can be placed. The CLI prints the error and points
/// at `export-image`, which writes the same bytes to files.
#[cfg(not(windows))]
pub fn place(_payload: &ClipboardPayload) -> Result<Placed, ClipboardError> {
    Err(ClipboardError::Unsupported)
}

/// Open the clipboard (waiting between attempts) and [`fill`] it.
///
/// # Errors
///
/// [`ClipboardError::Open`] when every attempt failed; otherwise [`fill`]'s.
#[cfg(windows)]
pub fn place_entries(entries: &[ClipboardEntry]) -> Result<Placed, ClipboardError> {
    let _guard = open_with_wait(
        OPEN_ATTEMPTS,
        OPEN_WAIT,
        clipboard_win::Clipboard::new,
        std::thread::sleep,
    )
    .ok_or(ClipboardError::Open)?;
    fill(&mut WinClipboard, entries)
}

/// The real clipboard; valid only while a `clipboard_win::Clipboard` guard
/// is held on this thread.
#[cfg(windows)]
struct WinClipboard;

#[cfg(windows)]
impl ClipboardSink for WinClipboard {
    fn empty(&mut self) -> Result<(), ClipboardError> {
        clipboard_win::raw::empty().map_err(|_| ClipboardError::Open)
    }

    fn set(&mut self, entry: &ClipboardEntry) -> Result<(), ClipboardError> {
        use clipboard_win::raw;
        let id = match entry.kind {
            EntryKind::EnhMetafile => return set_enh_metafile(&entry.bytes),
            EntryKind::Standard(id) => id,
            EntryKind::Registered => raw::register_format(entry.name)
                .ok_or(ClipboardError::Register(entry.name))?
                .get(),
        };
        raw::set_without_clear(id, &entry.bytes).map_err(|_| ClipboardError::Set(entry.name))
    }
}

/// `CF_ENHMETAFILE` is a GDI HANDLE rather than an `HGLOBAL`, so it goes
/// through `SetEnhMetaFileBits` + `SetClipboardData` directly.
#[cfg(windows)]
fn set_enh_metafile(emf: &[u8]) -> Result<(), ClipboardError> {
    // SAFETY: `SetEnhMetaFileBits` copies the bytes out of a live slice; the
    // returned handle is owned by the clipboard once `SetClipboardData`
    // succeeds (Win32 contract), so it is NOT deleted on success. The caller
    // holds the clipboard open on this thread, which `SetClipboardData`
    // requires. On failure the handle is ours and is deleted.
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Graphics::Gdi::{DeleteEnhMetaFile, SetEnhMetaFileBits};
    use windows::Win32::System::DataExchange::SetClipboardData;
    let hemf = unsafe { SetEnhMetaFileBits(emf) };
    if hemf.is_invalid() {
        return Err(ClipboardError::Set("CF_ENHMETAFILE"));
    }
    let format = clipboard_win::formats::CF_ENHMETAFILE;
    if unsafe { SetClipboardData(format, Some(HANDLE(hemf.0))) }.is_err() {
        unsafe {
            let _ = DeleteEnhMetaFile(Some(hemf));
        }
        return Err(ClipboardError::Set("CF_ENHMETAFILE"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const OVER_I32: u32 = 1 << 31;

    fn field_u32(d: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([d[at], d[at + 1], d[at + 2], d[at + 3]])
    }

    fn refusal(r: Result<([u8; DIB_V5_HEADER_LEN], usize), ClipboardError>) -> &'static str {
        match r {
            Err(ClipboardError::DibTooLarge(what)) => what,
            other => panic!("expected a DibTooLarge refusal, got {other:?}"),
        }
    }

    #[test]
    fn dib_v5_header_is_124_bytes_top_down_bitfields_bgra() {
        let mut p = Pixmap::new(2, 1).unwrap();
        p.fill(pdfcer_render::tiny_skia::Color::from_rgba8(255, 0, 0, 255));
        let d = dib_v5(&p, 3780).unwrap();
        assert_eq!(d.len(), 124 + 8);
        assert_eq!(field_u32(&d, 0), 124);
        assert_eq!(i32::from_le_bytes([d[4], d[5], d[6], d[7]]), 2);
        assert_eq!(i32::from_le_bytes([d[8], d[9], d[10], d[11]]), -1);
        assert_eq!(u16::from_le_bytes([d[14], d[15]]), 32);
        assert_eq!(field_u32(&d, 16), 3);
        assert_eq!(field_u32(&d, 20), 8); // bV5SizeImage
        assert_eq!(field_u32(&d, 24), 3780);
        assert_eq!(field_u32(&d, 56), 0x7352_4742); // LCS_sRGB
        assert_eq!(field_u32(&d, 108), 4); // LCS_GM_IMAGES
        // BGRA: red pixel = 00 00 FF FF
        assert_eq!(&d[124..128], &[0, 0, 255, 255]);
    }

    #[test]
    fn dib_v5_refuses_a_side_past_i32_max() {
        // Zero on the other side keeps bV5SizeImage at 0, so each check is
        // the only one that can fire.
        assert!(refusal(dib_v5_header(OVER_I32, 0, 0)).contains("width"));
        assert!(refusal(dib_v5_header(0, OVER_I32, 0)).contains("height"));
        let (h, _) = dib_v5_header(i32::MAX as u32, 0, 0).unwrap();
        assert_eq!(field_u32(&h, 4), i32::MAX as u32);
    }

    #[test]
    fn dib_v5_refuses_an_image_past_u32_max_bytes() {
        // 65536 * 4 * 16384 = 2^32: one past bV5SizeImage's range.
        assert!(refusal(dib_v5_header(65536, 16384, 0)).contains("4 GiB"));
        // 16384 * 4 * 65535 = u32::MAX - 65535: fits, and is written exactly.
        let (h, len) = dib_v5_header(16384, 65535, 0).unwrap();
        assert_eq!(field_u32(&h, 20), 16384 * 4 * 65535);
        assert_eq!(len, 16384 * 4 * 65535);
    }

    #[test]
    fn dib_v5_refuses_pixels_per_metre_past_i32_max() {
        assert!(refusal(dib_v5_header(1, 1, OVER_I32)).contains("resolution"));
        let (h, _) = dib_v5_header(1, 1, i32::MAX as u32).unwrap();
        assert_eq!(field_u32(&h, 24), i32::MAX as u32);
        // Through the payload: refused while building, before any open.
        let payload = ClipboardPayload {
            pixmap: Pixmap::new(1, 1),
            pixels_per_metre: u32::MAX,
            ..ClipboardPayload::default()
        };
        assert!(matches!(
            entries(&payload),
            Err(ClipboardError::DibTooLarge(_))
        ));
    }

    /// A clipboard double: `formats` is what a reader would see.
    #[derive(Default)]
    struct FakeClipboard {
        formats: Vec<&'static str>,
        fail_set: Option<&'static str>,
        empties_allowed: Option<usize>,
    }

    impl ClipboardSink for FakeClipboard {
        fn empty(&mut self) -> Result<(), ClipboardError> {
            if let Some(n) = &mut self.empties_allowed {
                if *n == 0 {
                    return Err(ClipboardError::Open);
                }
                *n -= 1;
            }
            self.formats.clear();
            Ok(())
        }
        fn set(&mut self, entry: &ClipboardEntry) -> Result<(), ClipboardError> {
            if self.fail_set == Some(entry.name) {
                return Err(ClipboardError::Set(entry.name));
            }
            self.formats.push(entry.name);
            Ok(())
        }
    }

    fn three() -> Vec<ClipboardEntry> {
        ["image/svg+xml", "PNG", "application/pdf"]
            .into_iter()
            .map(|n| ClipboardEntry::registered(n, vec![1]))
            .collect()
    }

    #[test]
    fn fill_places_every_entry_in_order() {
        let mut fake = FakeClipboard {
            formats: vec!["old"],
            ..FakeClipboard::default()
        };
        let placed = fill(&mut fake, &three()).unwrap();
        assert_eq!(placed.formats, ["image/svg+xml", "PNG", "application/pdf"]);
        assert_eq!(fake.formats, placed.formats);
    }

    #[test]
    fn a_later_failed_set_leaves_no_format_of_the_placement() {
        let mut fake = FakeClipboard {
            fail_set: Some("application/pdf"),
            ..FakeClipboard::default()
        };
        let err = fill(&mut fake, &three()).unwrap_err();
        assert!(
            matches!(err, ClipboardError::Set("application/pdf")),
            "{err:?}"
        );
        assert!(
            fake.formats.is_empty(),
            "partial copy left: {:?}",
            fake.formats
        );
    }

    #[test]
    fn a_failed_cleanup_is_reported_not_hidden() {
        let mut fake = FakeClipboard {
            fail_set: Some("PNG"),
            empties_allowed: Some(1),
            ..FakeClipboard::default()
        };
        let err = fill(&mut fake, &three()).unwrap_err();
        assert!(matches!(err, ClipboardError::NotCleared("PNG")), "{err:?}");
        assert!(err.to_string().contains("partial copy"));
    }

    #[test]
    fn opening_waits_between_attempts() {
        let mut sleeps = Vec::new();
        let mut tries = 0;
        let got = open_with_wait(
            OPEN_ATTEMPTS,
            OPEN_WAIT,
            || {
                tries += 1;
                if tries < 4 { Err(()) } else { Ok(tries) }
            },
            |d| sleeps.push(d),
        );
        assert_eq!(got, Some(4));
        assert_eq!(sleeps, [OPEN_WAIT; 3]);

        let mut sleeps = Vec::new();
        let none = open_with_wait(
            OPEN_ATTEMPTS,
            OPEN_WAIT,
            || Err::<(), ()>(()),
            |d| sleeps.push(d),
        );
        assert_eq!(none, None);
        let total: Duration = sleeps.iter().sum();
        assert!(
            (Duration::from_millis(100)..=Duration::from_millis(500)).contains(&total),
            "{total:?}"
        );
    }

    #[test]
    fn svg_payload_is_nul_terminated_utf8() {
        assert_eq!(svg_payload("<svg/>"), b"<svg/>\0");
    }

    #[test]
    fn unicode_text_payload_is_nul_terminated_utf16le() {
        assert_eq!(unicode_text_payload("a\u{e9}"), [b'a', 0, 0xE9, 0, 0, 0]);
    }

    #[cfg(windows)]
    #[test]
    fn format_numbers_match_clipboard_win() {
        assert_eq!(CF_DIBV5, clipboard_win::formats::CF_DIBV5);
        assert_eq!(CF_UNICODETEXT, clipboard_win::formats::CF_UNICODETEXT);
    }
}
