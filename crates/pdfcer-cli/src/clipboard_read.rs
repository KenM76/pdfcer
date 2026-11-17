//! Reading the OS clipboard for `paste-clipboard`: which forms are held,
//! which one is taken, and a raw device-independent bitmap as pixels.
//!
//! ## Contract
//!
//! - [`SOURCES`] is the rank: PDF, SVG, EMF, PNG, bitmap. A vector form is
//!   always taken before a raster one.
//! - [`read`] opens the clipboard once, lists every source present and
//!   fetches the bytes of the one [`choose`] picks.
//! - Nothing larger than [`MAX_BYTES`] is fetched, and no bitmap larger
//!   than [`MAX_PIXELS`] is decoded.

// Off Windows only the refusal and the bitmap reader are reached; the
// ranking is still compiled so its tests run everywhere.
#![cfg_attr(not(windows), allow(dead_code))]

use crate::clipboard::ClipboardError;

use pdfcer_render::tiny_skia::{Pixmap, PremultipliedColorU8};

/// The largest clipboard payload fetched.
pub const MAX_BYTES: usize = 512 << 20;
/// The largest bitmap decoded, in pixels.
pub const MAX_PIXELS: u64 = 100_000_000;

/// A form `paste-clipboard` can place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum PasteFormat {
    /// A PDF; its first page is placed as vectors.
    Pdf,
    /// An SVG drawing, placed as vectors.
    Svg,
    /// A Windows enhanced metafile, placed as vectors.
    Emf,
    /// A PNG image.
    Png,
    /// A device-independent bitmap (CF_DIBV5 or CF_DIB).
    Dib,
}

impl PasteFormat {
    /// The token printed as `took=` and accepted by `--format`.
    #[must_use]
    pub fn token(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Svg => "svg",
            Self::Emf => "emf",
            Self::Png => "png",
            Self::Dib => "dib",
        }
    }

    /// Whether the form is vector content.
    #[must_use]
    pub fn is_vector(self) -> bool {
        matches!(self, Self::Pdf | Self::Svg | Self::Emf)
    }
}

/// How a [`Source`] is identified to Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// A registered format, by name.
    Registered,
    /// A predefined `CF_*` number whose data is an `HGLOBAL`.
    Standard(u32),
    /// `CF_ENHMETAFILE`, a GDI handle.
    EnhMetafile,
}

/// One clipboard format `paste-clipboard` knows how to read.
#[derive(Debug, PartialEq, Eq)]
pub struct Source {
    /// The format's clipboard name, printed in `held=`.
    pub name: &'static str,
    /// How Windows identifies it.
    pub kind: SourceKind,
    /// What it is placed as.
    pub format: PasteFormat,
}

const fn registered(name: &'static str, format: PasteFormat) -> Source {
    Source {
        name,
        kind: SourceKind::Registered,
        format,
    }
}

/// Every format read, best first. `"Portable Document Format"` is the name
/// Adobe applications register; `"image/x-inkscape-svg"` is Inkscape's own
/// (docs/clipboard-interop-survey.md).
pub static SOURCES: [Source; 8] = [
    registered("application/pdf", PasteFormat::Pdf),
    registered("Portable Document Format", PasteFormat::Pdf),
    registered("image/svg+xml", PasteFormat::Svg),
    registered("image/x-inkscape-svg", PasteFormat::Svg),
    Source {
        name: "CF_ENHMETAFILE",
        kind: SourceKind::EnhMetafile,
        format: PasteFormat::Emf,
    },
    registered("PNG", PasteFormat::Png),
    Source {
        name: "CF_DIBV5",
        kind: SourceKind::Standard(17),
        format: PasteFormat::Dib,
    },
    Source {
        name: "CF_DIB",
        kind: SourceKind::Standard(8),
        format: PasteFormat::Dib,
    },
];

/// The source to take from `held` (which is in [`SOURCES`] order): the
/// first one, or the first of the `wanted` form. `None` when there is none.
#[must_use]
pub fn choose(held: &[&'static Source], wanted: Option<PasteFormat>) -> Option<&'static Source> {
    held.iter()
        .copied()
        .find(|s| wanted.is_none_or(|w| s.format == w))
}

/// What [`read`] found.
#[derive(Debug, Default)]
pub struct Snapshot {
    /// Every known source present, best first.
    pub held: Vec<&'static Source>,
    /// The chosen source and its bytes; `None` when nothing matched, the
    /// bytes were over [`MAX_BYTES`], or `fetch` was off.
    pub taken: Option<(&'static Source, Vec<u8>)>,
}

/// Open the clipboard, list the known sources on it and, when `fetch`, get
/// the bytes of the one [`choose`] picks for `wanted`.
///
/// # Errors
///
/// [`ClipboardError::Open`] when another application holds the clipboard.
#[cfg(windows)]
pub fn read(wanted: Option<PasteFormat>, fetch: bool) -> Result<Snapshot, ClipboardError> {
    use crate::clipboard::{OPEN_ATTEMPTS, OPEN_WAIT, open_with_wait};
    let _guard = open_with_wait(
        OPEN_ATTEMPTS,
        OPEN_WAIT,
        clipboard_win::Clipboard::new,
        std::thread::sleep,
    )
    .ok_or(ClipboardError::Open)?;
    let held: Vec<&'static Source> = SOURCES
        .iter()
        .filter(|s| win::format_id(s).is_some_and(clipboard_win::raw::is_format_avail))
        .collect();
    let taken = if fetch {
        choose(&held, wanted).and_then(|s| win::fetch(s).map(|bytes| (s, bytes)))
    } else {
        None
    };
    Ok(Snapshot { held, taken })
}

/// Not Windows: the clipboard cannot be read.
///
/// # Errors
///
/// Always [`ClipboardError::Unsupported`].
#[cfg(not(windows))]
pub fn read(_wanted: Option<PasteFormat>, _fetch: bool) -> Result<Snapshot, ClipboardError> {
    Err(ClipboardError::Unsupported)
}

#[cfg(windows)]
mod win {
    use super::{MAX_BYTES, Source, SourceKind};
    use clipboard_win::raw;

    const CF_ENHMETAFILE: u32 = 14;

    /// The format number Windows knows `source` by; `None` when a name
    /// cannot be registered.
    pub(super) fn format_id(source: &Source) -> Option<u32> {
        match source.kind {
            SourceKind::Registered => raw::register_format(source.name).map(std::num::NonZero::get),
            SourceKind::Standard(id) => Some(id),
            SourceKind::EnhMetafile => Some(CF_ENHMETAFILE),
        }
    }

    /// The bytes of `source`, or `None` when they are absent, unreadable
    /// or over [`MAX_BYTES`]. The clipboard must be open.
    pub(super) fn fetch(source: &Source) -> Option<Vec<u8>> {
        if source.kind == SourceKind::EnhMetafile {
            return enh_metafile();
        }
        let id = format_id(source)?;
        if raw::size(id)?.get() > MAX_BYTES {
            return None;
        }
        let mut bytes = Vec::new();
        raw::get_vec(id, &mut bytes).ok()?;
        Some(bytes)
    }

    fn enh_metafile() -> Option<Vec<u8>> {
        use windows::Win32::Graphics::Gdi::{GetEnhMetaFileBits, HENHMETAFILE};
        use windows::Win32::System::DataExchange::GetClipboardData;
        // SAFETY: the caller holds the clipboard open on this thread, which
        // `GetClipboardData` requires. The handle stays owned by the
        // clipboard and is not freed here. `GetEnhMetaFileBits` writes at
        // most the slice's length.
        let handle = unsafe { GetClipboardData(CF_ENHMETAFILE) }.ok()?;
        let hemf = HENHMETAFILE(handle.0);
        let len = unsafe { GetEnhMetaFileBits(hemf, None) };
        let size = usize::try_from(len).ok()?;
        if size == 0 || size > MAX_BYTES {
            return None;
        }
        let mut bytes = vec![0u8; size];
        let got = unsafe { GetEnhMetaFileBits(hemf, Some(&mut bytes)) };
        (got == len).then_some(bytes)
    }
}

/// A decoded device-independent bitmap.
#[derive(Debug)]
pub struct DibImage {
    /// The pixels, top row first.
    pub pixmap: Pixmap,
    /// The bitmap's fourth byte was not used as alpha: a 32-bit `BI_RGB`
    /// bitmap (where it is undefined), or an alpha channel that is zero in
    /// every pixel. The image is opaque.
    pub alpha_ignored: bool,
}

/// The header fields [`dib_to_pixmap`] reads.
struct DibHeader {
    width: u32,
    height: u32,
    top_down: bool,
    bytes_per_pixel: usize,
    /// R, G, B, A masks; `None` for `BI_RGB`.
    masks: Option<[u32; 4]>,
    pixel_offset: usize,
    stride: usize,
}

fn le_u32(d: &[u8], at: usize) -> Option<u32> {
    let b = d.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn dib_header(d: &[u8]) -> Result<DibHeader, String> {
    let short = || String::from("the bitmap is shorter than its header");
    let size = le_u32(d, 0).ok_or_else(short)? as usize;
    if size < 40 || d.len() < size {
        return Err(String::from(
            "the bitmap header is not a BITMAPINFOHEADER or later",
        ));
    }
    let field = |at: usize| le_u32(d, at).ok_or_else(short);
    let (w, h) = (field(4)?.cast_signed(), field(8)?.cast_signed());
    let bpp = field(12)? >> 16;
    let compression = field(16)?;
    if w <= 0 || h == 0 {
        return Err(format!("the bitmap is {w} x {h} pixels"));
    }
    let (width, height) = (w.unsigned_abs(), h.unsigned_abs());
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(format!(
            "the bitmap is {width} x {height} pixels, over the {MAX_PIXELS}-pixel limit"
        ));
    }
    let masks = match (bpp, compression) {
        (24 | 32, 0) => None,
        (32, 3) => Some([
            field(40)?,
            field(44)?,
            field(48)?,
            if size >= 56 { field(52)? } else { 0 },
        ]),
        _ => {
            return Err(format!(
                "a {bpp}-bit bitmap with compression {compression} is not read (24-bit and 32-bit uncompressed, and 32-bit BI_BITFIELDS, are)"
            ));
        }
    };
    let bytes_per_pixel = (bpp / 8) as usize;
    let stride = (width as usize * bytes_per_pixel).next_multiple_of(4);
    let need = stride * height as usize;
    // A 40-byte header keeps its BI_BITFIELDS masks after itself.
    let trailing_masks = if size == 40 && masks.is_some() { 12 } else { 0 };
    let palette = (field(32)? as usize).saturating_mul(4);
    let mut pixel_offset = size.saturating_add(trailing_masks).saturating_add(palette);
    // Some writers repeat the three masks after a V4/V5 header as well.
    if size > 40 && masks.is_some() && d.len().checked_sub(pixel_offset) == Some(need + 12) {
        pixel_offset += 12;
    }
    if d.len().checked_sub(pixel_offset).is_none_or(|n| n < need) {
        return Err(String::from(
            "the bitmap's pixel data is shorter than its header declares",
        ));
    }
    Ok(DibHeader {
        width,
        height,
        top_down: h < 0,
        bytes_per_pixel,
        masks,
        pixel_offset,
        stride,
    })
}

/// One channel of `px` under `mask`, scaled to 8 bits.
fn channel(px: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let max = u64::from(mask >> shift);
    let v = u64::from((px & mask) >> shift);
    u8::try_from(v * 255 / max).unwrap_or(255)
}

/// Decode a raw DIB (a `BITMAPINFOHEADER`, V4 or V5 header followed by the
/// pixel rows, with no `BM` file header) as the clipboard carries it.
///
/// 24-bit and 32-bit uncompressed and 32-bit `BI_BITFIELDS` are read. Alpha
/// is taken as premultiplied, which is what `copy-page` and browsers write.
///
/// # Errors
///
/// A message naming what is wrong: a truncated or unknown header, a size
/// over [`MAX_PIXELS`], or a bit depth or compression that is not read.
pub fn dib_to_pixmap(d: &[u8]) -> Result<DibImage, String> {
    let h = dib_header(d)?;
    let mut pixmap = Pixmap::new(h.width, h.height)
        .ok_or_else(|| String::from("not enough memory for the bitmap"))?;
    let rgba = |x: usize, row: usize| -> [u8; 4] {
        let at = h.pixel_offset + row * h.stride + x * h.bytes_per_pixel;
        let p = &d[at..at + h.bytes_per_pixel];
        match h.masks {
            None => [p[2], p[1], p[0], 255],
            Some([r, g, b, a]) => {
                let px = u32::from_le_bytes([p[0], p[1], p[2], p[3]]);
                [
                    channel(px, r),
                    channel(px, g),
                    channel(px, b),
                    channel(px, a),
                ]
            }
        }
    };
    let (width, height) = (h.width as usize, h.height as usize);
    let has_alpha = h.masks.is_some_and(|m| m[3] != 0)
        && (0..height).any(|row| (0..width).any(|x| rgba(x, row)[3] != 0));
    let out = pixmap.pixels_mut();
    for y in 0..height {
        let row = if h.top_down { y } else { height - 1 - y };
        for x in 0..width {
            let [r, g, b, a] = rgba(x, row);
            let a = if has_alpha { a } else { 255 };
            out[y * width + x] = PremultipliedColorU8::from_rgba(r.min(a), g.min(a), b.min(a), a)
                .unwrap_or(PremultipliedColorU8::TRANSPARENT);
        }
    }
    Ok(DibImage {
        pixmap,
        alpha_ignored: h.bytes_per_pixel == 4 && !has_alpha,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(names: &[&str]) -> Vec<&'static Source> {
        SOURCES.iter().filter(|s| names.contains(&s.name)).collect()
    }

    #[test]
    fn a_vector_form_is_taken_before_a_raster_one() {
        let all = held(&["PNG", "CF_DIBV5", "image/svg+xml", "CF_ENHMETAFILE"]);
        assert_eq!(choose(&all, None).map(|s| s.name), Some("image/svg+xml"));
        let with_pdf = held(&["PNG", "image/svg+xml", "Portable Document Format"]);
        assert_eq!(
            choose(&with_pdf, None).map(|s| s.format),
            Some(PasteFormat::Pdf)
        );
        assert_eq!(
            choose(&all, Some(PasteFormat::Png)).map(|s| s.name),
            Some("PNG")
        );
        assert_eq!(choose(&all, Some(PasteFormat::Pdf)), None);
        assert_eq!(choose(&[], None), None);
        assert!(PasteFormat::Emf.is_vector() && !PasteFormat::Dib.is_vector());
    }

    fn sample() -> Pixmap {
        let mut p = Pixmap::new(3, 2).unwrap();
        let px = |r, g, b, a| PremultipliedColorU8::from_rgba(r, g, b, a).unwrap();
        p.pixels_mut().copy_from_slice(&[
            px(255, 0, 0, 255),
            px(0, 128, 0, 128),
            px(0, 0, 0, 0),
            px(1, 2, 3, 255),
            px(10, 20, 30, 40),
            px(255, 255, 255, 255),
        ]);
        p
    }

    #[test]
    fn what_copy_page_writes_reads_back_pixel_for_pixel() {
        let original = sample();
        let dib = crate::clipboard::dib_v5(&original, 3780).unwrap();
        let read = dib_to_pixmap(&dib).unwrap();
        assert!(!read.alpha_ignored);
        assert_eq!(read.pixmap.data(), original.data());
    }

    /// A 40-byte-header bitmap: 2 x 2, bottom-up, `bpp` bits.
    fn plain(bpp: u16, rows: &[&[u8]]) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(&40u32.to_le_bytes());
        d.extend_from_slice(&2i32.to_le_bytes());
        d.extend_from_slice(&2i32.to_le_bytes());
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&bpp.to_le_bytes());
        d.extend_from_slice(&[0u8; 24]);
        for row in rows {
            d.extend_from_slice(row);
        }
        d
    }

    #[test]
    fn an_uncompressed_bitmap_is_bottom_up_and_opaque() {
        // 24-bit rows pad to 8 bytes; the first stored row is the bottom.
        let d = plain(
            24,
            &[&[1, 2, 3, 4, 5, 6, 0, 0], &[7, 8, 9, 10, 11, 12, 0, 0]],
        );
        let read = dib_to_pixmap(&d).unwrap();
        assert!(!read.alpha_ignored);
        assert_eq!(
            read.pixmap.data(),
            &[9, 8, 7, 255, 12, 11, 10, 255, 3, 2, 1, 255, 6, 5, 4, 255]
        );
        // 32-bit BI_RGB: the fourth byte is undefined, so it is not alpha.
        let d = plain(32, &[&[1, 2, 3, 0, 4, 5, 6, 0], &[7, 8, 9, 0, 0, 0, 0, 0]]);
        let read = dib_to_pixmap(&d).unwrap();
        assert!(read.alpha_ignored);
        assert_eq!(&read.pixmap.data()[..4], &[9, 8, 7, 255]);
    }

    #[test]
    fn a_bitmap_that_cannot_be_read_is_refused_by_name() {
        assert!(dib_to_pixmap(&[0; 2]).unwrap_err().contains("shorter"));
        assert!(
            dib_to_pixmap(&[0; 10])
                .unwrap_err()
                .contains("BITMAPINFOHEADER")
        );
        let mut eight = plain(24, &[&[0; 8], &[0; 8]]);
        eight[14] = 8;
        assert!(dib_to_pixmap(&eight).unwrap_err().contains("8-bit"));
        let mut cut = plain(24, &[&[0; 8], &[0; 8]]);
        cut.truncate(50);
        assert!(dib_to_pixmap(&cut).unwrap_err().contains("pixel data"));
        let mut huge = plain(24, &[&[0; 8], &[0; 8]]);
        huge[4..8].copy_from_slice(&1_000_000i32.to_le_bytes());
        huge[8..12].copy_from_slice(&1_000_000i32.to_le_bytes());
        assert!(dib_to_pixmap(&huge).unwrap_err().contains("limit"));
    }
}
