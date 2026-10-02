//! # GIF import (GIF87a and GIF89a) — the first frame, with its transparency
//!
//! Section references are to the *Graphics Interchange Format Version 89a*
//! specification (CompuServe, 1990). It is not a PDF-family standard, so it is
//! not in the PDF spec RAG; the clauses cited here are the ones the decoder
//! enforces.
//!
//! ## What is placed
//!
//! | Property | Handling | GIF89a |
//! |---|---|---|
//! | Version | `GIF87a` and `GIF89a` | §17 |
//! | Logical screen | the canvas; grown to cover a first frame that overhangs it | §18 |
//! | Colour table | the frame's local table, else the global one | §19, §21 |
//! | Image data | variable-length LZW, LSB-first codes, sub-blocked | §22, App. F |
//! | Interlacing | the four-pass row order, undone | App. E |
//! | Transparency | the Graphic Control Extension's transparent index → `/SMask` | §23 |
//! | Animation | first frame placed; the rest counted in [`ImportNotes::gif_frames_ignored`] | — |
//!
//! The result is `/Indexed /DeviceRGB` at 8 bits per index, Flate-compressed.
//! GIF's LZW cannot be passed through: PDF's `LZWDecode` reads codes
//! most-significant-bit first from one continuous stream (ISO 32000-2
//! §7.4.4.2), GIF packs them least-significant-bit first inside length-prefixed
//! sub-blocks. Hence [`RecompressReason::SourceCodecNotReusable`].
//!
//! ## The canvas is the first frame on an empty screen
//!
//! Frame 1 is composited onto a transparent logical screen. Pixels the frame
//! does not cover, and pixels holding the transparent index, are clear in the
//! soft mask. The screen's background colour index is not painted: browsers
//! ignore it, and painting it would make a placed GIF look different from the
//! same GIF on a web page. Disposal methods describe what happens *after* a
//! frame is shown, so they never affect frame 1.
//!
//! ## Ignored without refusal
//!
//! Comment, application (`NETSCAPE2.0` looping) and plain-text extensions
//! (§24–§26) are skipped; no browser renders plain-text extensions either. The
//! pixel aspect ratio byte (§18) is ignored for the same reason.
//!
//! ## Refused
//!
//! | Input | Error |
//! |---|---|
//! | No global or local colour table | `Unsupported { "GIF/no-colour-table" }` |
//! | LZW minimum code size outside 1–8 | `Corrupt` |
//! | Image data ending before `width × height` pixels | `Corrupt` |
//! | An opaque pixel indexing past the colour table | `Corrupt` |
//! | Any header, table or block cut short | `Corrupt` |
//!
//! The LZW output buffer is exactly `width × height` bytes after
//! the decode ceilings have bounded both, so a hostile code stream cannot
//! decode past the frame.

use super::{
    DpiSource, ImageFormat, ImageImportError, ImportColorSpace, ImportFilter, ImportNotes,
    ImportedImage, Orientation, PdfFeature, RecompressReason, SoftMask, check_dimensions, corrupt,
    flate_encode, raise_version,
};

const EXTENSION: u8 = 0x21;
const IMAGE_DESCRIPTOR: u8 = 0x2C;
const TRAILER: u8 = 0x3B;
const GRAPHIC_CONTROL: u8 = 0xF9;

/// Decode a GIF's first frame into an image XObject.
///
/// Further frames are counted, not decoded, into
/// [`ImportNotes::gif_frames_ignored`]; that count is a lower bound when the
/// file is damaged after the first frame, which is already complete.
///
/// # Errors
///
/// [`ImageImportError::Corrupt`] for a malformed or truncated file,
/// [`ImageImportError::Unsupported`] for a frame with no colour table, and
/// [`ImageImportError::TooLarge`] / [`ImageImportError::Empty`] for a
/// screen or frame outside the decode ceilings, checked before allocating.
pub fn import(data: &[u8]) -> Result<ImportedImage, ImageImportError> {
    let mut reader = Reader { data, pos: 0 };
    let screen = Screen::read(&mut reader)?;
    let (frame, transparent) = first_frame(&mut reader)?;
    let ignored = count_frames(&mut reader);
    assemble(&screen, &frame, transparent, ignored)
}

fn truncated(what: &str) -> ImageImportError {
    corrupt(&format!("the GIF ends inside its {what}"))
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn byte(&mut self, what: &str) -> Result<u8, ImageImportError> {
        let b = self
            .data
            .get(self.pos)
            .copied()
            .ok_or_else(|| truncated(what))?;
        self.pos += 1;
        Ok(b)
    }

    fn u16(&mut self, what: &str) -> Result<u16, ImageImportError> {
        let lo = self.byte(what)?;
        let hi = self.byte(what)?;
        Ok(u16::from_le_bytes([lo, hi]))
    }

    fn take(&mut self, n: usize, what: &str) -> Result<&'a [u8], ImageImportError> {
        let end = self.pos.checked_add(n).ok_or_else(|| truncated(what))?;
        let slice = self
            .data
            .get(self.pos..end)
            .ok_or_else(|| truncated(what))?;
        self.pos = end;
        Ok(slice)
    }

    /// Walk a sub-block chain (§15) to its zero-length terminator.
    fn sub_blocks(
        &mut self,
        what: &str,
        mut sink: impl FnMut(&'a [u8]),
    ) -> Result<(), ImageImportError> {
        loop {
            let len = self.byte(what)?;
            if len == 0 {
                return Ok(());
            }
            sink(self.take(usize::from(len), what)?);
        }
    }

    /// A colour table (§19, §21), present when bit 7 of `packed` is set.
    fn colour_table(
        &mut self,
        packed: u8,
        what: &str,
    ) -> Result<Option<&'a [u8]>, ImageImportError> {
        if packed & 0x80 == 0 {
            return Ok(None);
        }
        let entries = 2usize << (packed & 0x07);
        self.take(entries * 3, what).map(Some)
    }
}

/// The logical screen descriptor and global colour table (§17–§19).
struct Screen<'a> {
    width: u16,
    height: u16,
    global: Option<&'a [u8]>,
}

impl<'a> Screen<'a> {
    fn read(r: &mut Reader<'a>) -> Result<Self, ImageImportError> {
        let header = r.take(6, "header")?;
        if header != b"GIF87a" && header != b"GIF89a" {
            return Err(corrupt("the header is not GIF87a or GIF89a"));
        }
        let width = r.u16("logical screen descriptor")?;
        let height = r.u16("logical screen descriptor")?;
        let packed = r.byte("logical screen descriptor")?;
        // Background colour index and pixel aspect ratio: see the module docs.
        r.take(2, "logical screen descriptor")?;
        let global = r.colour_table(packed, "global colour table")?;
        Ok(Self {
            width,
            height,
            global,
        })
    }
}

/// One image descriptor (§20) with its decoded, de-interlaced indices.
struct Frame<'a> {
    left: u16,
    top: u16,
    width: u16,
    height: u16,
    local: Option<&'a [u8]>,
    indices: Vec<u8>,
}

/// Read blocks up to and including the first image. Returns the frame and
/// the transparent index its Graphic Control Extension declared, if any.
fn first_frame<'a>(r: &mut Reader<'a>) -> Result<(Frame<'a>, Option<u8>), ImageImportError> {
    let mut transparent = None;
    loop {
        match r.byte("block sequence")? {
            EXTENSION => {
                if r.byte("extension label")? == GRAPHIC_CONTROL {
                    transparent = read_graphic_control(r)?;
                } else {
                    r.sub_blocks("extension", |_| {})?;
                }
            }
            IMAGE_DESCRIPTOR => return Ok((read_frame(r)?, transparent)),
            TRAILER => return Err(corrupt("the GIF ends before its first image")),
            other => {
                return Err(corrupt(&format!(
                    "unknown block introducer 0x{other:02X} before the first image"
                )));
            }
        }
    }
}

/// §23: block size 4, packed fields (bit 0 = transparent colour flag), delay
/// time, transparent colour index.
fn read_graphic_control(r: &mut Reader<'_>) -> Result<Option<u8>, ImageImportError> {
    let mut first: Option<&[u8]> = None;
    r.sub_blocks("graphic control extension", |b| {
        first.get_or_insert(b);
    })?;
    Ok(match first {
        Some(&[packed, _, _, index, ..]) if packed & 0x01 != 0 => Some(index),
        _ => None,
    })
}

fn read_frame<'a>(r: &mut Reader<'a>) -> Result<Frame<'a>, ImageImportError> {
    let left = r.u16("image descriptor")?;
    let top = r.u16("image descriptor")?;
    let width = r.u16("image descriptor")?;
    let height = r.u16("image descriptor")?;
    let packed = r.byte("image descriptor")?;
    let local = r.colour_table(packed, "local colour table")?;
    let min_code_size = r.byte("image data")?;
    let mut compressed = Vec::new();
    r.sub_blocks("image data", |b| compressed.extend_from_slice(b))?;

    check_dimensions(u32::from(width), u32::from(height), 1, 8)?;
    let mut indices = decode_lzw(
        min_code_size,
        &compressed,
        usize::from(width) * usize::from(height),
    )?;
    if packed & 0x40 != 0 {
        indices = deinterlace(&indices, usize::from(width), usize::from(height));
    }
    Ok(Frame {
        left,
        top,
        width,
        height,
        local,
        indices,
    })
}

/// Decode exactly `pixels` indices (§22, Appendix F). Data past the last
/// pixel is ignored; data ending before it is refused.
fn decode_lzw(min_code_size: u8, data: &[u8], pixels: usize) -> Result<Vec<u8>, ImageImportError> {
    // weezl asserts the size is ≤ 12; GIF indices are at most 8 bits wide.
    if !(1..=8).contains(&min_code_size) {
        return Err(corrupt(&format!(
            "LZW minimum code size {min_code_size} is outside 1–8"
        )));
    }
    let mut decoder = weezl::decode::Decoder::new(weezl::BitOrder::Lsb, min_code_size);
    let mut out = vec![0u8; pixels];
    let mut filled = 0;
    let mut input = data;
    while filled < pixels {
        let Some(dest) = out.get_mut(filled..) else {
            break;
        };
        let result = decoder.decode_bytes(input, dest);
        filled += result.consumed_out;
        input = input.get(result.consumed_in..).unwrap_or_default();
        let stalled = result.consumed_in == 0 && result.consumed_out == 0;
        match result.status {
            Ok(weezl::LzwStatus::Ok) if !stalled => {}
            _ => break,
        }
    }
    if filled < pixels {
        return Err(corrupt(&format!(
            "the image data ends after {filled} of {pixels} pixels"
        )));
    }
    Ok(out)
}

/// Appendix E: decoded rows arrive as every 8th row from 0, every 8th from 4,
/// every 4th from 2, then every 2nd from 1.
fn deinterlace(decoded: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut out = vec![0u8; decoded.len()];
    let order = [(0, 8), (4, 8), (2, 4), (1, 2)]
        .into_iter()
        .flat_map(|(start, step)| (start..height).step_by(step));
    for (src_row, dst_row) in decoded.chunks_exact(width.max(1)).zip(order) {
        let start = dst_row * width;
        if let Some(dst) = out.get_mut(start..start + width) {
            dst.copy_from_slice(src_row);
        }
    }
    out
}

/// Count image descriptors after the first, skipping their data undecoded.
/// Stops at the trailer or the first malformation.
fn count_frames(r: &mut Reader<'_>) -> u32 {
    let mut frames = 0u32;
    while let Ok(introducer) = r.byte("block sequence") {
        let skipped = match introducer {
            EXTENSION => r
                .byte("extension label")
                .and_then(|_| r.sub_blocks("extension", |_| {})),
            IMAGE_DESCRIPTOR => {
                let Ok(descriptor) = r.take(9, "image descriptor") else {
                    break;
                };
                frames = frames.saturating_add(1);
                let packed = descriptor.get(8).copied().unwrap_or(0);
                r.colour_table(packed, "local colour table")
                    .and_then(|_| r.byte("image data"))
                    .and_then(|_| r.sub_blocks("image data", |_| {}))
            }
            _ => break,
        };
        if skipped.is_err() {
            break;
        }
    }
    frames
}

/// Composite the frame onto its canvas and build the XObject.
fn assemble(
    screen: &Screen<'_>,
    frame: &Frame<'_>,
    transparent: Option<u8>,
    frames_ignored: u32,
) -> Result<ImportedImage, ImageImportError> {
    let table = frame
        .local
        .or(screen.global)
        .ok_or(ImageImportError::Unsupported {
            feature: "GIF/no-colour-table",
        })?;
    let width = u32::from(screen.width).max(u32::from(frame.left) + u32::from(frame.width));
    let height = u32::from(screen.height).max(u32::from(frame.top) + u32::from(frame.height));
    check_dimensions(width, height, 1, 8)?;

    let canvas = composite(
        frame,
        transparent,
        table.len() / 3,
        width as usize,
        height as usize,
    )?;
    let mut notes = ImportNotes {
        recompressed: Some(RecompressReason::SourceCodecNotReusable),
        dpi_source: DpiSource::Assumed,
        gif_frames_ignored: frames_ignored,
        ..ImportNotes::default()
    };
    let soft_mask = if canvas.alpha.contains(&0) {
        notes.alpha_to_soft_mask = true;
        raise_version(&mut notes.requires_pdf_version, PdfFeature::SoftMask);
        Some(SoftMask {
            width,
            height,
            bits_per_component: 8,
            data: flate_encode(&canvas.alpha)?,
        })
    } else {
        None
    };

    Ok(ImportedImage {
        format: ImageFormat::Gif,
        width,
        height,
        bits_per_component: 8,
        color_space: ImportColorSpace::Indexed {
            // A table holds 2–256 entries, so `len / 3 − 1` fits a `u8`.
            hival: u8::try_from(table.len() / 3 - 1).unwrap_or(u8::MAX),
            lookup: table.to_vec(),
        },
        filter: ImportFilter::Flate,
        data: flate_encode(&canvas.indices)?,
        soft_mask,
        color_key_mask: None,
        orientation: Orientation::Identity,
        dpi: None,
        notes,
    })
}

struct Canvas {
    indices: Vec<u8>,
    alpha: Vec<u8>,
}

/// Place the frame's opaque pixels; everything else stays index 0, alpha 0.
fn composite(
    frame: &Frame<'_>,
    transparent: Option<u8>,
    entries: usize,
    width: usize,
    height: usize,
) -> Result<Canvas, ImageImportError> {
    let mut canvas = Canvas {
        indices: vec![0u8; width * height],
        alpha: vec![0u8; width * height],
    };
    let frame_width = usize::from(frame.width);
    let left = usize::from(frame.left);
    for (y, row) in frame.indices.chunks_exact(frame_width.max(1)).enumerate() {
        let start = (usize::from(frame.top) + y) * width + left;
        let range = start..start + frame_width;
        let (Some(dst), Some(dst_alpha)) = (
            canvas.indices.get_mut(range.clone()),
            canvas.alpha.get_mut(range),
        ) else {
            continue;
        };
        for ((d, a), &index) in dst.iter_mut().zip(dst_alpha.iter_mut()).zip(row) {
            if Some(index) == transparent {
                continue;
            }
            if usize::from(index) >= entries {
                return Err(corrupt(&format!(
                    "pixel index {index} is past the {entries}-entry colour table"
                )));
            }
            *d = index;
            *a = 0xFF;
        }
    }
    Ok(canvas)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    use crate::filters::flate;

    const BLACK_WHITE: &[u8] = &[0, 0, 0, 255, 255, 255];
    const FOUR: &[u8] = &[255, 0, 0, 0, 255, 0, 0, 0, 255, 9, 9, 9];

    struct Spec {
        left: u16,
        top: u16,
        width: u16,
        height: u16,
        local: Option<&'static [u8]>,
        interlaced: bool,
        transparent: Option<u8>,
        /// Row-major, as displayed; interlaced storage is derived from it.
        indices: Vec<u8>,
    }

    fn spec(width: u16, height: u16, indices: Vec<u8>) -> Spec {
        Spec {
            left: 0,
            top: 0,
            width,
            height,
            local: None,
            interlaced: false,
            transparent: None,
            indices,
        }
    }

    fn table_bits(table: &[u8]) -> u8 {
        let entries = table.len() / 3;
        assert!(entries.is_power_of_two() && entries >= 2);
        u8::try_from(entries.trailing_zeros() - 1).unwrap()
    }

    fn sub_blocks(out: &mut Vec<u8>, data: &[u8]) {
        for chunk in data.chunks(255) {
            out.push(u8::try_from(chunk.len()).unwrap());
            out.extend_from_slice(chunk);
        }
        out.push(0);
    }

    fn interlace_storage(s: &Spec) -> Vec<u8> {
        let w = usize::from(s.width);
        let h = usize::from(s.height);
        [(0, 8), (4, 8), (2, 4), (1, 2)]
            .into_iter()
            .flat_map(|(start, step)| (start..h).step_by(step))
            .flat_map(|row| s.indices[row * w..(row + 1) * w].to_vec())
            .collect()
    }

    fn frame_bytes(out: &mut Vec<u8>, s: &Spec) {
        if let Some(index) = s.transparent {
            out.extend_from_slice(&[EXTENSION, GRAPHIC_CONTROL, 4, 0x01, 0, 0, index, 0]);
        }
        out.push(IMAGE_DESCRIPTOR);
        for v in [s.left, s.top, s.width, s.height] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        let mut packed = if s.interlaced { 0x40 } else { 0 };
        if let Some(t) = s.local {
            packed |= 0x80 | table_bits(t);
        }
        out.push(packed);
        if let Some(t) = s.local {
            out.extend_from_slice(t);
        }
        out.push(8);
        let stored = if s.interlaced {
            interlace_storage(s)
        } else {
            s.indices.clone()
        };
        let lzw = weezl::encode::Encoder::new(weezl::BitOrder::Lsb, 8)
            .encode(&stored)
            .unwrap();
        sub_blocks(out, &lzw);
    }

    fn gif(screen: (u16, u16), global: Option<&[u8]>, frames: &[Spec]) -> Vec<u8> {
        let mut out = b"GIF89a".to_vec();
        out.extend_from_slice(&screen.0.to_le_bytes());
        out.extend_from_slice(&screen.1.to_le_bytes());
        out.push(global.map_or(0, |t| 0x80 | table_bits(t)));
        out.extend_from_slice(&[0, 0]);
        if let Some(t) = global {
            out.extend_from_slice(t);
        }
        // An application extension before the first frame, as animated-GIF
        // writers emit, to exercise the skip path.
        out.extend_from_slice(&[EXTENSION, 0xFF, 11]);
        out.extend_from_slice(b"NETSCAPE2.0");
        out.extend_from_slice(&[3, 1, 0, 0, 0]);
        for f in frames {
            frame_bytes(&mut out, f);
        }
        out.push(TRAILER);
        out
    }

    fn indices(img: &ImportedImage) -> Vec<u8> {
        flate::decode(&img.data, None).unwrap()
    }

    fn mask(img: &ImportedImage) -> Vec<u8> {
        flate::decode(&img.soft_mask.as_ref().unwrap().data, None).unwrap()
    }

    #[test]
    fn a_transparent_index_is_clear_in_the_soft_mask() {
        let mut s = spec(2, 2, vec![0, 1, 1, 0]);
        s.transparent = Some(1);
        let img = import(&gif((2, 2), Some(BLACK_WHITE), &[s])).unwrap();
        assert_eq!(mask(&img), [255, 0, 0, 255]);
        assert_eq!(indices(&img), [0, 0, 0, 0]);
        assert!(img.notes.alpha_to_soft_mask);
        assert_eq!(img.notes.requires_pdf_version, Some(PdfFeature::SoftMask));
        assert_eq!(
            img.color_space,
            ImportColorSpace::Indexed {
                hival: 1,
                lookup: BLACK_WHITE.to_vec()
            }
        );
    }

    #[test]
    fn an_opaque_gif_writes_no_soft_mask() {
        let img = import(&gif((2, 1), Some(BLACK_WHITE), &[spec(2, 1, vec![1, 0])])).unwrap();
        assert!(img.soft_mask.is_none());
        assert!(!img.notes.alpha_to_soft_mask);
        assert_eq!(indices(&img), [1, 0]);
        assert_eq!(
            img.notes.recompressed,
            Some(RecompressReason::SourceCodecNotReusable)
        );
    }

    #[test]
    fn interlaced_rows_land_where_a_progressive_image_puts_them() {
        let (w, h) = (3u16, 11u16);
        let pixels: Vec<u8> = (0..w * h)
            .map(|i| u8::try_from(i / w % 4).unwrap())
            .collect();
        let plain = import(&gif((w, h), Some(FOUR), &[spec(w, h, pixels.clone())])).unwrap();
        let mut s = spec(w, h, pixels.clone());
        s.interlaced = true;
        let interlaced = import(&gif((w, h), Some(FOUR), &[s])).unwrap();
        assert_eq!(indices(&plain), pixels);
        assert_eq!(indices(&interlaced), pixels);
    }

    #[test]
    fn later_frames_are_counted_not_placed() {
        let frames = [
            spec(2, 1, vec![0, 1]),
            spec(2, 1, vec![1, 1]),
            spec(2, 1, vec![1, 0]),
        ];
        let img = import(&gif((2, 1), Some(BLACK_WHITE), &frames)).unwrap();
        assert_eq!(img.notes.gif_frames_ignored, 2);
        assert_eq!(indices(&img), [0, 1]);
    }

    #[test]
    fn a_later_frames_transparency_does_not_reach_the_first() {
        let mut second = spec(2, 1, vec![0, 0]);
        second.transparent = Some(0);
        let frames = [spec(2, 1, vec![0, 1]), second];
        let img = import(&gif((2, 1), Some(BLACK_WHITE), &frames)).unwrap();
        assert!(img.soft_mask.is_none());
        assert_eq!(img.notes.gif_frames_ignored, 1);
    }

    #[test]
    fn the_local_table_wins_over_the_global_one() {
        let mut s = spec(1, 1, vec![2]);
        s.local = Some(FOUR);
        let img = import(&gif((1, 1), Some(BLACK_WHITE), &[s])).unwrap();
        let ImportColorSpace::Indexed { hival, lookup } = img.color_space else {
            panic!("indexed expected");
        };
        assert_eq!((hival, lookup.as_slice()), (3, FOUR));
    }

    #[test]
    fn a_frame_smaller_than_the_screen_leaves_the_rest_clear() {
        let mut s = spec(1, 1, vec![1]);
        s.left = 2;
        s.top = 1;
        let img = import(&gif((3, 2), Some(BLACK_WHITE), &[s])).unwrap();
        assert_eq!((img.width, img.height), (3, 2));
        assert_eq!(mask(&img), [0, 0, 0, 0, 0, 255]);
        assert_eq!(indices(&img), [0, 0, 0, 0, 0, 1]);
    }

    #[test]
    fn a_frame_overhanging_the_screen_grows_the_canvas() {
        let mut s = spec(2, 1, vec![1, 1]);
        s.left = 1;
        let img = import(&gif((1, 1), Some(BLACK_WHITE), &[s])).unwrap();
        assert_eq!((img.width, img.height), (3, 1));
        assert_eq!(mask(&img), [0, 255, 255]);
    }

    #[test]
    fn long_runs_decode_through_the_kwkwk_case() {
        let pixels = vec![1u8; 5000];
        let bytes = gif(
            (100, 50),
            Some(BLACK_WHITE),
            &[spec(100, 50, pixels.clone())],
        );
        assert_eq!(indices(&import(&bytes).unwrap()), pixels);
    }

    #[test]
    fn every_truncation_inside_the_first_frame_is_corrupt() {
        let first: Vec<u8> = (0..16).map(|i| i % 4).collect();
        let full = gif(
            (4, 4),
            Some(FOUR),
            &[spec(4, 4, first), spec(1, 1, vec![0])],
        );
        let mut first_ok = None;
        for n in 0..full.len() {
            match import(&full[..n]) {
                Ok(_) => {
                    first_ok.get_or_insert(n);
                }
                Err(ImageImportError::Corrupt { .. }) => {
                    assert!(
                        first_ok.is_none(),
                        "prefix {n} failed after a shorter one passed"
                    );
                }
                Err(other) => panic!("prefix {n}: {other:?}"),
            }
        }
        let second_frame = full.iter().rposition(|&b| b == IMAGE_DESCRIPTOR).unwrap();
        let ok_from = first_ok.unwrap();
        assert_eq!(
            ok_from, second_frame,
            "the first frame ends where the second begins"
        );
        assert_eq!(
            import(&full[..ok_from]).unwrap().notes.gif_frames_ignored,
            0
        );
    }

    #[test]
    fn an_index_past_the_colour_table_is_corrupt() {
        let err = import(&gif((1, 1), Some(BLACK_WHITE), &[spec(1, 1, vec![2])])).unwrap_err();
        assert!(matches!(err, ImageImportError::Corrupt { .. }), "{err:?}");
    }

    #[test]
    fn a_transparent_index_past_the_table_is_still_clear() {
        let mut s = spec(2, 1, vec![7, 1]);
        s.transparent = Some(7);
        let img = import(&gif((2, 1), Some(BLACK_WHITE), &[s])).unwrap();
        assert_eq!(mask(&img), [0, 255]);
    }

    #[test]
    fn no_colour_table_is_refused_by_name() {
        let err = import(&gif((1, 1), None, &[spec(1, 1, vec![0])])).unwrap_err();
        assert_eq!(
            err,
            ImageImportError::Unsupported {
                feature: "GIF/no-colour-table"
            }
        );
    }

    #[test]
    fn a_minimum_code_size_outside_1_to_8_is_corrupt() {
        let good = gif((1, 1), Some(BLACK_WHITE), &[spec(1, 1, vec![0])]);
        // The byte after the 10-byte image descriptor.
        let at = good.iter().rposition(|&b| b == IMAGE_DESCRIPTOR).unwrap() + 10;
        assert_eq!(good[at], 8);
        for size in [0u8, 9, 12, 13, 255] {
            let mut bad = good.clone();
            bad[at] = size;
            assert!(
                matches!(import(&bad), Err(ImageImportError::Corrupt { .. })),
                "min code size {size}"
            );
        }
    }

    #[test]
    fn a_huge_logical_screen_is_refused_before_allocating() {
        let bytes = gif((65535, 65535), Some(BLACK_WHITE), &[spec(1, 1, vec![0])]);
        assert_eq!(import(&bytes).unwrap_err(), ImageImportError::TooLarge);
    }

    #[test]
    fn gif87a_is_accepted() {
        let mut bytes = gif((1, 1), Some(BLACK_WHITE), &[spec(1, 1, vec![1])]);
        bytes[..6].copy_from_slice(b"GIF87a");
        assert_eq!(indices(&import(&bytes).unwrap()), [1]);
    }
}
